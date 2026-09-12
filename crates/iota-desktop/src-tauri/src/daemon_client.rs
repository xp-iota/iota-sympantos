use anyhow::{Context, Result, anyhow};
use iota_core::daemon::{
    DESKTOP_PROTOCOL_VERSION, DaemonClientMessage, DaemonServerMessage, daemon_addr,
};
use iota_core::ipc_client::{
    ConnectionState, HeartbeatConfig, ReconnectConfig, backoff_delay_ms, next_backoff_delay_ms,
    time_jitter_factor,
};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{OnceLock, RwLock};
use std::time::Duration;
use tauri::Emitter;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::Mutex;

// ---------------------------------------------------------------------------
// Connection state
// ---------------------------------------------------------------------------

struct DaemonConnection {
    ping_seq: AtomicU64,
    missed_pongs: u8,
    state: ConnectionState,
    negotiated_version: u32,
}

impl DaemonConnection {
    fn new() -> Self {
        Self {
            ping_seq: AtomicU64::new(0),
            missed_pongs: 0,
            state: ConnectionState::Disconnected,
            negotiated_version: DESKTOP_PROTOCOL_VERSION,
        }
    }
}

static PERSISTENT_CONNECTION: OnceLock<Mutex<DaemonConnection>> = OnceLock::new();

fn persistent_connection() -> &'static Mutex<DaemonConnection> {
    PERSISTENT_CONNECTION.get_or_init(|| Mutex::new(DaemonConnection::new()))
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn hello_message() -> DaemonClientMessage {
    DaemonClientMessage::hello(
        "iota-desktop".to_string(),
        iota_core::daemon::auth::load_or_create_token().ok(),
    )
}

pub async fn start_turn(
    window: tauri::Window,
    turn_id: String,
    cwd: PathBuf,
    backend: String,
    prompt: String,
) -> Result<()> {
    let mut stream = connect_or_start().await?;
    write_message(
        &mut stream,
        &DaemonClientMessage::StartTurn {
            request_id: None,
            turn_id: turn_id.clone(),
            cwd,
            backend,
            prompt,
            timeout_ms: Some(600_000),
        },
    )
    .await?;

    tokio::spawn(async move {
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        let mut saw_terminal = false;
        loop {
            let bytes_read = match reader.read_line(&mut line).await {
                Ok(bytes_read) => bytes_read,
                Err(err) => {
                    emit_client_error(&window, Some(&turn_id), err.to_string());
                    return;
                }
            };
            if bytes_read == 0 {
                break;
            }
            match serde_json::from_str::<DaemonServerMessage>(line.trim()) {
                Ok(message) => {
                    saw_terminal = is_terminal_turn_message(&message) || saw_terminal;
                    let _ = window.emit("daemon-message", message);
                }
                Err(err) => {
                    emit_client_error(&window, Some(&turn_id), err.to_string());
                }
            }
            line.clear();
        }
        if !saw_terminal {
            emit_client_error(
                &window,
                Some(&turn_id),
                "daemon stream ended before the turn reached a terminal state".to_string(),
            );
        }
    });

    Ok(())
}

pub async fn send_one(message: DaemonClientMessage) -> Result<Vec<DaemonServerMessage>> {
    let mut stream = connect_or_start().await?;
    write_message(&mut stream, &message).await?;

    let mut reader = BufReader::new(stream);
    let mut messages = Vec::new();
    let mut line = String::new();
    while reader.read_line(&mut line).await? > 0 {
        messages.push(serde_json::from_str(line.trim())?);
        if matches!(
            messages.last(),
            Some(DaemonServerMessage::ConfigSnapshot { .. })
                | Some(DaemonServerMessage::BackendCheckResult { .. })
                | Some(DaemonServerMessage::ObservabilitySummary { .. })
                | Some(DaemonServerMessage::ApprovalResponded { .. })
                | Some(DaemonServerMessage::TurnCancelled { .. })
                | Some(DaemonServerMessage::ProtocolError { .. })
                | Some(DaemonServerMessage::MemoryContextSnapshot { .. })
                | Some(DaemonServerMessage::Pong { .. })
        ) {
            break;
        }
        line.clear();
    }
    Ok(messages)
}

// ---------------------------------------------------------------------------
// Reconnection with exponential backoff
// ---------------------------------------------------------------------------

