//! Agent service – background daemon that keeps [`IotaEngine`] alive across
//! CLI invocations so ACP subprocess connections are reused.
//!
//! # Protocol
//! TCP JSON-line on `127.0.0.1:47661` (default, overridable via
//! `IOTA_DAEMON_ADDR`).  Each connection carries exactly one request line and
//! receives exactly one response line before the connection is closed.
//!
//! # Security / trust boundary
//! Every TCP connection must authenticate with a per-user, CSPRNG-generated
//! token before any execution, configuration, memory, context, approval, or
//! observability operation is processed. The token is stored through the
//! owner-only secure-file layer and compared in constant time. The listener
//! is always restricted to loopback; authentication failures and sensitive
//! operations are emitted through the structured daemon audit target.
//! `Hello` performs protocol negotiation and authenticates the whole desktop
//! connection; legacy prompt/warm requests carry the token per request.
//!
//! Sub-modules:
//! - [`pool`]  — [`EnginePool`] / [`EngineKey`]: backend×cwd engine buckets
//! - [`proto`] — wire types: [`DaemonPromptRequest`], [`DaemonPromptResponse`],
//!   [`DaemonWarmRequest`]

pub mod audit;
pub mod auth;
mod desktop;
mod pool;
mod proto;

pub use proto::{
    DESKTOP_PROTOCOL_VERSION, DESKTOP_SCHEMA_VERSION, DaemonClientMessage, DaemonErrorCode,
    DaemonPromptRequest, DaemonPromptResponse, DaemonServerMessage, DaemonWarmRequest,
    DesktopBackendSnapshot, DesktopConfigSnapshot, DesktopContextBudgetsSnapshot,
    DesktopContextEngineSnapshot, DesktopContextSection, DesktopMemoryBuckets,
    DesktopMemoryContextSnapshot, DesktopMemoryRecord, DesktopMemoryScopeMode,
    DesktopMemorySummary, DesktopModelConfig, DesktopRuntimeContextSnapshot, DesktopSnapshotError,
    LatencyPercentiles, ObservabilitySummaryResponse, PROTOCOL_VERSION_MAX, PROTOCOL_VERSION_MIN,
    RecentTokenExecution, ThroughputSummary, TokenSummaryEntry, apply_desktop_model_update,
};

use anyhow::{Context, Result};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, Semaphore};
use tokio_util::sync::CancellationToken;

use crate::acp::AcpBackend;
use crate::config::{NimiaConfig, backend_config};

use pool::EnginePool;

pub const DEFAULT_DAEMON_ADDR: &str = "127.0.0.1:47661";

/// Returns the daemon TCP address, honouring `IOTA_DAEMON_ADDR`.
pub fn daemon_addr() -> String {
    std::env::var("IOTA_DAEMON_ADDR")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_DAEMON_ADDR.to_string())
}

/// Reject a non-loopback daemon binding unless the operator explicitly opts in
/// through `IOTA_DAEMON_ALLOW_NON_LOOPBACK=1`.
///
/// Sensitive requests still require the daemon token, but TCP JSON lines are
/// not transport-encrypted. Keeping the endpoint on loopback prevents an
/// accidental network exposure; a deliberate remote deployment must add an
/// appropriate encrypted, access-controlled transport around this endpoint.
fn guard_daemon_bind_addr(addr: &str) -> Result<()> {
    let allow_non_loopback = std::env::var("IOTA_DAEMON_ALLOW_NON_LOOPBACK")
        .map(|value| value.trim() == "1")
        .unwrap_or(false);
    if allow_non_loopback {
        eprintln!(
            "WARNING: IOTA_DAEMON_ALLOW_NON_LOOPBACK=1 set; binding daemon to {addr} beyond \
             loopback. Sensitive requests still require the daemon token, but TCP JSON lines \
             are not transport-encrypted. Use only with a trusted encrypted transport and do \
             not expose the token in logs or configuration."
        );
        return Ok(());
    }
    let is_loopback = addr
        .rsplit_once(':')
        .map(|(host, _port)| host.trim_start_matches('[').trim_end_matches(']'))
        .and_then(|host| host.parse::<std::net::IpAddr>().ok())
        .map(|ip| ip.is_loopback())
        // An address that fails to parse as `host:port` (e.g. a bare
        // hostname) is not verifiably loopback; treat it the same as a
        // non-loopback address rather than assuming it is safe.
        .unwrap_or(false);
    anyhow::ensure!(
        is_loopback,
        "refusing to bind iota daemon to non-loopback address '{addr}'. Sensitive requests \
         require a daemon token, but the TCP JSON-line transport is not encrypted. Use a \
         loopback address (e.g. 127.0.0.1:47661), or set IOTA_DAEMON_ALLOW_NON_LOOPBACK=1 \
         only behind a trusted encrypted transport."
    );
    Ok(())
}

