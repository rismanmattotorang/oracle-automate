---
name: oracle.skill.rap_service_scaffolding
description: Generate the canonical RAP (RESTful ABAP Programming) service scaffolding for a CDS entity.
tags: [rap, abap, cds, scaffolding]
requires_tools: [oracle.oic.get_bip_report, oracle.oic.get_project_contents, oracle.docs.search]
arguments:
  - name: cds_view
    description: Root CDS view name, e.g. "Z_C_SALES_ORDER_KPI"
    required: true
  - name: behavior_kind
    description: BDEF behavior kind (managed | unmanaged | abstract | projection)
    required: false
---

Scaffold a RAP service over **{{cds_view}}** (behavior kind: {{behavior_kind}}).

Read-only investigation phase (always run, even if writes are enabled):

1. **Inspect the CDS view** — `oracle.oic.get_bip_report` with name={{cds_view}}. Extract annotations, key fields, associations, and aggregation columns.
2. **Locate the parent package** — derive package from `oracle.oic.get_bip_report` response or, failing that, call `oracle.oic.search` filtered to `kind=cds_view`.
3. **Sibling RAP artefacts** — call `oracle.oic.get_project_contents` on the parent package; identify any existing Behavior Definition, Service Definition, or Metadata Extension for this view to avoid duplicates.
4. **RAP procedure reference** — call `oracle.docs.search` with `"RAP behavior definition managed projection draft"` to retrieve the canonical procedure.

Production phase (only when `--enable-writes` is active and user confirms):

5. Produce a **plan** with:
   - Target Behavior Definition name (`ZBP_{{cds_view}}` convention)
   - Target Service Definition name (`ZSD_{{cds_view}}` convention)
   - Whether to use draft (`with draft`) — default no for read-mostly KPIs
   - Authorization stub (`#NOT_REQUIRED` is acceptable for analytical views)
6. Ask the user to confirm before invoking any write tool.

Do NOT scaffold for views with `@AccessControl.authorizationCheck: #CHECK` until you've called `oracle.docs.search` with `"DCL access control RAP"` and surfaced the relevant procedure.
