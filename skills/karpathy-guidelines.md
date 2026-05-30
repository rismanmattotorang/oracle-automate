---
name: oracle.skill.karpathy_guidelines
description: Behavioural guidelines that reduce common LLM coding/agent mistakes — surface assumptions, simplicity first, surgical changes, goal-driven execution. Apply to any Oracle-Automate task before you touch code, tables, or transports.
tags: [behaviour, guidelines, meta, karpathy]
requires_tools: []
arguments:
  - name: task
    description: One-line description of the task at hand (e.g. "rewrite ZCL_FIN_POSTER to use ACDOCA")
    required: true
---

# Karpathy guidelines — applied to Oracle-Automate

Ported with attribution from [`multica-ai/andrej-karpathy-skills`](https://github.com/multica-ai/andrej-karpathy-skills)
(MIT). Original guidelines distilled from
[Andrej Karpathy's observations](https://x.com/karpathy/status/2015883857489522876)
on LLM coding pitfalls. The four principles below are restated verbatim in
spirit; the SAP-specific examples and ABAP/RFC adaptations are
Oracle-Automate's contribution.

**Tradeoff:** these guidelines bias toward caution over speed. For trivial
SAP tasks (one-shot table lookup, a single `oracle.docs.search`) skip down to
section 4 only.

The task you are about to perform: **{{task}}**.

## 1. Think before coding

Don't assume. Don't hide confusion. Surface tradeoffs.

Before invoking any write-side tool (`oracle.rest.call` with `read_only=false`,
`oracle.oic.activate`, any `oracle.workflow.*`):

- **State your SAP assumptions explicitly.** Which company code? Which
  fiscal variant? Which transport layer? If uncertain, call the
  corresponding read-only tool (`oracle.system.info`, `oracle.object.read` on
  `T001` / `T009`) *before* the write.
- **If multiple BAPIs could satisfy the goal, present them.** PO_CREATE1
  vs PO_CREATE_NOITEMVIEW; CUSTOMER_CREATEFROMDATA1 vs `BAPI_CUSTOMER_*`
  modular set. Don't pick silently.
- **If a simpler approach exists, say so.** Direct table read often beats
  a custom RFC. `oracle.docs.search` often beats a code dive.
- **If a precondition is unclear, stop.** Name what's confusing (e.g.
  "I cannot tell from the metadata whether `IMPORTING` is required or
  optional"). Use the `oracle.review-rest-call` prompt to summarise the
  intended call before invoking it.

## 2. Simplicity first

Minimum tool calls that solve the problem. Nothing speculative.

- **No retrieval layer escalation beyond what's needed.** Start with
  `oracle.docs.search` (L2 hybrid). Promote to `kb.multi_hop` (L4 HippoRAG)
  *only* when the user explicitly asks about dependencies / impact / callers.
  Do not pre-emptively fan out across L2/L3/L4/L5.
- **No unbounded table reads.** Always set `fields` (column projection).
  Always set `where_conditions` for tables larger than ~1k rows. Never
  raise `max_rows` above the default 100 unless the user requests it.
- **No fabricated parameter defaults.** If the user hasn't supplied a
  cost centre / customer number / transport ID, use the workflow tool's
  elicitation — never hard-code.
- **No defensive error handling for impossible scenarios.** The
  structured `RfcError` taxonomy already classifies transient vs
  permanent. Don't wrap tool calls in extra `try/except`-style logic at
  the prompt layer.

Ask yourself: "Would a senior SAP basis admin say this is overcomplicated?"
If yes, simplify.

## 3. Surgical changes

Touch only what the user asked you to touch. Clean up only your own mess.

When editing ABAP via `oracle.oic.activate` (or any future write-side ADT
tool):

- **Don't "improve" adjacent code, comments, or formatting.** That's the
  Clean Core team's job, on its own change request.
- **Don't refactor things that aren't broken.** A bug fix doesn't need
  surrounding cleanup.
- **Match existing ABAP style** (Hungarian vs Clean ABAP) even if you'd
  do it differently. Style discipline is a per-system policy.
- **If you notice unrelated dead code, mention it — don't delete it.**
  Add a `# TODO(@<owner>):` comment to the impact analysis report; don't
  silently remove.

When your changes create orphans (deleted callers, dangling form
routines): remove only the orphans *your* change created. Pre-existing
dead code stays. Always call `oracle.oic.where_used` first.

The test: **every changed line should trace directly to the user's
request or to an orphan your change created.**

## 4. Goal-driven execution

Define success criteria up front. Loop until verified.

Transform fuzzy SAP tasks into verifiable goals:

- "Investigate period close" → "List the open postings in `ACDOCA` for
  the affected fiscal period, then map each to the SAP-canonical clearing
  procedure via `oracle.docs.search`."
- "Fix the dump" → "Reproduce the dump in `ST22`, find the failing
  statement via `oracle.oic.where_used`, write a unit test that triggers
  it, then make it pass."
- "Add validation to ZCL_X" → "Write unit tests for invalid inputs
  (negative quantity, future posting date, blocked customer), then make
  them pass."

For multi-step SAP tasks, state a brief plan before kicking it off:

```
1. <action> → verify: <check>
2. <action> → verify: <check>
3. <action> → verify: <check>
```

Strong success criteria let you (the agent) loop independently. Weak
criteria ("make it work") force constant clarification round-trips with
the user.

## Acceptance checklist (paste into your final report)

- [ ] I stated my SAP assumptions before any write-side call.
- [ ] I used the lowest retrieval layer that worked.
- [ ] I cited every claim with a `oracle-help://` / `oracle-rest://` /
      `oracle-object://` URI.
- [ ] My change touches only what was asked.
- [ ] My change has explicit, verifiable success criteria.
- [ ] I ran `oracle.oic.where_used` before activating any ABAP object.
- [ ] No write-side tool was called without `--enable-writes` AND
      explicit user authorisation in the current session.
