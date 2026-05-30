# Oracle Fusion Cloud ERP Correctness Audit

> The Oracle analog of the original SAP correctness audit. Every fixture in
> `oracle-automate-erp` is mapped to its Oracle source-of-truth, and seven
> precision tests fail CI the moment a fixture drifts. See
> [`PORTING_STRATEGY.md`](PORTING_STRATEGY.md) §2.5 for how these replace the
> SAP precision gates.

## Why correctness-as-tests

An agent that drives a production ERP must not hallucinate object shapes,
operation contracts, or security requirements. Oracle-Automate encodes the
non-negotiable invariants of Oracle Fusion Cloud ERP as unit tests over the
mock catalogue (`crates/oracle-automate-erp/src/client.rs`). They run on every
commit (the `oracle-correctness` CI job) so any fixture that drifts from the
Oracle canon fails loudly.

## The seven invariants

| # | Test | What it guarantees | Oracle source-of-truth |
|---|---|---|---|
| 1 | `every_write_op_returns_standard_result` | Every write operation surfaces the FND standard return contract — an `X_RETURN_STATUS` export (`S`/`E`/`U`) **and** an `X_MSG_DATA` (FND_MSG_PUB) stack — so agents can read business-side messages instead of guessing from HTTP alone. | EBS PL/SQL public-API standard (`FND_API` / `FND_MSG_PUB`); Fusion REST mirrors the severity via HTTP status + error payload. |
| 2 | `every_bulk_write_uses_interface_then_import` | `commit_required` is `true` **iff** the operation belongs to the ERP Integration Service (FBDI) family — the only writes that defer persistence to a follow-up import job. Synchronous Fusion REST writes auto-commit. Bulk ops must document the interface→import two-step. | FBDI `importBulkData` → interface table → import ESS job (e.g. Journal Import). |
| 3 | `every_op_declares_required_privilege` | Every operation declares at least one Oracle RBAC privilege (code + duty role + action) — the analog of an SAP `S_RFC` entry — so the agent (and the SoD audit) can reason about access before a call goes out. | Fusion function-security privileges aggregated into duty/job/data roles. |
| 4 | `every_business_object_declares_scoping_id_key` | Oracle is **not** client-first (no `MANDT`/`RCLNT`). Instead every business object is keyed by an Oracle surrogate/scoping id ending in `_ID`, and that key is a declared field. | Oracle surrogate-key convention (`*_ID`); data scope via Data Access Set / Business Unit / Ledger. |
| 5 | `item_number_is_varchar2_300_per_fusion` | The single most-cited type difference: the item number is `VARCHAR2(300)` in Fusion Product Hub, not the SAP `MATNR CHAR(40)`. | `EGP_SYSTEM_ITEMS_B.ITEM_NUMBER`. |
| 6 | `gl_je_lines_is_present_as_accounting_backbone` | `GL_JE_LINES` is present as the GL accounting backbone, and its note records that **Oracle has no single universal journal** (unlike SAP ACDOCA); nothing may claim to *be* a universal journal. | `GL_JE_LINES` (GL leg) + `XLA_AE_LINES` (subledger) + `GL_BALANCES`. |
| 7 | `subledger_objects_note_xla_to_gl_transfer` | Subledger objects (`XLA_AE_LINES`) document the Create-Accounting / Transfer-to-GL flow into `GL_JE_LINES` — the Oracle analog of the SAP compatibility-view storage note. | Subledger Accounting (XLA) → GL transfer programs. |

## Object fixtures → Oracle source

| Fixture | Oracle object | Note |
|---|---|---|
| `EGP_SYSTEM_ITEMS_B` | Product Hub item master | item number `VARCHAR2(300)` |
| `GL_LEDGERS` | GL ledgers | company-code analog → Ledger + Legal Entity |
| `GL_PERIOD_STATUSES` | accounting periods | `CLOSING_STATUS` gates posting (T001B analog) |
| `GL_JE_LINES` | GL journal lines | accounting backbone (GL leg) |
| `XLA_AE_LINES` | Subledger Accounting lines | transfers to GL via Create Accounting |
| `DOO_HEADERS_ALL` | Order Management header | sold-to is a TCA party; scoped by Business Unit |
| `FND_SANDBOXES` | configuration sandboxes | change-promotion unit (transport analog) |

## Operation catalogue → Oracle source

| Operation | Oracle endpoint / mechanism |
|---|---|
| `fusion.system.serverInformation` | Fusion REST framework `serverInformation` |
| `fusion.scm.itemsV2.get` | `GET /fscmRestApi/.../itemsV2` |
| `fusion.gl.journalEntries.post` | `POST /fscmRestApi/.../journalEntries` (sync, auto-commit) |
| `fusion.erpintegrations.importBulkData.journalImport` | FBDI `importBulkData` → `GL_INTERFACE` → Journal Import ESS job |
| `fusion.po.purchaseOrders.post` | `POST /fscmRestApi/.../purchaseOrders` |
| `fusion.doo.salesOrdersForOrderHub.post` | `POST /fscmRestApi/.../salesOrdersForOrderHub` |
| `fusion.inv.receivingReceiptRequests.post` | `POST /fscmRestApi/.../receivingReceiptRequests` |
| `fusion.poz.suppliers.patch` | `PATCH /fscmRestApi/.../suppliers` |
| `fusion.fnd.sandbox.publish` | publish sandbox to mainline (FSM config package for cross-pod) |
| `fusion.bip.runReport` | BI Publisher report run (`RFC_READ_TABLE` analog) |
| `fusion.rest.describe` | REST resource `describe` (`DDIF_FIELDINFO_GET` analog) |
| `ebs.fnd.transaction.commit` / `…rollback` | EBS `FND_API` two-phase commit (on-prem backend) |

## DDIC → Oracle type mapping

`CHAR(n)`→`VARCHAR2(n)`, `NUMC`→`VARCHAR2`/`NUMBER`, `DATS`→`DATE`, `TIMS`→`DATE`/`TIMESTAMP`,
`CURR`/`DEC`→`NUMBER`, `CLNT(3)`→ *(no analog)* / `ORG_ID NUMBER` (EBS), `UNIT`/`CUKY`→`VARCHAR2`.

## Running the gate

```bash
# the dedicated CI job:
cargo test -p oracle-automate-erp --lib -- \
  tests::every_write_op_returns_standard_result \
  tests::every_bulk_write_uses_interface_then_import \
  tests::every_op_declares_required_privilege \
  tests::every_business_object_declares_scoping_id_key \
  tests::item_number_is_varchar2_300_per_fusion \
  tests::gl_je_lines_is_present_as_accounting_backbone \
  tests::subledger_objects_note_xla_to_gl_transfer
```

All seven pass on `main`. They are intentionally cheap (pure functions over the
fixtures) so they can gate every commit without slowing CI.
