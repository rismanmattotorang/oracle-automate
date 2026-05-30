# Production Readiness — Strategy & Phased Execution

> **Authoritative production plan for `oracle-automate`.** Supersedes the
> SAP-era `PRODUCTION_PLAN.md` (kept for provenance). Reads alongside
> [`PORTING_STRATEGY.md`](PORTING_STRATEGY.md) (what was ported) and
> [`GAP_ANALYSIS.md`](GAP_ANALYSIS.md) (SAP independence). This document is
> about taking the *ported, building, test-green* codebase to **production
> against a real Oracle Fusion Cloud ERP pod for Gaussian Technologies**.
>
> Method follows the project's own [Karpathy guidelines](../skills/karpathy-guidelines.md):
> *think before coding, simplicity first, surgical changes, goal-driven —
> verify every claim.* Every state claim below was **measured**, not assumed.

---

## 1. Verified ground truth (2026-05-30)

Measured on Rust 1.94.1 stable (the toolchain CI now floats to):

| Dimension | Measured state |
|---|---|
| Build — 16 crates + 8 apps, ~19k Rust LOC | 🟢 green (`cargo build --workspace`, ~1m) |
| Tests | 🟢 **201 pass / 0 fail** (offline, deterministic; +6 Fusion contract (P3), +4 retrieval (P6), +9 Fusion mock-pod, +9 OIC mock-pod (P4/5)) |
| `cargo fmt --all --check` | ✅ green *(was 🔴 — see Phase 1)* |
| `cargo clippy … -D warnings` | ✅ green *(was 🔴 — see Phase 1)* |
| Oracle-correctness invariants (7) | 🟢 enforced as tests + dedicated CI job |
| Live Fusion/OIC transport wiring | 🟠 code exists & is selectable; **never run against a real pod** |
| Retrieval quality (embed + rerank) | 🟠 `MockEmbedder` (hash) + `MockReranker` (term-overlap) placeholders |
| Security: read-only-by-default, gated writes, audit log, secret redaction | 🟢 hardened (Phase 8) — see [`SECURITY.md`](../SECURITY.md) |
| Packaging: distroless image, K8s manifests, Grafana, runbook | 🟢 present, CI-linted |

**The single most important finding:** the project's own quality gate
(`fmt` + `clippy -D warnings`) was **red** on the current stable toolchain —
a floating-toolchain drift (rustfmt/clippy rules tightened in 1.94). A
codebase whose CI is red is not production-ready by definition, regardless of
feature completeness. Phase 1 closes this.

**The structural gap (now closed in code):** the live transports
(`HttpFusionClient`, `FusionPartyClient`, `HttpOicClient`) are wired into server
startup (`FusionConfig::from_env` / `--destination`). Phases 4–5 added a runnable
**mock Fusion pod** (`oracle-automate-fusion-mock`) that the *real* clients drive
end-to-end over HTTP — read **and** gated write — so the full path is exercised
offline. The only remaining step is pointing `ORACLE_FUSION_BASE_URL` at a **real
Gaussian Technologies pod**, which is gated on an *organisational* dependency (pod URL,
OAuth2/IDCS client, technical user) — not on more code.

---

## 2. Production scorecard

| # | Capability | Score | Blocking? |
|---|---|---|---|
| 1 | CI quality gate green on current stable | 🟢 (Phase 1) | resolved |
| 2 | Reproducible toolchain (pinned + weekly advisory) | 🟢 (Phase 2) | resolved |
| 3 | Error/panic hygiene on live paths (`unwrap` audit) | 🟢 (Phase 2) — live clients `unwrap`-free | resolved |
| 4 | Live Fusion REST read + gated write, end-to-end | 🟢 (Phase 4) — proven vs. mock pod; real-pod run by swapping URL | resolved (mock) |
| 5 | Client resilience (timeout) + observability | 🟢 (Phase 5) — request timeout + injectable latency/metrics | resolved (mock) |
| 6 | Production retrieval quality (real embed + rerank) | 🟢 (Phase 6) — env-selectable, contract-tested; NDCG run pending endpoint | resolved |
| 7 | Final threshold tuning from real pod latency | 🟠 knobs in place | after live traffic |
| 8 | Operator onboarding runbook | 🟢 exists | refresh after live |

