//! Library surface for the Oracle-Automate server binary.
//!
//! Exposes the same builder functions the binary uses internally so
//! integration tests can construct a server in-process — no subprocess,
//! no cold-start seeding cost.

pub mod context;
pub mod prompts;
pub mod resources;
pub mod seed;
pub mod tools;

use std::sync::Arc;
use std::time::Duration;

use mcp_server::Server;
use oracle_automate_adt::{OicClient, OicDestination, MockOicClient};
use oracle_automate_graph::InMemoryGraph;
use oracle_automate_ingest::{EmbeddingClient, MockEmbedder};
use oracle_automate_kb::{InMemoryKb, KnowledgeStore};
use oracle_automate_observability::{AuditLog, JsonStderrSink};
use oracle_automate_rag::{GraphEngine, MockReranker, RagEngine};
use oracle_automate_erp::{MetadataCache, MockErpClient, ErpClient};
use oracle_automate_skills::SkillRegistry;

pub use context::ServerContext;

/// How the test harness wants its context built.
#[derive(Clone)]
pub struct TestServerOptions {
    pub read_only: bool,
    pub metadata_cache_ttl: Duration,
    pub seed_kb: bool,
    pub embedding_dim: usize,
    pub agents_md: Option<String>,
}

impl Default for TestServerOptions {
    fn default() -> Self {
        Self {
            read_only: true,
            metadata_cache_ttl: Duration::from_secs(300),
            seed_kb: false,
            embedding_dim: 64,
            agents_md: None,
        }
    }
}

/// Build a ready-to-run `Server` for integration tests.  Identical wiring
/// to `main.rs`, minus the network transport setup and (optionally) the
/// KB seed step.
pub async fn build_test_server(
    opts: TestServerOptions,
) -> (Server, Arc<ServerContext>) {
    let store: Arc<dyn KnowledgeStore> = Arc::new(InMemoryKb::new());
    let embedder: Arc<dyn EmbeddingClient> = Arc::new(MockEmbedder::new(opts.embedding_dim));
    if opts.seed_kb {
        seed::populate_with_embeddings(&store, embedder.as_ref())
            .await
            .expect("seed");
    }
    let rag = Arc::new(RagEngine::new(store.clone()).with_reranker(Arc::new(MockReranker::new())));

    let kg = Arc::new(InMemoryGraph::with_demo_corpus());
    let graph_engine = Arc::new(GraphEngine::new(kg));

    let inner = MockErpClient::new(4, serde_json::json!({}));
    let metadata_cache = MetadataCache::new(inner.clone(), opts.metadata_cache_ttl);
    let erp_client: Arc<dyn ErpClient> = metadata_cache.clone();

    let adt_destination = OicDestination::mock("test".to_string());
    let adt_client: Arc<dyn OicClient> = MockOicClient::new(adt_destination);

    let ctx = Arc::new(ServerContext {
        rag,
        graph: graph_engine,
        embedder,
        erp_client,
        metadata_cache: Some(metadata_cache),
        adt_client,
        party_client: None,
        read_only: opts.read_only,
        agents_md: opts.agents_md.clone(),
        audit: Arc::new(AuditLog::new(Arc::new(JsonStderrSink::new()))),
        erp_system: Some("MOCK/100".into()),
    });

    let policy = if opts.read_only {
        mcp_server::ExposurePolicy::ReadOnlyOnly
    } else {
        mcp_server::ExposurePolicy::All
    };
    let mut builder = Server::builder("oracle-automate-test-server", env!("CARGO_PKG_VERSION"))
        .exposure(policy)
        .instructions("integration test".to_string());

    for desc in tools::rag_tools(&ctx) { builder = builder.tool(desc); }
    for desc in tools::sap_tools(&ctx) { builder = builder.tool(desc); }
    for desc in tools::adt_tools(&ctx) { builder = builder.tool(desc); }
    for desc in tools::graph_tools(&ctx) { builder = builder.tool(desc); }
    for desc in tools::workflow_tools(&ctx) { builder = builder.tool(desc); }
    for desc in resources::all(&ctx) { builder = builder.resource(desc); }
    let skills = SkillRegistry::new();
    for desc in prompts::all(&skills) { builder = builder.prompt(desc); }
    builder = register_completers(builder);

    (builder.build(), ctx)
}

/// Register `completion/complete` providers for the prompt arguments
/// that benefit most from autocomplete in MCP clients (Inspector, Claude
/// Desktop, our own web UI Skill Lab).
///
/// MCP 2025-06-18 client utility: each completer takes the typed prefix
/// and returns matching candidates.  Returning `[]` is spec-compliant.
pub fn register_completers(builder: mcp_server::ServerBuilder) -> mcp_server::ServerBuilder {
    let starts_with = |options: &[&'static str], prefix: &str| -> Vec<String> {
        let p = prefix.to_ascii_lowercase();
        options.iter()
            .filter(|o| o.to_ascii_lowercase().starts_with(&p))
            .map(|o| (*o).to_string())
            .collect()
    };
    builder
        // SoD audit: scope enum.
        .completer("oracle.skill.security_sod_audit", "scope", move |prefix, _| {
            starts_with(&["user", "role", "system"], prefix)
        })
        // Custom-code review: artifact kind enum.
        .completer("oracle.skill.custom_code_review", "kind", move |prefix, _| {
            starts_with(&["integration", "groovy_script", "connection", "bip_report"], prefix)
        })
        // Analytics migration: target platform dropdown.
        .completer("oracle.skill.analytics_migration", "target_release", move |prefix, _| {
            starts_with(&[
                "Fusion Analytics Warehouse",
                "Oracle Analytics Cloud",
                "Autonomous Data Warehouse",
                "OTBI",
                "BICC extract",
            ], prefix)
        })
}
