# Porting Strategy — `sap-automate` → `oracle-automate`

> **Goal.** Port the ParagonCorp **SAP-Automate** platform (an MCP-native, Rust,
> on-prem agentic OS for SAP S/4HANA) into **Oracle-Automate** — the same
> architecture, re-fitted for **Oracle Fusion Cloud ERP (latest release,
> 24D / 25-series)** and rebranded for **Kalbe** (PT Kalbe Farma Tbk).
>
> This is a *faithful structural port*: we keep the proven architecture
> (trait-based seams, layered RAG, MCP 2025-06-18 coverage, read-only-by-default
> safety, correctness-as-tests) and replace the **SAP domain model** with the
> **Oracle Fusion Cloud ERP domain model**.

---

## 1. Source vs. target at a glance

| Axis | Source (SAP-Automate) | Target (Oracle-Automate) |
|---|---|---|
| Product name | SAP-Automate | Oracle-Automate |
| Owner / brand | ParagonCorp (TPO R&D) | Kalbe (PT Kalbe Farma Tbk) |
| ERP system | SAP S/4HANA 2024 / ECC / ABAP Cloud | **Oracle Fusion Cloud ERP** (24D+), with Oracle EBS 12.2 as the on-prem fallback |
| Integration protocol | RFC / BAPI (SOAP `/sap/bc/soap/rfc`), OData v4, ADT REST | **Oracle Fusion REST APIs** (`fscmRestApi`, `crmRestApi`, `hcmRestApi`), **SOAP** (ERP Integration Service), **BI Publisher / OTBI** for queries |
| Bulk / write pattern | BAPI + `BAPI_TRANSACTION_COMMIT`; transports | **FBDI** (File-Based Data Import) + ERP Integration `importBulkData`; REST POST; **Sandboxes** for config |
| Custom code surface | ABAP / ADT (programs, classes, CDS, function modules) | **Oracle Integration Cloud (OIC)** integrations, **Application Composer** (Groovy), **BI Publisher** reports, **PL/SQL** (EBS) |
| "Universal Journal" | `ACDOCA` | Oracle GL: `GL_JE_LINES` + Subledger Accounting `XLA_AE_LINES` + `GL_BALANCES` |
| Tenant scoping | Client (`MANDT` / `RCLNT`, CLNT(3) first key) | **Ledger / Business Unit / Legal Entity** (`LEDGER_ID`, `BU_ID`, `LEGAL_ENTITY_ID`); EBS multi-org `ORG_ID` |
| Return contract | `BAPIRET2` (TYPE S/E/W/I/A + message class/number) | Fusion REST error payload + EBS API standard (`x_return_status` S/E/U/W + `FND_MSG_PUB`) |
| AuthZ objects | `S_RFC`, `S_TABU_DIS` | Oracle **RBAC**: privileges, duty/job/data roles; SoD via **Oracle Risk Management Cloud (Advanced Access Controls)** |
| Master data | Business Partner (`API_BUSINESS_PARTNER`) | **Oracle TCA** parties — Suppliers (`suppliers`), Customers (`crmRestApi`) |
| Docs corpus | SAP Help Portal | **Oracle Help Center** (`docs.oracle.com`, Fusion Apps guides) |
| License | Apache-2.0 | Apache-2.0 (unchanged) |

### Why Oracle Fusion Cloud ERP (not EBS) as primary target

"Oracle ERP latest edition" = **Oracle Fusion Cloud ERP**. It is REST/SOAP-first,
which maps cleanly onto the existing HTTP transport layer. Where a concept only
has a faithful analog in **Oracle E-Business Suite 12.2** (e.g. the explicit
`p_commit` two-phase write, `ORG_ID` multi-org, PL/SQL `FND_MSG_PUB`), we model
it as the **on-prem backend** behind the same trait — exactly as the source kept
ECC alongside S/4HANA.

---

## 2. Domain mapping — the substance of the port

### 2.1 ERP module map

