//! Oracle-Automate REST operation and table client abstraction.
//!
//! Brings together the design insights from the reference projects we
//! studied (paper §III + new comparative analysis in `docs/COMPARISON.md`):
//!
//! - **From `a reference REST-metadata-cache design`**: connection pooling, metadata
//!   caching, bulk metadata loads, version-aware behaviour.
//! - **From `a reference read-only ERP MCP design`**: schema-discovery-first
//!   tool design (`get_tables` → `get_columns` → `run_query`) and the
//!   read-only-by-default safety posture.
//! - **From `a reference guardrails design`**: constrained-enum tool parameters,
//!   project-aware tool calls, AGENTS.md guardrails.
//!
//! The crate is split into:
//! - `client`: the `ErpClient` trait + `MockErpClient` (offline)
//! - `credentials`: layered credential provider (env / keyring / file)
//! - `error`: structured REST operation error taxonomy mapped to MCP error codes
//! - `pool`: tokio-semaphore-based connection limiter
//! - `retry`: exponential-backoff helper + circuit-breaker primitive

pub mod erp_result;
pub mod client;
pub mod credentials;
pub mod error;
pub mod metadata_cache;
#[cfg(feature = "fusion")]
pub mod fusion;
pub mod pool;
pub mod retry;
pub mod transaction;

pub use erp_result::{ErpMessage, ErpSeverity, parse_erp_messages};
pub use metadata_cache::{CacheStats, MetadataCache};
#[cfg(feature = "fusion")]
pub use fusion::{FusionAuth, FusionConfig, FusionPartyClient, HttpFusionClient, Party};

pub use client::{
    BulkMetadata, MockErpClient, ReadTableRequest, ErpCallRequest, ErpOperationMeta,
    ErpOperationSummary, ErpParameter, ErpParamDirection, ErpSearchResult, ErpClient,
    SystemInfo, TableRow, TableStructure, TableField, MAX_ROWS_HARD_CAP,
};
pub use credentials::{
    Credentials, CredentialProvider, CredentialSource, EnvCredentialProvider,
    LayeredCredentialProvider, StaticCredentialProvider,
};
pub use error::{ErpError, ErpErrorCode, ErpResult};
pub use transaction::{execute_write_bapi, has_failure, WriteOutcome};
pub use pool::ConnectionPool;
pub use retry::{retry_with_backoff, BackoffPolicy, CircuitBreaker, CircuitState};
