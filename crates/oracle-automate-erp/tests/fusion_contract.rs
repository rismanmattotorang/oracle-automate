//! `HttpFusionClient` / `FusionPartyClient` REST contract tests.
//!
//! Spins an in-process axum mock of the Oracle Fusion Cloud ERP REST API
//! (`/fscmRestApi/resources/11.13.18.05/...`) and exercises the **live**
//! clients against it — the same `reqwest` path that will run against a real
//! Gaussian Technologies pod, minus the network. The point (Phase 3 of
//! `docs/PRODUCTION_READINESS.md`) is to pin the parse/dispatch contract for
//! realistic Fusion response shapes so the eventual dev-pod run (Phase 4) is a
//! smoke test, not a debug session.
//!
//! Covered:
//!   1. TCA supplier collection + pagination metadata → `Vec<Party>`.
//!   2. Customer-account field fallback (`PartyId`/`PartyName`) → `Party`.
//!   3. `404` → `ErpError::NotFound`.
//!   4. `call_operation` REST dispatch → `{ http_status, outputs }` envelope.
//!   5. FND/REST error envelope (`400` + `o:errorCode`) surfaced, not dropped.
//!   6. `system_info` reachability against the REST catalog root.
//!
//! The *live* (real-pod) counterpart is the env-gated path: set
//! `ORACLE_FUSION_BASE_URL` (+ `ORACLE_FUSION_AUTH`/token) and the server wires
//! `HttpFusionClient` instead of the mock. These contract tests run offline and
//! unconditionally in CI.

#![cfg(feature = "fusion")]

use axum::{extract::Path, http::StatusCode, response::IntoResponse, routing::get, Json, Router};
use oracle_automate_erp::client::{ErpCallRequest, ErpClient, MockErpClient};
use oracle_automate_erp::error::ErpError;
use oracle_automate_erp::fusion::{FusionAuth, FusionConfig, FusionPartyClient, HttpFusionClient};
use serde_json::{json, Value};
use std::net::SocketAddr;

const REST_BASE: &str = "/fscmRestApi/resources/11.13.18.05";

/// In-process mock of the Fusion REST surface the clients call.
async fn spawn_mock() -> SocketAddr {
    let app = Router::new()
        // REST catalog root — `system_info` reachability probe.
        .route(REST_BASE, get(|| async { Json(json!({ "items": [] })) }))
        // Supplier search collection — realistic paginated TCA shape.
        .route(
            &format!("{REST_BASE}/suppliers"),
            get(|| async {
                Json(json!({
                    "items": [
                        { "SupplierId": 300100, "Supplier": "PT Sumber Daya Komputasi",
                          "SupplierNumber": "S-300100", "Status": "ACTIVE",
                          "links": [{ "rel": "self", "href": "https://pod/.../suppliers/300100" }] },
                        { "SupplierId": 300101, "Supplier": "PT Nusantara Semikonduktor",
                          "SupplierNumber": "S-300101", "Status": "ACTIVE" }
                    ],
                    "count": 2, "hasMore": true, "limit": 25, "offset": 0,
                    "links": [{ "rel": "self", "href": "https://pod/.../suppliers" }]
                }))
            }),
        )
        // Single supplier by id — `404` for "999", else a customer-account
        // shape exercising the `PartyId`/`PartyName` fallback in `party_from_obj`.
        .route(
            &format!("{REST_BASE}/suppliers/:id"),
            get(|Path(id): Path<String>| async move {
                if id == "999" {
                    return (
                        StatusCode::NOT_FOUND,
                        Json(json!({ "title": "Not Found", "status": 404 })),
                    )
                        .into_response();
                }
                Json(json!({
                    "PartyId": 555000,
                    "PartyName": "PT Andalan Cloud Indonesia",
                    "Status": "ACTIVE"
                }))
                .into_response()
            }),
        )
        // `itemsV2` — `call_operation` success path (`fusion.scm.itemsV2.get`).
        .route(
            &format!("{REST_BASE}/itemsV2"),
            get(|| async {
                Json(json!({
                    "items": [{ "ItemId": 100, "ItemNumber": "GT-EDGE-1000",
                                "ItemDescription": "Edge AI Inference Module" }],
                    "count": 1, "hasMore": false
                }))
            }),
        )
        // `system` resource — `call_operation` error-envelope path
        // (`fusion.system.serverInformation` → `400` FND/REST error envelope).
        .route(
            &format!("{REST_BASE}/system"),
            get(|| async {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "title": "Bad Request",
                        "status": 400,
                        "detail": "The request could not be understood by the server.",
                        "o:errorCode": "FND-12345",
                        "o:errorDetails": [
                            { "detail": "Invalid filter expression", "o:errorCode": "FND-67890" }
                        ]
                    })),
                )
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    addr
}