| SAP module | Oracle Fusion Cloud pillar |
|---|---|
| FI (Financial Accounting) | **Oracle Financials Cloud** — General Ledger, Payables, Receivables, Assets, Cash Mgmt |
| CO (Controlling) | Financials Cloud — **Cost Accounting**, Project Costing, Profitability |
| MM (Materials Mgmt) | **Oracle Procurement Cloud** + **Inventory Management (SCM)** |
| SD (Sales & Distribution) | **Oracle Order Management Cloud** (SCM) |
| PP / Logistics | **Oracle Manufacturing / SCM Cloud** |
| HCM / HR | **Oracle HCM Cloud** (Core HR, Payroll) |

### 2.2 Tables / data objects

| SAP table | Oracle (Fusion / EBS) object | Notes |
|---|---|---|
| `MARA` (material master) | `EGP_SYSTEM_ITEMS_B` (EBS `MTL_SYSTEM_ITEMS_B`) | Item number VARCHAR2(300) in Fusion |
| `T001` (company codes) | `GL_LEDGERS` + `XLE_ENTITY_PROFILES` (legal entities) | Company code → Ledger / LE |
| `T001B` (posting periods) | `GL_PERIOD_STATUSES` | "Manage Accounting Periods" |
| `BSEG` (doc segment) | `XLA_AE_LINES` (subledger) / `GL_JE_LINES` | compatibility-view analog |
| `FAGLFLEXA` (new GL) | `GL_JE_LINES` + `GL_BALANCES` | |
| `ACDOCA` (Universal Journal) | `GL_JE_LINES` + `XLA_AE_LINES` + `GL_BALANCES` | Oracle has **no single** universal journal; we model the GL+SLA backbone and document the difference |
| `VBAK` (sales header) | `DOO_HEADERS_ALL` (EBS `OE_ORDER_HEADERS_ALL`) | Fusion Order Mgmt / EBS OM |
| `E070` (transport header) | **Sandbox** metadata / FSM **Configuration Package** | "transport" → sandbox publish / config-set migration |

### 2.3 Remote functions (BAPI/RFC → REST/SOAP/FBDI)

| SAP function | Oracle Fusion operation |
|---|---|
| `RFC_SYSTEM_INFO` | `GET .../fscmRestApi/resources/...` health + `serverTimezone`; OIC ping |
| `RFC_READ_TABLE` | **BI Publisher report** run (`/xmlpserver/services/...`) or OTBI analysis; bounded row cap retained |
| `DDIF_FIELDINFO_GET` | REST `describe` (`.../resources/.../itemsV2/describe`) / `GET ?metadataMode=full` |
| `BAPI_ACC_DOCUMENT_POST` | **Journal Import**: FBDI `JournalImportTemplate` → `GL_INTERFACE` → `importBulkData`, or REST `journalEntries` POST |
| `BAPI_GOODSMVT_CREATE` | `receivingReceiptRequests` REST (Receiving) |
| `BAPI_SALESORDER_CREATEFROMDAT2` | `salesOrdersForOrderHub` REST (Order Import) |
| `BAPI_PO_CREATE1` | `purchaseOrders` / `draftPurchaseOrders` REST |
| `BAPI_CUSTOMER_CHANGEFROMDATA1` | `crmRestApi/.../accounts` PATCH; supplier via `suppliers` PATCH |
| `BAPI_TRANSACTION_COMMIT` | (Fusion: implicit per-request commit) / (EBS: `p_commit => FND_API.G_TRUE`) |
| `TMS_MGR_FORWARD_TR_REQUEST` (transport release) | **Sandbox publish** / FSM config-package export → import to target pod |

### 2.4 DDIC types → Oracle types

| ABAP DDIC | Oracle |
|---|---|
| `CHAR(n)` | `VARCHAR2(n)` |
| `NUMC(n)` | `VARCHAR2(n)` (zero-padded) / `NUMBER` |
| `DATS` | `DATE` |
| `TIMS` | `DATE` / `TIMESTAMP` |
| `CURR`, `DEC` | `NUMBER` |
| `CLNT(3)` | *(no analog)* → `ORG_ID NUMBER` (EBS) / scoping columns |
| `UNIT`, `CUKY` | `VARCHAR2` (UOM / currency code) |