pub async fn run_daemon(
    config: NimiaConfig,
    addr: &str,
    timeout_ms: u64,
    warm_on_start: bool,
) -> Result<()> {
    guard_daemon_bind_addr(addr)?;
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    let engine_pool = Arc::new(Mutex::new(EnginePool::new(config, false, timeout_ms)));
    if warm_on_start {
        eprintln!("warming enabled ACP backends before accepting daemon requests");
        let warmed = warm_all_backends(Arc::clone(&engine_pool), cwd.clone()).await?;
        eprintln!("warmed {} ACP backend(s)", warmed);
    }
    let listener = TcpListener::bind(addr)
        .await
        .with_context(|| format!("Failed to bind daemon at {}", addr))?;
    eprintln!("iota agent daemon listening on {}", addr);
    eprintln!(
        "SECURITY: daemon is loopback-only and requires the owner-only session token for all \
         sensitive requests; authentication decisions are audit logged."
    );
    eprintln!("Press Ctrl+C to shut down gracefully");

    let concurrency = Arc::new(Semaphore::new(8));

    let shutdown_token = CancellationToken::new();
    let shutdown_signal = shutdown_token.clone();

    tokio::spawn(async move {
        match tokio::signal::ctrl_c().await {
            Ok(()) => {
                eprintln!("\nReceived Ctrl+C, shutting down daemon...");
                shutdown_signal.cancel();
            }
            Err(err) => {
                eprintln!("Failed to listen for Ctrl+C: {}", err);
            }
        }
    });

    let desktop_approvals = desktop::ApprovalRegistry::default();
    let desktop_turns = desktop::TurnRegistry::default();

    // Periodically reap engines for workspaces nobody has used recently, so a
    // long-lived daemon does not hold ACP subprocesses open indefinitely.
    {
        let reaper_pool = Arc::clone(&engine_pool);
        let reaper_shutdown = shutdown_token.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                tokio::select! {
                    _ = reaper_shutdown.cancelled() => return,
                    _ = ticker.tick() => {
                        let mut pool = reaper_pool.lock().await;
                        let evicted = pool.reap_idle(std::time::Instant::now());
                        pool.record_pool_size();
                        drop(pool);
                        for evicted in evicted {
                            let mut engine = evicted.engine.lock().await;
                            engine.shutdown_open_clients().await;
                        }
                    }
                }
            }
        });
    }

    loop {
        tokio::select! {
            _ = shutdown_token.cancelled() => {
                // Order matters: stop accepting new work, cancel in-flight
                // turns and let them unwind (which tells their ACP backends to
                // stop), and only then drain the client pool. Shutting clients
                // down first would pull the backend out from under a turn that
                // is still mid-flight.
                let cancelled = desktop_turns
                    .cancel_all_and_wait(std::time::Duration::from_secs(10))
                    .await;
                if cancelled > 0 {
                    eprintln!("Cancelled {} in-flight turn(s)", cancelled);
                }
                eprintln!("Shutting down ACP clients...");
                let engines = engine_pool.lock().await.all_engines();
                let mut open_client_count = 0;
                for engine in engines {
                    let mut engine_guard = engine.lock().await;
                    open_client_count += engine_guard.open_client_count();
                    engine_guard.shutdown_open_clients().await;
                }
                eprintln!("Shut down {} ACP client(s)", open_client_count);
                eprintln!("Daemon shutdown complete");
                return Ok(());
            }
            accept_result = listener.accept() => {
                let (stream, _) = accept_result?;
                crate::daemon::audit::log_connection_established("tcp-loopback");
                let engine_pool = Arc::clone(&engine_pool);
                let permit = Arc::clone(&concurrency);
                let desktop_approvals = desktop_approvals.clone();
                let desktop_turns = desktop_turns.clone();
                tokio::spawn(async move {
                    let _permit = permit.acquire_owned().await;
                    if let Err(err) =
                        handle_connection(stream, engine_pool, desktop_approvals, desktop_turns)
                            .await
                    {
                        eprintln!("daemon request failed: {}", err);
                    }
                });
            }
        }
    }
}

