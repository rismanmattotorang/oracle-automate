<div align="center">

# Oracle-Automate

### The agentic operating system for Oracle Fusion Cloud ERP — Rust core, on-premise by default.

**Sub-millisecond retrieval · correctness-as-tests · read-only by default · Apache-2.0**

Built by **[Gaussian Technologies](#about-gaussian-technologies)** — a deep-tech startup from Indonesia.

[![Rust](https://img.shields.io/badge/Rust-1.80%2B-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)
[![MCP](https://img.shields.io/badge/MCP-2025--06--18-8b5cf6?style=flat-square)](https://modelcontextprotocol.io)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue?style=flat-square)](LICENSE)

[Quick start](#quick-start) · [Architecture](#architecture) · [What ships](#what-ships-in-this-repo) · [Production readiness](docs/PRODUCTION_READINESS.md)

</div>

---

Enterprises run their financial and supply-chain core on **Oracle Fusion Cloud
ERP**. AI agents are good at reasoning over it — and dangerous at acting on it.
**Oracle-Automate closes that gap**: an MCP-native agent runtime that gives any
LLM client (Claude, Cursor, your own gateway) safe, cited, sub-millisecond
access to Fusion — read-only by default, with transactional writes gated behind
explicit approval, elicitation, and an audit trail.

It is **open-source, on-premise, and vendor-neutral** — no agent SaaS sits
between your data and your ERP.

```bash
# One command: the full agent runtime wired to mock Fusion + OIC pods.
docker compose up --build        # then point an MCP client at http://localhost:3030/mcp
```

> **Status.** All ERP-domain, retrieval, MCP-surface, and live-transport layers
> are complete and test-green (205 offline tests). The live Oracle clients are
> exercised end-to-end against runnable mock pods; going live is a URL change.
> See [`docs/PRODUCTION_READINESS.md`](docs/PRODUCTION_READINESS.md) for the
> phased path to a production pod.

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

# Run the full test suite.
cargo test --workspace
```

**One-command local demo** — the MCP server wired to a mock Oracle Fusion pod
and a mock OIC pod, no real Oracle access needed (swap two URLs to go live):

```bash
docker compose up --build      # see deploy/demo/README.md
```

---

## Why we built it

Gaussian Technologies builds agentic infrastructure for the systems enterprises
can't afford to get wrong. Oracle Fusion Cloud ERP is the canonical example: a
general-purpose agent that can *post a journal* or *create a purchase order* is
useful; one that does so without guardrails is a liability. Our design
principles:

- **MCP-native, on-prem by default** — the runtime lives next to your data; no
  third-party agent cloud, no SaaS lock-in.
- **Read-only by default, gated writes** — write tools are *hidden* from
  `tools/list` until the operator opts in (`--enable-writes`); high-stakes
  workflows require elicitation + a re-typed confirmation.
- **Correctness written down as tests** — Oracle-Fusion invariants (item-number
  length, GL + Subledger accounting backbone, scoping columns, REST/FND return
  contracts) are enforced as a dedicated CI gate, not left to prose.
- **Cite every claim** — answers carry `oracle-help://` / `oracle-rest://` /
  `oracle-object://` provenance URIs.
- **Rust core for sub-millisecond retrieval** — no Python/Node latency tails;
  the P95 retrieval gate is < 80 ms in CI.

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

## What ships in this repo

- **18 Rust crates** (`crates/`) + **9 binaries** (`apps/` + the mock pods) + a **Next.js 14 web UI** (`apps/web`).
- **MCP 2025-06-18** coverage: `initialize`, `tools/*`, `resources/*`, `prompts/*`,
  `elicitation/create`, `logging/setLevel`, `completion/complete`, HTTP `Origin`
  validation, bearer auth.
- **Layered retrieval**: hybrid (dense + BM25 + RRF + rerank), GraphRAG (Louvain),
  HippoRAG (PPR), RAPTOR — with a real cross-encoder reranker behind a feature flag.
- **Live Oracle transports**: Fusion REST (read + gated write), TCA party search,
  OIC / Application Composer / BI Publisher artifact retrieval, where-used, gated
  activation — each with request timeouts and contract tests.
- **Runnable mock pods** for Fusion and OIC, with auth + latency injection and a
  no-auth `/healthz` — one-command demo via `docker compose`.
- **Agentic gateway**: multi-channel adapters, four-tier memory, TOML scheduler.
- **13 agentic skills** (period close, SoD audit, REST service design, sandbox
  impact analysis, …) auto-loaded as MCP prompts.
- **Production posture**: Prometheus metrics, redacted audit log, distroless
  image, K8s manifests, pinned-toolchain CI with an Oracle-correctness gate.

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
│   └── web/                         Next.js 14 web UI
├── skills/                          ← auto-loaded agentic skills
├── deploy/                          ← Dockerfile · K8s manifests · docker-compose demo
└── docs/
    ├── PRODUCTION_READINESS.md      ← phased path to a production pod (authoritative)
    └── ORACLE_CORRECTNESS.md        ← the correctness invariants, as tests
```

---

## About Gaussian Technologies

**Gaussian Technologies** is a deep-tech startup based in Indonesia, building
agentic infrastructure for enterprise systems where correctness, latency, and
data sovereignty are non-negotiable. Oracle-Automate is our open-source flagship:
a Rust-native, on-premise agent runtime for Oracle Fusion Cloud ERP. We believe
the safe path to enterprise AI is local-first software with guardrails written
down as code — not another cloud sitting between a company and its core systems.

## License

Oracle-Automate is built by **Gaussian Technologies** and released under
[Apache-2.0](LICENSE).
