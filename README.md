<div align="center">

# Oracle-Automate

### The agentic operating system for Oracle Fusion Cloud ERP.

**Rust core · sub-millisecond retrieval · correctness-as-tests · read-only by default · on-premise · Apache-2.0**

Built by **[Gaussian Technologies](#about-gaussian-technologies)** — a deep-tech startup from Indonesia.

[![Rust](https://img.shields.io/badge/Rust-1.80%2B-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)
[![MCP](https://img.shields.io/badge/MCP-2025--06--18-8b5cf6?style=flat-square)](https://modelcontextprotocol.io)
[![Tests](https://img.shields.io/badge/tests-216%20passing-brightgreen?style=flat-square)](#by-the-numbers)
[![P95](https://img.shields.io/badge/retrieval%20P95-%3C80ms-brightgreen?style=flat-square)](#by-the-numbers)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue?style=flat-square)](LICENSE)

[Quick start](#quick-start) · [Architecture](#architecture) · [What's inside](#whats-inside) · [Production readiness](docs/PRODUCTION_READINESS.md)

</div>

---

## The problem

Every large enterprise runs its money and its supply chain on an ERP like
**Oracle Fusion Cloud**. LLM agents are already good at *reasoning* over that
data — and genuinely dangerous at *acting* on it. One hallucinated journal post
or purchase order is not a bad demo; it's a financial incident.

The reflex answer — pipe your ERP into yet another agent SaaS — trades the
problem for two more: your most sensitive financial data leaves the building,
and a vendor sits in the critical path of your close.

## Our approach

**Oracle-Automate is an MCP-native agent runtime that runs *next to* your ERP,
not in someone else's cloud.** Any MCP client — Claude, Cursor, or your own
gateway — gets safe, cited, sub-millisecond access to Oracle Fusion:

- **Read-only by default.** Write tools are *hidden* from `tools/list` until an
  operator explicitly opts in; high-stakes workflows (PO create, master-data,
  sandbox publish) pause for elicitation and a re-typed confirmation.
- **Correctness is code, not prose.** Oracle-Fusion invariants — item-number
  length, the GL + Subledger accounting backbone, ledger/BU scoping, REST/FND
  return contracts — fail CI the moment a fixture drifts.
- **Every answer is cited.** Responses carry `oracle-help://` / `oracle-rest://`
  / `oracle-object://` provenance URIs — no ungrounded claims.
- **Rust, on-prem, vendor-neutral.** A single static binary or a distroless
  container in your own cluster. No Python/Node latency tails; no SaaS lock-in.

```bash
# One command: the full agent runtime wired to mock Fusion + OIC pods.
docker compose up --build        # then point an MCP client at http://localhost:3030/mcp
```

---

## By the numbers

| | |
|---|---|
| **37** production MCP tools | RAG search · REST ops · master data · custom-code · graph · gated workflows |
| **4** retrieval layers | hybrid (dense+BM25+RRF+rerank) · GraphRAG · HippoRAG · RAPTOR |
| **&lt; 80 ms** P95 retrieval | enforced as a CI acceptance gate |
| **216** tests, **0** flaky | deterministic, offline, run on every push |
| **18** Rust crates · **9** binaries | + a Next.js 14 web console |
| **7** correctness invariants | Oracle-Fusion semantics, enforced in CI |
| **0** secrets in logs | hand-written redaction; constant-time auth; signed release images |

---

## Quick start

```bash
# Build everything (Rust 1.80+).
cargo build --release --bins

# Single binary, stdio MCP server — drop into Claude Code, Cursor, or any MCP client.
./target/release/oracle-automate-server

# Or HTTP for browser / remote agents.
./target/release/oracle-automate-server --transport http --bind 127.0.0.1:3030
curl http://127.0.0.1:3030/health      # → "ok"
curl http://127.0.0.1:3030/metrics     # → Prometheus exposition

# Ratatui operator console.
./target/release/oracle-automate-tui

# Full test suite.
cargo test --workspace
```

**One-command local demo** — the server wired to a mock Oracle Fusion pod and a
mock OIC pod, no real Oracle access needed (swap two URLs to go live):

```bash
docker compose up --build      # see deploy/demo/README.md
```

> **Status.** Every layer — ERP domain, layered retrieval, the full MCP surface,
> and the live Fusion/OIC transports — is complete and test-green (**216**
> offline tests). The live clients are exercised end-to-end against runnable mock
> pods; going to a real pod is a URL change. See
> [`docs/PRODUCTION_READINESS.md`](docs/PRODUCTION_READINESS.md) for the phased
> path to production.

---

## Architecture

Every layer is a trait-based seam (`KnowledgeStore`, `EmbeddingClient`,
`ErpClient`, `OicClient`, `Reranker`, `ChannelAdapter`, `AuditSink`), so every
backend is independently replaceable — mock in development, live in production.

```
┌──────────────────────────────────────────────────────────────────────┐
│  Channels: Teams · Slack · Telegram · WhatsApp · Email · CLI          │  oracle-automate-channels
├──────────────────────────────────────────────────────────────────────┤
│  Gateway: intent routing · 4-tier memory · proactive scheduler        │  oracle-automate-gw
├──────────────────────────────────────────────────────────────────────┤
│  MCP transports: stdio · HTTP+SSE · streaming HTTP                     │  mcp-transport
├──────────────────────────────────────────────────────────────────────┤
│  MCP server: tools · resources · prompts · elicitation                │  mcp-server + apps/oracle-automate-server
├──────────────────────────────────────────────────────────────────────┤
│  RAG engine: dense + BM25 + RRF + cross-encoder reranker              │  oracle-automate-rag
│  Graph engine: GraphRAG (Louvain) · HippoRAG (PPR) · RAPTOR           │  oracle-automate-graph
├──────────────────────────────────────────────────────────────────────┤
│  Knowledge base: in-memory · Qdrant · ArangoDB · DocumentTree         │  oracle-automate-kb
│  Ingestion: HTML crawler · contextual chunker · embedding pipeline    │  oracle-automate-ingest
├──────────────────────────────────────────────────────────────────────┤
│  Oracle Fusion REST · BI Publisher · TCA parties (live + mock pod)    │  oracle-automate-erp
│  Custom-code surface: OIC · Application Composer · BIP (live + mock)  │  oracle-automate-adt
├──────────────────────────────────────────────────────────────────────┤
│  Observability: Prometheus · audit log · OpenTelemetry ready          │  oracle-automate-observability
└──────────────────────────────────────────────────────────────────────┘
```

The live Fusion / OIC clients (`HttpFusionClient`, `FusionPartyClient`,
`HttpOicClient`) speak real Fusion REST / OIC shapes. In development they target
**runnable mock pods** (`oracle-automate-fusion-mock`, `oracle-automate-oic-mock`);
in production they target a real pod — the only change is the base URL.

---

## What's inside

- **37 MCP tools** across six surfaces — RAG/doc search, Fusion REST + system
  ops, TCA master data, OIC/Application-Composer/BIP custom-code retrieval,
  knowledge-graph traversal, and gated write workflows.
- **MCP 2025-06-18** coverage: `initialize`, `tools/*`, `resources/*`,
  `prompts/*`, `elicitation/create`, `logging/setLevel`, `completion/complete`,
  HTTP `Origin` validation, bearer auth.
- **Layered retrieval** — hybrid (dense + BM25 + RRF + rerank), GraphRAG
  (Louvain), HippoRAG (PPR), RAPTOR — with a real cross-encoder reranker behind
  a feature flag.
- **Live Oracle transports** — Fusion REST (read + gated write), TCA party
  search, OIC / Application Composer / BI Publisher artifact retrieval,
  where-used, gated activation — each with request timeouts and contract tests.
- **Runnable mock pods** for Fusion and OIC, with auth + latency injection and a
  no-auth `/healthz` — one-command demo via `docker compose`.
- **Agentic gateway** — multi-channel adapters, four-tier memory, TOML scheduler.
- **13 agentic skills** (period close, SoD audit, REST service design, sandbox
  impact analysis, …) auto-loaded as MCP prompts.
- **Production posture** — Prometheus metrics + SLO alert rules, redacted audit
  log, distroless image, K8s manifests, secrets-manager credential provider,
  and a scanned + signed (cosign) release pipeline.

---

## Why it's hard (and why Rust)

ERP automation is unforgiving in three dimensions at once — **correctness**
(a wrong post is a financial event), **latency** (agents make many retrieval
calls per turn), and **data sovereignty** (financial data can't leave the
perimeter). We picked Rust precisely because it lets one binary be fast,
memory-safe, and self-contained: sub-millisecond retrieval with no garbage-
collection tails, and a distroless image with no shell to attack.

---

## Repository layout

```
oracle-automate/
├── crates/                          ← 18 Rust crates
│   ├── mcp-core / mcp-transport / mcp-server / mcp-client   (generic MCP)
│   ├── oracle-automate-kb / -rag / -graph / -ingest         (layered retrieval)
│   ├── oracle-automate-memory / -observability / -channels / -scheduler / -skills
│   ├── oracle-automate-erp          (Oracle Fusion REST / BIP / TCA — live + mock)
│   ├── oracle-automate-adt          (OIC / Application Composer / BIP custom code)
│   ├── oracle-automate-fusion-mock  (runnable mock Fusion pod)
│   ├── oracle-automate-oic-mock     (runnable mock OIC pod)
│   └── oracle-automate-connectors
├── apps/                            ← binaries + Next.js web UI
│   ├── oracle-automate-server / -gw / -tui / -ingest / -bench
│   ├── sample-server / sample-client
│   └── web/                         Next.js 14 web console
├── skills/                          ← auto-loaded agentic skills
├── deploy/                          ← Dockerfile · K8s · Prometheus SLOs · docker-compose demo
├── SECURITY.md · RELEASING.md       ← security posture · release + rollback runbook
└── docs/
    ├── PRODUCTION_READINESS.md      ← phased path to a production pod (authoritative)
    ├── SLO.md                       ← SLIs / SLO targets / error budgets
    └── ORACLE_CORRECTNESS.md        ← the correctness invariants, as tests
```

---

## About Gaussian Technologies

**Gaussian Technologies** is a deep-tech startup based in Indonesia, building
agentic infrastructure for the enterprise systems where correctness, latency,
and data sovereignty are non-negotiable. Oracle-Automate is our open-source
flagship: a Rust-native, on-premise agent runtime for Oracle Fusion Cloud ERP.
We believe the safe path to enterprise AI is local-first software with
guardrails written down as code — not another cloud between a company and its
core systems.

## License

Built by **Gaussian Technologies** and released under [Apache-2.0](LICENSE).