pub async fn send_prompt(
    addr: &str,
    request: &DaemonPromptRequest,
) -> Result<DaemonPromptResponse> {
    send_request_with_retry(addr, request, 2, 100).await
}

async fn send_request_with_retry<T: Serialize + ?Sized>(
    addr: &str,
    request: &T,
    max_retries: usize,
    retry_delay_ms: u64,
) -> Result<DaemonPromptResponse> {
    let mut last_error = None;

    for attempt in 0..=max_retries {
        match send_request(addr, request).await {
            Ok(response) => return Ok(response),
            Err(err) => {
                last_error = Some(err);
                if attempt < max_retries {
                    tokio::time::sleep(tokio::time::Duration::from_millis(retry_delay_ms)).await;
                }
            }
        }
    }

    Err(last_error.unwrap())
}

pub async fn send_warm(addr: &str, request: &DaemonWarmRequest) -> Result<DaemonPromptResponse> {
    send_request(addr, request).await
}

async fn send_request<T: Serialize + ?Sized>(
    addr: &str,
    request: &T,
) -> Result<DaemonPromptResponse> {
    let mut stream = TcpStream::connect(addr)
        .await
        .with_context(|| format!("Failed to connect to daemon at {}", addr))?;
    let mut line = serde_json::to_vec(request).context("Failed to encode daemon request")?;
    line.push(b'\n');
    stream.write_all(&line).await?;
    stream.flush().await?;

    let mut reader = BufReader::new(stream);
    let mut response = String::new();
    reader.read_line(&mut response).await?;
    if response.trim().is_empty() {
        anyhow::bail!("Daemon returned an empty response");
    }
    serde_json::from_str(response.trim()).context("Failed to decode daemon response")
}

