//! Audit logging for daemon connection lifecycle and sensitive-request
//! authentication decisions.
//!
//! Every log line goes through `tracing`, never `eprintln!`, so operators
//! can route it into structured log storage. Log fields intentionally never
//! include token values or other secret material — only whether
//! authentication succeeded/failed, the request type, and non-sensitive
//! connection metadata (remote-ish identifier, timestamp is added by the
//! `tracing` subscriber).

/// Records that a connection presented no or an invalid auth token while
/// attempting a sensitive request.
pub fn log_auth_rejected(request_type: &str, reason: &str) {
    tracing::warn!(
        target: "iota_core::daemon::audit",
        request_type,
        reason,
        "daemon.auth.rejected"
    );
}

/// Records that a connection successfully authenticated for a sensitive
/// request.
pub fn log_auth_accepted(request_type: &str) {
    tracing::info!(
        target: "iota_core::daemon::audit",
        request_type,
        "daemon.auth.accepted"
    );
}

/// Records that a sensitive operation was executed after successful
/// authentication (or, for the legacy non-desktop protocol prior to full
/// enforcement, unauthenticated — callers pass `authenticated` explicitly so
/// the audit trail always reflects ground truth rather than assuming).
pub fn log_sensitive_operation(request_type: &str, authenticated: bool) {
    tracing::info!(
        target: "iota_core::daemon::audit",
        request_type,
        authenticated,
        "daemon.sensitive_operation"
    );
}

/// Records a new inbound daemon connection (transport-agnostic).
pub fn log_connection_established(transport: &str) {
    tracing::debug!(
        target: "iota_core::daemon::audit",
        transport,
        "daemon.connection.established"
    );
}

#[cfg(test)]
mod tests {
    // These smoke-test that the audit functions do not panic; assertions on
    // log *content* are out of scope without a test subscriber wired in,
    // which would add a tracing-subscriber test dependency for marginal
    // value here. The call sites in `daemon::mod`/`daemon::desktop` are
    // covered by their own auth-outcome tests.
    use super::*;

    #[test]
    fn audit_calls_do_not_panic() {
        log_auth_rejected("start_turn", "missing token");
        log_auth_accepted("start_turn");
        log_sensitive_operation("start_turn", true);
        log_connection_established("tcp");
    }
}
