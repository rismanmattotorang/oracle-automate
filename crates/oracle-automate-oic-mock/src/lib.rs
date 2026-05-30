//! Standalone mock Oracle Integration Cloud (OIC) / custom-code REST API.
//!
//! Emulates the subset of the OIC + BI Publisher + Fusion-REST surface that
//! Oracle-Automate's `HttpOicClient` calls, so the `oracle.oic.*` custom-code
//! path (artifact retrieval, search, where-used, activation) can be exercised
//! end-to-end with **no real pod** — the OIC counterpart to
//! `oracle-automate-fusion-mock`.
//!
//! Point an OIC destination at it and swap to a real pod later by changing the
//! destination's `base_url`; the client is unchanged.
//!
//! Surface:
//! - `GET  /ic/api/integration/v1/integrations`             search (`?q={name:'…'}`)
//! - `GET  /ic/api/integration/v1/integrations/{name}`      integration (404 for `MISSING`)
//! - `POST /ic/api/integration/v1/integrations/{name}`      activate (write)
//! - `GET  /ic/api/integration/v1/integrations/{name}/groovy`  Application Composer Groovy
//! - `GET  /ic/api/integration/v1/{integrations|connections|lookups}/{name}/usages`  where-used
//! - `GET  /ic/api/integration/v1/connections/{name}`       connection
//! - `GET  /ic/api/integration/v1/lookups/{name}`           lookup
//! - `GET  /ic/api/integration/v1/projects/{name}`          project contents
//! - `GET  /fscmRestApi/resources/11.13.18.05/erpintegrations/{name}`  ESS job
//! - `GET  /xmlpserver/services/rest/v1/reports/{name}`     BI Publisher report
//!
//! Knobs ([`MockConfig`]): `latency_ms` (inject latency for timeout tuning) and
//! `require_auth` (reject requests with no `Authorization` header).

