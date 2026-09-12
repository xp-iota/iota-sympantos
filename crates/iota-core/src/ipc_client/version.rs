//! Pure protocol-version negotiation, shared by every desktop IPC server and
//! client. Extracted from `iota-core::daemon::desktop::negotiate_version` so
//! non-daemon IPC servers (e.g. cockpit-runner) can reuse the same min/max
//! negotiation rule without depending on the daemon's wire message enum.

use std::fmt;

/// The client's declared version range failed to overlap with the server's
/// supported range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionNegotiationError {
    pub client_min: u32,
    pub client_max: u32,
    pub server_min: u32,
    pub server_max: u32,
}

impl fmt::Display for VersionNegotiationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Protocol version mismatch: client [{},{}] vs server [{},{}]",
            self.client_min, self.client_max, self.server_min, self.server_max
        )
    }
}

impl std::error::Error for VersionNegotiationError {}

/// Negotiate a protocol version given a client's declared `[client_min,
/// client_max]` range and a server's supported `[server_min, server_max]`
/// range.
///
/// The negotiated version is the highest version both sides can speak: the
/// smaller of the two `max` values. This is only valid if it still falls
/// within both sides' `min` bound; otherwise the ranges do not overlap and
/// negotiation fails.
///
/// This mirrors a client that only sends a single `protocol_version` (no
/// explicit range) by having the caller pass `client_min == client_max ==
/// protocol_version`, preserving backward compatibility with older clients.
pub fn negotiate_version(
    client_min: u32,
    client_max: u32,
    server_min: u32,
    server_max: u32,
) -> Result<u32, VersionNegotiationError> {
    let negotiated = client_max.min(server_max);
    if negotiated < server_min || negotiated < client_min {
        return Err(VersionNegotiationError {
            client_min,
            client_max,
            server_min,
            server_max,
        });
    }
    Ok(negotiated)
}

#[cfg(test)]
#[path = "version_tests.rs"]
mod tests;