fn party_client(addr: SocketAddr) -> FusionPartyClient {
    let cfg = FusionConfig::new(
        format!("http://{addr}"),
        FusionAuth::Bearer("test-token".into()),
    );
    FusionPartyClient::new(cfg).unwrap()
}

fn erp_client(addr: SocketAddr) -> HttpFusionClient {
    let cfg = FusionConfig::new(
        format!("http://{addr}"),
        FusionAuth::Bearer("test-token".into()),
    );
    // Curated catalogue backs the read-only gate + metadata.
    let catalogue = MockErpClient::new(2, json!({}));
    HttpFusionClient::new(cfg, catalogue).unwrap()
}

#[tokio::test]
async fn supplier_search_parses_paginated_collection() {
    let addr = spawn_mock().await;
    let parties = party_client(addr).search_parties("PT", 25).await.unwrap();
    // Pagination metadata (count/hasMore/limit/offset/links) must not leak into
    // the item count — only `items` are parties.
    assert_eq!(parties.len(), 2);
    assert_eq!(parties[0].id, "300100");
    assert_eq!(parties[0].name, "PT Sumber Daya Komputasi");
    assert_eq!(parties[0].party_number.as_deref(), Some("S-300100"));
    assert_eq!(parties[0].status.as_deref(), Some("ACTIVE"));
    assert_eq!(parties[1].id, "300101");
}

#[tokio::test]
async fn get_party_uses_party_id_name_fallback() {
    let addr = spawn_mock().await;
    let p = party_client(addr).get_party("555000").await.unwrap();
    assert_eq!(p.id, "555000");
    assert_eq!(p.name, "PT Andalan Cloud Indonesia");
}

#[tokio::test]
async fn get_party_404_maps_to_not_found() {
    let addr = spawn_mock().await;
    let err = party_client(addr).get_party("999").await.unwrap_err();
    assert!(matches!(err, ErpError::NotFound(id) if id == "999"));
}

#[tokio::test]
async fn call_operation_returns_envelope_on_success() {
    let addr = spawn_mock().await;
    let req = ErpCallRequest {
        function: "fusion.scm.itemsV2.get".into(),
        parameters: json!({}),
        timeout_ms: 5000,
        require_read_only_safe: true,
    };
    let out: Value = erp_client(addr).call_operation(req, true).await.unwrap();
    assert_eq!(out["http_status"], 200);
    assert_eq!(out["function"], "fusion.scm.itemsV2.get");
    assert!(out["outputs"]["items"].is_array());
    assert_eq!(out["outputs"]["items"][0]["ItemNumber"], "GT-EDGE-1000");
}

#[tokio::test]
async fn call_operation_surfaces_fnd_error_envelope() {
    let addr = spawn_mock().await;
    let req = ErpCallRequest {
        function: "fusion.system.serverInformation".into(),
        parameters: json!({}),
        timeout_ms: 5000,
        require_read_only_safe: true,
    };
    let out: Value = erp_client(addr).call_operation(req, true).await.unwrap();
    // The FND/REST error envelope is surfaced (status + o:errorCode), not dropped.
    assert_eq!(out["http_status"], 400);
    assert_eq!(out["outputs"]["o:errorCode"], "FND-12345");
}

#[tokio::test]
async fn system_info_reaches_rest_catalog_root() {
    let addr = spawn_mock().await;
    let info = erp_client(addr).system_info().await.unwrap();
    assert_eq!(info.client, "LIVE");
    assert!(info.release.contains("Oracle Fusion"));
}
