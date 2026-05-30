//! Oracle-Automate ABAP Development Tools (ADT) client.
//!
//! Brings the design ideas from `mario-andreschak/mcp-abap-adt` (clean
//! object-type-specific read-only tools over the ADT REST API) and
//! `fr0ster/mcp-abap-adt` (CRUD breadth, RAP-first, multi-transport,
//! destination model, "AI pairing, not vibing" safety stance) into a
//! Rust trait-based architecture that matches the rest of Oracle-Automate.
//!
//! The crate is split into:
//!   - `types`    — request/response shapes shared by every backend
//!   - `client`   — the `AdtClient` async trait
//!   - `mock`     — offline `MockAdtClient` with realistic ABAP fixtures
//!   - `error`    — structured `AdtError` taxonomy mapped to MCP codes
//!   - `destination` — destination model (name, base URL, auth method)
//!   - `http` (feature `http`) — `HttpOicClient` against live Oracle OIC / Fusion REST
//!
//! Read-only-by-default safety is enforced by the `AdtCallContext::read_only`
//! flag, mirroring the `oracle-automate-rfc` pattern.

pub mod client;
pub mod destination;
pub mod error;
pub mod mock;
pub mod types;

#[cfg(feature = "http")]
pub mod http;

pub use client::{AdtCallContext, AdtClient};
pub use destination::{AdtAuth, AdtDestination};
pub use error::{AdtError, AdtErrorCode, AdtResult};
pub use mock::MockAdtClient;
pub use types::{
    ActivationOutcome, ActivationRequest, AdtSearchHit, AdtSearchRequest, CdsView,
    OracleArtifactKind, PackageContents, PackageMember, ProgramSource, TableRow, WhereUsedHit,
    WhereUsedRequest, MAX_TABLE_ROWS,
};

#[cfg(feature = "http")]
pub use http::HttpOicClient;
