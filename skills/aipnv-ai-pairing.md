---
name: oracle.skill.aipnv_ai_pairing
description: AI-Pairing-Not-Vibing (AIPNV) pre-flight checklist — anti-autopilot guardrails for Oracle write operations. Forces an explicit human-in-the-loop confirmation before sandbox publishes, REST writes, or integration activations.
tags: [behaviour, guardrails, aipnv, safety]
requires_tools: [oracle.system.info, oracle.oic.where_used, oracle.review-rest-call]
arguments:
  - name: intended_action
    description: One-line description of the write you are about to perform (e.g. "activate GT_GL_JOURNAL_IMPORT", "publish sandbox GT_AR_AUTOINVOICE_FIX", "post a GL journal")
    required: true
---

# AI-Pairing-Not-Vibing (AIPNV)

Convergent pattern from [`fr0ster/mcp-abap-adt`](https://github.com/fr0ster/mcp-abap-adt) — *"built for AI-assisted pair programming, not autopilot vibe coding"*.

Oracle-Automate enforces AIPNV at three runtime layers: exposure policy (write tools hidden in read-only mode), per-call `read_only=false` flag, and AGENTS.md guardrails surfaced in `initialize.instructions`. This skill is the **fourth** layer — the agent's own pre-flight checklist, run before the write.

**Intended action:** {{intended_action}}

## The five-question checklist

Answer every question explicitly. If you cannot answer one, **stop** and ask the user before invoking the write tool.

### Q1. What environment am I targeting?

Call `oracle.system.info`. State the pod / ledger scope / environment role (`DEV` / `TEST` / `PROD`) verbatim.

**Stop conditions:**
- environment role is `PROD` and the user has not explicitly authorised a production write in *this* session.
- the pod does not match the environment the user named.

### Q2. What is the blast radius?

For integration/Groovy activations, call `oracle.oic.where_used` on the target and quote the impacted-consumer count.

For REST writes, name every downstream effect (e.g. posting a journal ⇒ writes `GL_JE_LINES`; if sourced from a subledger, `XLA_AE_LINES` transfers via Create Accounting).

For sandbox publishes, call `oracle.object.read` on `FND_SANDBOXES` to enumerate the touched artifacts.

**Stop conditions:** more than 50 consumers without user acknowledgement; the change touches a shared connection/lookup used across offerings.

### Q3. What is the rollback path?

Name it explicitly:

- Integration activation: previous activated version is retained (revert in OIC).
- REST posting: the reversal/cancel operation and the record key needed.
- Sandbox publish: publishing to the mainline is irreversible — only a *new* compensating sandbox can undo it (note this loudly).

**Stop conditions:** no rollback path. Do not proceed.

### Q4. Have I cited the Oracle canon for this operation?

Call `oracle.docs.search` for the resource / process name. Cite the returned `oracle-help://` URI. If the docs contradict your intended call signature, fix the call — don't override the canon.

### Q5. Has the user explicitly authorised this write in this session?

The authorisation must be **explicit**, **scoped** (matches the action), and **current** (this session). If any are false, **invoke the elicitation flow** via the matching workflow tool (`oracle.workflow.create_purchase_order`, `oracle.workflow.maintain_customer_master`, `oracle.workflow.publish_sandbox`) which renders a structured confirmation form on the client. Never fabricate the confirmation.

## Final gate

Only after Q1–Q5 are answered may you invoke the write tool. Include the answers in your final report so the audit log captures them alongside the call.

---

*Reference: `fr0ster/mcp-abap-adt` README — "AI Pairing, Not Vibing". Oracle-Automate's runtime layers are documented in `AGENTS.md` and `crates/mcp-server/src/lib.rs` (`ExposurePolicy`).*
