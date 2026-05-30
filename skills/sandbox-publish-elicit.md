---
name: oracle.skill.sandbox_publish_elicit
description: Sandbox publish with re-typed confirmation phrase and explicit opt-in for dangerous flags.
tags: [config, sandbox, elicitation, workflow]
requires_tools: [oracle.workflow.publish_sandbox, oracle.oic.where_used, oracle.docs.search]
arguments:
  - name: sandbox
    description: Sandbox name, e.g. "KLB_AR_AUTOINVOICE_FIX"
    required: true
  - name: target
    description: Publish target (MAINLINE | QA_POD | PRODUCTION)
    required: false
---

Publish sandbox **{{sandbox}}** to **{{target}}**.

The `oracle.workflow.publish_sandbox` tool elicits:

- **Sandbox name** (pre-filled from the argument hint)
- **Target** (enum: MAINLINE / QA_POD / PRODUCTION)
- **Publish dependent sandboxes?** (boolean; default false)
- **Skip pre-publish validation?** (boolean; default false — `true` here is dangerous and the agent should warn the user)
- **Confirmation phrase** (the user must re-type the sandbox name to proceed)

The tool refuses to execute if the confirmation phrase doesn't match the sandbox name, and refuses outright on clients that don't advertise the `elicitation` capability — there is no way to silently publish a sandbox.

Pre-flight checklist before invoking the tool:

1. Call `oracle.oic.where_used` on the most critical artifacts in the sandbox to surface unexpected impact.
2. Call `oracle.docs.search` with `"Oracle Fusion publish sandbox to mainline configuration package"` to confirm the canonical procedure (cross-pod promotion uses an FSM Configuration Package).
3. Call `oracle.workflow.publish_sandbox` with the sandbox hint.

Production publishes SHOULD NOT skip validation. If the user requests `skip_validation=true`, push back and ask the user to confirm in plain text before submitting the elicitation form.