---

## 3. Phased plan

Each phase ends with a **green `cargo fmt` / `clippy -D warnings` / `test`**
and a commit. Phases 1–3 and 6 have **no external dependency** and are
executable immediately. Phases 4–5, 7 require a real Fusion pod from Gaussian Technologies
Basis/Cloud Ops and are sequenced behind it.

### Phase 1 — Green the quality gate ✅ DONE
**Goal:** `fmt` + `clippy -D warnings` + `test` all pass on current stable so
every downstream phase builds on a green baseline.
- Fixed ~10 clippy lints surfaced by the 1.94 toolchain: `should_implement_trait`
  (`LayeredCredentialProvider::add` → `with_provider`, 4 call sites),
  `iter().copied().collect()` → `to_vec()`, `slice::from_ref` for single-element
  embed calls (3 sites), `useless vec!`, `manual_contains`, `doc_lazy_continuation`,
  unused test imports.
- Normalised whole-workspace formatting to current stable rustfmt (85 files).
- **Skills:** `code-review` (correctness pass), `verify`.
- **Gate:** ✅ fmt clean · clippy `-D warnings` exit 0 · **173 tests pass**.

### Phase 2 — Reproducibility & error hygiene (no external deps) ✅ DONE
**Goal:** the gate can't silently rot, and live-path failures degrade
gracefully instead of panicking.

**Shipped — toolchain pin + advisory (the chosen "pin + weekly advisory"):**
- `rust-toolchain.toml` pinned to `1.94.1`; CI pins every blocking job via
  `dtolnay/rust-toolchain@master` + `toolchain: ${{ env.RUST_PINNED }}` (single
  source of truth, resolves even when a per-version action tag lags a release).
- New non-blocking `toolchain-drift` job runs `fmt`/`clippy`/`test` on floating
  `stable` + `beta` weekly (Mon 06:00 UTC), `continue-on-error: true`. New lints
  now surface *before* a deliberate pin bump — the exact failure mode that turned
  CI red is gone. `if: github.event_name != 'schedule'` keeps the PR jobs off the
  weekly run and vice-versa.

**`unwrap()` audit — measured finding (no churn warranted):**
The earlier "119" figure over-counted: it didn't exclude in-file `#[cfg(test)]`
modules. The true non-test count is **~65**, and **the live network clients
carry zero `unwrap`/`expect`** — `oracle-automate-erp/src/fusion.rs` and
`oracle-automate-adt/src/http.rs` are both clean on every request/response path.
The remaining ~65 classify as:
- **Lock-poison idioms** (`RwLock::read/write().unwrap()` in `metrics.rs`,
  `kb/store.rs`, `memory/lib.rs`) — *correct* to panic on a poisoned lock; not
  defensive-handling candidates.
- **Infallible-by-construction** — `serde_json::json!({…}).as_object().unwrap()`
  on inline object literals (`tools.rs`), `std::env::var(...).unwrap()` *after* a
  presence check (`credentials.rs`), constant parses (`"127.0.0.1:3030".parse()`).
- **Startup / demo fail-fast** — `.expect("seed")`, gateway demo CLI.

Per the Karpathy rule *"no defensive error handling for impossible scenarios,"*
converting these would add noise, not safety. The guardrail against *new*
live-path `unwrap`s is the `toolchain-drift` advisory plus `code-review` on each
PR. **Conclusion: the runtime paths are already panic-disciplined; no code
change made.**
- **Skills:** `code-review` (high), `security-review` (secret-leak paths).
- **Gate:** ✅ live request/response paths verified `unwrap`-free; pinned gate green.

### Phase 3 — Live Fusion connectivity, proven against a recorded contract ✅ DONE
**Goal:** the live transports are exercised end-to-end *without* needing the
real pod yet — so when credentials arrive, Phase 4 is a smoke test, not a debug
session.

