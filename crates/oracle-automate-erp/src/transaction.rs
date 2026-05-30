//! Transactional write orchestration.
//!
//! Oracle write paths differ by backend, but the *safety property* is the
//! same as the SAP original: never persist an unverified change. Fusion
//! REST writes auto-commit per request; Oracle EBS PL/SQL APIs defer
//! persistence to an explicit `p_commit` / `COMMIT` (with `ROLLBACK` on
//! failure); FBDI bulk loads defer to an import job. This module wraps the
//! verify-then-finalize protocol so every write path enforces it
//! identically:
//!
//! 1. refuse outright in read-only mode (fail-closed);
//! 2. call the write operation;
//! 3. inspect its FND return stack (`X_RETURN_STATUS` / `X_MSG_DATA`) — any
//!    `E`/`U`/unknown severity is a failure;
//! 4. failure → transaction rollback, success → transaction commit.
//!
//! The decision (`has_failure`) is a pure function so it can be tested
//! directly; the orchestration is generic over the `ErpClient` trait, so it
//! works against the mock and the live REST/SOAP backends alike.

use crate::erp_result::{parse_erp_messages, ErpMessage, ErpSeverity};
use crate::client::{ErpCallRequest, ErpClient};
use crate::error::{ErpError, ErpResult};
use serde::Serialize;
use serde_json::{json, Value};

const COMMIT_FN: &str = "ebs.fnd.transaction.commit";
const ROLLBACK_FN: &str = "ebs.fnd.transaction.rollback";

/// Build a synthetic message so the orchestration can surface decisions
/// (e.g. "outcome unconfirmed") in the same `messages` list as SAP's own.
fn note(severity: ErpSeverity, text: &str) -> ErpMessage {
    ErpMessage {
        severity,
        message_class: "ORAAUTO".into(),
        message_number: "000".into(),
        text: text.into(),
        parameter: None,
        row: None,
        field: None,
        system: None,
    }
}

/// Issue `the EBS rollback op`, returning whether it was *confirmed*
/// and any messages (including a synthetic warning when it couldn't be
/// confirmed, so a `rolled_back: true` is never reported on faith).
async fn rollback(client: &dyn ErpClient) -> (bool, Vec<ErpMessage>) {
    match client.call_operation(rollback_req(), false).await {
        Ok(v) => {
            let msgs = parse_erp_messages(&v);
            (!has_failure(&msgs), msgs)
        }
        Err(e) => (
            false,
            vec![note(
                ErpSeverity::Warning,
                &format!("rollback could not be confirmed: {e}"),
            )],
        ),
    }
}

/// Result of a transactional write.
#[derive(Debug, Clone, Serialize)]
pub struct WriteOutcome {
    pub function: String,
    /// Whether the LUW was committed (true) — i.e. the change is persisted.
    pub committed: bool,
    /// Whether a rollback was issued (because the REST operation or the commit failed).
    pub rolled_back: bool,
    /// Combined FND return stack messages from the REST operation and the commit.
    pub messages: Vec<ErpMessage>,
    /// The raw REST operation result (export/tables), for the caller to mine for the
    /// resulting document number etc.
    pub result: Value,
}

/// True if any message indicates the REST operation failed (so we must NOT commit).
/// Unknown severities count as failures — fail-closed (see `erp_result`).
pub fn has_failure(messages: &[ErpMessage]) -> bool {
    messages.iter().any(ErpMessage::is_failure)
}