pub async fn reconnect_with_backoff(app: Option<&tauri::AppHandle>) -> Result<TcpStream> {
    let conn = persistent_connection();
    let mut guard = conn.lock().await;
    guard.state = ConnectionState::Reconnecting;
    guard.missed_pongs = 0;
    if let Some(app) = app {
        let _ = app.emit("daemon-connection-state", &ConnectionState::Reconnecting);
    }
    drop(guard);

    let config = ReconnectConfig::default();
    let mut current_delay_ms = config.initial_delay_ms;

    loop {
        let actual_delay = backoff_delay_ms(current_delay_ms, &config, time_jitter_factor());
        current_delay_ms = next_backoff_delay_ms(current_delay_ms, &config);

        tokio::time::sleep(Duration::from_millis(actual_delay)).await;

        let addr = desktop_daemon_addr();
        if let Ok(stream) = connect_and_handshake(&addr).await {
            let mut guard = conn.lock().await;
            guard.state = ConnectionState::Connected;
            guard.missed_pongs = 0;
            if let Some(app) = app {
                let _ = app.emit("daemon-connection-state", &ConnectionState::Connected);
            }
            tokio::spawn(async {
                drain_pending_queue().await;
            });
            return Ok(stream);
        }
    }
}

// ---------------------------------------------------------------------------
// Heartbeat
// ---------------------------------------------------------------------------

pub async fn start_heartbeat_loop(app: tauri::AppHandle) {
    let config = HeartbeatConfig::default();
    let interval = Duration::from_secs(config.interval_secs);
    let max_misses = config.max_misses;

    loop {
        tokio::time::sleep(interval).await;

        let conn = persistent_connection();
        let guard = conn.lock().await;

        if guard.state != ConnectionState::Connected {
            continue;
        }

        if guard.negotiated_version < 3 {
            continue;
        }

        let seq = guard.ping_seq.fetch_add(1, Ordering::Relaxed);
        drop(guard);

        let ping_result = send_ping(seq).await;
        let mut guard = conn.lock().await;
        match ping_result {
            Ok(true) => {
                guard.missed_pongs = 0;
            }
            _ => {
                guard.missed_pongs += 1;
                if guard.missed_pongs >= max_misses {
                    guard.state = ConnectionState::Disconnected;
                    let _ = app.emit("daemon-connection-state", &ConnectionState::Disconnected);
                    drop(guard);
                    let _ = reconnect_with_backoff(Some(&app)).await;
                }
            }
        }
    }
}

async fn send_ping(seq: u64) -> Result<bool> {
    let addr = desktop_daemon_addr();
    let stream = connect_and_handshake(&addr).await;
    match stream {
        Ok(mut stream) => {
            write_message(
                &mut stream,
                &DaemonClientMessage::Ping {
                    seq,
                    request_id: None,
                },
            )
            .await?;
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            let read_result =
                tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut line)).await;
            match read_result {
                Ok(Ok(n)) if n > 0 => {
                    if let Ok(DaemonServerMessage::Pong { seq: pong_seq }) =
                        serde_json::from_str(line.trim())
                    {
                        Ok(pong_seq == seq)
                    } else {
                        Ok(false)
                    }
                }
                _ => Ok(false),
            }
        }
        Err(_) => Ok(false),
    }
}

// ---------------------------------------------------------------------------
// Operation queue (pending operations during reconnection)
// ---------------------------------------------------------------------------

struct PendingOperation {
    message: DaemonClientMessage,
    reply: tokio::sync::oneshot::Sender<Result<Vec<DaemonServerMessage>>>,
}

static PENDING_QUEUE: OnceLock<Mutex<Vec<PendingOperation>>> = OnceLock::new();

fn pending_queue() -> &'static Mutex<Vec<PendingOperation>> {
    PENDING_QUEUE.get_or_init(|| Mutex::new(Vec::new()))
}

pub async fn send_or_queue(message: DaemonClientMessage) -> Result<Vec<DaemonServerMessage>> {
    let conn = persistent_connection();
    let guard = conn.lock().await;
    if guard.state == ConnectionState::Reconnecting {
        drop(guard);
        let (tx, rx) = tokio::sync::oneshot::channel();
        pending_queue()
            .lock()
            .await
            .push(PendingOperation { message, reply: tx });
        rx.await
            .map_err(|_| anyhow!("pending operation cancelled"))?
    } else {
        drop(guard);
        send_one(message).await
    }
}

