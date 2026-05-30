---
name: oracle.skill.customer_master_elicit
description: Two-step elicitation for customer/party master maintenance — pick the view, then fill scoped fields.
tags: [ar, customer, tca, elicitation, workflow]
requires_tools: [oracle.workflow.maintain_customer_master, oracle.docs.search]
arguments:
  - name: customer_hint
    description: Customer / TCA party hint (party number or name)
    required: false
---

Maintain customer (TCA party) master data for **{{customer_hint}}** using the chained elicitation workflow.

The `oracle.workflow.maintain_customer_master` tool issues **two elicitations**:

1. **Scope selection** — which view to maintain (party / account | bill-to & ship-to sites | receivables business-unit profile).
2. **Scoped fields** — the form fields depend on the chosen view:
   - *party*: organization/person name, country, tax identifiers
   - *sites*: address, site purpose (bill-to / ship-to), business unit
   - *bu_profile*: receipt method, payment terms, dunning / statement cycle

Steps:

1. Search Oracle Help first with `oracle.docs.search` and `"customer account REST crmRestApi TCA party"` to confirm the canonical procedure.
2. Call `oracle.workflow.maintain_customer_master`. Walk the user through the two elicitations.
3. Echo the confirmed changes back to the user before the (eventual, write-mode-gated) account/site PATCH.

This skill exists specifically to demonstrate **chained elicitation** — declining the first form aborts cleanly without ever showing the second.
