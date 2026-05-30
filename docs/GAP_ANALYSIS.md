# Gap Analysis — Independence from SAP-Automate

> Outcome of the post-port review: re-surveyed the upstream **SAP-Automate**
> source, closed the remaining implementation gaps, and verified that
> **Oracle-Automate no longer depends on SAP-Automate** to build, test, or run.

## Method

Re-cloned the upstream `rismanmattotorang/sap-automate` and diffed it,
layer by layer, against `oracle-automate`. Every layer that still carried an
SAP *implementation*, *runtime dependency*, *data fixture*, or *identifier*
was closed. The remaining mentions of "SAP" are explanatory comments only.

## Gaps found and closed (this review)

| Gap | Status |
|---|---|
| **Live SOAP RFC client** (`soap.rs`, 891 lines — SAP `/sap/bc/soap/rfc`) | **Removed.** Replaced by `fusion.rs` `HttpFusionClient` over Oracle Fusion REST. |
| **Live OData client** (`odata.rs`, 565 lines — SAP Business Accelerator Hub) | **Removed.** Replaced by `FusionPartyClient` (TCA suppliers/accounts) over Fusion REST. |
| **Live ADT client** (`http.rs`, 930 lines — SAP ADT, CSRF, `X-SAP-Client`, XSUAA) | **Removed.** Replaced by `HttpOicClient` over Oracle OIC / BI Publisher / Fusion REST. |
| **SAP-jargon types** (`Rfc*`, `BapiRet2*`, `S_RfcAuth`, `Adt*`) | **Renamed** to `Erp*` / `ErpMessage` / `RequiredPrivilege` / `Oic*`. |
| **SAP-jargon trait methods** (`call_rfc`, `rfc_metadata`, `get_program`, `get_class`, …) | **Renamed** to `call_operation`, `operation_metadata`, `get_integration`, `get_groovy_script`, … |
| **SAP credential fields/env** (`ashost`/`sysnr`/`saprouter`, `SAP_ASHOST`…) | **Renamed** to `base_url`/`instance`/`proxy_url`, `ORACLE_FUSION_*`. |
| **GraphRAG demo corpus** (SAP nodes: `ZFIN_POST_JE`, `BAPI_*`, `T001`, `BSEG`, `LeanIX`) | **Rewritten** to Oracle nodes (`GT_*` integrations, `fusion.*` REST ops, `GL_*`/`XLA_*` objects, app-catalog, Oracle Help); `EntityKind` Oracle-renamed. |
| **Server var names** (`sap_client`, `business_hub`, `sap_system`, `sap_audit`) | **Renamed** to `erp_client`, `party_client`, `erp_system`, `oracle_audit`. |

Earlier phases (P0–P8) had already ported the operation/object catalogue,
the MCP tool/resource/prompt surface, the retrieval `Domain` enum + seed
corpus, the skills, scheduler, gateway, TUI, web UI, deploy manifests, CI
gate, and docs.

## Independence: verified

- **Build/runtime:** no crate, dependency, env var, endpoint, or data fixture
  references SAP. `cargo build --workspace` and `cargo test --workspace`
  (**173 tests**) are green with no SAP code in the path.
- **Self-contained:** a developer never needs the SAP-Automate repo to
  understand or operate Oracle-Automate. All domain vocabulary is Oracle
  Fusion Cloud ERP.

## Intentionally retained (not gaps)

- **Apache-2.0 attribution** to ParagonCorp / SAP-Automate in `README`,
  `CHANGELOG`, `LICENSE`, and `docs/PORTING_STRATEGY.md`. This is the upstream
  project credit the license **requires**; it is provenance, not a dependency.
- **Provenance banners** on the original historical narrative docs
  (`ROADMAP` / `COMPARISON` / `INTEGRATION` / `RUNBOOK` / `PRODUCTION_PLAN`),
  which point to `PORTING_STRATEGY.md` + `ORACLE_CORRECTNESS.md` as authoritative.

## Known residual (cosmetic)

- ~95 lines of **explanatory code comments** still phrase an Oracle concept by
  reference to its legacy equivalent (e.g. "the change-promotion unit").
  These are documentation only — no functional impact — and can be trimmed
  opportunistically. They do not constitute a dependency on SAP-Automate.

## Recommended Oracle-specific next steps (new session)

- Wire the live `HttpFusionClient` / `HttpOicClient` / `FusionPartyClient`
  against a real Fusion pod (OAuth2/IDCS), and add live integration tests
  behind an env gate.
- Expand the Fusion REST operation catalogue (AP/AR/Assets, HCM payroll,
  Procurement contracts) and the BI Publisher / OTBI extract surface.
- Replace the deterministic `MockReranker` / `MockEmbedder` with a real
  embedding + cross-encoder for production retrieval quality.
