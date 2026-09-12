//! Authentication baseline for the daemon's local trust boundary.
//!
//! The daemon historically accepted any local TCP connection with no
//! authentication (see the module docs in `daemon::mod`). This module gives
//! the daemon two authentication mechanisms so sensitive requests can be
//! gated:
//!
//! 1. **Unix domain socket + peer credentials** (preferred, Unix only): the
//!    socket file itself is created with `0600` permissions, and on
//!    connection the daemon additionally verifies the connecting process'
//!    effective UID matches the daemon's own UID via `SO_PEERCRED` (Linux)
//!    or `LOCAL_PEERCRED` (macOS). This defends against a misconfigured
//!    socket path (e.g. shared `/tmp`) in addition to file permissions.
//! 2. **TCP fallback token** (all platforms, used when UDS is unavailable
//!    e.g. on Windows or when explicitly configured): a CSPRNG-generated
//!    token of at least 32 bytes is written to an owner-only file next to
//!    the daemon's config; clients must present it verbatim, compared in
//!    constant time.
//!
//! Callers should prefer the UDS transport; the TCP token path exists only
//! as a documented, explicit fallback.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::fs_secure::{atomic_write_secure, constant_time_eq, generate_csprng_token_hex};

/// Minimum token length in raw bytes before hex-encoding (256 bits).
pub const MIN_TOKEN_BYTES: usize = 32;

/// Result of authenticating an inbound daemon connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthOutcome {
    /// Connection is authenticated and may proceed to sensitive requests.
    Authenticated,
    /// Connection failed authentication and must be rejected before any
    /// sensitive request is processed.
    Rejected(String),
}

impl AuthOutcome {
    pub fn is_authenticated(&self) -> bool {
        matches!(self, AuthOutcome::Authenticated)
    }
}

/// Names of daemon request types that require authentication before being
/// processed. `hello` and `ping` are intentionally excluded so clients can
/// perform version negotiation / liveness checks without a token, but they
/// must not leak any sensitive data.
pub const SENSITIVE_REQUEST_TYPES: &[&str] = &[
    "start_turn",
    "respond_approval",
    "cancel_turn",
    "get_config",
    "save_backend_model",
    "get_observability_summary",
    "get_memory_context_snapshot",
    "warm",
];

pub fn is_sensitive_request(request_type: &str) -> bool {
    SENSITIVE_REQUEST_TYPES.contains(&request_type)
}

// ---------------------------------------------------------------------------
// TCP fallback token
// ---------------------------------------------------------------------------

/// Returns the daemon's config directory (`~/.i6`), the parent of
/// `config_path()`. Shared by [`token_path`] and [`uds_socket_path`] so both
/// live alongside `nimia.yaml` rather than each re-deriving `~/.i6`
/// independently.
fn daemon_config_dir() -> Result<PathBuf> {
    let config_path = crate::config::config_path()?;
    config_path
        .parent()
        .map(Path::to_path_buf)
        .context("config path has no parent directory")
}

/// Returns the path of the daemon auth token file, honouring
/// `IOTA_DAEMON_TOKEN_PATH` for tests/overrides, otherwise defaulting to
/// `<config_dir>/daemon.token`.
pub fn token_path() -> Result<PathBuf> {
    if let Ok(path) = std::env::var("IOTA_DAEMON_TOKEN_PATH") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed));
        }
    }
    Ok(daemon_config_dir()
        .context("failed to resolve config directory for daemon auth token")?
        .join("daemon.token"))
}

/// Loads the existing daemon auth token, or generates and persists a new
/// CSPRNG token (>= [`MIN_TOKEN_BYTES`] bytes, hex-encoded) if none exists.
///
/// The token file is written with [`atomic_write_secure`] so it is always
/// `0600` and never observed half-written, and its parent directory is
/// locked to `0700`.
pub fn load_or_create_token() -> Result<String> {
    let path = token_path()?;
    if let Some(parent) = path.parent() {
        crate::fs_secure::create_missing_dir_owner_only(parent)?;
    }
    if let Ok(existing) = std::fs::read_to_string(&path) {
        let trimmed = existing.trim().to_string();
        if trimmed.len() >= MIN_TOKEN_BYTES * 2 {
            // The token authenticates every desktop/CLI connection; keep it out
            // of logs even when a handshake error quotes what it received.
            crate::telemetry::redact::register_secret(&trimmed);
            return Ok(trimmed);
        }
        // Existing token is too short (e.g. legacy/corrupt) — regenerate.
    }
    let token = generate_csprng_token_hex(MIN_TOKEN_BYTES);
    atomic_write_secure(&path, token.as_bytes()).context("failed to persist daemon auth token")?;
    crate::telemetry::redact::register_secret(&token);
    Ok(token)
}