### 2.5 Correctness invariants (the "X-correctness tests")

The source ships 7 SAP-correctness precision tests. We replace them with
**Oracle-correctness** invariants:

| SAP invariant (drop) | Oracle invariant (add) |
|---|---|
| `every_write_bapi_has_bapiret2_in_tables` | `every_write_rest_op_returns_standard_result` (REST error envelope / EBS `x_return_status`+`x_msg_data`) |
| `every_write_bapi_requires_commit` | `every_bulk_write_uses_interface_then_import` (FBDI/interface two-step) **or** EBS `p_commit` present |
| `every_rfc_has_at_least_one_authorization_entry` | `every_op_declares_required_privilege` (Fusion privilege / duty role) |
| `every_table_has_client_as_first_key` | `every_business_object_declares_scoping_column` (`LEDGER_ID` / `BU_ID` / `ORG_ID` where applicable) |
| `material_number_is_char_40_per_s4hana` | `item_number_is_varchar2_300_per_fusion` |
| `acdoca_is_present_and_marked_as_universal_journal` | `gl_je_lines_is_present_as_accounting_backbone` + a note that Oracle has no single universal journal |
| `compatibility_views_carry_s4hana_storage_note` | `subledger_objects_note_xla_to_gl_transfer` (XLA → GL transfer) |

### 2.6 Retrieval `Domain` enum

| SAP variant | Oracle variant | Source |
|---|---|---|
| `Abap` | `Integration` | OIC integrations / Application Composer Groovy / PL/SQL |
| `Bpmn` | `Bpmn` *(kept — vendor-neutral)* | Oracle Process Automation models |
| `Leanix` | `AppCatalog` | Fusion application portfolio / EA fact sheets |
| `SapHelp` | `OracleHelp` | Oracle Help Center (`docs.oracle.com`) |

### 2.7 Tool namespace

| SAP tool prefix | Oracle tool prefix |
|---|---|
| `sap.*` (`sap.docs.search`, `sap.rfc.call`, `sap.table.read`, `sap.bp.*`, `sap.workflow.*`, `sap.system.*`, `sap.kb.navigate`) | `oracle.*` (`oracle.docs.search`, `oracle.rest.call`, `oracle.bip.read`, `oracle.party.*`, `oracle.workflow.*`, `oracle.system.*`, `oracle.kb.navigate`) |
| `abap.adt.*` | `oracle.oic.*` (get_integration / get_lookup / where_used / activate) |
| `sap.rfc.*` | `oracle.rest.*` (search / metadata / bulk_metadata / call) |
| `bpmn.find_process`, `eam.search_apps` | `bpmn.find_process`, `eam.search_apps` *(kept)* |

### 2.8 Crate / binary renames

| SAP crate | Oracle crate | Domain depth |
|---|---|---|
| `mcp-core`, `mcp-transport`, `mcp-server`, `mcp-client` | *(unchanged names — generic MCP)* | none |
| `sap-automate-kb` | `oracle-automate-kb` | light (Domain enum) |
| `sap-automate-rag` | `oracle-automate-rag` | light |
| `sap-automate-graph` | `oracle-automate-graph` | light |
| `sap-automate-ingest` | `oracle-automate-ingest` | light |
| `sap-automate-memory` | `oracle-automate-memory` | none |
| `sap-automate-observability` | `oracle-automate-observability` | none |
| `sap-automate-skills` | `oracle-automate-skills` | light (skill content) |
| `sap-automate-scheduler` | `oracle-automate-scheduler` | light (job names) |
| `sap-automate-channels` | `oracle-automate-channels` | none |
| `sap-automate-connectors` | `oracle-automate-connectors` | medium |
| **`sap-automate-rfc`** | **`oracle-automate-rfc`** (logical: `erp`) | **heavy** — REST/SOAP/BIP catalogue, FND/REST return parser, Fusion fixtures |
| **`sap-automate-adt`** | **`oracle-automate-adt`** (logical: `oic`) | **heavy** — OIC/Application Composer/BIP artifact retrieval |
| `apps/sap-automate-*` | `apps/oracle-automate-*` | tie-up |

