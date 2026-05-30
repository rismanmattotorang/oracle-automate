---
name: oracle.skill.custom_code_review
description: Structured review of Oracle custom code (OIC integrations, Application Composer Groovy, BI Publisher) with explicit checks for Oracle Fusion anti-patterns.
tags: [oic, groovy, review, quality]
requires_tools: [oracle.oic.get_groovy_script, oracle.oic.get_integration, oracle.oic.where_used, oracle.docs.search]
arguments:
  - name: object_name
    description: Artifact name to review, e.g. "GT_INVOICE_HOLD_RULE"
    required: true
  - name: kind
    description: Artifact kind (integration | groovy_script | connection | bip_report)
    required: true
---

Review the **{{kind}}** **{{object_name}}** for Oracle Fusion custom-code quality issues.

1. **Fetch source** — `oracle.oic.get_{{kind}}` with name={{object_name}}.
2. **Static checks** (silent unless violations found):
   - **Unbounded REST query** — a `GET` on a Fusion resource with no `q=` filter or `limit`, risking full-collection pulls.
   - **Direct SQL against unsupported objects** — BI Publisher data models selecting from internal `_ALL`/`_B` tables that have a supported REST/BICC source.
   - **Hard-coded ledger / business-unit / id values** — should be DVM lookups or integration properties, not literals.
   - **Currency/amount as float** — Fusion amounts must be carried as strings/decimals, never binary floats.
   - **Credentials in the flow** — connection secrets or basic-auth headers inline instead of in the OIC connection.
   - **Groovy that mutates standard attributes without a guard** — Application Composer triggers that write protected fields unconditionally.
3. **Architectural checks**:
   - Where-used (`oracle.oic.where_used`) — is this artifact referenced only inside its own project? If so it need not be a shared connection/lookup.
   - Synchronous invoke of a long-running ESS job without polling — should use the async callback pattern.
4. **Oracle canon** — for any non-trivial pattern, call `oracle.docs.search` for the relevant Oracle Help topic to confirm the supported approach.

Produce a markdown review with severity tags (`error`, `warning`, `info`), each citing the artifact location and the Oracle Help URI. Do NOT modify the artifact.
