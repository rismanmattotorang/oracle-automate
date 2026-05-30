# Releasing & deploying Oracle-Automate

The release pipeline ([`.github/workflows/release.yml`](.github/workflows/release.yml))
turns a version tag into signed, scanned artifacts. This is the operator runbook
for cutting a release and promoting / rolling it back.

## Versioning

[SemVer](https://semver.org). The workspace version lives in the root
`Cargo.toml` (`[workspace.package].version`) and is stamped into every crate.

## Cutting a release

1. **Land all changes** on the default branch; CI green.
2. **Update the changelog** — rename the `[Unreleased]` heading in
   [`CHANGELOG.md`](CHANGELOG.md) to `## [X.Y.Z] — <date>` and add a fresh empty
   `[Unreleased]` above it.
3. **Bump the version** in `Cargo.toml` (`[workspace.package].version = "X.Y.Z"`);
   run `cargo build` so `Cargo.lock` updates; commit.
4. **Tag and push**:
   ```bash
   git tag vX.Y.Z
   git push origin vX.Y.Z
   ```
   The tag (`v*.*.*`) triggers `release.yml`.

## What the pipeline does (on a `v*.*.*` tag)

1. **Binaries** — builds `oracle-automate-server` + `oracle-automate-gw` for
   `x86_64` and `aarch64` Linux (pinned Rust `1.94.1`), with `.sha256` checksums.
2. **Container** — builds the amd64 image, **Trivy-scans it and fails the release
   on any fixable CRITICAL/HIGH CVE** *before* pushing; then builds + pushes the
   multi-arch image to `ghcr.io/<owner>/<repo>` with an **SBOM + SLSA provenance**
   attestation, and **keyless-signs** it with cosign (Sigstore, GitHub OIDC).
3. **Publish** — cuts a GitHub Release with the binaries, checksums, the image
   reference, and the cosign verification command.

## Verifying an image before deploy

```bash
cosign verify ghcr.io/<owner>/<repo>:vX.Y.Z \
  --certificate-identity-regexp "^https://github.com/<owner>/<repo>/" \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com
```

Pin production to the **digest**, not a tag:

```bash
docker buildx imagetools inspect ghcr.io/<owner>/<repo>:vX.Y.Z --format '{{.Manifest.Digest}}'
```

## Deploying (GitOps with Kustomize)

The manifests in [`deploy/k8s/`](deploy/k8s) are Kustomize-based — natively
GitOps-friendly (Argo CD / Flux). Promotion is a Git change to the pinned image:

```bash
# In your environment overlay (staging then prod), pin the verified digest:
cd deploy/k8s
kustomize edit set image ghcr.io/acme/oracle-automate=ghcr.io/<owner>/<repo>@sha256:<digest>
git commit -am "deploy: oracle-automate vX.Y.Z (sha256:<digest>) to staging"
```

- **Staging → prod promotion:** merge the same digest from the staging overlay
  to the prod overlay. Never promote a tag that wasn't verified in staging.
- Also install the SLO rules + scrape config once per cluster:
  [`deploy/prometheus/alerts.yaml`](deploy/prometheus/alerts.yaml),
  [`deploy/prometheus/servicemonitor.yaml`](deploy/prometheus/servicemonitor.yaml).

A rolling update is gated by the readiness probe (`/health`); the
`PodDisruptionBudget` keeps capacity during the roll.

## Rollback

**Fast path (imperative):**
```bash
kubectl -n oracle-automate rollout undo deployment/oracle-automate-server
kubectl -n oracle-automate rollout status deployment/oracle-automate-server
```

**GitOps path (preferred — keeps Git the source of truth):** revert the deploy
commit (re-pin the previous digest) and let Argo/Flux sync:
```bash
git revert <deploy-commit> && git push      # re-pins the previous sha256 digest
```

**Verify recovery:** `oracle_automate_tool_error_ratio` back under 1% and P95
under 80 ms (see [`docs/SLO.md`](docs/SLO.md)); the `OracleAutomateDown` /
`HighErrorRate` alerts clear.

## Rollback decision triggers

Roll back if, post-deploy: the `OracleAutomateHighErrorRatePage` alert fires,
P95 latency breaches the SLO for >10m, readiness probes fail to stabilise, or a
write path returns unexpected results. Prefer rollback over hotfix-forward when
an SLO is actively burning.
