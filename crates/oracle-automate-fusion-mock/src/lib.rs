//! Standalone mock Oracle Fusion Cloud ERP REST API.
//!
//! Emulates the subset of the Fusion REST surface that Oracle-Automate's live
//! clients call (`HttpFusionClient` / `FusionPartyClient`), so Phases 4–5 of
//! `docs/PRODUCTION_READINESS.md` — live read + gated write end-to-end, and
//! observability / latency tuning — can run with **no real pod**.
//!
//! When a real Fusion dev pod becomes available, point `ORACLE_FUSION_BASE_URL`
//! at it: the clients are unchanged, so nothing here needs deleting. The JSON
//! shapes and error envelopes mirror the real API so the swap is transparent.
//!
//! Surface (under `/fscmRestApi/resources/11.13.18.05`):
//! - `GET   /`               catalog root (`system_info` reachability)
//! - `GET   /suppliers`      TCA supplier search (`?q=&limit=`), paginated
//! - `GET   /suppliers/{id}` single supplier (404 when unknown)
//! - `PATCH /suppliers/{id}` supplier update (write)
//! - `GET   /itemsV2`        item read
//! - `POST  /journalEntries` GL journal post (write) → `201` + `JournalEntryId`
//! - `POST  /purchaseOrders` PO create (write)        → `201` + `OrderNumber`
//!
//! Knobs ([`MockConfig`]): `latency_ms` (inject fixed latency for timeout /
//! circuit-breaker tuning) and `require_auth` (reject requests with no
//! `Authorization` header, matching a real pod).

