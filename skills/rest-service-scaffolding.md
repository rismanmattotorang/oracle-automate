---
name: oracle.skill.rest_service_scaffolding
description: Generate canonical scaffolding for a custom Oracle service over a data source — an OIC integration façade or an Application Composer custom REST resource/object — with read-only investigation first.
tags: [rest, oic, app-composer, scaffolding]
requires_tools: [oracle.oic.get_bip_report, oracle.oic.get_project_contents, oracle.docs.search]
arguments:
  - name: data_source
    description: Backing data source — a BI Publisher data model / view, or a Fusion REST resource, e.g. "KLB_GL_JOURNAL_EXTRACT"
    required: true
  - name: service_kind
    description: Scaffolding target (oic_integration | app_composer_object | custom_rest)
    required: false
---

Scaffold a custom service over **{{data_source}}** (kind: {{service_kind}}).

Read-only investigation phase (always run, even when writes are enabled):

1. **Inspect the source** — `oracle.oic.get_bip_report` with name={{data_source}}. Extract the query, parameters, key columns, and joins.
2. **Locate the parent project** — derive the project from the response or call `oracle.oic.search` filtered to `kind=bip_report`.
3. **Sibling artefacts** — call `oracle.oic.get_project_contents` on the parent project; identify any existing integration, connection, or custom REST resource for this source to avoid duplicates.
4. **Procedure reference** — call `oracle.docs.search` for `"Oracle Integration REST adapter trigger"` (for an OIC façade) or `"Application Composer custom object REST"` (for App Composer) to retrieve the canonical procedure.

Production phase (only when `--enable-writes` is active and the user confirms):

5. Produce a **plan** with:
   - Target integration / object name (`KLB_<source>_SVC` convention)
   - Trigger contract (REST adapter request/response, or object fields)
   - Pagination + `limit`/`offset` handling for collection reads
   - Security: connection (OAuth2/Basic) + role/privilege required to invoke
6. Ask the user to confirm before invoking any write/publish tool.

Do NOT scaffold a write-capable service over a sensitive resource (journals, suppliers, payments) until you've called `oracle.docs.search` for the resource's security model and surfaced the required privilege.
