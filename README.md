<div align="center">

# Oracle-Automate

### The agentic OS for Oracle ERP — built in Rust, on-premise by default.

**Sub-millisecond retrieval. Correctness-as-tests. Apache-2.0.**
**Made by [Kalbe](#about-kalbe).**

[![Rust](https://img.shields.io/badge/Rust-1.80%2B-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)
[![MCP](https://img.shields.io/badge/MCP-2025--06--18-8b5cf6?style=flat-square)](https://modelcontextprotocol.io)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue?style=flat-square)](LICENSE)

[**Porting strategy →**](docs/PORTING_STRATEGY.md) · [Architecture](#architecture) · [What ships](#what-ships-in-this-repo)

</div>

---

> ### 🚧 Port in progress
>
> **Oracle-Automate is a port of [SAP-Automate](https://github.com/rismanmattotorang/sap-automate)** — the same
> proven MCP-native agentic architecture — re-fitted from **SAP S/4HANA** to
> **Oracle Fusion Cloud ERP** (latest release) and rebranded from ParagonCorp to
> **Kalbe**.
>
> The port runs in **phases** so the workspace builds and tests green at every
> step. See [`docs/PORTING_STRATEGY.md`](docs/PORTING_STRATEGY.md) for the full
> SAP→Oracle domain mapping and the phase plan. Current status: **Phase 1
> (foundation / rebrand) complete** — the full architecture has been lifted and
> rebranded, the workspace compiles, and the generic MCP / RAG / graph / KB
> layers are Oracle-ready. The deep ERP domain model (the BAPI/RFC catalogue,
> ABAP/ADT surface, fixtures, tool namespace, skills, and docs) is being
> re-modeled to Oracle Fusion in subsequent phases — until those land you will
> still see SAP-domain identifiers in the lower layers.

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

---

## Why Kalbe is building this

Kalbe (PT Kalbe Farma Tbk) runs a large **Oracle Fusion Cloud ERP** estate
across pharmaceutical manufacturing, distribution, and consumer-health
operations. The gap between *what AI agents can do generally* and *what they
can do safely against a production ERP* is the same gap ParagonCorp identified
for SAP — and the same architecture closes it for Oracle:

- **MCP-native**, on-prem by default, no vendor SaaS lock-in.
- **Read-only by default**, gated transactional writes (`--enable-writes`).
- **Correctness written down as tests** — the SAP precision gates are being
  replaced by Oracle-Fusion invariants (item-number length, GL/SLA accounting
  backbone, scoping columns, REST return contracts). See the strategy doc.
- **Rust core** for sub-millisecond retrieval — no Python/Node latency tails.

---

## Architecture

The architecture is preserved from the source; only the bottom ERP-backend
layer and the domain vocabulary change.

```
┌──────────────────────────────────────────────────────────────────────┐
│  Channels: Teams · Slack · Telegram · WhatsApp · Email · CLI         │  oracle-automate-channels
├──────────────────────────────────────────────────────────────────────┤
│  Gateway: intent routing · 4-tier memory · proactive scheduler       │  oracle-automate-gw
├──────────────────────────────────────────────────────────────────────┤
│  MCP transports: stdio · HTTP+SSE · Streaming HTTP                   │  mcp-transport
├──────────────────────────────────────────────────────────────────────┤
│  MCP server: tools · resources · prompts · elicitation              │  mcp-server  + apps/oracle-automate-server
├──────────────────────────────────────────────────────────────────────┤
│  RAG engine: dense + BM25 + RRF + cross-encoder reranker             │  oracle-automate-rag
│  Graph engine: GraphRAG (Louvain) · HippoRAG (PPR) · RAPTOR          │  oracle-automate-graph
├──────────────────────────────────────────────────────────────────────┤
│  Knowledge base: in-memory · Qdrant · ArangoDB · DocumentTree        │  oracle-automate-kb
│  Ingestion: HTML crawler · contextual chunker · embedding pipeline   │  oracle-automate-ingest
├──────────────────────────────────────────────────────────────────────┤
│  Oracle ERP backends (porting): Fusion REST · SOAP · BI Publisher    │  oracle-automate-rfc (→ erp)
│  Custom-code surface (porting): OIC · App Composer · BIP             │  oracle-automate-adt (→ oic)
├──────────────────────────────────────────────────────────────────────┤
│  Observability: Prometheus · audit log · OpenTelemetry ready         │  oracle-automate-observability
└──────────────────────────────────────────────────────────────────────┘
```

Every layer is a trait-based seam: `KnowledgeStore`, `EmbeddingClient`,
`SapClient` (→ `ErpClient`), `AdtClient`, `Reranker`, `ChannelAdapter`,
`AuditSink`. **Every backend is independently replaceable** — which is exactly
what makes the SAP→Oracle port tractable: swap the bottom layer, keep the rest.

---

## What ships in this repo

- **16 Rust crates** (`crates/`) + **7 binaries** (`apps/`) + a **Next.js 14 web UI** (`apps/web`).
- **MCP 2025-06-18** coverage: `initialize`, `tools/*`, `resources/*`, `prompts/*`,
  `elicitation/create`, `logging/setLevel`, `completion/complete`, HTTP `Origin`
  validation, bearer auth.
- **Layered retrieval**: hybrid (dense + BM25 + RRF + rerank), GraphRAG (Louvain),
  HippoRAG (PPR), RAPTOR.
- **Agentic gateway**: multi-channel adapters, four-tier memory, TOML scheduler.
- **Production posture**: Prometheus metrics, audit log, K8s manifests, Dockerfile, CI.

See [`docs/PORTING_STRATEGY.md`](docs/PORTING_STRATEGY.md) for which layers are
Oracle-ready and which are mid-port.

---

## Repository layout

```
oracle-automate/
├── crates/                          ← 16 Rust crates
│   ├── mcp-core / mcp-transport / mcp-server / mcp-client   (generic MCP — Oracle-ready)
│   ├── oracle-automate-kb / -rag / -graph / -ingest         (retrieval — light domain)
│   ├── oracle-automate-memory / -observability / -channels / -scheduler / -skills
│   ├── oracle-automate-rfc          (ERP backend — being re-modeled to Fusion REST/SOAP/BIP)
│   ├── oracle-automate-adt          (custom-code surface — being re-modeled to OIC/App Composer)
│   └── oracle-automate-connectors
├── apps/                            ← 7 binaries + Next.js web UI
│   ├── oracle-automate-server / -gw / -tui / -ingest / -bench
│   ├── sample-server / sample-client
│   └── web/                         Next.js 14 web UI
├── skills/                          ← auto-loaded agentic skills (porting to Oracle)
├── deploy/                          ← Dockerfile + K8s manifests
└── docs/
    ├── PORTING_STRATEGY.md          ← the SAP→Oracle phased port plan
    └── …                            (ROADMAP / CORRECTNESS / COMPARISON / INTEGRATION — porting)
```

---

## About Kalbe

**Kalbe** (PT Kalbe Farma Tbk) is Indonesia's largest publicly-listed
pharmaceutical company, running Oracle Fusion Cloud ERP across manufacturing,
distribution, and consumer-health operations.

## Credit

Oracle-Automate is a port of **SAP-Automate** by **ParagonCorp** (TPO R&D),
released under Apache-2.0. The architecture, layering, and MCP/RAG engineering
are theirs; this repository re-fits the ERP-domain layer for Oracle.

---

## License

[Apache-2.0](LICENSE).
