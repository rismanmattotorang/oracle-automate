---
name: oracle.skill.karpathy_guidelines
description: Behavioural guidelines that reduce common LLM coding/agent mistakes — surface assumptions, simplicity first, surgical changes, goal-driven execution. Apply to any Oracle-Automate task before you touch code, data, or sandboxes.
tags: [behaviour, guidelines, meta, karpathy]
requires_tools: []
arguments:
  - name: task
    description: One-line description of the task at hand (e.g. "rewrite KLB_GL_JOURNAL_IMPORT to post via REST instead of FBDI")
    required: true
---

# Karpathy guidelines — applied to Oracle-Automate

Ported with attribution from [`multica-ai/andrej-karpathy-skills`](https://github.com/multica-ai/andrej-karpathy-skills)
(MIT). The four principles below are restated in spirit; the Oracle-specific
examples are Oracle-Automate's contribution.

**Tradeoff:** these guidelines bias toward caution over speed. For trivial tasks (one-shot object read, a single `oracle.docs.search`) skip to section 4 only.

The task you are about to perform: **{{task}}**.

## 1. Think before coding

Don't assume. Don't hide confusion. Surface tradeoffs.

Before invoking any write-side tool (`oracle.rest.call` with `read_only=false`, `oracle.oic.activate`, any `oracle.workflow.*`):

- **State your Oracle assumptions explicitly.** Which ledger? Which business unit? Which accounting period? Which pod (DEV/TEST/PROD)? If uncertain, call the read-only tool (`oracle.system.info`, `oracle.object.read` on `GL_LEDGERS` / `GL_PERIOD_STATUSES`) *before* the write.
- **If multiple operations could satisfy the goal, present them.** Synchronous REST `journalEntries.post` vs FBDI `importBulkData` (Journal Import); REST PATCH vs a bulk interface load. Don't pick silently.
- **If a simpler approach exists, say so.** A bounded `oracle.object.read` often beats a custom BI Publisher extract. `oracle.docs.search` often beats a code dive.
- **If a precondition is unclear, stop.** Name what's confusing. Use the `oracle.review-rest-call` prompt to summarise the intended call before invoking it.

## 2. Simplicity first

Minimum tool calls that solve the problem. Nothing speculative.

- **No retrieval-layer escalation beyond what's needed.** Start with `oracle.docs.search` (L2 hybrid). Promote to `kb.multi_hop` (L4) *only* when the user explicitly asks about dependencies / impact / consumers.
- **No unbounded reads.** Always set `fields`. Always set `where_conditions` for objects larger than ~1k rows. Never raise `max_rows` above the default 100 unless asked.
- **No fabricated parameter defaults.** If the user hasn't supplied a charge account / party / sandbox name, use the workflow tool's elicitation — never hard-code.
- **No defensive error handling for impossible scenarios.** The structured error taxonomy already classifies transient vs permanent.

Ask: "Would a senior Oracle Fusion functional consultant say this is overcomplicated?" If yes, simplify.

## 3. Surgical changes

Touch only what the user asked you to touch. Clean up only your own mess.

When editing a custom artifact via `oracle.oic.activate` (or publishing a sandbox):

- **Don't "improve" adjacent integrations, mappings, or formatting.**
- **Don't refactor things that aren't broken.**
- **Match existing conventions** (naming, project structure) even if you'd do it differently.
- **If you notice unrelated dead config, mention it — don't delete it.** Add a `TODO(@owner)` note to the impact report.

Always call `oracle.oic.where_used` first. The test: **every changed line traces directly to the user's request or to an orphan your change created.**

## 4. Goal-driven execution

Define success criteria up front. Loop until verified.

Transform fuzzy tasks into verifiable goals:

- "Investigate period close" → "List subledger lines in `XLA_AE_LINES` not yet transferred to `GL_JE_LINES` for the period, then map each to the canonical Create-Accounting/Transfer-to-GL procedure via `oracle.docs.search`."
- "Fix the failing integration" → "Reproduce the fault, find the failing activity via `oracle.oic.where_used`, then make a minimal fix and re-run."
- "Add validation to KLB_INVOICE_HOLD_RULE" → "Define the invalid cases (negative amount, closed period, blocked supplier), then make the Groovy guard cover them."

For multi-step tasks, state a brief plan before kicking it off:

```
1. <action> → verify: <check>
2. <action> → verify: <check>
```

## Acceptance checklist (paste into your final report)

- [ ] I stated my Oracle assumptions before any write-side call.
- [ ] I used the lowest retrieval layer that worked.
- [ ] I cited every claim with an `oracle-help://` / `oracle-rest://` / `oracle-object://` URI.
- [ ] My change touches only what was asked.
- [ ] My change has explicit, verifiable success criteria.
- [ ] I ran `oracle.oic.where_used` before activating/publishing anything.
- [ ] No write-side tool was called without `--enable-writes` AND explicit user authorisation in the current session.