use axum::{
    extract::{Path, Query, Request, State},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::Duration;

/// Behavioural knobs for the mock OIC pod.
#[derive(Clone)]
pub struct MockConfig {
    /// Fixed per-request latency injected before handling (timeout tuning).
    pub latency_ms: u64,
    /// Reject requests with no `Authorization` header (matching a real pod).
    pub require_auth: bool,
}

impl Default for MockConfig {
    fn default() -> Self {
        Self {
            latency_ms: 0,
            require_auth: true,
        }
    }
}

/// Build the mock OIC / custom-code router.
pub fn router(cfg: MockConfig) -> Router {
    Router::new()
        .route("/ic/api/integration/v1/integrations", get(search))
        .route(
            "/ic/api/integration/v1/integrations/:name",
            get(integration).post(activate),
        )
        .route(
            "/ic/api/integration/v1/integrations/:name/groovy",
            get(groovy),
        )
        .route(
            "/ic/api/integration/v1/integrations/:name/usages",
            get(usages),
        )
        .route("/ic/api/integration/v1/connections/:name", get(connection))
        .route(
            "/ic/api/integration/v1/connections/:name/usages",
            get(usages),
        )
        .route("/ic/api/integration/v1/lookups/:name", get(lookup))
        .route("/ic/api/integration/v1/lookups/:name/usages", get(usages))
        .route("/ic/api/integration/v1/projects/:name", get(project))
        .route(
            "/fscmRestApi/resources/11.13.18.05/erpintegrations/:name",
            get(ess_job),
        )
        .route(
            "/xmlpserver/services/rest/v1/reports/:name",
            get(bip_report),
        )
        .layer(middleware::from_fn_with_state(cfg, guard))
}

/// Latency injection + auth gate, applied to every route.
async fn guard(
    State(cfg): State<MockConfig>,
    headers: HeaderMap,
    req: Request,
    next: Next,
) -> Response {
    if cfg.latency_ms > 0 {
        tokio::time::sleep(Duration::from_millis(cfg.latency_ms)).await;
    }
    if cfg.require_auth && !headers.contains_key("authorization") {
        return error_envelope(
            StatusCode::UNAUTHORIZED,
            "OIC-AUTH-401",
            "Missing Authorization header",
        );
    }
    next.run(req).await
}

fn error_envelope(status: StatusCode, code: &str, detail: &str) -> Response {
    (
        status,
        Json(json!({
            "title": status.canonical_reason().unwrap_or("Error"),
            "status": status.as_u16(),
            "detail": detail,
            "o:errorCode": code,
        })),
    )
        .into_response()
}

async fn integration(Path(name): Path<String>) -> Response {
    if name.eq_ignore_ascii_case("MISSING") {
        return error_envelope(StatusCode::NOT_FOUND, "OIC-404", "Integration not found");
    }
    Json(json!({
        "code": format!("<integration name=\"{name}\" version=\"01.00.0000\"/>"),
        "description": "Kalbe OIC integration (mock)",
        "status": "ACTIVATED",
        "project": "KLB_FINANCE_INTEGRATIONS",
    }))
    .into_response()
}

async fn activate(Path(name): Path<String>) -> impl IntoResponse {
    // POST .../integrations/{id}?integrationInstruction=activate
    (
        StatusCode::OK,
        Json(json!({ "code": name, "status": "ACTIVATED" })),
    )
}

async fn groovy(Path(name): Path<String>) -> impl IntoResponse {
    Json(json!({
        "source": format!(
            "// Application Composer Groovy: {name}\nif (InvoiceAmount < 0) {{ throw new oracle.jbo.ValidationException('Amount must be >= 0') }}"
        ),
        "description": "Invoice hold rule (mock)",
        "status": "ACTIVE",
        "package": "KLB_FINANCE_INTEGRATIONS",
    }))
}

async fn connection(Path(name): Path<String>) -> impl IntoResponse {
    Json(json!({
        "content": format!("{{\"connection\":\"{name}\",\"role\":\"invoke\",\"adapter\":\"Oracle ERP Cloud\"}}"),
        "description": "Fusion ERP REST connection (mock)",
        "status": "CONFIGURED",
        "package": "KLB_FINANCE_INTEGRATIONS",
    }))
}

async fn lookup(Path(name): Path<String>) -> impl IntoResponse {
    Json(json!({
        "content": format!("{{\"lookup\":\"{name}\",\"rows\":[[\"1000\",\"KLB-ID\"],[\"2000\",\"KLB-SG\"]]}}"),
        "description": "Company cross-reference (mock)",
        "status": "ACTIVE",
    }))
}

async fn project(Path(name): Path<String>) -> impl IntoResponse {
    Json(json!({
        "description": format!("Kalbe project {name} (mock)"),
        "integrations": [
            { "code": "KLB_GL_JOURNAL_IMPORT", "description": "GL journal FBDI import" },
            { "code": "KLB_PO_RECEIPT_SYNC", "description": "PO receiving sync" }
        ]
    }))
}

async fn ess_job(Path(name): Path<String>) -> impl IntoResponse {
    Json(json!({
        "code": name,
        "description": "GL Journal Import ESS job (mock)",
        "status": "SCHEDULED",
    }))
}

async fn bip_report(Path(name): Path<String>) -> impl IntoResponse {
    Json(json!({
        "dataModel": "SELECT je.je_header_id, je.period_name, je.status FROM GL_JE_LINES je WHERE je.ledger_id = :ledgerId",
        "dataSource": "ApplicationDB_FSCM",
        "description": format!("BI Publisher report {name} (mock)"),
    }))
}

async fn usages(Path(_name): Path<String>) -> impl IntoResponse {
    Json(json!({ "items": [
        { "code": "KLB_GL_JOURNAL_IMPORT", "usage": "invoke activity importJournals" },
        { "code": "KLB_PO_RECEIPT_SYNC", "usage": "invoke activity postReceipt" }
    ]}))
}

async fn search(Query(params): Query<HashMap<String, String>>) -> impl IntoResponse {
    let needle = params
        .get("q")
        .and_then(|q| extract_name(q))
        .unwrap_or_default()
        .to_lowercase();
    let all = [
        json!({ "code": "KLB_GL_JOURNAL_IMPORT", "description": "GL journal FBDI import", "project": "KLB_FINANCE_INTEGRATIONS" }),
        json!({ "code": "KLB_PO_RECEIPT_SYNC", "description": "PO receiving sync", "project": "KLB_FINANCE_INTEGRATIONS" }),
        json!({ "code": "KLB_INVOICE_HOLD_RULE", "description": "AP invoice hold rule", "project": "KLB_FINANCE_INTEGRATIONS" }),
    ];
    let items: Vec<Value> = all
        .into_iter()
        .filter(|i| {
            needle.is_empty()
                || i["code"]
                    .as_str()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&needle)
                || i["description"]
                    .as_str()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&needle)
        })
        .collect();
    Json(json!({ "items": items }))
}

/// Pull the search term out of an OIC `q` filter, e.g. `"{name:'journal'}"` → `"journal"`.
fn extract_name(q: &str) -> Option<String> {
    let start = q.find('\'')? + 1;
    let end = q.rfind('\'')?;
    if end <= start {
        return None;
    }
    Some(q[start..end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_name_pulls_term() {
        assert_eq!(extract_name("{name:'journal'}").as_deref(), Some("journal"));
        assert_eq!(extract_name("no quotes"), None);
    }
}
