---
name: oracle.skill.extensibility_audit
description: Audit an Oracle Fusion project against supported-extensibility principles (personalization-not-customization — App Composer / sandboxes / released REST APIs, no unsupported DB changes).
tags: [extensibility, fusion, audit, compliance]
requires_tools: [oracle.oic.get_project_contents, oracle.oic.get_groovy_script, oracle.oic.where_used, oracle.docs.search]
arguments:
  - name: project
    description: OIC project / Application Composer offering to audit, e.g. "KLB_FINANCE_INTEGRATIONS"
    required: true
---

Audit the **{{project}}** project against Oracle Fusion supported-extensibility principles.

1. **Inventory** — call `oracle.oic.get_project_contents` on `{{project}}`. For each member note its kind (integration, groovy_script, connection, lookup, bip_report, rest_resource).
2. **Sample three artifacts** — pick the largest integration, the largest Groovy script, and one BIP report. For each:
   a. Fetch its source via `oracle.oic.get_integration` / `oracle.oic.get_groovy_script` / `oracle.oic.get_bip_report`.
   b. Scan for **unsupported access** — direct SQL/DML against base tables that have a supported REST/BICC source, or use of internal endpoints not in the Fusion REST API catalog.
   c. Scan for changes that belong in a **sandbox** (Application Composer / Page Composer) being done outside one.
3. **Where-used cross-check** — for each unsupported touchpoint, call `oracle.oic.where_used` to see whether the dependency is contained in this project or leaks into others.
4. **Supported-extensibility canon** — call `oracle.docs.search` with `"Oracle Fusion extensibility Application Composer sandbox supported"` for the canonical guidance (personalization → configuration → App Composer → custom REST, in that order of preference).

Produce a 4-section report:
- **Supported-API compliance**: percentage of touchpoints using published REST APIs.
- **Unsupported access**: count + worst offenders (direct DB, internal endpoints).
- **Sandbox discipline**: customizations made outside a sandbox.
- **Recommended remediation**: ranked by effort vs benefit.

Do NOT propose code changes; produce only the audit report.
