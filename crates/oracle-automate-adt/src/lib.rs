//! Oracle-Automate OIC/custom-code Development Tools (ADT) client.
//!
//! Brings the design ideas from `a reference artifact-retrieval design` (clean
//! object-type-specific read-only tools over the ADT REST API) and
//! `a reference exposure-policy design` (CRUD breadth, RAP-first, multi-transport,
//! destination model, "AI pairing, not vibing" safety stance) into a
//! Rust trait-based architecture that matches the rest of Oracle-Automate.
//!
//! The crate is split into:
//!   - `types`    — request/response shapes shared by every backend
//!   - `client`   — the `OicClient` async trait
//!   - `mock`     — offline `MockOicClient` with realistic OIC/custom-code fixtures
//!   - `error`    — structured `OicError` taxonomy mapped to MCP codes
//!   - `destination` — destination model (name, base URL, auth method)
//!   - `http` (feature `http`) — `HttpOicClient` against live Oracle OIC / Fusion REST
//!
//! Read-only-by-default safety is enforced by the `OicCallContext::read_only`
//! flag, mirroring the `oracle-automate-rfc` pattern.

pub mod client;
pub mod destination;
pub mod error;
pub mod mock;
pub mod types;

#[cfg(feature = "http")]
pub mod http;

pub use client::{OicCallContext, OicClient};
pub use destination::{OicAuth, OicDestination};
pub use error::{OicError, OicErrorCode, OicResult};
pub use mock::MockOicClient;
pub use types::{
    ActivationOutcome, ActivationRequest, CdsView, OicSearchHit, OicSearchRequest,
    OracleArtifactKind, PackageContents, PackageMember, ProgramSource, TableRow, WhereUsedHit,
    WhereUsedRequest, MAX_TABLE_ROWS,
};

#[cfg(feature = "http")]
pub use http::HttpOicClient;
