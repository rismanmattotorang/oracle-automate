# Agent Guardrails — Oracle-Automate

These rules apply to any AI agent driving this MCP server.

## Behavioural guidelines (apply before any tool call)

Oracle-Automate adopts the four Karpathy guidelines, ported with attribution
from [`multica-ai/andrej-karpathy-skills`](https://github.com/multica-ai/andrej-karpathy-skills)
(MIT). Run them as a mental pre-flight:

1. **Think before coding** — state your Oracle assumptions explicitly; if a
   simpler approach exists, say so; if a precondition is unclear, stop.
2. **Simplicity first** — minimum tool calls that solve the problem; no
   retrieval-layer escalation beyond what's needed; no unbounded table
   reads; no fabricated parameter defaults.
3. **Surgical changes** — touch only what the user asked you to touch;
   clean up only your own mess; match existing style; mention unrelated
   dead code, never delete it.
4. **Goal-driven execution** — define success criteria up front; loop
   until verified; one bullet per step with an explicit `verify:` check.

The full text — adapted with Oracle-specific examples — lives in
`skills/karpathy-guidelines.md` and is auto-loaded as the
`oracle.skill.karpathy_guidelines` MCP prompt.

The anti-autopilot stance from [`fr0ster/mcp-abap-adt`](https://github.com/fr0ster/mcp-abap-adt)
("AI Pairing, Not Vibing") is captured as `oracle.skill.aipnv_ai_pairing` —
a five-question pre-flight checklist that every write-side call must
pass.

## Read-only by default

- Production / QA systems: use `oracle.docs.search`, `oracle.system.info`,
  `oracle.system.health`, `oracle.system.cache_stats`, `oracle.system.cache_invalidate`,
  `oracle.rest.search`, `oracle.rest.metadata`, `oracle.rest.bulk_metadata`,
  `oracle.object.read`, `oracle.object.structure`, `oracle.rest.parse_result`,
  `oracle.party.search`, `oracle.party.get`,
  `oracle.kb.navigate`,
  `abap.adt.get_program`, `abap.adt.get_class`, `abap.adt.get_interface`,
  `abap.adt.get_include`, `abap.adt.get_function_module`, `abap.adt.get_cds_view`,
  `abap.adt.get_package_contents`, `oracle.oic.where_used`, `abap.adt.search`,
  `oracle.oic.preview_data`.
- Do NOT call write-side RFCs (anything where `read_only=false` in its metadata)
  or `oracle.oic.activate` unless the server was started with `--enable-writes` AND
  the user has explicitly authorised the change in the current session.
- The server hides write tools from `tools/list` entirely when in read-only mode
  (fr0ster exposure-policy pattern). If you can see a write tool, the operator
  has opted in.

## Cite every claim

Every answer that references Oracle behaviour must cite either:
- a `oracle-help://` URI from `oracle.docs.search`, OR
- a `oracle-rest://` URI from `oracle.rest.metadata`, OR
- a `oracle-object://` URI from `oracle.object.structure`.

## Before any `oracle.rest.call`

1. Invoke `oracle.rest.metadata` first to confirm the parameter signature.
2. Use the `oracle.review-rest-call` prompt to summarise the intended call.
3. Only then call `oracle.rest.call`.

## Before any `oracle.oic.activate` (publish/activate a custom-code artifact)

1. Always call `oracle.oic.where_used` first to enumerate impacted dependents.
2. Use the `oracle.review-where-used` prompt to structure the impact summary.
3. Only then activate.

## When `oracle.oic.preview_data` returns DataPreviewBlocked

Some Fusion objects (subledger detail, large fact tables) are not exposed for
direct REST/describe preview. The server surfaces this as a structured
`[DataPreviewBlocked]` error. Fall back to `oracle.object.read` (BI Publisher
path) — it has its own buffer-overflow safety (max 1000 rows).

## Workflow tools use elicitation — never fabricate confirmations

Three high-stakes workflows pause mid-execution and ask the user to
confirm cost centres, party numbers, or sandbox names via a
structured form rendered by the client:

- `oracle.workflow.create_purchase_order`
- `oracle.workflow.maintain_customer_master` (chained two-step elicitation)
- `oracle.workflow.publish_sandbox` (re-typed confirmation phrase)

The agent's role is to *kick off the workflow* with the best hints it
has — never to hard-code cost centres, party keys, or sandbox names.
If the user declines or the client lacks the elicitation capability,
the tool aborts safely with no write side-effect.

## Choose the right retrieval layer

The server exposes four retrieval surfaces; pick deliberately:

| Layer | Tool | When |
|---|---|---|
| **L2 Hybrid** | `oracle.docs.search` | Default. Lexical + semantic + RRF + rerank over the document corpus. |
| **L3 GraphRAG** | `kb.global_query` | Global / analytical questions ("which apps touch period close?"). Returns community summaries spanning multiple domains. |
| **L4 HippoRAG** | `kb.multi_hop` | Multi-hop / impact / where-used queries ("what depends on GT_FUSION_ERP_REST?"). PPR-ranked, hop-distance-bounded. |
| **L5 RAPTOR** | `kb.summarise` | Granularity-aware orientation. Level 0 = leaves, 1 = communities, 2 = Oracle product roll-ups. |

When in doubt, start with `oracle.docs.search`. Promote to `kb.multi_hop` only
when the user explicitly asks about dependencies, impact, or callers.

## Table reads

- Always set `fields` (column projection) — do not fetch all columns by default.
- Always set a `where_conditions` clause for tables larger than ~1k rows.
- Never raise `max_rows` above the default 100 unless the user requests it.
