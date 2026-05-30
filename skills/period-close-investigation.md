---
name: oracle.skill.period_close_investigation
description: Investigate root causes of an Oracle General Ledger period-close delay or block.
tags: [gl, period-close, investigation]
requires_tools: [oracle.docs.search, oracle.object.read, oracle.rest.metadata]
arguments:
  - name: ledger
    description: Ledger name or id, e.g. "Gaussian Technologies Primary Ledger"
    required: true
  - name: accounting_period
    description: Accounting period being closed, e.g. "MAR-26"
    required: false
---

Investigate why the GL period close for **{{ledger}}** ({{accounting_period}}) is delayed.

Work through the following steps and cite every claim with an `oracle-help://`, `oracle-rest://`, or `oracle-object://` URI:

1. **Procedure baseline** — call `oracle.docs.search` with `"general ledger period close revaluation transfer to GL"` to retrieve the canonical procedure. Confirm the standard order: close subledgers → Create Accounting → Transfer to GL → revalue balances → close GL period.
2. **Period state** — call `oracle.object.read` on `GL_PERIOD_STATUSES` filtered by the ledger and `PERIOD_NAME = '{{accounting_period}}'` to confirm the period is Open/Closed as expected (`CLOSING_STATUS` O/C/P/F).
3. **Subledger transfer status** — call `oracle.object.read` on `XLA_AE_LINES` (filtered by ledger/period) and check `GL_TRANSFER_STATUS_CODE` for entries not yet transferred to `GL_JE_LINES` — un-transferred subledger accounting is the most common close blocker.
4. **Validation blockers** — if intercompany or revaluation is in scope, call `oracle.rest.metadata` for `fusion.gl.journalEntries.post` and report the parameter/validation shape; check for unposted journals in the period.
5. **Summary** — produce a 3-section report: *What's blocking*, *Recommended remediation*, *Pre-close checklist for next period*.

Do NOT call any state-modifying operation. If the user authorises a remediation, propose it but require explicit confirmation before invoking `oracle.rest.call`.