/// Execute a write REST operation and finish its LUW with commit-or-rollback.
///
/// `read_only_mode` mirrors the server's safety flag; when true this refuses
/// to run at all.  The underlying `call_operation` still applies the client's own
/// read-only gate, so this is defence in depth.
pub async fn execute_write_bapi(
    client: &dyn ErpClient,
    request: ErpCallRequest,
    read_only_mode: bool,
) -> ErpResult<WriteOutcome> {
    if read_only_mode {
        return Err(ErpError::PermissionDenied(format!(
            "write workflow for '{}' requires write mode (--enable-writes)",
            request.function
        )));
    }
    if request.function == COMMIT_FN || request.function == ROLLBACK_FN {
        return Err(ErpError::InvalidParameter {
            name: "function".into(),
            reason: "commit/rollback are issued automatically; call the business REST operation instead"
                .into(),
        });
    }

    let function = request.function.clone();
    let result = client.call_operation(request, false).await?;
    let mut messages = parse_erp_messages(&result);

    if has_failure(&messages) {
        let (rolled_back, rb_msgs) = rollback(client).await;
        messages.extend(rb_msgs);
        return Ok(WriteOutcome { function, committed: false, rolled_back, messages, result });
    }

    // FAIL-CLOSED: if the REST operation returned no parseable FND return stack at all, we have
    // no positive confirmation of success — do NOT commit on faith.  Roll
    // back and report the outcome as unconfirmed (a REST operation that genuinely
    // returns zero rows on success is rare; the safe default for a write
    // gate is to refuse to persist an unverified change).
    if messages.is_empty() {
        let (rolled_back, rb_msgs) = rollback(client).await;
        let mut out = vec![note(
            ErpSeverity::Warning,
            "REST operation returned no FND return stack; outcome unconfirmed — not committed",
        )];
        out.extend(rb_msgs);
        return Ok(WriteOutcome { function, committed: false, rolled_back, messages: out, result });
    }

    // Non-empty and no failure → commit synchronously (WAIT = 'X').
    let commit_result = client.call_operation(commit_req(), false).await?;
    messages.extend(parse_erp_messages(&commit_result));

    if has_failure(&messages) {
        // Commit itself reported an error — roll back to be safe.
        let (rolled_back, rb_msgs) = rollback(client).await;
        messages.extend(rb_msgs);
        return Ok(WriteOutcome { function, committed: false, rolled_back, messages, result });
    }

    Ok(WriteOutcome { function, committed: true, rolled_back: false, messages, result })
}

fn commit_req() -> ErpCallRequest {
    ErpCallRequest {
        function: COMMIT_FN.into(),
        parameters: json!({ "WAIT": "X" }),
        timeout_ms: 30_000,
        require_read_only_safe: false,
    }
}

