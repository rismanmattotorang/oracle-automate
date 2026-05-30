//! Shared server context — held in an `Arc` and cloned into every tool.

use oracle_automate_adt::OicClient;
use oracle_automate_ingest::EmbeddingClient;
use oracle_automate_observability::AuditLog;
use oracle_automate_rag::{GraphEngine, RagEngine};
use oracle_automate_erp::{FusionPartyClient, MetadataCache, MockErpClient, ErpClient};
use std::sync::Arc;

pub struct ServerContext {
    pub rag: Arc<RagEngine>,
    pub graph: Arc<GraphEngine>,
    pub embedder: Arc<dyn EmbeddingClient>,
    /// The cache-decorated ErpClient used by every tool.  Identical
    /// trait surface to the underlying `MockErpClient` / future
    /// `NetweaverErpClient`; metadata reads are TTL-cached.
    pub erp_client: Arc<dyn ErpClient>,
    /// Direct handle to the metadata cache for the cache-stats /
    /// invalidate tools and the `oracle-cache://stats` resource.  `None`
    /// when caching is disabled via `--metadata-cache-ttl-secs=0`.
    pub metadata_cache: Option<Arc<MetadataCache<MockErpClient>>>,
    pub adt_client: Arc<dyn OicClient>,
    /// Oracle Fusion TCA party client (suppliers / customer accounts). `None`
    /// when no `ORACLE_FUSION_BASE_URL` is configured — the `oracle.party.*` tools then
    /// return a friendly "feature disabled" error instead of crashing.
    pub party_client: Option<Arc<FusionPartyClient>>,
    pub read_only: bool,
    pub agents_md: Option<String>,
    /// Append-only audit log for state-mutating tool calls (SOX / GDPR
    /// evidence).  Arguments are redacted by `AuditLog::record`.
    pub audit: Arc<AuditLog>,
    /// ERP environment identity (host/client) recorded on each audit entry.
    pub erp_system: Option<String>,
}
