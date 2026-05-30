# Production Readiness — Strategy & Phased Execution

> **Authoritative production plan for `oracle-automate`.** Supersedes the
> SAP-era `PRODUCTION_PLAN.md` (kept for provenance). Reads alongside
> [`PORTING_STRATEGY.md`](PORTING_STRATEGY.md) (what was ported) and
> [`GAP_ANALYSIS.md`](GAP_ANALYSIS.md) (SAP independence). This document is
> about taking the *ported, building, test-green* codebase to **production
> against a real Oracle Fusion Cloud ERP pod for Kalbe**.
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
| Tests | 🟢 **179 pass / 0 fail** (offline, deterministic; +6 Fusion REST contract tests, Phase 3) |
| `cargo fmt --all --check` | ✅ green *(was 🔴 — see Phase 1)* |
| `cargo clippy … -D warnings` | ✅ green *(was 🔴 — see Phase 1)* |
| Oracle-correctness invariants (7) | 🟢 enforced as tests + dedicated CI job |
| Live Fusion/OIC transport wiring | 🟠 code exists & is selectable; **never run against a real pod** |
| Retrieval quality (embed + rerank) | 🟠 `MockEmbedder` (hash) + `MockReranker` (term-overlap) placeholders |
| Security: read-only-by-default, gated writes, audit log, secret redaction | 🟢 implemented & security-reviewed (v1.4.0) |
| Packaging: distroless image, K8s manifests, Grafana, runbook | 🟢 present, CI-linted |

**The single most important finding:** the project's own quality gate
(`fmt` + `clippy -D warnings`) was **red** on the current stable toolchain —
a floating-toolchain drift (rustfmt/clippy rules tightened in 1.94). A
codebase whose CI is red is not production-ready by definition, regardless of
feature completeness. Phase 1 closes this.

**The structural gap:** every test runs offline against in-memory Fusion
fixtures. The live transports (`HttpFusionClient`, `FusionPartyClient`,
`HttpOicClient`) compile and are wired into server startup
(`FusionConfig::from_env` / `--destination`), but **no code path has ever
reached a real Kalbe Fusion pod.** Closing that is gated on an *organisational*
dependency — pod URL, OAuth2/IDCS client, technical user — not on more code.

---

## 2. Production scorecard

| # | Capability | Score | Blocking? |
|---|---|---|---|
| 1 | CI quality gate green on current stable | 🟢 (Phase 1) | resolved |
| 2 | Reproducible toolchain (pinned + weekly advisory) | 🟢 (Phase 2) | resolved |
| 3 | Error/panic hygiene on live paths (`unwrap` audit) | 🟢 (Phase 2) — live clients `unwrap`-free | resolved |
| 4 | Live Fusion REST read against a real pod | 🟠 contract-pinned (Phase 3); pod run pending | **blocked (creds)** |
| 5 | Live gated write (PO/journal) + audit on a real pod | 🔴 unverified | **blocked (creds)** |
| 6 | Production retrieval quality (real embed + rerank) | 🟠 mock | quality |
| 7 | Observability tuned to real latency | 🟠 untuned | after live traffic |
| 8 | Operator onboarding runbook | 🟢 exists | refresh after live |

---

## 3. Phased plan

Each phase ends with a **green `cargo fmt` / `clippy -D warnings` / `test`**
and a commit. Phases 1–3 and 6 have **no external dependency** and are
executable immediately. Phases 4–5, 7 require a real Fusion pod from Kalbe
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

### Phase 4 — Live pod validation 🔒 BLOCKED on Kalbe Basis credentials
**Goal:** a configured destination drives a **real** Fusion REST read and one
gated write against the Kalbe dev pod.
- **External dependency (sequence first):** pod base URL, OAuth2/IDCS client id
  + secret (or basic technical user), confirmed network egress, a test Business
  Unit/Ledger. *This is an org hand-off, not code — track it as a blocker.*
- Run the env-gated live read tests (`oracle.rest.*`, `oracle.party.*`,
  `oracle.object.read` via BI Publisher). Confirm CSRF/OAuth token refresh under
  real latency.
- One **gated write** (`oracle.workflow.create_purchase_order`) under the
  read-only→elicitation→re-typed-confirmation guardrails; verify the audit line.
- **Skills:** `verify` / `run` (drive the real server), `investigate`
  (root-cause auth/latency), `security-review` (blocking, before `--enable-writes`).
- **Gate:** real document number returned + audit-logged; offline 173 stay green.

### Phase 5 — Observability tuned to real traffic 🔒 follows Phase 4
**Goal:** timeouts/retries/circuit-breaker thresholds set from measured pod
latency, not guesses; Grafana panels show live P95/P99 vs the 80 ms gate.
- **Skills:** `verify`, dashboard refresh.
- **Gate:** an operator follows `RUNBOOK_DEV_TENANT.md` and gets a live cited
  answer end-to-end.

### Phase 6 — Production retrieval quality (no external deps; can parallel 3)
**Goal:** replace deterministic placeholders with real models behind the
*existing* `EmbeddingClient` / `Reranker` traits — the seams are already clean.
- Add a real embedder (e.g. an ONNX/`fastembed` local model or a remote
  embedding endpoint) and a cross-encoder reranker, **feature-gated** so the
  default offline build + CI keep using the deterministic mocks (no network in
  CI, no quality regression in the bench gate).
- **Skills:** `claude-api` (if a hosted embedding endpoint is chosen),
  `code-review`, bench-gate `verify` (P95 < 80 ms must hold for the mock path).
- **Gate:** real retrieval improves NDCG on a small labelled set; mock path and
  the 80 ms bench gate unchanged.

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

Production-ready means, against the Kalbe Fusion dev pod:
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
