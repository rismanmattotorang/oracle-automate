# Service Level Objectives — Oracle-Automate

The SLOs the service is operated to, the SLIs that measure them, and the alerts
that fire on a breach. Alert + recording rules live in
[`deploy/prometheus/alerts.yaml`](../deploy/prometheus/alerts.yaml); the scrape
config is [`deploy/prometheus/servicemonitor.yaml`](../deploy/prometheus/servicemonitor.yaml).

> Targets below are **starting points**, set against the in-CI bench gate
> (P95 < 80 ms) and conservative error budgets. Tune them against real dev-pod
> latency once Phase 7 (live pod) lands — Phase 9.

## SLIs (what we measure)

The server records these per `tools/call` dispatch (HTTP transport):

| Metric | Type | Labels | Meaning |
|---|---|---|---|
| `mcp_tool_calls_total` | counter | `tool` | every tool invocation |
| `mcp_tool_errors_total` | counter | `tool` | invocations returning a JSON-RPC error or `isError: true` |
| `mcp_tool_latency_seconds` | histogram | `tool` | wall-clock latency (buckets straddle the 80 ms gate) |
| `oracle_authz_denied_total` | counter | `tool` | errors that were read-only-gate denials |

Recording rules precompute the SLIs:
- `job:oracle_automate_tool_error_ratio:rate5m` = errors ÷ calls (5m)
- `job:oracle_automate_tool_latency_p95_seconds:5m` = P95 latency (5m)

## SLOs (what we promise)

| # | SLO | Target | Window | SLI / alert |
|---|---|---|---|---|
| 1 | **Availability** — the server is scrapable/serving | 99.9% | 30d | `up == 0` for 5m → `OracleAutomateDown` (critical) |
| 2 | **Latency** — P95 tool latency | < 80 ms | 30d | P95 > 0.08s for 10m → `OracleAutomateLatencySLOBreach` (warning) |
| 3 | **Correctness** — tool error ratio | < 1% | 30d | ratio > 1% for 10m → `HighErrorRate` (warning); > 5% for 5m → `HighErrorRatePage` (critical, fast burn) |

### Error budgets

- Availability 99.9% → **~43 min/month** of unavailability.
- Error ratio < 1% → at 1M calls/month, **~10k** errored calls is the budget.

A multi-window strategy is used for correctness: a slow burn (>1% for 10m,
warning) and a fast burn (>5% for 5m, page) so a sudden regression escalates
immediately while a slow drift opens a ticket.

## Security signals (not SLOs, but alerted)

- `OracleAutomateAuthzDenialSpike` — >20 read-only-gate denials in 5m. Not a
  reliability SLO, but a spike signals a misconfigured client or a probe against
  hidden write tools; worth a look.

## Dashboards

The Grafana board [`deploy/grafana/oracle-automate-overview.json`](../deploy/grafana/oracle-automate-overview.json)
renders P95/P99 vs the 80 ms gate, error rate, call volume, and authz denials
over these series.

## What is not yet an SLI

- **Live Fusion/OIC call latency & error rate** — surfaced today only as the
  `oracle_rest_calls_total` / `oracle_pool_in_use` series (registered; populated
  once the pool + REST client are instrumented in Phase 9 against a real pod).
- **Distributed traces** — `tracing` spans are emitted; an OpenTelemetry
  exporter (OTLP) is the remaining Phase-10 follow-up for end-to-end traces.
