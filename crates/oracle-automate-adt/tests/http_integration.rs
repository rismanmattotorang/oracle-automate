//! HttpOicClient integration-test suite.
//!
//! Spins up an axum-based mock Oracle Integration Cloud (OIC) / Fusion REST
//! server in-process and exercises `HttpOicClient` against it.  Asserts:
//!
//!   1. **URL patterns** match the OIC / Fusion REST conventions
//!      (`/ic/api/integration/v1/...`).
//!   2. **Auth** — an HTTP Basic / Bearer header is emitted.
//!   3. **JSON parsers** — integration, search, and usages payloads parse.
//!   4. **Error mapping** — 404 → NotFound, table preview → DataPreviewBlocked.
//!   5. **Read-only-mode safety gate** — activate blocked when
//!      ctx.read_only = true.

#![cfg(feature = "http")]

use axum::{
    extract::Path,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use oracle_automate_adt::{
    ActivationRequest, HttpOicClient, OicAuth, OicCallContext, OicClient, OicDestination, OicError,
    OicSearchRequest, OracleArtifactKind, WhereUsedRequest,
};
use serde_json::json;
use std::net::SocketAddr;

async fn spawn_mock() -> SocketAddr {
    let app = Router::new()
        .route(
            "/ic/api/integration/v1/integrations/:name",
            get(|Path(name): Path<String>| async move {
                if name == "MISSING" {
                    return (StatusCode::NOT_FOUND, Json(json!({"error": "not found"}))).into_response();
                }
                Json(json!({
                    "code": "<integration name=\"GT_GL_JOURNAL_IMPORT\"/>",
                    "description": "GL journal FBDI import",
                    "status": "ACTIVATED",
                    "project": "GT_FINANCE_INTEGRATIONS"
                }))
                .into_response()
            })
            // Activation is POST on the same path (?integrationInstruction=activate).
            .post(|Path(_name): Path<String>| async { StatusCode::OK }),
        )
        .route(
            "/ic/api/integration/v1/integrations",
            get(|headers: HeaderMap| async move {
                // Confirm the Authorization header was emitted.
                assert!(headers.contains_key("authorization"), "missing Authorization header");
                Json(json!({ "items": [
                    { "code": "GT_GL_JOURNAL_IMPORT", "description": "GL journal FBDI import", "project": "GT_FINANCE_INTEGRATIONS" },
                    { "code": "GT_PO_RECEIPT_SYNC", "description": "Receiving sync", "project": "GT_FINANCE_INTEGRATIONS" }
                ]}))
            }),
        )
        .route(
            "/ic/api/integration/v1/connections/:name/usages",
            get(|Path(_name): Path<String>| async {
                Json(json!({ "items": [
                    { "code": "GT_GL_JOURNAL_IMPORT", "usage": "invoke activity importJournals" }
                ]}))
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    addr
}

fn client(addr: SocketAddr) -> HttpOicClient {
    let dest = OicDestination {
        name: "test".into(),
        base_url: format!("http://{addr}"),
        client: "LIVE".into(),
        language: "EN".into(),
        auth: OicAuth::Basic {
            user: "u".into(),
            password: "p".into(),
        },
        timeout_ms: 30_000,
    };
    HttpOicClient::new(dest).unwrap()
}

#[tokio::test]
async fn get_program_fetches_and_projects_integration() {
    let addr = spawn_mock().await;
    let c = client(addr);
    let p = c.get_integration("GT_GL_JOURNAL_IMPORT").await.unwrap();
    assert_eq!(p.name, "GT_GL_JOURNAL_IMPORT");
    assert_eq!(p.kind, OracleArtifactKind::Integration);
    assert!(p.source.contains("GT_GL_JOURNAL_IMPORT"));
    assert!(p.active);
    assert_eq!(p.package.as_deref(), Some("GT_FINANCE_INTEGRATIONS"));
}

#[tokio::test]
async fn missing_artifact_maps_to_not_found() {
    let addr = spawn_mock().await;
    let c = client(addr);
    let err = c.get_integration("MISSING").await.unwrap_err();
    assert!(matches!(err, OicError::NotFound { .. }), "got {err:?}");
}

#[tokio::test]
async fn search_parses_items_and_emits_auth() {
    let addr = spawn_mock().await;
    let c = client(addr);
    let hits = c
        .search(OicSearchRequest {
            query: "journal".into(),
            kind: Some(OracleArtifactKind::Integration),
            max_results: 10,
        })
        .await
        .unwrap();
    assert_eq!(hits.len(), 2);
    assert!(hits.iter().any(|h| h.name == "GT_GL_JOURNAL_IMPORT"));
}

#[tokio::test]
async fn where_used_parses_connection_usages() {
    let addr = spawn_mock().await;
    let c = client(addr);
    let hits = c
        .where_used(WhereUsedRequest {
            name: "GT_FUSION_ERP_REST".into(),
            kind: OracleArtifactKind::Connection,
        })
        .await
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].object, "GT_GL_JOURNAL_IMPORT");
    assert_eq!(hits[0].usage, "invoke");
}

#[tokio::test]
async fn table_preview_is_blocked_directs_to_bip() {
    let addr = spawn_mock().await;
    let c = client(addr);
    let err = c.preview_data("XLA_AE_LINES", 10).await.unwrap_err();
    assert!(
        matches!(err, OicError::DataPreviewBlocked(_)),
        "got {err:?}"
    );
}

#[tokio::test]
async fn activate_blocked_in_read_only_mode() {
    let addr = spawn_mock().await;
    let c = client(addr);
    let err = c
        .activate(
            ActivationRequest {
                name: "GT_GL_JOURNAL_IMPORT".into(),
                kind: OracleArtifactKind::Integration,
            },
            OicCallContext { read_only: true },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, OicError::PermissionDenied(_)), "got {err:?}");
}

#[tokio::test]
async fn activate_succeeds_when_writes_enabled() {
    let addr = spawn_mock().await;
    let c = client(addr);
    let outcome = c
        .activate(
            ActivationRequest {
                name: "GT_GL_JOURNAL_IMPORT".into(),
                kind: OracleArtifactKind::Integration,
            },
            OicCallContext { read_only: false },
        )
        .await
        .unwrap();
    assert!(outcome.activated, "messages: {:?}", outcome.messages);
}
