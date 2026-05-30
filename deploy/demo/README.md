# Local demo — MCP server + mock Oracle pods (one command)

Boots the whole stack with **no real Oracle access**:

- **`fusion-mock`** — mock Oracle Fusion Cloud ERP REST pod (`:8088`)
- **`oic-mock`** — mock Oracle Integration Cloud / custom-code pod (`:8089`)
- **`server`** — the Oracle-Automate MCP server (HTTP transport, `:3030`),
  wired to both mocks

```bash
# from the repo root
docker compose up --build
```

That's it. The MCP server's tools now route to the mocks:

| Tool family | Reaches |
|---|---|
| `oracle.rest.*`, `oracle.party.*`, `oracle.object.*` | `fusion-mock` |
| `oracle.oic.*` (custom-code retrieval / search / where-used / activate) | `oic-mock` |
| `oracle.docs.search`, `kb.*` | in-process RAG (offline, deterministic) |

## Go live (swap mocks → real pods)

Nothing in the stack changes except two URLs:

- **Fusion:** set `ORACLE_FUSION_BASE_URL` (and `ORACLE_FUSION_AUTH`/credentials)
  to your real pod in `docker-compose.yml`.
- **OIC:** edit `deploy/demo/destinations/mock-oic.toml` — change `base_url`
  (and the `[auth]` block) to your real OIC/Fusion endpoint.

The live clients (`HttpFusionClient`, `FusionPartyClient`, `HttpOicClient`) are
identical in mock and live modes.

## Verify

The two mock pods speak the real Fusion / OIC REST shapes — probe them directly:

```bash
# Fusion mock — supplier search (auth required)
curl -s -H "Authorization: Bearer demo" \
  "http://localhost:8088/fscmRestApi/resources/11.13.18.05/suppliers?q=Supplier%20LIKE%20'%25PT%25'&limit=2"

# Fusion mock — gated write (PO create) returns a document number
curl -s -X POST -H "Authorization: Bearer demo" -H "Content-Type: application/json" \
  -d '{"Supplier":"PT Kimia Farma","CurrencyCode":"IDR"}' \
  "http://localhost:8088/fscmRestApi/resources/11.13.18.05/purchaseOrders"

# OIC mock — fetch an integration artifact
curl -s -H "Authorization: Basic ZGVtbzpkZW1v" \
  "http://localhost:8089/ic/api/integration/v1/integrations/KLB_GL_JOURNAL_IMPORT"

# MCP server — Prometheus metrics (latency histograms, etc.)
curl -s "http://localhost:3030/metrics" | head
```

Connect any MCP client to **`http://localhost:3030/mcp`** (SSE events at
`/mcp/events`). Read-only is the default; the server is started without
`--enable-writes`, so write tools stay hidden.

## Latency / resilience knob (Phase 5)

Inject latency into either mock to exercise the client request timeout
(`ORACLE_FUSION_TIMEOUT_MS` / the OIC destination's `timeout_ms`), which the
retry / circuit-breaker layers act on:

```yaml
# docker-compose.yml → fusion-mock.command
command: ["/usr/local/bin/oracle-automate-fusion-mock", "--bind", "0.0.0.0:8088", "--latency-ms", "2000"]
```
