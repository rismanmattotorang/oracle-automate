---
name: oracle.skill.sandbox_impact_analysis
description: Cross-domain impact analysis for an Oracle configuration sandbox before publishing it to the mainline (or promoting via an FSM configuration package).
tags: [config, sandbox, impact-analysis]
requires_tools: [oracle.oic.where_used, oracle.oic.get_project_contents, oracle.object.read, oracle.docs.search]
arguments:
  - name: sandbox
    description: Sandbox name, e.g. "KLB_AR_AUTOINVOICE_FIX"
    required: true
  - name: target
    description: Publish target (MAINLINE / QA_POD / PRODUCTION)
    required: false
---

Analyse the impact of publishing sandbox **{{sandbox}}** to **{{target}}** before it goes out.

1. **Enumerate sandbox contents** — call `oracle.object.read` on `FND_SANDBOXES` filtered by `SANDBOX_NAME = '{{sandbox}}'` to confirm its status and what it touches (App Composer objects, page customizations, flexfields, value sets).
2. **Direct impact** — for each changed artifact, call `oracle.oic.where_used` to enumerate every integration, report, and REST consumer that references it.
3. **Project context** — for each affected project/offering, call `oracle.oic.get_project_contents` to identify sibling artifacts that may share state.
4. **Business-process impact** — call `oracle.docs.search` and `bpmn.find_process` with the offering and module names to find which business processes the change touches.
5. **Pre-publish checks** — call `oracle.docs.search` with `"Oracle Fusion sandbox publish validation configuration package"` to retrieve the standard pre-publish procedure (validation run, dependent sandboxes, FSM configuration-package coordination for cross-pod promotion).

Produce a 3-section report: *Direct impact*, *Indirect dependents*, *Recommended pre-publish checks*. Cite every claim. If the change crosses three or more offerings, recommend splitting the sandbox.
