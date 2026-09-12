//! Sidecar process autostart: spawn a local helper binary and wait for it to
//! start accepting loopback TCP connections. Shared by every desktop app
//! that lazily starts its own backend daemon/runner process on first use
//! instead of requiring the user to start it manually.

use std::ffi::OsStr;
use std::io;
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Failure spawning or waiting for a sidecar process to become reachable.
#[derive(Debug)]
pub enum AutostartError {
    /// Could not locate the sidecar binary at all.
    BinaryNotFound(String),
    /// The binary was found but the OS refused to spawn it.
    Spawn(io::Error),
    /// The sidecar process status could not be queried.
    Status(io::Error),
    /// The process exited before its readiness probe succeeded.
    Exited { status: std::process::ExitStatus },
    /// The process was spawned but never became ready within the configured
    /// timeout.
    Timeout {
        addr: Option<SocketAddr>,
        waited: Duration,
    },
}

impl std::fmt::Display for AutostartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BinaryNotFound(message) => write!(f, "{message}"),
            Self::Spawn(err) => write!(f, "failed to spawn sidecar process: {err}"),
            Self::Status(err) => write!(f, "failed to query sidecar process status: {err}"),
            Self::Exited { status } => {
                write!(f, "sidecar process exited before becoming ready: {status}")
            }
            Self::Timeout { addr, waited } => match addr {
                Some(addr) => write!(f, "sidecar at {addr} did not become ready after {waited:?}"),
                None => write!(f, "sidecar did not become ready after {waited:?}"),
            },
        }
    }
}

impl std::error::Error for AutostartError {}

/// Locate a sidecar executable by, in order:
/// 1. an explicit override environment variable (e.g. `IOTA_CLI_PATH`),
/// 2. a binary named `exe_name` next to the current process's executable,
/// 3. a binary named `exe_name` on `PATH`.
///
/// `exe_name` should be the platform-appropriate bare name (this helper adds
/// no `.exe` suffix itself; pass e.g. `"iota.exe"` on Windows at the call
/// site if needed).
pub fn locate_sidecar_binary(
    env_override: &str,
    exe_name: &str,
) -> Result<PathBuf, AutostartError> {
    let explicit_override = std::env::var_os(env_override);
    locate_sidecar_binary_with_override(explicit_override.as_deref(), env_override, exe_name)
}

fn locate_sidecar_binary_with_override(
    explicit_override: Option<&OsStr>,
    env_override: &str,
    exe_name: &str,
) -> Result<PathBuf, AutostartError> {
    if let Some(path) = explicit_override {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
    }

    if let Ok(current) = std::env::current_exe()
        && let Some(dir) = current.parent()
    {
        let sibling = dir.join(exe_name);
        if sibling.is_file() {
            return Ok(sibling);
        }
    }

    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let candidate = dir.join(exe_name);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }

    Err(AutostartError::BinaryNotFound(format!(
        "set {env_override} or install {exe_name} in PATH"
    )))
}

/// Poll `addr` until a TCP connection succeeds or `timeout` elapses.
///
/// Blocking: intended to be called from a `spawn_blocking` context or a
/// dedicated autostart task, not directly on an async executor thread.
pub fn wait_for_port(
    addr: SocketAddr,
    timeout: Duration,
    poll_interval: Duration,
) -> Result<(), AutostartError> {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if TcpStream::connect_timeout(&addr, poll_interval).is_ok() {
            return Ok(());
        }
        std::thread::sleep(poll_interval);
    }
    Err(AutostartError::Timeout {
        addr: Some(addr),
        waited: started.elapsed(),
    })
}

/// Spawn a sidecar process and block until it accepts connections at `addr`
/// or `timeout` elapses. On timeout, the spawned child is killed before
/// returning the error, avoiding an orphaned process that never got wired up.
///
/// `configure` receives the freshly constructed [`std::process::Command`] so
/// callers can add args/env specific to their sidecar (e.g. `--bind`,
/// `--session-token`) before it is spawned.
pub fn spawn_sidecar<F>(
    binary: &OsStr,
    addr: SocketAddr,
    timeout: Duration,
    poll_interval: Duration,
    configure: F,
) -> Result<std::process::Child, AutostartError>
where
    F: FnOnce(&mut std::process::Command),
{
    spawn_sidecar_with_probe(binary, timeout, poll_interval, configure, || {
        TcpStream::connect_timeout(&addr, poll_interval).is_ok()
    })
    .map_err(|err| match err {
        AutostartError::Timeout { waited, .. } => AutostartError::Timeout {
            addr: Some(addr),
            waited,
        },
        other => other,
    })
}

/// Spawn a sidecar and wait for a caller-provided protocol-level readiness
/// probe. The child is checked for early exit before every probe and is killed
/// and reaped on timeout. Consumers should prefer this over raw port probing
/// when another process could already own the configured address.
pub fn spawn_sidecar_with_probe<F, P>(
    binary: &OsStr,
    timeout: Duration,
    poll_interval: Duration,
    configure: F,
    mut is_ready: P,
) -> Result<std::process::Child, AutostartError>
where
    F: FnOnce(&mut std::process::Command),
    P: FnMut() -> bool,
{
    let mut command = std::process::Command::new(binary);
    configure(&mut command);
    let mut child = command.spawn().map_err(AutostartError::Spawn)?;
    let started = Instant::now();

    while started.elapsed() < timeout {
        if let Some(status) = child.try_wait().map_err(AutostartError::Status)? {
            return Err(AutostartError::Exited { status });
        }
        if is_ready() {
            return Ok(child);
        }
        std::thread::sleep(poll_interval);
    }

    let _ = child.kill();
    let _ = child.wait();
    Err(AutostartError::Timeout {
        addr: None,
        waited: started.elapsed(),
    })
}

#[cfg(test)]
#[path = "autostart_tests.rs"]
mod tests;