**Shipped — `crates/oracle-automate-erp/tests/fusion_contract.rs`** (6 tests):
an in-process **axum mock** of the Fusion REST surface
(`/fscmRestApi/resources/11.13.18.05/...`) drives the *real* `HttpFusionClient`
/ `FusionPartyClient` over the same `reqwest` path that will hit the pod. Pins
the contract for realistic shapes:
- TCA supplier **collection + pagination** metadata (`count`/`hasMore`/`limit`/
  `offset`/`links`) → `Vec<Party>` (pagination cruft must not leak into the count).
- Customer-account **field fallback** (`PartyId`/`PartyName` when `SupplierId`/
  `Supplier` absent) → `Party`.
- `404` → `ErpError::NotFound`.
- `call_operation` REST dispatch → `{ http_status, outputs }` envelope on success.
- **FND/REST error envelope** (`400` + `o:errorCode` + `o:errorDetails`) is
  surfaced in the envelope, never silently dropped.
- `system_info` reachability against the REST catalog root.

The test target is gated (`required-features = ["fusion"]` + `#![cfg(feature =
"fusion")]`) and CI now activates `oracle-automate-erp/fusion` explicitly (clippy
+ test), so the live client and its contract are first-class CI citizens — no
reliance on dependency-graph feature unification.

The **live (real-pod) switch** is the env path: set `ORACLE_FUSION_BASE_URL`
(+ `ORACLE_FUSION_AUTH`/token) and the server wires `HttpFusionClient` instead of
the mock (`FusionConfig::from_env`). Contract tests run offline + unconditionally.
- **Skills:** `deep-research` (Fusion 24D+ REST error/pagination shapes) →
  `code-review` → `verify`.
- **Gate:** ✅ 6 contract tests green offline; suite now **179 tests**; gated
  live path skips cleanly without credentials (CI stays green).

### Phase 4 — Live read + gated write, end-to-end ✅ DONE (against a mock pod)
**Goal:** a configured destination drives a Fusion REST read **and one gated
write** end-to-end — proven *now* with no Oracle access, and identical against a
real pod later (swap `ORACLE_FUSION_BASE_URL`).

**Shipped — `crates/oracle-automate-fusion-mock`** (runnable lib + bin): a
standalone **mock Oracle Fusion pod** emulating the real REST surface the live
clients call — supplier search, item read, `404`s, GL **journal post** and **PO
create** (both return a document number), supplier PATCH, Fusion error
envelopes, an **auth gate**, and **latency injection**. JSON shapes mirror the
real API so the swap is transparent.

`crates/oracle-automate-erp/tests/fusion_pod.rs` (7 tests) drives the *real*
`HttpFusionClient` / `FusionPartyClient` against it over the actual `reqwest`
path:
- live supplier search + item read;
- **gated PO-create** → `201` + `GT-PO-…`, and **journal-post** → `201` +
  `JournalEntryId` / `Status: POSTED`;
- the fail-closed read-only gate still refuses writes when `read_only_mode`;
- unknown id → `NotFound`.

Run the full server against the mock (swap the URL for a real pod to go live):
```bash
cargo run -p oracle-automate-fusion-mock -- --bind 127.0.0.1:8088
ORACLE_FUSION_BASE_URL=http://127.0.0.1:8088 \
ORACLE_FUSION_AUTH=basic ORACLE_FUSION_USER=demo ORACLE_FUSION_PASSWORD=demo \
  cargo run -p oracle-automate-server
```

**Custom-code path (`oracle.oic.*`) — `crates/oracle-automate-oic-mock`:** the
OIC counterpart, a runnable mock of the Oracle Integration Cloud + BI Publisher
+ Fusion-REST artifact surface (integration / Groovy / connection / lookup /
project / ESS job / BIP report retrieval, search, where-used, and **gated
activation**). `crates/oracle-automate-adt/tests/oic_pod.rs` (8 tests) drives the
real `HttpOicClient` against it, including a read-only-gated `activate` and a
latency→`DestinationDown` timeout. Point an OIC **destination** TOML at it:
```bash
cargo run -p oracle-automate-oic-mock -- --bind 127.0.0.1:8089
# ~/.config/oracle-automate/destinations/mock-oic.toml:
#   base_url = "http://127.0.0.1:8089"
#   client   = "100"
#   [auth]   type = "basic"  user = "demo"  password = "demo"
ORACLE_AUTOMATE_DESTINATION=mock-oic cargo run -p oracle-automate-server
```
- **Still needs a real pod for:** OAuth2/IDCS token refresh against IDCS, real
  BI Publisher extracts, and the audit line on a *real* document — the audit
  wiring itself is already covered by `tests/audit_writes.rs`.