/// Verifies a client-presented token against the daemon's token in constant
/// time. Returns `Rejected` on any mismatch, missing token, or length below
/// [`MIN_TOKEN_BYTES`] * 2 hex chars (defends against trivially short/guessed
/// tokens being accepted if the on-disk token were ever corrupted shorter).
pub fn verify_token(presented: &str, expected: &str) -> AuthOutcome {
    if expected.len() < MIN_TOKEN_BYTES * 2 {
        return AuthOutcome::Rejected("server token below minimum length".to_string());
    }
    if constant_time_eq(presented.as_bytes(), expected.as_bytes()) {
        AuthOutcome::Authenticated
    } else {
        AuthOutcome::Rejected("invalid daemon auth token".to_string())
    }
}

// ---------------------------------------------------------------------------
// Unix domain socket + peer credentials
// ---------------------------------------------------------------------------

/// Returns the default Unix domain socket path for the daemon, honouring
/// `IOTA_DAEMON_SOCKET_PATH`.
pub fn uds_socket_path() -> Result<PathBuf> {
    if let Ok(path) = std::env::var("IOTA_DAEMON_SOCKET_PATH") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed));
        }
    }
    Ok(daemon_config_dir()
        .context("failed to resolve config directory for daemon socket")?
        .join("daemon.sock"))
}

/// Locks down the UDS socket file to `0600` immediately after `bind()`.
/// Must be called before the listener starts `accept()`ing connections, to
/// avoid a race where another local user could connect during the window
/// between bind and permission tightening.
#[cfg(unix)]
pub fn secure_uds_socket(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .with_context(|| format!("failed to set 0600 permissions on UDS socket {:?}", path))
}

#[cfg(not(unix))]
pub fn secure_uds_socket(_path: &Path) -> Result<()> {
    Ok(())
}

/// Peer credentials of a connecting Unix domain socket client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerCredentials {
    pub uid: u32,
    #[allow(dead_code)]
    pub pid: Option<i32>,
}

/// Verifies that a connecting UDS peer's UID matches the daemon process'
/// own effective UID (i.e. the peer is the same local user). This is the
/// primary authentication check for the UDS transport; file permissions on
/// the socket are a defense-in-depth measure, not a substitute, since some
/// platforms/filesystems (e.g. shared temp dirs with permissive umask
/// windows) can weaken the file-permission guarantee.
#[cfg(target_os = "linux")]
pub fn verify_peer_credentials(stream: &tokio::net::UnixStream) -> Result<AuthOutcome> {
    use std::os::fd::AsRawFd;

    let fd = stream.as_raw_fd();
    // SAFETY: `fd` is a valid, open socket fd owned by `stream` for the
    // duration of this call; `getsockopt(SO_PEERCRED)` only reads kernel
    // state associated with that fd and does not retain the pointer beyond
    // the call.
    let peer = unsafe {
        let mut ucred: libc::ucred = std::mem::zeroed();
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        let ret = libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut ucred as *mut _ as *mut libc::c_void,
            &mut len,
        );
        if ret != 0 {
            return Err(std::io::Error::last_os_error()).context("getsockopt(SO_PEERCRED) failed");
        }
        PeerCredentials {
            uid: ucred.uid,
            pid: Some(ucred.pid),
        }
    };

    let own_uid = unsafe { libc::getuid() };
    if peer.uid == own_uid {
        Ok(AuthOutcome::Authenticated)
    } else {
        Ok(AuthOutcome::Rejected(format!(
            "peer uid {} does not match daemon uid {}",
            peer.uid, own_uid
        )))
    }
}

#[cfg(all(unix, not(target_os = "linux")))]
pub fn verify_peer_credentials(_stream: &tokio::net::UnixStream) -> Result<AuthOutcome> {
    // macOS/BSD use LOCAL_PEERCRED with a different ucred layout
    // (`libc::xucred`) than Linux's SO_PEERCRED. Rather than guess at an
    // unverified struct layout, fail closed: rely on the 0600 socket file
    // permission check alone is not sufficient per design.md §2.3, so
    // sensitive requests over UDS on non-Linux Unix are rejected until
    // LOCAL_PEERCRED support is implemented and tested on that platform.
    Ok(AuthOutcome::Rejected(
        "peer credential verification is not yet implemented on this platform; refusing to \
         authenticate UDS connection for a sensitive request"
            .to_string(),
    ))
}

#[cfg(not(unix))]
pub fn verify_peer_credentials(_stream: &()) -> Result<AuthOutcome> {
    Ok(AuthOutcome::Rejected(
        "Unix domain sockets are not available on this platform".to_string(),
    ))
}

#[cfg(test)]
pub(crate) fn test_token_path_env_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .expect("daemon token-path test lock poisoned")
}

#[cfg(test)]
#[path = "auth_tests.rs"]
mod tests;