pub async fn drain_pending_queue() {
    let ops: Vec<PendingOperation> = {
        let mut queue = pending_queue().lock().await;
        std::mem::take(&mut *queue)
    };
    for op in ops {
        let result = send_one(op.message).await;
        let _ = op.reply.send(result);
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

#[derive(Clone, serde::Serialize)]
struct DaemonClientErrorPayload<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    turn_id: Option<&'a str>,
    message: String,
}

fn emit_client_error(window: &tauri::Window, turn_id: Option<&str>, message: String) {
    let _ = window.emit(
        "daemon-client-error",
        DaemonClientErrorPayload { turn_id, message },
    );
}

fn is_terminal_turn_message(message: &DaemonServerMessage) -> bool {
    matches!(
        message,
        DaemonServerMessage::TurnCompleted { .. }
            | DaemonServerMessage::TurnFailed { .. }
            | DaemonServerMessage::TurnCancelled { accepted: true, .. }
    )
}

async fn connect_or_start() -> Result<TcpStream> {
    let primary_addr = desktop_daemon_addr();
    if let Ok(stream) = connect_and_handshake(&primary_addr).await {
        return Ok(stream);
    }

    let _guard = autostart_lock().lock().await;
    let primary_addr = desktop_daemon_addr();
    if let Ok(stream) = connect_and_handshake(&primary_addr).await {
        return Ok(stream);
    }

    let fallback_addr = fallback_daemon_addr();
    set_desktop_daemon_addr(fallback_addr.clone());
    if let Ok(stream) = connect_and_handshake(&fallback_addr).await {
        return Ok(stream);
    }

    autostart_daemon(&fallback_addr)
        .await
        .context("Failed to autostart daemon")?;
    wait_for_daemon(&fallback_addr).await
}

fn desktop_daemon_addr() -> String {
    DESKTOP_DAEMON_ADDR
        .get_or_init(|| RwLock::new(daemon_addr()))
        .read()
        .map(|addr| addr.clone())
        .unwrap_or_else(|_| daemon_addr())
}

fn set_desktop_daemon_addr(addr: String) {
    if let Ok(mut current) = DESKTOP_DAEMON_ADDR
        .get_or_init(|| RwLock::new(daemon_addr()))
        .write()
    {
        *current = addr;
    }
}

fn fallback_daemon_addr() -> String {
    std::env::var("IOTA_DESKTOP_DAEMON_ADDR")
        .ok()
        .map(|addr| addr.trim().to_string())
        .filter(|addr| !addr.is_empty())
        .unwrap_or_else(|| "127.0.0.1:47662".to_string())
}

static DESKTOP_DAEMON_ADDR: OnceLock<RwLock<String>> = OnceLock::new();

fn autostart_lock() -> &'static Mutex<()> {
    static AUTOSTART_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    AUTOSTART_LOCK.get_or_init(|| Mutex::new(()))
}

async fn wait_for_daemon(addr: &str) -> Result<TcpStream> {
    let started = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(5);
    let mut last_error = None;
    while started.elapsed() < timeout {
        match connect_and_handshake(addr).await {
            Ok(stream) => return Ok(stream),
            Err(err) => {
                last_error = Some(err);
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }
    }
    match last_error {
        Some(err) => anyhow::bail!(
            "Failed to connect to daemon at {} after autostart: {}",
            addr,
            err
        ),
        None => anyhow::bail!("Failed to connect to daemon at {} after autostart", addr),
    }
}

async fn connect_and_handshake(addr: &str) -> Result<TcpStream> {
    let mut stream = TcpStream::connect(addr)
        .await
        .with_context(|| format!("Failed to connect to daemon at {}", addr))?;
    write_message(&mut stream, &hello_message()).await?;
    let negotiated = wait_for_hello(&mut stream).await?;

    let conn = persistent_connection();
    let mut guard = conn.lock().await;
    guard.state = ConnectionState::Connected;
    guard.negotiated_version = negotiated;
    guard.missed_pongs = 0;
    drop(guard);

    Ok(stream)
}

async fn autostart_daemon(addr: &str) -> Result<()> {
    let daemon_exe = locate_iota_cli().context("Failed to locate iota CLI for daemon autostart")?;
    let mut child = tokio::process::Command::new(daemon_exe)
        .arg("__daemon")
        .env("IOTA_DAEMON_ADDR", addr)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .context("Failed to spawn iota daemon")?;
    tokio::spawn(async move {
        let _ = child.wait().await;
    });
    Ok(())
}

fn locate_iota_cli() -> Result<std::path::PathBuf> {
    let exe_name = if cfg!(windows) { "iota.exe" } else { "iota" };
    iota_core::ipc_client::locate_sidecar_binary("IOTA_CLI_PATH", exe_name)
        .map_err(|err| anyhow!("{err}"))
}

async fn write_message(stream: &mut TcpStream, message: &DaemonClientMessage) -> Result<()> {
    let mut line = serde_json::to_vec(message).context("Failed to encode daemon message")?;
    line.push(b'\n');
    stream.write_all(&line).await?;
    stream.flush().await?;
    Ok(())
}

async fn wait_for_hello(stream: &mut TcpStream) -> Result<u32> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    let bytes_read = reader.read_line(&mut line).await?;
    if bytes_read == 0 {
        return Err(anyhow!(
            "Connected daemon does not support the desktop protocol. Stop the existing iota daemon and restart iota-desktop."
        ));
    }
    let message: DaemonServerMessage = serde_json::from_str(line.trim()).with_context(|| {
        "Connected daemon returned an invalid desktop handshake response; restart the iota daemon"
    })?;
    match message {
        DaemonServerMessage::HelloAccepted {
            protocol_version,
            negotiated_version,
        } => Ok(negotiated_version.unwrap_or(protocol_version)),
        DaemonServerMessage::ProtocolError { message, .. } => anyhow::bail!(message),
        other => anyhow::bail!("daemon returned unexpected handshake message: {:?}", other),
    }
}