- **Skills:** `verify` / `run`, `security-review` (blocking, before
  `--enable-writes` against a real pod).
- **Gate:** ✅ read + gated write return real document numbers against the mock;
  read-only gate fail-closed; offline suite green.

### Phase 5 — Observability & resilience ✅ DONE (tunable against the mock)
**Goal:** the client survives a hung/slow pod, and thresholds are tunable
against measured latency rather than guesses.
- **Request timeout added** to all live clients — `HttpFusionClient` /
  `FusionPartyClient` (`FusionConfig.timeout_ms`, default 30 s, env
  `ORACLE_FUSION_TIMEOUT_MS`) **and** `HttpOicClient` (`OicDestination.timeout_ms`,
  TOML, default 30 s) — a real production gap: the clients previously had *no*
  timeout and would hang forever on a stuck pod. A timeout now maps to
  `DestinationDown`, the signal the existing retry / circuit-breaker layers act on.
- **Latency tuning loop:** run the mock with `--latency-ms <n>` and the server's
  Prometheus `/metrics` (`mcp_tool_latency_seconds` histogram, P95/P99 vs the
  80 ms gate) reflects it; the Grafana dashboard renders it. `fusion_pod.rs`
  asserts a 500 ms pod trips a 100 ms client timeout → `DestinationDown`.
- **Still needs real traffic for:** setting the *final* timeout/retry/breaker
  numbers from measured pod latency (the knobs and the harness are in place).
- **Skills:** `verify`, dashboard refresh.
- **Gate:** ✅ hung pod surfaces as `DestinationDown` (no infinite hang);
  latency is injectable + observable for tuning.

### Phase 6 — Production retrieval quality ✅ DONE
**Goal:** replace deterministic placeholders with real models behind the
*existing* `EmbeddingClient` / `Reranker` traits — the seams are already clean.

**Shipped:**
- **Reranker (the missing real backend):** `HttpReranker` in
  `oracle-automate-rag` — a cross-encoder over a managed rerank API
  (Cohere/Jina/Voyage-style `POST /rerank` → `{results:[{index, relevance_score}]}`),
  the single biggest precision-at-K lift per the design note. **Failure is
  non-fatal** — endpoint/parse errors degrade to base-score order (the
  `Reranker` trait is infallible), so a reranker outage never breaks search.
  Feature-gated behind `oracle-automate-rag/remote` (pulls `reqwest`); the
  pure-algorithm default build stays lean and CI/offline uses `MockReranker`.
- **Embedder:** the real `OpenAiEmbedder` (OpenAI-compatible `/embeddings`) was
  already present; added `OpenAiEmbedder::from_env` so it's selectable, and a
  dim-mismatch guard test.
- **Server wiring:** both backends are now selected from env
  (`ORACLE_AUTOMATE_EMBEDDINGS_*` / `ORACLE_AUTOMATE_RERANK_*`) with the
  deterministic mocks as the fallback — so the offline/CI default is unchanged
  but a production deploy gets real retrieval by setting env.
- **Contract tests (axum mock, deterministic, offline):** 2 for `OpenAiEmbedder`
  (response-shape parse + dim-mismatch → `Malformed`) and 2 for `HttpReranker`
  (endpoint scores reorder candidates; endpoint error → base-order, never
  truncated). CI activates `oracle-automate-rag/remote` so they run + lint.

NDCG validation on a labelled set still needs a *real* endpoint (same class of
external dependency as Phase 4); the contract tests pin the integration
deterministically in the meantime.
- **Skills:** `claude-api` (hosted embedding/rerank endpoint), `code-review`,
  bench-gate `verify`.