async fn handle_connection(
    stream: TcpStream,
    engine_pool: Arc<Mutex<EnginePool>>,
    desktop_approvals: desktop::ApprovalRegistry,
    desktop_turns: desktop::TurnRegistry,
) -> Result<()> {
    // Limit inbound request size to 10 MiB to prevent memory exhaustion from
    // a malicious or misbehaving client sending an unbounded line.
    const MAX_REQUEST_BYTES: u64 = 10 * 1024 * 1024;
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut request_line = String::new();
    let bytes_read = read_limited_line(&mut reader, &mut request_line, MAX_REQUEST_BYTES).await?;
    if bytes_read as u64 > MAX_REQUEST_BYTES {
        anyhow::bail!("daemon request exceeded {} byte limit", MAX_REQUEST_BYTES);
    }
    let request: serde_json::Value =
        serde_json::from_str(request_line.trim()).context("Failed to decode daemon request")?;

    if matches!(
        request.get("type").and_then(serde_json::Value::as_str),
        Some(
            "hello"
                | "start_turn"
                | "respond_approval"
                | "cancel_turn"
                | "get_config"
                | "save_backend_model"
                | "check_backend"
                | "get_observability_summary"
                | "get_memory_context_snapshot"
                | "ping"
        )
    ) {
        let first_message: DaemonClientMessage =
            serde_json::from_value(request).context("Failed to decode desktop daemon message")?;
        desktop::handle_desktop_connection(
            first_message,
            reader,
            write_half,
            engine_pool,
            desktop_approvals,
            desktop_turns,
        )
        .await?;
        return Ok(());
    }

    let request_type = request
        .get("type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_string();
    let is_warm = request_type == "warm";
    let presented_token = request
        .get("auth_token")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    let auth_outcome = authenticate_legacy_request(&request_type, presented_token.as_deref());
    if !auth_outcome.is_authenticated() {
        crate::daemon::audit::log_auth_rejected(
            &request_type,
            match &auth_outcome {
                crate::daemon::auth::AuthOutcome::Rejected(reason) => reason.as_str(),
                crate::daemon::auth::AuthOutcome::Authenticated => unreachable!(),
            },
        );
        let response = DaemonPromptResponse {
            ok: false,
            text: None,
            error: Some("unauthenticated: missing or invalid auth_token".to_string()),
            timing: None,
            execution_id: None,
            warmed: None,
            events: Vec::new(),
        };
        let mut line = serde_json::to_vec(&response).context("Failed to encode daemon response")?;
        line.push(b'\n');
        write_half.write_all(&line).await?;
        write_half.flush().await?;
        return Ok(());
    }
    crate::daemon::audit::log_auth_accepted(&request_type);
    crate::daemon::audit::log_sensitive_operation(&request_type, true);

    let response = if is_warm {
        let request: DaemonWarmRequest =
            serde_json::from_value(request).context("Failed to decode daemon warm request")?;
        handle_warm(request, engine_pool).await
    } else {
        let request: DaemonPromptRequest =
            serde_json::from_value(request).context("Failed to decode daemon prompt request")?;
        handle_prompt(request, engine_pool).await
    };
    let mut line = serde_json::to_vec(&response).context("Failed to encode daemon response")?;
    line.push(b'\n');
    write_half.write_all(&line).await?;
    write_half.flush().await?;
    Ok(())
}

/// Authenticates a legacy (non-desktop-protocol) `warm`/prompt request
/// against the daemon's CSPRNG token (see `daemon::auth`). Both
/// `DaemonPromptRequest` and `DaemonWarmRequest` are always sensitive (they
/// submit prompts or pre-start backends), so this is called unconditionally
/// for every request reaching this branch.
fn authenticate_legacy_request(
    request_type: &str,
    presented_token: Option<&str>,
) -> crate::daemon::auth::AuthOutcome {
    use crate::daemon::auth::{self, AuthOutcome};

    let Some(presented) = presented_token else {
        return AuthOutcome::Rejected(format!(
            "request '{}' requires auth_token but none was provided",
            request_type
        ));
    };
    let expected = match auth::load_or_create_token() {
        Ok(token) => token,
        Err(err) => {
            return AuthOutcome::Rejected(format!("failed to load daemon auth token: {}", err));
        }
    };
    auth::verify_token(presented, &expected)
}

pub(crate) async fn read_limited_line<R>(
    reader: &mut BufReader<R>,
    line: &mut String,
    max_bytes: u64,
) -> Result<usize>
where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::{AsyncBufReadExt, AsyncReadExt};

    let mut limited = reader.take(max_bytes + 1);
    let bytes_read = limited.read_line(line).await?;
    anyhow::ensure!(
        bytes_read as u64 <= max_bytes,
        "daemon request exceeded {} byte limit",
        max_bytes
    );
    Ok(bytes_read)
}

