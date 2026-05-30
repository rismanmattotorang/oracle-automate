//! End-to-end tests against the standalone **mock OIC pod**
//! (`oracle-automate-oic-mock`).
//!
//! These drive the *real* `HttpOicClient` over the same `reqwest` path that
//! hits a real Oracle Integration Cloud / Fusion endpoint — proving the
//! `oracle.oic.*` custom-code flows (artifact retrieval, search, where-used,
//! gated activation) work end-to-end now, with no Oracle access. When a real
//! pod is provisioned, only the destination `base_url` changes.
//!
//! Also covers Phase-5 resilience: an injected-latency pod trips the client
//! request timeout, mapping to `OicError::DestinationDown`.

#![cfg(feature = "http")]

use oracle_automate_adt::{
    ActivationRequest, HttpOicClient, OicAuth, OicCallContext, OicClient, OicDestination, OicError,
    OicSearchRequest, OracleArtifactKind, WhereUsedRequest,
};
use oracle_automate_oic_mock::{router, MockConfig};
use std::net::SocketAddr;

async fn spawn_pod(cfg: MockConfig) -> SocketAddr {
    let app = router(cfg);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    addr
}

fn client(addr: SocketAddr, timeout_ms: u64) -> HttpOicClient {
    let dest = OicDestination {
        name: "mock-oic".into(),
        base_url: format!("http://{addr}"),
        client: "100".into(),
        language: "EN".into(),
        auth: OicAuth::Basic {
            user: "demo".into(),
            password: "demo".into(),
        },
        timeout_ms,
    };
    HttpOicClient::new(dest).unwrap()
}

#[tokio::test]
async fn get_integration_artifact() {
    let addr = spawn_pod(MockConfig::default()).await;
    let p = client(addr, 30_000)
        .get_integration("GT_GL_JOURNAL_IMPORT")
        .await
        .unwrap();
    assert_eq!(p.name, "GT_GL_JOURNAL_IMPORT");
    assert_eq!(p.kind, OracleArtifactKind::Integration);
    assert!(p.source.contains("GT_GL_JOURNAL_IMPORT"));
    assert!(p.active);
    assert_eq!(p.package.as_deref(), Some("GT_FINANCE_INTEGRATIONS"));
}

#[tokio::test]
async fn get_groovy_and_bip_report() {
    let addr = spawn_pod(MockConfig::default()).await;
    let c = client(addr, 30_000);
    let g = c.get_groovy_script("GT_INVOICE_HOLD_RULE").await.unwrap();
    assert!(g.source.contains("ValidationException"));
    let r = c.get_bip_report("GT_GL_TRIAL_BALANCE").await.unwrap();
    assert!(r.source.contains("GL_JE_LINES"));
}

#[tokio::test]
async fn missing_artifact_maps_to_not_found() {
    let addr = spawn_pod(MockConfig::default()).await;
    let err = client(addr, 30_000)
        .get_integration("MISSING")
        .await
        .unwrap_err();
    assert!(matches!(err, OicError::NotFound { .. }), "got {err:?}");
}

#[tokio::test]
async fn search_filters_by_query() {
    let addr = spawn_pod(MockConfig::default()).await;
    let hits = client(addr, 30_000)
        .search(OicSearchRequest {
            query: "journal".into(),
            kind: Some(OracleArtifactKind::Integration),
            max_results: 10,
        })
        .await
        .unwrap();
    assert!(hits.iter().any(|h| h.name == "GT_GL_JOURNAL_IMPORT"));
    assert!(!hits.iter().any(|h| h.name == "GT_PO_RECEIPT_SYNC"));
}

#[tokio::test]
async fn where_used_lists_dependents() {
    let addr = spawn_pod(MockConfig::default()).await;
    let hits = client(addr, 30_000)
        .where_used(WhereUsedRequest {
            name: "GT_FUSION_ERP_REST".into(),
            kind: OracleArtifactKind::Connection,
        })
        .await
        .unwrap();
    assert!(hits.iter().any(|h| h.object == "GT_GL_JOURNAL_IMPORT"));
}

#[tokio::test]
async fn activate_blocked_in_read_only_mode() {
    let addr = spawn_pod(MockConfig::default()).await;
    let err = client(addr, 30_000)
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
async fn gated_activate_succeeds_when_writes_enabled() {
    let addr = spawn_pod(MockConfig::default()).await;
    let outcome = client(addr, 30_000)
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

// Phase 5: a slow pod must trip the client timeout, not hang forever.
#[tokio::test]
async fn slow_pod_trips_client_timeout() {
    let addr = spawn_pod(MockConfig {
        latency_ms: 500,
        require_auth: true,
    })
    .await;
    let err = client(addr, 100)
        .get_integration("GT_GL_JOURNAL_IMPORT")
        .await
        .unwrap_err();
    assert!(
        matches!(err, OicError::DestinationDown { .. }),
        "a hung pod must surface as DestinationDown, got {err:?}"
    );
}