use axum::{
    extract::{Path, Query, Request, State},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const REST_BASE: &str = "/fscmRestApi/resources/11.13.18.05";

/// Behavioural knobs for the mock pod.
#[derive(Clone)]
pub struct MockConfig {
    /// Fixed per-request latency injected before handling — used to exercise
    /// the client timeout / circuit-breaker (Phase 5).
    pub latency_ms: u64,
    /// When true, requests without an `Authorization` header get `401`
    /// (matching a real pod).  Disable for quick `curl` probes.
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

#[derive(Clone)]
struct AppState {
    cfg: MockConfig,
    store: Arc<Mutex<Store>>,
}

/// In-memory pod state, seeded with Gaussian Technologies-flavoured master data.
struct Store {
    suppliers: Vec<Value>,
    items: Vec<Value>,
    journal_seq: u64,
    po_seq: u64,
}

impl Store {
    fn seeded() -> Self {
        Self {
            suppliers: vec![
                json!({ "SupplierId": 300100, "Supplier": "PT Sumber Daya Komputasi", "SupplierNumber": "S-300100", "Status": "ACTIVE" }),
                json!({ "SupplierId": 300101, "Supplier": "PT Nusantara Semikonduktor", "SupplierNumber": "S-300101", "Status": "ACTIVE" }),
                json!({ "SupplierId": 300102, "Supplier": "PT Andalan Cloud Indonesia", "SupplierNumber": "S-300102", "Status": "ACTIVE" }),
            ],
            items: vec![
                json!({ "ItemId": 100, "ItemNumber": "GT-EDGE-1000", "ItemDescription": "Edge AI Inference Module", "OrganizationId": 204 }),
                json!({ "ItemId": 101, "ItemNumber": "GT-SENS-2000", "ItemDescription": "Industrial IoT Sensor Array", "OrganizationId": 204 }),
            ],
            journal_seq: 90_000,
            po_seq: 700_000,
        }
    }
}

/// Build the mock Fusion REST router.  The returned `Router` can be served
/// directly (`axum::serve`) or mounted in an integration test.
pub fn router(cfg: MockConfig) -> Router {
    let state = AppState {
        cfg: cfg.clone(),
        store: Arc::new(Mutex::new(Store::seeded())),
    };
    Router::new()
        .route(REST_BASE, get(catalog_root))
        .route(&format!("{REST_BASE}/suppliers"), get(suppliers_search))
        .route(
            &format!("{REST_BASE}/suppliers/:id"),
            get(supplier_get).patch(supplier_patch),
        )
        .route(&format!("{REST_BASE}/itemsV2"), get(items_read))
        .route(&format!("{REST_BASE}/journalEntries"), post(journal_post))
        .route(&format!("{REST_BASE}/purchaseOrders"), post(po_post))
        .layer(middleware::from_fn_with_state(state.clone(), guard))
        // Registered AFTER the guard layer, so /healthz skips both auth and
        // latency injection — a clean liveness probe for Docker / k8s.
        .route("/healthz", get(healthz))
        .with_state(state)
}

/// No-auth liveness probe (skips the guard layer).
async fn healthz() -> impl IntoResponse {
    Json(json!({ "status": "ok", "pod": "fusion-mock" }))
}

/// Latency injection + auth gate, applied to every route.
async fn guard(
    State(st): State<AppState>,
    headers: HeaderMap,
    req: Request,
    next: Next,
) -> Response {
    if st.cfg.latency_ms > 0 {
        tokio::time::sleep(Duration::from_millis(st.cfg.latency_ms)).await;
    }
    if st.cfg.require_auth && !headers.contains_key("authorization") {
        return error_envelope(
            StatusCode::UNAUTHORIZED,
            "FND-AUTH-401",
            "Missing Authorization header",
        );
    }
    next.run(req).await
}

/// Fusion REST error envelope (`title` / `status` / `detail` / `o:errorCode`).
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

async fn catalog_root() -> impl IntoResponse {
    // The client only needs a 200 here for the reachability probe.
    Json(json!({ "items": [], "version": "11.13.18.05" }))
}

async fn suppliers_search(
    State(st): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let limit: usize = params
        .get("limit")
        .and_then(|s| s.parse().ok())
        .unwrap_or(25);
    // Fusion `q` looks like "Supplier LIKE '%PT%'"; pull the substring out.
    let needle = params
        .get("q")
        .and_then(|q| extract_like(q))
        .unwrap_or_default()
        .to_lowercase();
    let store = st.store.lock().unwrap();
    let items: Vec<Value> = store
        .suppliers
        .iter()
        .filter(|s| {
            needle.is_empty()
                || s.get("Supplier")
                    .and_then(|v| v.as_str())
                    .map(|n| n.to_lowercase().contains(&needle))
                    .unwrap_or(false)
        })
        .take(limit)
        .cloned()
        .collect();
    let count = items.len();
    Json(json!({ "items": items, "count": count, "hasMore": false, "limit": limit, "offset": 0 }))
}

async fn supplier_get(State(st): State<AppState>, Path(id): Path<String>) -> Response {
    let store = st.store.lock().unwrap();
    match store.suppliers.iter().find(|s| supplier_id_eq(s, &id)) {
        Some(s) => Json(s.clone()).into_response(),
        None => error_envelope(StatusCode::NOT_FOUND, "FND-404", "Supplier not found"),
    }
}

async fn supplier_patch(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let mut store = st.store.lock().unwrap();
    if let Some(s) = store.suppliers.iter_mut().find(|s| supplier_id_eq(s, &id)) {
        if let Some(status) = body.get("Status") {
            s["Status"] = status.clone();
        }
        return Json(s.clone()).into_response();
    }
    error_envelope(StatusCode::NOT_FOUND, "FND-404", "Supplier not found")
}

async fn items_read(State(st): State<AppState>) -> impl IntoResponse {
    let store = st.store.lock().unwrap();
    Json(json!({ "items": store.items.clone(), "count": store.items.len(), "hasMore": false }))
}

async fn journal_post(State(st): State<AppState>, Json(body): Json<Value>) -> Response {
    let mut store = st.store.lock().unwrap();
    store.journal_seq += 1;
    let id = store.journal_seq;
    let name = body
        .get("JournalEntryName")
        .or_else(|| body.get("JeBatchName"))
        .and_then(|v| v.as_str())
        .unwrap_or("KLB Journal Import")
        .to_string();
    (
        StatusCode::CREATED,
        Json(json!({
            "JournalEntryId": id,
            "JeHeaderId": id,
            "JournalEntryName": name,
            "Status": "POSTED",
            "PostedDate": "2026-05-30",
            "links": [{ "rel": "self", "href": format!("{REST_BASE}/journalEntries/{id}") }]
        })),
    )
        .into_response()
}

async fn po_post(State(st): State<AppState>, Json(body): Json<Value>) -> Response {
    let mut store = st.store.lock().unwrap();
    store.po_seq += 1;
    let id = store.po_seq;
    let supplier = body
        .get("Supplier")
        .and_then(|v| v.as_str())
        .unwrap_or("PT Sumber Daya Komputasi")
        .to_string();
    (
        StatusCode::CREATED,
        Json(json!({
            "POHeaderId": id,
            "OrderNumber": format!("GT-PO-{id}"),
            "Supplier": supplier,
            "Status": "OPEN",
            "CurrencyCode": body.get("CurrencyCode").and_then(|v| v.as_str()).unwrap_or("IDR"),
            "links": [{ "rel": "self", "href": format!("{REST_BASE}/purchaseOrders/{id}") }]
        })),
    )
        .into_response()
}

fn supplier_id_eq(s: &Value, id: &str) -> bool {
    s.get("SupplierId").map(|v| v.to_string()).as_deref() == Some(id)
}

/// Extract the search substring from a Fusion `q` filter, e.g.
/// `"Supplier LIKE '%PT%'"` → `"PT"`.
fn extract_like(q: &str) -> Option<String> {
    let start = q.find('\'')? + 1;
    let end = q.rfind('\'')?;
    if end <= start {
        return None;
    }
    Some(q[start..end].trim_matches('%').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_like_pulls_substring() {
        assert_eq!(extract_like("Supplier LIKE '%PT%'").as_deref(), Some("PT"));
        assert_eq!(extract_like("no quotes"), None);
    }

    #[test]
    fn supplier_id_eq_matches_numeric_id() {
        let s = json!({ "SupplierId": 300100 });
        assert!(supplier_id_eq(&s, "300100"));
        assert!(!supplier_id_eq(&s, "999"));
    }

    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt; // oneshot

    #[tokio::test]
    async fn healthz_is_unauthenticated() {
        let resp = router(MockConfig::default())
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn business_route_still_requires_auth() {
        let resp = router(MockConfig::default())
            .oneshot(
                Request::builder()
                    .uri("/fscmRestApi/resources/11.13.18.05/itemsV2")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}