> **Note on crate names.** Phase 1 renames the *product prefix* only
> (`sap-automate-rfc` → `oracle-automate-rfc`) to keep the workspace building.
> The deeper *logical* renames (`-rfc`→`-erp`, `-adt`→`-oic`) are applied in
> Phases 2–3 once the domain content is re-modeled, so directory churn lands
> together with the semantic change.

---

## 3. Phased plan

Each phase ends with a **green `cargo build` / `cargo test`** (the workspace
`members` list grows phase-by-phase so the build is never red) and a commit.

| Phase | Title | Scope | Exit gate |
|---|---|---|---|
| **P0** | **Strategy & survey** | This document; full source survey; domain mapping locked | strategy reviewed |
| **P1** | **Foundation / rebrand** | Lift the full tree; product-token rename (`sap-automate`→`oracle-automate`, `ParagonCorp`→`Kalbe`); rename crate/app dirs; fix manifests; rewrite README + AGENTS.md for Oracle/Kalbe; drop SAP-only binaries (whitepaper PDF, SAP screenshots) | `cargo build --workspace` green; generic `mcp-*` crates test-green |
| **P2** | **Core ERP domain** | Re-model `oracle-automate-rfc` (→ logical `erp`): `ErpClient` trait, Fusion REST/BIP catalogue, FND/REST return parser (`BAPIRET2`→`ErpResult`), Oracle fixtures (`GL_JE_LINES`, `GL_LEDGERS`, `EGP_SYSTEM_ITEMS_B`, `GL_PERIOD_STATUSES`, …), Oracle-correctness tests; map `Domain` enum | Oracle invariants enforced; crate test-green |
| **P3** | **Custom-code surface** | Re-model `oracle-automate-adt` (→ logical `oic`): artifact retrieval for OIC integrations / Application Composer / BIP; `where_used`/`activate`→sandbox publish; mock + HTTP backends | crate test-green |
| **P4** | **Retrieval & seed corpus** | Port `kb`/`rag`/`graph`/`ingest` (Domain enum, tokeniser identifier rules for Oracle item/PO numbers); rewrite seed corpus to Oracle docs (Financials/Procurement/Order Mgmt/HCM); Oracle Help crawler targets | P95 gates hold; corpus is Oracle |
| **P5** | **Server / tools / resources / prompts** | Re-namespace tools (`sap.*`→`oracle.*`, `abap.adt.*`→`oracle.oic.*`); resources (`sap-system://`→`oracle-erp://`); workflow tools (PO create / customer master / **sandbox publish**) | server boots; `tools/list` is Oracle; integration tests green |
| **P6** | **Skills, scheduler, gateway, channels** | Rewrite 13 skills for Oracle (period close, SoD via AAC, config-set impact, REST service design, **Fusion personalization-not-customization** audit); scheduler jobs; gateway/channels | gw end-to-end; skills auto-load |
| **P7** | **Apps & web UI** | TUI tab labels/data; bench corpus; Next.js web UI copy/routes; screenshots regenerated | web build green; TUI runs |
| **P8** | **Deploy, CI, docs** | K8s/Docker rebrand; CI workflow (Oracle precision gate); rewrite `docs/*` (ROADMAP, CORRECTNESS→`ORACLE_CORRECTNESS.md`, COMPARISON vs Oracle MCP options, INTEGRATION 3-tier for Oracle, RUNBOOK for a Fusion dev pod) | CI green; docs Oracle-faithful |

### Live-backend integration tiers (mirrors source `docs/INTEGRATION.md`)

