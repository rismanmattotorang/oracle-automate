//! OicClient async trait.
//!
//! Phase 2 finalisation: every method that modifies state takes an
//! `OicCallContext` carrying the server's read-only-mode flag.  Mock and
//! HTTP backends both honour the flag, refusing writes when set.

use crate::destination::OicDestination;
use crate::error::OicResult;
use crate::types::{
    ActivationOutcome, ActivationRequest, OicSearchHit, OicSearchRequest, CdsView,
    PackageContents, ProgramSource, TableRow, WhereUsedHit, WhereUsedRequest,
};
use async_trait::async_trait;

/// Per-call security / observability context.
#[derive(Debug, Clone, Copy, Default)]
pub struct OicCallContext {
    pub read_only: bool,
}

#[async_trait]
pub trait OicClient: Send + Sync {
    /// Destination metadata (redacted form is safe for logs).
    fn destination(&self) -> &OicDestination;

    // --- Read-only ---------------------------------------------------------

    async fn get_integration(&self, name: &str) -> OicResult<ProgramSource>;
    async fn get_groovy_script(&self, name: &str) -> OicResult<ProgramSource>;
    async fn get_connection(&self, name: &str) -> OicResult<ProgramSource>;
    async fn get_lookup(&self, name: &str) -> OicResult<ProgramSource>;
    async fn get_ess_job(&self, group: &str, name: &str) -> OicResult<ProgramSource>;
    async fn get_project_contents(&self, package: &str) -> OicResult<PackageContents>;
    async fn get_bip_report(&self, name: &str) -> OicResult<CdsView>;

    async fn search(&self, request: OicSearchRequest) -> OicResult<Vec<OicSearchHit>>;
    async fn where_used(&self, request: WhereUsedRequest) -> OicResult<Vec<WhereUsedHit>>;

    /// Read table contents through the ADT Data Preview API.  On SAP BTP
    /// this is blocked at the backend; the call returns
    /// `OicError::DataPreviewBlocked` so the agent can fall back to RFC
    /// (`sap.table.read`).
    async fn preview_data(&self, table: &str, max_rows: usize) -> OicResult<Vec<TableRow>>;

    // --- Write (gated by `ctx.read_only`) ---------------------------------

    async fn activate(&self, request: ActivationRequest, ctx: OicCallContext) -> OicResult<ActivationOutcome>;
}