fn rollback_req() -> ErpCallRequest {
    ErpCallRequest {
        function: ROLLBACK_FN.into(),
        parameters: Value::Null,
        timeout_ms: 30_000,
        require_read_only_safe: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::erp_result::ErpSeverity;
    use crate::client::MockErpClient;

    fn msg(sev: ErpSeverity) -> ErpMessage {
        ErpMessage {
            severity: sev,
            message_class: "X".into(),
            message_number: "000".into(),
            text: "t".into(),
            parameter: None,
            row: None,
            field: None,
            system: None,
        }
    }

    #[test]
    fn has_failure_treats_error_abort_unknown_as_failure() {
        assert!(has_failure(&[msg(ErpSeverity::Error)]));
        assert!(has_failure(&[msg(ErpSeverity::Abort)]));
        assert!(has_failure(&[msg(ErpSeverity::Unknown('Z'))]));
        assert!(!has_failure(&[msg(ErpSeverity::Success)]));
        assert!(!has_failure(&[msg(ErpSeverity::Warning), msg(ErpSeverity::Info)]));
        assert!(!has_failure(&[]));
    }

    #[tokio::test]
    async fn refuses_in_read_only_mode() {
        let client = MockErpClient::new(2, json!({"client": "100"}));
        let req = ErpCallRequest {
            function: "fusion.po.purchaseOrders.post".into(),
            parameters: json!({ "POHEADER": {}, "POHEADERX": {} }),
            timeout_ms: 1000,
            require_read_only_safe: true,
        };
        let err = execute_write_bapi(client.as_ref(), req, true).await.unwrap_err();
        assert!(matches!(err, ErpError::PermissionDenied(_)), "got {err:?}");
    }

    #[tokio::test]
    async fn rejects_direct_commit_call() {
        let client = MockErpClient::new(2, json!({"client": "100"}));
        let req = ErpCallRequest {
            function: "ebs.fnd.transaction.commit".into(),
            parameters: json!({ "WAIT": "X" }),
            timeout_ms: 1000,
            require_read_only_safe: false,
        };
        let err = execute_write_bapi(client.as_ref(), req, false).await.unwrap_err();
        assert!(matches!(err, ErpError::InvalidParameter { .. }), "got {err:?}");
    }

    #[tokio::test]
    async fn empty_bapiret2_is_fail_closed_not_committed() {
        // The mock returns no FND return stack, which is an *unconfirmed* outcome —
        // the gate must NOT commit it.
        let client = MockErpClient::new(2, json!({"client": "100"}));
        let req = ErpCallRequest {
            function: "fusion.po.purchaseOrders.post".into(),
            parameters: json!({ "PURCHASE_ORDER": {}, "DRAFT": "N" }),
            timeout_ms: 1000,
            require_read_only_safe: true,
        };
        let outcome = execute_write_bapi(client.as_ref(), req, false).await.unwrap();
        assert!(!outcome.committed, "empty FND return must not commit");
        assert!(outcome.messages.iter().any(|m| m.text.contains("unconfirmed")));
    }

    // A scripted client that returns a canned FND return stack for the business REST operation
    // and a clean success for commit/rollback, so the commit/rollback
    // decision can be exercised deterministically.
    struct ScriptedClient {
        bapi_return: Value,
    }

    #[async_trait::async_trait]
    impl ErpClient for ScriptedClient {
        async fn call_operation(&self, request: ErpCallRequest, _ro: bool) -> ErpResult<Value> {
            if request.function == COMMIT_FN || request.function == ROLLBACK_FN {
                return Ok(json!({ "outputs": { "RETURN": { "TYPE": "S", "MESSAGE": "done" } } }));
            }
            Ok(json!({ "outputs": { "RETURN": self.bapi_return.clone() } }))
        }
        async fn system_info(&self) -> ErpResult<crate::client::SystemInfo> {
            Err(ErpError::Internal("unused".into()))
        }
        async fn search_operations(&self, _q: &str, _n: usize) -> ErpResult<crate::client::ErpSearchResult> {
            Err(ErpError::Internal("unused".into()))
        }
        async fn operation_metadata(&self, _f: &str, _l: &str) -> ErpResult<crate::client::ErpOperationMeta> {
            Err(ErpError::Internal("unused".into()))
        }
        async fn bulk_operation_metadata(&self, _f: &[String], _l: &str) -> ErpResult<crate::client::BulkMetadata> {
            Err(ErpError::Internal("unused".into()))
        }
        async fn read_table(&self, _r: crate::client::ReadTableRequest) -> ErpResult<Vec<crate::client::TableRow>> {
            Err(ErpError::Internal("unused".into()))
        }
        async fn table_structure(&self, _t: &str) -> ErpResult<crate::client::TableStructure> {
            Err(ErpError::Internal("unused".into()))
        }
    }

    fn scripted(bapi_return: Value) -> ScriptedClient {
        ScriptedClient { bapi_return }
    }

    #[tokio::test]
    async fn commits_on_explicit_success_row() {
        let client = scripted(json!([{ "TYPE": "S", "ID": "06", "NUMBER": "017", "MESSAGE": "PO 4500000001 created" }]));
        let req = ErpCallRequest {
            function: "fusion.po.purchaseOrders.post".into(),
            parameters: json!({ "POHEADER": {} }),
            timeout_ms: 1000,
            require_read_only_safe: true,
        };
        let outcome = execute_write_bapi(&client, req, false).await.unwrap();
        assert!(outcome.committed, "expected commit; messages={:?}", outcome.messages);
        assert!(!outcome.rolled_back);
    }

    #[tokio::test]
    async fn rolls_back_on_error_row() {
        let client = scripted(json!([{ "TYPE": "E", "ID": "06", "NUMBER": "055", "MESSAGE": "Vendor 1 blocked" }]));
        let req = ErpCallRequest {
            function: "fusion.po.purchaseOrders.post".into(),
            parameters: json!({ "POHEADER": {} }),
            timeout_ms: 1000,
            require_read_only_safe: true,
        };
        let outcome = execute_write_bapi(&client, req, false).await.unwrap();
        assert!(!outcome.committed, "error row must not commit");
        assert!(outcome.rolled_back, "error row must roll back");
    }
}
