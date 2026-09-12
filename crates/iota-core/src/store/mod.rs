//! SQLite store layer.
//!
//! - [`cache`]         — [`CacheStore`]: execution lifecycle
//! - [`approvals`]     — [`ApprovalStore`]: tool approval events and policy
//! - [`ledger`]        — [`SessionLedger`]: sessions, backend sessions, turns, handoffs
//! - [`observability`] — [`ObservabilityStore`]: token usage events
//!
//! ## Write failure classification
//!
//! Stores fall into two classes, and callers must treat their failures
//! differently:
//!
//! - **Authoritative** ([`ledger`], [`approvals`]): the record *is* the state
//!   the user asked for. A failed write must propagate as an error.
//! - **Auxiliary** ([`cache`], [`observability`]): derived telemetry that the
//!   request does not depend on succeeding. A failed write must not fail the
//!   user's turn; it is downgraded to a structured `degraded` event via
//!   [`degraded_write`] and counted in metrics.
//!
//! Use [`degraded_write`] for the auxiliary class so every downgrade carries
//! the store name, error category, and the execution/session it belonged to.

pub mod approvals;
pub mod cache;
pub mod db;
pub mod observability;

pub mod ledger;
pub mod migrations;

use crate::telemetry::metrics;

/// Classifies a store write failure for the `degraded` event payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCategory {
    /// The database rejected the write (constraint, type, or schema mismatch).
    Constraint,
    /// The database was locked or busy past the busy timeout.
    Locked,
    /// The database file or its directory is unavailable.
    Unavailable,
    /// Anything else, including I/O failures mid-write.
    Other,
}

impl ErrorCategory {
    /// Best-effort classification of a `rusqlite`/`anyhow` error chain.
    pub fn classify(error: &anyhow::Error) -> Self {
        for cause in error.chain() {
            if let Some(sql) = cause.downcast_ref::<rusqlite::Error>() {
                return match sql {
                    rusqlite::Error::SqliteFailure(code, _) => match code.code {
                        rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked => {
                            Self::Locked
                        }
                        rusqlite::ErrorCode::ConstraintViolation => Self::Constraint,
                        rusqlite::ErrorCode::CannotOpen
                        | rusqlite::ErrorCode::ReadOnly
                        | rusqlite::ErrorCode::NotADatabase => Self::Unavailable,
                        _ => Self::Other,
                    },
                    rusqlite::Error::SqliteSingleThreadedMode
                    | rusqlite::Error::InvalidPath(_)
                    | rusqlite::Error::InvalidQuery => Self::Unavailable,
                    _ => Self::Constraint,
                };
            }
        }
        Self::Other
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Constraint => "constraint",
            Self::Locked => "locked",
            Self::Unavailable => "unavailable",
            Self::Other => "other",
        }
    }
}

/// Records a failed write against an **auxiliary** store without failing the
/// caller's request.
///
/// Emits a structured `degraded` warning carrying enough context to correlate
/// the loss (which store, which category, which execution/session) and counts
/// it in `iota.storage.degraded`. Returns the error so callers that want it
/// can still log or assert on it.
pub fn degraded_write(
    store: &'static str,
    error: anyhow::Error,
    execution_id: Option<&str>,
    session_id: Option<&str>,
) -> anyhow::Error {
    let category = ErrorCategory::classify(&error);
    metrics::get().record_storage_degraded(store, category.as_str());
    tracing::warn!(
        store,
        category = category.as_str(),
        execution_id,
        session_id,
        error = %error,
        degraded = true,
        "auxiliary store write failed; continuing"
    );
    error
}

#[cfg(test)]
mod observability_tests;
