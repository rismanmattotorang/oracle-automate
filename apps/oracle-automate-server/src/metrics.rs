//! Per-call metric recording for the MCP HTTP transport (Phase 10).
//!
//! The dispatch closure sees every JSON-RPC [`Message`]. Previously the
//! Prometheus registry was *registered* but never *recorded into*, so
//! `/metrics` emitted only `HELP`/`TYPE` lines with no data — useless for
//! SLOs. This module classifies `tools/call` requests and records
//! calls / errors / latency / authz-denials per tool, so the dashboards and
//! alert rules in `deploy/` have real series to act on.
//!
//! The classification helpers are pure and unit-tested; [`record`] is the
//! recording entry point wired into `main.rs`.

use mcp_core::Message;
use oracle_automate_observability::metrics::MetricsRegistry;

/// The tool a `tools/call` request targets, if this message is one.
pub fn tool_name(msg: &Message) -> Option<String> {
    match msg {
        Message::Request(r) if r.method == "tools/call" => r
            .params
            .as_ref()
            .and_then(|p| p.get("name"))
            .and_then(|n| n.as_str())
            .map(|s| s.to_string()),
        _ => None,
    }
}

/// Whether a dispatch response is an error — a JSON-RPC protocol error, or a
/// tool result carrying `isError: true`.
pub fn response_is_error(resp: &Option<Message>) -> bool {
    match resp {
        Some(Message::Response(r)) => {
            r.error.is_some()
                || r.result
                    .as_ref()
                    .and_then(|v| v.get("isError"))
                    .and_then(|b| b.as_bool())
                    .unwrap_or(false)
        }
        _ => false,
    }
}

/// Whether an error response was a read-only-gate / authorization denial.
/// Keys on the gate's stable message markers (e.g. "not callable in read-only
/// mode", "blocked: read-only mode", `PermissionDenied`).
pub fn response_denied(resp: &Option<Message>) -> bool {
    let text = match resp {
        Some(Message::Response(r)) => match &r.error {
            Some(e) => e.message.to_ascii_lowercase(),
            None => r
                .result
                .as_ref()
                .map(|v| v.to_string().to_ascii_lowercase())
                .unwrap_or_default(),
        },
        _ => return false,
    };
    text.contains("read-only") || text.contains("permissiondenied") || text.contains("not callable")
}

/// Record metrics for one dispatched `tools/call`.
///
/// - `mcp_tool_calls_total{tool}` — every call
/// - `mcp_tool_latency_seconds{tool}` — wall-clock latency histogram
/// - `mcp_tool_errors_total{tool}` — calls that returned an error
/// - `oracle_authz_denied_total{tool}` — errors that were read-only-gate denials
pub fn record(metrics: &MetricsRegistry, tool: &str, elapsed_secs: f64, resp: &Option<Message>) {
    let labels = &[("tool", tool)];
    metrics.inc_counter("mcp_tool_calls_total", labels);
    metrics.observe_histogram("mcp_tool_latency_seconds", labels, elapsed_secs);
    if response_is_error(resp) {
        metrics.inc_counter("mcp_tool_errors_total", labels);
        if response_denied(resp) {
            metrics.inc_counter("oracle_authz_denied_total", labels);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcp_core::{ErrorObject, Id, Request, Response};
    use serde_json::json;

    fn call(tool: &str) -> Message {
        Message::Request(Request::new(
            Id::Number(1),
            "tools/call",
            Some(json!({ "name": tool, "arguments": {} })),
        ))
    }

    #[test]
    fn tool_name_extracted_for_tools_call_only() {
        assert_eq!(
            tool_name(&call("oracle.docs.search")).as_deref(),
            Some("oracle.docs.search")
        );
        let other = Message::Request(Request::new(Id::Number(2), "tools/list", None));
        assert_eq!(tool_name(&other), None);
    }

    #[test]
    fn ok_result_is_not_error() {
        let resp = Some(Message::Response(Response::success(
            Id::Number(1),
            json!({ "content": [{ "type": "text", "text": "ok" }], "isError": false }),
        )));
        assert!(!response_is_error(&resp));
        assert!(!response_denied(&resp));
    }

    #[test]
    fn tool_iserror_is_error() {
        let resp = Some(Message::Response(Response::success(
            Id::Number(1),
            json!({ "content": [{ "type": "text", "text": "boom" }], "isError": true }),
        )));
        assert!(response_is_error(&resp));
    }

    #[test]
    fn read_only_denial_is_flagged() {
        let resp = Some(Message::Response(Response::success(
            Id::Number(1),
            json!({ "content": [{ "type": "text", "text": "operation 'x' is not callable in read-only mode" }], "isError": true }),
        )));
        assert!(response_is_error(&resp));
        assert!(response_denied(&resp));
    }

    #[test]
    fn jsonrpc_error_is_error() {
        let resp = Some(Message::Response(Response::failure(
            Id::Number(1),
            ErrorObject::new(-32603, "internal"),
        )));
        assert!(response_is_error(&resp));
        assert!(!response_denied(&resp));
    }

    #[test]
    fn record_populates_registry() {
        let m = MetricsRegistry::new();
        record(
            &m,
            "oracle.docs.search",
            0.01,
            &Some(Message::Response(Response::success(
                Id::Number(1),
                json!({ "isError": false }),
            ))),
        );
        let out = m.render();
        assert!(out.contains("mcp_tool_calls_total{tool=\"oracle.docs.search\"} 1"));
        assert!(out.contains("mcp_tool_latency_seconds"));
        assert!(!out.contains("mcp_tool_errors_total{tool=\"oracle.docs.search\"}"));
    }
}