async fn handle_prompt(
    request: DaemonPromptRequest,
    engine_pool: Arc<Mutex<EnginePool>>,
) -> DaemonPromptResponse {
    let backend = match AcpBackend::parse(&request.backend) {
        Ok(backend) => backend,
        Err(err) => {
            return DaemonPromptResponse {
                ok: false,
                text: None,
                error: Some(err.to_string()),
                timing: None,
                execution_id: None,
                warmed: None,
                events: Vec::new(),
            };
        }
    };
    let cwd = PathBuf::from(request.cwd);
    let checkout = engine_pool.lock().await.engine_for(cwd.clone());
    // Shut down any engine evicted to make room, outside the pool lock.
    checkout.shutdown_evicted().await;
    let engine = checkout.engine;
    // The pool hands out one engine per workspace, and `engine.lock()` below
    // serializes turns *within* a workspace while different workspaces proceed
    // in parallel on their own engine. Time spent here is therefore queue wait.
    let queued_at = std::time::Instant::now();
    let mut engine = engine.lock().await;
    crate::telemetry::metrics::get().record_pool_queue_wait(queued_at.elapsed().as_secs_f64());
    if let Some(timeout_ms) = request.timeout_ms {
        if timeout_ms == 0 {
            return DaemonPromptResponse {
                ok: false,
                text: None,
                error: Some("timeout_ms must be greater than 0".to_string()),
                timing: None,
                execution_id: None,
                warmed: None,
                events: Vec::new(),
            };
        }
        engine.set_acp_timeout_ms(timeout_ms);
    }
    match engine
        .run(
            backend,
            cwd,
            &request.prompt,
            request.execution_id.as_deref(),
        )
        .await
    {
        Ok(output) => {
            let execution_id = output.execution_id.clone();
            DaemonPromptResponse {
                ok: true,
                text: Some(output.text),
                error: None,
                timing: Some(output.timing),
                execution_id,
                warmed: None,
                events: output.events,
            }
        }
        Err(err) => DaemonPromptResponse {
            ok: false,
            text: None,
            error: Some(err.to_string()),
            timing: None,
            execution_id: None,
            warmed: None,
            events: Vec::new(),
        },
    }
}

async fn handle_warm(
    request: DaemonWarmRequest,
    engine_pool: Arc<Mutex<EnginePool>>,
) -> DaemonPromptResponse {
    let cwd = PathBuf::from(request.cwd);
    let result = if request.backends.is_empty() {
        warm_all_backends(Arc::clone(&engine_pool), cwd).await
    } else {
        warm_selected_backends(engine_pool, cwd, &request.backends).await
    };

    match result {
        Ok(warmed) => DaemonPromptResponse {
            ok: true,
            text: None,
            error: None,
            timing: None,
            execution_id: None,
            warmed: Some(warmed),
            events: Vec::new(),
        },
        Err(err) => DaemonPromptResponse {
            ok: false,
            text: None,
            error: Some(err.to_string()),
            timing: None,
            execution_id: None,
            warmed: None,
            events: Vec::new(),
        },
    }
}

async fn warm_all_backends(engine_pool: Arc<Mutex<EnginePool>>, cwd: PathBuf) -> Result<usize> {
    let config = engine_pool.lock().await.config();
    let enabled = crate::acp::ALL_BACKENDS
        .iter()
        .copied()
        .filter(|backend| {
            backend_config(&config, *backend)
                .map(|section| section.enabled)
                .unwrap_or(false)
        })
        .map(|backend| backend.to_string())
        .collect::<Vec<_>>();
    warm_selected_backends(engine_pool, cwd, &enabled).await
}

async fn warm_selected_backends(
    engine_pool: Arc<Mutex<EnginePool>>,
    cwd: PathBuf,
    backends: &[String],
) -> Result<usize> {
    let mut warmed = 0;
    for backend in backends {
        let backend = AcpBackend::parse(backend)?;
        let checkout = engine_pool.lock().await.engine_for(cwd.clone());
        checkout.shutdown_evicted().await;
        let engine = checkout.engine;
        let started = engine
            .lock()
            .await
            .warm_backend(backend, cwd.clone())
            .await?;
        record_warm_result(&mut warmed, started);
    }
    Ok(warmed)
}

fn record_warm_result(warmed: &mut usize, started: bool) {
    if started {
        *warmed += 1;
    }
}

#[cfg(test)]
#[path = "daemon_tests.rs"]
mod tests;
