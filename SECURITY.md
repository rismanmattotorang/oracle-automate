# Security

Oracle-Automate is built to act on a production ERP, so security is a
first-class property, not an afterthought. This document records the security
posture, the Phase-8 hardening review, and how to deploy securely.

> Authoritative production plan: [`docs/PRODUCTION_READINESS.md`](docs/PRODUCTION_READINESS.md).

## Reporting a vulnerability

Email **security@gaussian.technology** with details and a reproduction. Please
do not open a public issue for undisclosed vulnerabilities. We aim to
acknowledge within 3 business days.

## Security model

| Control | Status |
|---|---|
| **Read-only by default** | Write tools are *hidden* from `tools/list` until the operator starts with `--enable-writes` (or `ORACLE_AUTOMATE_ENABLE_WRITES=1`). |
| **Fail-closed write gate** | `call_operation` refuses any state-mutating op in read-only mode; a BAPI/REST call with no parseable result is treated as *unconfirmed* and rolled back, never committed on faith. |
| **High-stakes elicitation** | PO create / customer master / sandbox publish pause mid-flow for typed confirmation; the agent can never fabricate the confirmation. |
| **Cite every claim** | Answers carry `oracle-help://` / `oracle-rest://` / `oracle-object://` provenance URIs. |
| **Redacted audit log** | Every state-mutating call records a redacted `AuditEntry` (event id, timestamp, tool, system, redacted args, outcome, duration). |
| **TLS verification on** | All HTTP clients use `reqwest` + `rustls`; certificate verification is on by default and there is **no** `danger_accept_invalid_certs` anywhere in the tree. |
| **No secrets in logs** | `Credentials`, `FusionAuth`, and `OicAuth` have hand-written `Debug` impls that print only a label / `***`; `password` is `#[serde(skip_serializing)]`; `redacted()` is the only log-safe view. |
| **Constant-time bearer check** | The HTTP transport compares the bearer token in constant time (no early-exit per byte). |
| **Origin validation** | HTTP transport validates the `Origin` header (MCP 2025-06-18 §4.6, DNS-rebinding mitigation) when an allow-list is configured. |

## Secrets management

Order of preference, most to least secure:

1. **Secrets-manager file** (recommended for production). Set
   `ORACLE_AUTOMATE_CREDENTIALS_FILE=/run/secrets/oracle.json`; the
   `FileCredentialProvider` reads it on every fetch (so rotation needs no
   restart) and the secret never enters the process environment. Project the
   secret as a file via:
   - a **Kubernetes Secret** mounted as a volume,
   - a **HashiCorp Vault** Agent / **OCI Vault** sidecar that syncs to a path.

   Expected JSON: `{ "base_url": "...", "client": "100", "user": "SVC", "password": "..." }`.
   Keep the file mode at `0600`; the provider warns on Unix if it is
   group/world-readable.

2. **Environment variables** (`ORACLE_FUSION_*`). Convenient, but env leaks via
   `/proc/<pid>/environ`, crash dumps, and child processes — prefer the file
   provider in production.

3. **Static** (offline demo / tests only — never real credentials).

Destinations for the OIC client live as TOML under
`~/.config/oracle-automate/destinations/<name>.toml` (or
`ORACLE_AUTOMATE_DESTINATION_DIR`); these files are git-ignored and the loader
warns on loose Unix permissions.

For Fusion auth, prefer **OAuth2 / IDCS** bearer tokens over HTTP Basic; the
token is injected (`ORACLE_FUSION_ACCESS_TOKEN`) and never logged.

## HTTP transport hardening

When running `--transport http`:

- Set `--bearer-token` (or front the service with an authenticating proxy /
  service mesh). Without it the endpoint is unauthenticated.
- Set `--allowed-origin` for every browser origin you trust; leave empty only
  for stdio or trusted in-cluster traffic.
- Terminate TLS at an ingress / mesh (the server speaks plain HTTP and expects
  to sit behind TLS termination in-cluster).
- The `/metrics` and `/health` endpoints are intentionally unauthenticated for
  scrapers; do not expose them publicly.

## Secure-deploy checklist

- [ ] Credentials supplied via `ORACLE_AUTOMATE_CREDENTIALS_FILE` (mounted secret), not env or static.
- [ ] Secret file mode `0600`; destination TOMLs not world-readable.
- [ ] `--enable-writes` only where writes are required; default elsewhere.
- [ ] `--bearer-token` set and `--allowed-origin` constrained for HTTP transport.
- [ ] TLS terminated at ingress; mTLS between mesh peers where applicable.
- [ ] Audit sink routed to tamper-evident storage (S3 Object Lock / Loki / Splunk HEC).
- [ ] `cargo audit` clean in CI (advisory job); image scanned before push.
- [ ] Distroless runtime image (no shell / package manager) — already the default.

## Phase-8 hardening review (2026-05)

Reviewed the credential, TLS, transport, and audit surface. Findings and fixes:

| Finding | Severity | Resolution |
|---|---|---|
| `Credentials` derived `Debug` with a plaintext `password` field — a stray `{:?}` / `tracing::debug!(?creds)` would leak it. | Medium | Replaced with a hand-written `Debug` that prints `password: ***`; added a regression test. |
| Bearer token compared with `==` (`String` equality) — timing side-channel. | Low | Constant-time byte comparison (`constant_time_eq`); added tests. |
| No first-class secrets-manager integration (env / static only). | Hardening | Added `FileCredentialProvider` (mounted-secret pattern) + env opt-in + loose-permission warning; wired into the server's layered chain ahead of env. |
| TLS verification, Origin validation, fail-closed write gate, audit redaction. | — | Reviewed; already correct. No change. |

No behaviour change to the offline/default path; 210 offline tests remain green.
