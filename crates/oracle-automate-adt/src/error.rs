//! Structured ADT error taxonomy.
//!
//! ADT-specific failure modes that the two reference projects surface as
//! either text strings or generic HTTP errors are split into typed
//! variants here so the MCP layer can map each to its appropriate
//! JSON-RPC error code (paper §IV-I).

use thiserror::Error;

pub type OicResult<T> = std::result::Result<T, OicError>;

/// Structured error codes for SAP ADT operations.  Numeric values are
/// stable across releases; `#[non_exhaustive]` lets us add new variants
/// without breaking downstream matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum OicErrorCode {
    // Transient (-32100..-32199)
    Timeout = -32160,
    DestinationDown = -32161,
    CsrfRefresh = -32162,
    RateLimited = -32163,

    // Permanent (-32200..-32299)
    AuthFailed = -32260,
    NotFound = -32261,
    Forbidden = -32262,
    InvalidObjectName = -32263,
    InactiveObject = -32264,
    /// ADT data preview blocked on BTP-hosted systems (fr0ster note).
    DataPreviewBlocked = -32265,
    PermissionDenied = -32266,
    /// Object exists but in a locked state (transport not released, locked
    /// by another user, etc.).
    Locked = -32267,
    /// Server bug / programming error.  Never retried.
    Internal = -32298,
}

impl OicErrorCode {
    pub fn as_i32(self) -> i32 {
        self as i32
    }
    pub fn is_transient(self) -> bool {
        let v = self as i32;
        (-32199..=-32100).contains(&v)
    }
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum OicError {
    #[error("ADT timeout after {timeout_ms} ms")]
    Timeout { timeout_ms: u64 },

    #[error("ADT destination '{destination}' unreachable: {reason}")]
    DestinationDown { destination: String, reason: String },

    #[error("CSRF token refresh required")]
    CsrfRefresh,

    #[error("rate limited; retry after {retry_after_ms} ms")]
    RateLimited { retry_after_ms: u64 },

    #[error("authentication failed: {0}")]
    AuthFailed(String),

    #[error("object not found: {kind} '{name}'")]
    NotFound { kind: String, name: String },

    #[error("forbidden: {0}")]
    Forbidden(String),

    #[error("invalid object name '{0}'")]
    InvalidObjectName(String),

    #[error("object is inactive: {0}")]
    InactiveObject(String),

    #[error("data preview blocked: {0}")]
    DataPreviewBlocked(String),

    #[error("permission denied: {0}")]
    PermissionDenied(String),

    #[error("object locked: {0}")]
    Locked(String),

    #[error("internal: {0}")]
    Internal(String),
}

impl OicError {
    pub fn code(&self) -> OicErrorCode {
        match self {
            OicError::Timeout { .. } => OicErrorCode::Timeout,
            OicError::DestinationDown { .. } => OicErrorCode::DestinationDown,
            OicError::CsrfRefresh => OicErrorCode::CsrfRefresh,
            OicError::RateLimited { .. } => OicErrorCode::RateLimited,
            OicError::AuthFailed(_) => OicErrorCode::AuthFailed,
            OicError::NotFound { .. } => OicErrorCode::NotFound,
            OicError::Forbidden(_) => OicErrorCode::Forbidden,
            OicError::InvalidObjectName(_) => OicErrorCode::InvalidObjectName,
            OicError::InactiveObject(_) => OicErrorCode::InactiveObject,
            OicError::DataPreviewBlocked(_) => OicErrorCode::DataPreviewBlocked,
            OicError::PermissionDenied(_) => OicErrorCode::PermissionDenied,
            OicError::Locked(_) => OicErrorCode::Locked,
            // Internal errors are programmer bugs, not transient — must
            // NOT be retried.  See Phase 7 code-review pass.
            OicError::Internal(_) => OicErrorCode::Internal,
        }
    }

    pub fn is_transient(&self) -> bool {
        self.code().is_transient()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adt_internal_is_permanent() {
        let e = OicError::Internal("bug".into());
        assert!(
            !e.is_transient(),
            "OicError::Internal must be permanent to prevent retry-loop on programmer bugs"
        );
    }

    #[test]
    fn adt_transient_kinds_are_classified_correctly() {
        for code in [
            OicErrorCode::Timeout,
            OicErrorCode::DestinationDown,
            OicErrorCode::CsrfRefresh,
            OicErrorCode::RateLimited,
        ] {
            assert!(code.is_transient(), "{code:?} should be transient");
        }
        for code in [
            OicErrorCode::AuthFailed,
            OicErrorCode::NotFound,
            OicErrorCode::Forbidden,
            OicErrorCode::Internal,
            OicErrorCode::DataPreviewBlocked,
            OicErrorCode::Locked,
        ] {
            assert!(!code.is_transient(), "{code:?} should NOT be transient");
        }
    }
}