1. **CI mocks** — `MockErpClient` Fusion fixtures (offline, deterministic).
2. **Oracle sandbox** — a Fusion Cloud test pod / Oracle APEX + ORDS demo, or the
   public Oracle Fusion REST API reference for shapes.
3. **Live dev pod** — real Fusion REST (OAuth2 / Basic), SOAP ERP Integration
   Service, BI Publisher — gated writes behind `--enable-writes`.

---

## 4. Risks & decisions

- **No universal journal in Oracle.** ACDOCA has no 1:1. We model the
  GL + Subledger Accounting backbone (`GL_JE_LINES`/`XLA_AE_LINES`/`GL_BALANCES`)
  and *document* the architectural difference rather than fake an equivalence.
- **"Transport" → "Sandbox/Config Package".** Oracle's change-promotion model is
  Sandboxes (for customization) + FSM Configuration Packages (for setup). The
  re-typed-confirmation guardrail is preserved for **sandbox publish to PROD**.
- **Client/MANDT first-key invariant doesn't port.** Oracle is not client-first;
  we replace it with a scoping-column invariant (`LEDGER_ID`/`BU_ID`/`ORG_ID`).
- **Commit semantics differ.** Fusion REST auto-commits per request; the explicit
  two-phase commit maps to EBS `p_commit` or to the FBDI interface→import
  two-step. The fail-closed write gate is preserved either way.
- **Faithfulness over literalism.** Where SAP and Oracle genuinely differ, we
  keep the *safety property* (read-only default, cite-every-claim, gated writes,
  bounded reads) and re-express the *mechanism* in Oracle terms.

---

## 5. Progress log

- **P0 — done.** Strategy authored; source surveyed (16 crates, 7 bins, web UI,
  ~21k Rust LOC); domain mapping locked (this document).
- **P1 — done.** Tree lifted; product-token rename applied; crate/app
  directories renamed; manifests fixed; README + AGENTS rebranded; workspace
  builds and all tests pass.
- **P2 — done (core ERP domain).** `oracle-automate-rfc` → `oracle-automate-erp`.
  `SapClient` → `ErpClient` / `MockErpClient`; `S_RfcAuth` → `RequiredPrivilege`
  (Oracle RBAC); `SystemInfo` → Fusion pod identity. The operation catalogue is
  now **Oracle Fusion Cloud ERP**: `fusion.scm.itemsV2.get`,
  `fusion.gl.journalEntries.post`, `fusion.erpintegrations.importBulkData.journalImport`
  (FBDI), `fusion.po.purchaseOrders.post`, `fusion.doo.salesOrdersForOrderHub.post`,
  `fusion.inv.receivingReceiptRequests.post`, `fusion.poz.suppliers.patch`,
  `fusion.fnd.sandbox.publish`, `fusion.bip.runReport`, `fusion.rest.describe`,
  plus EBS transaction-control ops. The object fixtures are Oracle:
  `EGP_SYSTEM_ITEMS_B`, `GL_LEDGERS`, `GL_PERIOD_STATUSES`, `GL_JE_LINES`,
  `XLA_AE_LINES`, `DOO_HEADERS_ALL`, `FND_SANDBOXES` (Kalbe/IDR data).
  Seven **Oracle-correctness invariants** replace the SAP precision gates
  (FND return contract, FBDI interface→import, RBAC privilege, `*_ID` scoping
  key, `VARCHAR2(300)` item number, GL/SLA backbone with "no universal journal"
  note, XLA→GL transfer). `transaction.rs` re-framed for Fusion auto-commit /
  EBS `p_commit` / FBDI two-step. **Whole workspace builds; 208 tests pass.**
  Deferred (documented): logical type renames still pending are the `Rfc*`
  request/meta structs and the `BapiRet2*` return-parser (functionally generic,
  re-framed in comments); live SOAP/OData transports await the Fusion REST
  rewrite; the server tool namespace (`sap.*`→`oracle.*`) lands in P5.
- _Subsequent phases (P3+) update this section as they land._