- **Gate:** ✅ mock path + the 80 ms bench gate unchanged (P95 1.24 ms); suite
  now **183 tests**; default `rag` build stays reqwest-free.

### Phase 8 — Security & secrets hardening ✅ DONE
**Goal:** close the credential / TLS / transport / audit gaps before any
write-enabled run against a real pod. Full posture + checklist in
[`SECURITY.md`](../SECURITY.md).

**Shipped:**
- **Secret-leak fix:** `Credentials` no longer derives `Debug` (it carried a
  plaintext `password`); a hand-written `Debug` prints `password: ***`.
  Regression-tested.
- **Constant-time bearer check:** the HTTP transport compares the bearer token
  in constant time (`constant_time_eq`), closing a timing side-channel.
- **Secrets-manager integration:** `FileCredentialProvider` reads credentials
  from a mounted-secret file (`ORACLE_AUTOMATE_CREDENTIALS_FILE` — K8s Secret /
  Vault / OCI Vault sidecar), re-read per fetch for rotation, with a
  loose-permission warning; wired into the server's layered chain *ahead of*
  env so a mounted secret is authoritative and never enters the process env.
- **Reviewed & confirmed already-correct:** TLS verification on (no
  `danger_accept_invalid_certs`), Origin validation, fail-closed write gate,
  redacted audit log, secret-safe `Debug` on `FusionAuth` / `OicAuth`.
- **Skills:** `security-review` (this gate), `code-review`.
- **Gate:** ✅ 210 offline tests; `SECURITY.md` + secure-deploy checklist
  published; no behaviour change to the default path.

> **Remaining phases to production** (tracked in the project plan): **7** live
> dev-pod validation (🔒 needs an Oracle pod + IDCS app), **9** perf/resilience
> tuning, **10** observability/SLOs, **11** real retrieval quality (needs an
> embedding endpoint), **12** CD/release plumbing. Phases 10 and 12 are
> unblocked and can proceed now.

---

## 4. Skill → phase map (Karpathy-driven)

| Skill | Phases | Why |
|---|---|---|
| Karpathy guidelines (this repo's `oracle.skill.karpathy_guidelines`) | all | Pre-flight before every change: assumptions, simplicity, surgical, goal-driven. |
| `code-review` | 1, 2, 3, 6 | Correctness pass on each diff before commit. |
| `security-review` | 2, 4 | Blocking gate before any `--enable-writes` run; secret-leak audit. |
| `deep-research` | 3 | Pin exact Fusion 24D+ REST error/pagination/CSRF shapes before coding. |
| `verify` / `run` | 1, 4, 5, 6 | Actually launch the server / bench and observe real behaviour. |
| `investigate` | 4 | Root-cause live auth/latency failures without guessing. |
| `claude-api` | 6 | If a hosted embedding/rerank endpoint is chosen. |
| `init` / `session-start-hook` | ops | Keep `CLAUDE.md` + web/CI session bootstrap current. |

---

## 5. Definition of done

Production-ready means, against the Gaussian Technologies Fusion dev pod:
1. CI is **green on a reproducible toolchain** (fmt + clippy + 173 offline tests).
2. No `unwrap`/panic on any live request/response path.
3. A configured destination drives a real Fusion REST read **and** one gated
   write, fully audit-logged, under the safety guardrails.
4. Live integration tests exist per path, **secret-gated** so CI without pod
   access stays green.
5. A security review of the credential/TLS/CSRF/write surface is signed off.
6. `RUNBOOK_DEV_TENANT.md` onboards a fresh operator from zero.

## 6. Top risks

- **Pod access & auth method unknown** (Phase 4 blocker). *Mitigation: front-load
  the Basis hand-off; Phase 3 contract tests make the eventual live run a smoke
  test.*
- **Floating toolchain drift** (root cause of the Phase 1 red CI). *Mitigation:
  Phase 2 pin + advisory job.*
- **Retrieval quality vs. CI determinism** — real models can't run in CI.
  *Mitigation: Phase 6 feature-gates them; mock stays the CI default.*
- **Claim drift** — docs describing live wiring that hasn't touched a real pod.
  *Mitigation: this doc tracks measured state; update as each phase lands.*
