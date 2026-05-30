---
name: oracle.skill.period_close_investigation
description: Investigate root causes of an FI period-close delay or block.
tags: [fi, period-close, investigation]
requires_tools: [oracle.docs.search, oracle.object.read, oracle.rest.metadata]
arguments:
  - name: company_code
    description: Company code (BUKRS), e.g. "1000"
    required: true
  - name: fiscal_period
    description: Fiscal period being closed, e.g. "2026-M03"
    required: false
---

Investigate why the FI period close for **{{company_code}}** ({{fiscal_period}}) is delayed.

Work through the following steps and cite every claim with a `oracle-help://`, `oracle-rest://`, or `oracle-object://` URI:

1. **Procedure baseline** — call `oracle.docs.search` with `"period close foreign currency revaluation"` to retrieve the canonical procedure. Confirm the agent understands the standard order: T001B → FAGL_FC_VAL → FAGLF03.
2. **Posting period state** — call `oracle.object.read` on `T001B` filtered by `BUKRS = '{{company_code}}'` to confirm the periods are open / closed as expected.
3. **Reconciliation status** — call `oracle.docs.search` with `"FAGLF03 BSEG FAGLFLEXA reconciliation"` to retrieve the FAGLF03 sub-procedure; flag any discrepancies the user reported against this baseline.
4. **Inter-company blockers** — if inter-company postings are in scope, call `oracle.rest.metadata` for `BAPI_ACC_GL_POSTING_CHECK` and report the parameter shape expected by validation routines.
5. **Summary** — produce a 3-section report: *What's blocking*, *Recommended remediation*, *Pre-close checklist for next month*.

Do NOT call any state-modifying RFC. If the user authorises a remediation, propose it but require explicit confirmation before invoking `oracle.rest.call`.
