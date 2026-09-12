use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::io::{AsyncRead, AsyncWriteExt, BufReader};
use tokio::net::tcp::OwnedWriteHalf;
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use crate::acp::{AcpBackend, permission::ApprovalRequest};
use crate::config::{RecallThresholdsConfig, backend_readiness, read_config, save_config};
use crate::daemon::pool::EnginePool;
use crate::daemon::proto::{
    DaemonClientMessage, DaemonErrorCode, DaemonServerMessage, DesktopConfigSnapshot,
    DesktopContextEngineSnapshot, DesktopMemoryBuckets, DesktopMemoryContextSnapshot,
    DesktopMemoryRecord, DesktopMemoryScopeMode, DesktopMemorySummary, DesktopSnapshotError,
    PROTOCOL_VERSION_MAX, PROTOCOL_VERSION_MIN, apply_desktop_model_update,
};
use crate::daemon::read_limited_line;
use crate::memory::{MemoryRecord, MemoryStore, RecallBuckets};
use crate::store::observability::ObservabilityStore;
use std::path::Path;

#[derive(Default, Clone)]
pub(crate) struct ApprovalRegistry {
    pending: Arc<Mutex<BTreeMap<String, PendingApproval>>>,
}

struct PendingApproval {
    turn_id: String,
    tx: oneshot::Sender<bool>,
}

impl ApprovalRegistry {
    pub async fn insert(&self, turn_id: String, approval_id: String, tx: oneshot::Sender<bool>) {
        self.pending
            .lock()
            .await
            .insert(approval_id, PendingApproval { turn_id, tx });
    }

    pub async fn respond(&self, approval_id: &str, approved: bool) -> bool {
        let pending = self.pending.lock().await.remove(approval_id);
        if let Some(pending) = pending {
            let _ = pending.tx.send(approved);
            true
        } else {
            false
        }
    }

    pub async fn deny_for_turn(&self, turn_id: &str) -> usize {
        let mut pending = self.pending.lock().await;
        let approval_ids = pending
            .iter()
            .filter(|(_, approval)| approval.turn_id == turn_id)
            .map(|(approval_id, _)| approval_id.clone())
            .collect::<Vec<_>>();
        let denied_count = approval_ids.len();
        for approval_id in approval_ids {
            if let Some(approval) = pending.remove(&approval_id) {
                let _ = approval.tx.send(false);
            }
        }
        denied_count
    }
}

#[derive(Default, Clone)]
pub(crate) struct TurnRegistry {
    active: Arc<Mutex<BTreeMap<String, ActiveTurn>>>,
}

#[derive(Clone)]
struct ActiveTurn {
    handle: Arc<tokio::task::JoinHandle<()>>,
    writer: Arc<Mutex<OwnedWriteHalf>>,
    cancel: CancellationToken,
}

impl TurnRegistry {
    pub async fn insert(
        &self,
        turn_id: String,
        handle: tokio::task::JoinHandle<()>,
        writer: Arc<Mutex<OwnedWriteHalf>>,
        cancel: CancellationToken,
    ) {
        self.active.lock().await.insert(
            turn_id,
            ActiveTurn {
                handle: Arc::new(handle),
                writer,
                cancel,
            },
        );
    }

    async fn remove(&self, turn_id: &str) -> Option<ActiveTurn> {
        self.active.lock().await.remove(turn_id)
    }

    /// Hard-stop a turn by aborting its task. Used for connection teardown, where
    /// we no longer have anyone to deliver a graceful result to.
    pub async fn abort(&self, turn_id: &str) -> Option<Arc<Mutex<OwnedWriteHalf>>> {
        let turn = self.remove(turn_id).await?;
        turn.cancel.cancel();
        turn.handle.abort();
        Some(turn.writer)
    }

    /// Cooperatively cancel a turn: fire its cancellation token so the engine tells
    /// the live ACP backend to stop, then let the turn task finish and report the
    /// cancelled result itself. Returns the writer so the caller can acknowledge.
    pub async fn request_cancel(&self, turn_id: &str) -> Option<Arc<Mutex<OwnedWriteHalf>>> {
        let turn = self.remove(turn_id).await?;
        turn.cancel.cancel();
        Some(turn.writer)
    }

    /// Cancels every in-flight turn and waits for each to finish.
    ///
    /// Used at daemon shutdown: cancelling cooperatively lets the engine tell
    /// its ACP backend to stop and then unwind, so the subprocess is not
    /// orphaned. Aborting instead would leave a live backend process behind,
    /// which is exactly what shutdown must not do.
    ///
    /// Each wait is bounded, so one wedged turn cannot block shutdown
    /// indefinitely; any that outlive the deadline are aborted and reported.
    pub async fn cancel_all_and_wait(&self, grace: std::time::Duration) -> usize {
        let turns: Vec<ActiveTurn> = {
            let mut active = self.active.lock().await;
            std::mem::take(&mut *active).into_values().collect()
        };
        let count = turns.len();
        for turn in &turns {
            turn.cancel.cancel();
        }
        let mut still_running = 0;
        for turn in turns {
            // `JoinHandle` is not `Clone`, so it lives behind an `Arc` shared
            // with `ActiveTurn`. Awaiting a shared handle is not possible
            // directly; poll it through a clone of the `Arc` instead by waiting
            // on the token plus a completion check.
            let handle = Arc::clone(&turn.handle);
            let finished = tokio::time::timeout(grace, async move {
                while !handle.is_finished() {
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
            })
            .await;
            if finished.is_err() {
                still_running += 1;
                turn.handle.abort();
            }
        }
        if still_running > 0 {
            tracing::warn!(
                still_running,
                "turns did not stop within the shutdown grace period and were aborted"
            );
        }
        count
    }
}

pub(crate) fn negotiate_version(msg: &DaemonClientMessage) -> Result<u32, String> {
    match msg {
        DaemonClientMessage::Hello {
            protocol_version,
            min_version,
            max_version,
            ..
        } => {
            let client_min = min_version.unwrap_or(*protocol_version);
            let client_max = max_version.unwrap_or(*protocol_version);
            crate::ipc_client::negotiate_version(
                client_min,
                client_max,
                PROTOCOL_VERSION_MIN,
                PROTOCOL_VERSION_MAX,
            )
            .map_err(|err| err.to_string())
        }
        _ => Err("Expected Hello message".to_string()),
    }
}

pub(crate) async fn handle_desktop_connection<R>(
    first_message: DaemonClientMessage,
    reader: BufReader<R>,
    write_half: OwnedWriteHalf,
    engine_pool: Arc<Mutex<EnginePool>>,
    approvals: ApprovalRegistry,
    turns: TurnRegistry,
) -> Result<()>
where
    R: AsyncRead + Unpin,
{
    let writer = Arc::new(Mutex::new(write_half));
    let connection_turns = Arc::new(Mutex::new(Vec::<String>::new()));
    // Correlation ids already handled on this connection. A client that
    // retries an in-flight request after a dropped reply must not cause the
    // work to run twice, so a repeat id is acknowledged idempotently rather
    // than re-executed. `request_id` is client-chosen, so this is deliberately
    // per-connection: two clients may pick the same id independently.
    let mut seen_request_ids = std::collections::HashSet::<String>::new();
    let mut handshake_ok = false;
    if !matches!(first_message, DaemonClientMessage::Hello { .. }) {
        send_message(
            &writer,
            &DaemonServerMessage::protocol_error(
                "desktop daemon hello is required before other messages",
                DaemonErrorCode::InvalidRequest,
                first_message.request_id(),
            ),
        )
        .await?;
        return Ok(());
    }
    if negotiate_version(&first_message).is_ok() {
        handshake_ok = true;
    }
    // Authenticate the whole connection from the Hello message's
    // `auth_token`. This is a one-time check per connection rather than
    // per-message: once a connection has proven it can read the daemon's
    // token file (i.e. it runs as the same local OS user as the daemon),
    // every subsequent message on that same TCP connection is trusted. A
    // new connection must present the token again.
    let authenticated = match &first_message {
        DaemonClientMessage::Hello { auth_token, .. } => {
            authenticate_desktop_connection(auth_token.as_deref())
        }
        _ => false,
    };
    if !authenticated {
        crate::daemon::audit::log_auth_rejected("hello", "missing or invalid auth_token");
        send_message(
            &writer,
            &DaemonServerMessage::protocol_error(
                "unauthenticated: missing or invalid auth_token",
                DaemonErrorCode::Unauthenticated,
                None,
            ),
        )
        .await?;
        return Ok(());
    }
    crate::daemon::audit::log_auth_accepted("hello");
    handle_message(
        first_message,
        Arc::clone(&writer),
        Arc::clone(&engine_pool),
        approvals.clone(),
        turns.clone(),
        Arc::clone(&connection_turns),
    )
    .await?;
    if !handshake_ok {
        return Ok(());
    }

    let mut reader = reader;
    let mut line = String::new();

    const MAX_DESKTOP_MESSAGE_BYTES: u64 = 10 * 1024 * 1024;
    while read_limited_line(&mut reader, &mut line, MAX_DESKTOP_MESSAGE_BYTES).await? > 0 {
        let message: DaemonClientMessage =
            serde_json::from_str(line.trim()).context("Failed to decode desktop daemon message")?;
        if !handshake_ok {
            send_message(
                &writer,
                &DaemonServerMessage::protocol_error(
                    "desktop daemon hello with matching protocol version is required",
                    DaemonErrorCode::UnsupportedVersion,
                    message.request_id(),
                ),
            )
            .await?;
            break;
        }
        if matches!(message, DaemonClientMessage::Hello { .. }) {
            handshake_ok = negotiate_version(&message).is_ok();
        }
        if let Some(request_id) = message.request_id()
            && !seen_request_ids.insert(request_id.to_string())
        {
            // Already handled: report success-shaped idempotence without
            // repeating the side effect (starting a turn, saving config).
            send_message(
                &writer,
                &DaemonServerMessage::protocol_error(
                    format!(
                        "duplicate request_id {request_id} on this connection; \
                         the original request was already handled"
                    ),
                    DaemonErrorCode::InvalidRequest,
                    Some(request_id),
                ),
            )
            .await?;
            line.clear();
            continue;
        }
        handle_message(
            message,
            Arc::clone(&writer),
            Arc::clone(&engine_pool),
            approvals.clone(),
            turns.clone(),
            Arc::clone(&connection_turns),
        )
        .await?;
        line.clear();
    }
    cleanup_connection_turns(connection_turns, turns, approvals).await;
    Ok(())
}

/// Authenticates a desktop-protocol connection using the token presented in
/// its `Hello` message, against the daemon's CSPRNG token (`daemon::auth`).
/// The entire desktop protocol is treated as sensitive (it can start turns,
/// read config, read memory/context/observability snapshots, and approve
/// tool calls), so every connection must authenticate at handshake time.
fn authenticate_desktop_connection(presented_token: Option<&str>) -> bool {
    use crate::daemon::auth;

    let Some(presented) = presented_token else {
        return false;
    };
    let expected = match auth::load_or_create_token() {
        Ok(token) => token,
        Err(_) => return false,
    };
    auth::verify_token(presented, &expected).is_authenticated()
}

async fn handle_message(
    message: DaemonClientMessage,
    writer: Arc<Mutex<OwnedWriteHalf>>,
    engine_pool: Arc<Mutex<EnginePool>>,
    approvals: ApprovalRegistry,
    turns: TurnRegistry,
    connection_turns: Arc<Mutex<Vec<String>>>,
) -> Result<()> {
    let request_type = message.kind();
    // Correlation id echoed on every failure this message produces.
    let request_id = message.request_id().map(str::to_string);
    if crate::daemon::auth::is_sensitive_request(request_type) || request_type == "check_backend" {
        crate::daemon::audit::log_sensitive_operation(request_type, true);
    }
    match message {
        DaemonClientMessage::Hello { .. } => match negotiate_version(&message) {
            Ok(negotiated) => {
                send_message(
                    &writer,
                    &DaemonServerMessage::HelloAccepted {
                        protocol_version: negotiated,
                        negotiated_version: Some(negotiated),
                    },
                )
                .await?;
            }
            Err(err_msg) => {
                // A version mismatch is the one failure the client cannot fix
                // by retrying, so it is reported with its own code.
                let code = if err_msg.contains("protocol version") {
                    DaemonErrorCode::UnsupportedVersion
                } else {
                    DaemonErrorCode::InvalidRequest
                };
                send_message(
                    &writer,
                    &DaemonServerMessage::protocol_error(err_msg, code, request_id.as_deref()),
                )
                .await?;
            }
        },
        DaemonClientMessage::StartTurn {
            turn_id,
            cwd,
            backend,
            prompt,
            timeout_ms,
            ..
        } => {
            start_turn(
                turn_id,
                cwd,
                backend,
                prompt,
                timeout_ms,
                writer,
                engine_pool,
                approvals,
                turns,
                connection_turns,
            )
            .await?;
        }
        DaemonClientMessage::RespondApproval {
            approval_id,
            approved,
            ..
        } => {
            let accepted = approvals.respond(&approval_id, approved).await;
            send_message(
                &writer,
                &DaemonServerMessage::ApprovalResponded {
                    approval_id: approval_id.clone(),
                    accepted,
                },
            )
            .await?;
            if !accepted {
                send_message(
                    &writer,
                    &DaemonServerMessage::protocol_error(
                        format!("approval id {} was not pending", approval_id),
                        DaemonErrorCode::NotFound,
                        request_id.as_deref(),
                    ),
                )
                .await?;
            }
        }
        DaemonClientMessage::CancelTurn { turn_id, .. } => {
            let accepted = cancel_turn(&turns, &approvals, &turn_id).await;
            send_message(
                &writer,
                &DaemonServerMessage::TurnCancelled { turn_id, accepted },
            )
            .await?;
        }
        DaemonClientMessage::GetConfig { .. } => {
            let config = read_config().context("Failed to read config")?;
            send_message(
                &writer,
                &DaemonServerMessage::ConfigSnapshot {
                    config: DesktopConfigSnapshot::from_config(&config),
                },
            )
            .await?;
        }
        DaemonClientMessage::SaveBackendModel { backend, model, .. } => {
            let backend = AcpBackend::parse(&backend)?;
            let mut config = read_config().context("Failed to read config")?;
            apply_desktop_model_update(&mut config, backend, model);
            save_config(&config).context("Failed to save config")?;
            engine_pool
                .lock()
                .await
                .replace_config(config.clone())
                .await;
            send_message(
                &writer,
                &DaemonServerMessage::ConfigSnapshot {
                    config: DesktopConfigSnapshot::from_config(&config),
                },
            )
            .await?;
        }
        DaemonClientMessage::CheckBackend { backend, .. } => {
            let (ok, details) = match AcpBackend::parse(&backend) {
                Ok(backend) => {
                    let config = read_config().context("Failed to read config")?;
                    let result = backend_readiness(&config, backend);
                    (result.ok, result.details)
                }
                Err(err) => (false, err.to_string()),
            };
            send_message(
                &writer,
                &DaemonServerMessage::BackendCheckResult {
                    backend,
                    ok,
                    details,
                },
            )
            .await?;
        }
        DaemonClientMessage::GetObservabilitySummary { cwd, .. } => {
            let summary = observability_summary(cwd)?;
            send_message(
                &writer,
                &DaemonServerMessage::ObservabilitySummary { summary },
            )
            .await?;
        }
        DaemonClientMessage::GetMemoryContextSnapshot {
            cwd, scope_mode, ..
        } => {
            let snapshot = memory_context_snapshot(cwd, scope_mode, engine_pool).await;
            send_message(
                &writer,
                &DaemonServerMessage::MemoryContextSnapshot { snapshot },
            )
            .await?;
        }
        DaemonClientMessage::Ping { seq, .. } => {
            send_message(&writer, &DaemonServerMessage::Pong { seq }).await?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn start_turn(
    turn_id: String,
    cwd: PathBuf,
    backend: String,
    prompt: String,
    timeout_ms: Option<u64>,
    writer: Arc<Mutex<OwnedWriteHalf>>,
    engine_pool: Arc<Mutex<EnginePool>>,
    approvals: ApprovalRegistry,
    turns: TurnRegistry,
    connection_turns: Arc<Mutex<Vec<String>>>,
) -> Result<()> {
    let backend = AcpBackend::parse(&backend)?;
    send_message(
        &writer,
        &DaemonServerMessage::TurnStarted {
            turn_id: turn_id.clone(),
        },
    )
    .await?;

    let checkout = engine_pool.lock().await.engine_for(cwd.clone());
    // Shut down any engine evicted to make room, outside the pool lock.
    checkout.shutdown_evicted().await;
    let engine = checkout.engine;
    let (stream_tx, mut stream_rx) = mpsc::channel::<String>(100);
    let (event_tx, mut event_rx) = mpsc::channel(100);
    let streamed_events = Arc::new(Mutex::new(Vec::<String>::new()));
    let (approval_tx, mut approval_rx) = mpsc::channel::<ApprovalRequest>(10);
    crate::acp::permission::install_scoped_approval_channel(turn_id.clone(), approval_tx).await;

    let stream_writer = Arc::clone(&writer);
    let stream_turn_id = turn_id.clone();
    tokio::spawn(async move {
        while let Some(chunk) = stream_rx.recv().await {
            let _ = send_message(
                &stream_writer,
                &DaemonServerMessage::TextChunk {
                    turn_id: stream_turn_id.clone(),
                    chunk,
                },
            )
            .await;
        }
    });

    let event_writer = Arc::clone(&writer);
    let event_turn_id = turn_id.clone();
    let event_seen = Arc::clone(&streamed_events);
    tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            if let Ok(key) = serde_json::to_string(&event) {
                event_seen.lock().await.push(key);
            }
            let _ = send_message(
                &event_writer,
                &DaemonServerMessage::TurnEvent {
                    turn_id: event_turn_id.clone(),
                    event: Box::new(event),
                },
            )
            .await;
        }
    });

    let approval_writer = Arc::clone(&writer);
    let approval_turn_id = turn_id.clone();
    tokio::spawn(async move {
        while let Some(req) = approval_rx.recv().await {
            let approval_id = uuid::Uuid::new_v4().to_string();
            let (reply_tx, reply_rx) = oneshot::channel();
            approvals
                .insert(approval_turn_id.clone(), approval_id.clone(), reply_tx)
                .await;
            let sent = send_message(
                &approval_writer,
                &DaemonServerMessage::ApprovalRequested {
                    turn_id: approval_turn_id.clone(),
                    approval_id: approval_id.clone(),
                    tool_name: req.tool_name,
                    params: req.params,
                },
            )
            .await;
            if sent.is_err() {
                approvals.respond(&approval_id, false).await;
                let _ = req.reply.send(false);
                continue;
            }
            let decision = tokio::select! {
                decision = reply_rx => decision.unwrap_or(false),
                _ = tokio::time::sleep(std::time::Duration::from_secs(300)) => {
                    approvals.respond(&approval_id, false).await;
                    false
                }
            };
            let _ = req.reply.send(decision);
        }
    });

    let cancel = CancellationToken::new();
    let task_writer = Arc::clone(&writer);
    let task_turn_id = turn_id.clone();
    let task_turns = turns.clone();
    let task_streamed_events = Arc::clone(&streamed_events);
    let task_cancel = cancel.clone();
    let handle = tokio::spawn(async move {
        let result = {
            let mut engine = engine.lock().await;
            if let Some(timeout_ms) = timeout_ms {
                engine.set_acp_timeout_ms(timeout_ms);
            }
            engine.set_stream_output_sender(Some(stream_tx));
            engine.set_stream_event_sender(Some(event_tx));
            let result = engine
                .run_cancellable(
                    backend,
                    cwd,
                    &prompt,
                    Some(&task_turn_id),
                    Some(&task_cancel),
                )
                .await;
            engine.set_stream_output_sender(None);
            engine.set_stream_event_sender(None);
            result
        };

        task_turns.remove(&task_turn_id).await;
        crate::acp::permission::remove_scoped_approval_channel(&task_turn_id).await;

        match result {
            Ok(output) => {
                let mut seen = task_streamed_events.lock().await;
                for event in output.events {
                    let key = serde_json::to_string(&event).unwrap_or_default();
                    if seen.iter().any(|sent| sent == &key) {
                        continue;
                    }
                    seen.push(key);
                    let _ = send_message(
                        &task_writer,
                        &DaemonServerMessage::TurnEvent {
                            turn_id: task_turn_id.clone(),
                            event: Box::new(event),
                        },
                    )
                    .await;
                }
                let _ = send_message(
                    &task_writer,
                    &DaemonServerMessage::TurnCompleted {
                        turn_id: task_turn_id,
                        text: output.text,
                        timing: output.timing,
                    },
                )
                .await;
            }
            Err(err) if err.downcast_ref::<crate::acp::TurnCancelled>().is_some() => {
                // The turn was stopped by a cancel request; the backend was told to
                // stop. Report it as cancelled rather than a failure.
                let _ = send_message(
                    &task_writer,
                    &DaemonServerMessage::TurnCancelled {
                        turn_id: task_turn_id,
                        accepted: true,
                    },
                )
                .await;
            }
            Err(err) => {
                let _ = send_message(
                    &task_writer,
                    &DaemonServerMessage::TurnFailed {
                        turn_id: task_turn_id,
                        error: err.to_string(),
                    },
                )
                .await;
            }
        }
    });

    connection_turns.lock().await.push(turn_id.clone());
    turns
        .insert(turn_id, handle, Arc::clone(&writer), cancel)
        .await;

    Ok(())
}

async fn abort_turn(turns: &TurnRegistry, approvals: &ApprovalRegistry, turn_id: &str) -> bool {
    if turns.abort(turn_id).await.is_none() {
        return false;
    }
    approvals.deny_for_turn(turn_id).await;
    crate::acp::permission::remove_scoped_approval_channel(turn_id).await;
    true
}

async fn cancel_turn(turns: &TurnRegistry, approvals: &ApprovalRegistry, turn_id: &str) -> bool {
    // Cooperative cancel: fire the turn's token so the engine tells the live ACP
    // backend to stop, and deny any pending approval so a blocked turn can proceed
    // to observe the cancellation. The turn task itself emits the final
    // `TurnCancelled` once the backend has actually stopped; the caller sends the
    // immediate `accepted` acknowledgment.
    if turns.request_cancel(turn_id).await.is_none() {
        return false;
    }
    approvals.deny_for_turn(turn_id).await;
    crate::acp::permission::remove_scoped_approval_channel(turn_id).await;
    true
}

async fn cleanup_connection_turns(
    connection_turns: Arc<Mutex<Vec<String>>>,
    turns: TurnRegistry,
    approvals: ApprovalRegistry,
) {
    let turn_ids = std::mem::take(&mut *connection_turns.lock().await);
    for turn_id in turn_ids {
        abort_turn(&turns, &approvals, &turn_id).await;
    }
}

async fn send_message(
    writer: &Arc<Mutex<OwnedWriteHalf>>,
    message: &DaemonServerMessage,
) -> Result<()> {
    let mut line =
        serde_json::to_vec(message).context("Failed to encode desktop daemon message")?;
    line.push(b'\n');
    let mut writer = writer.lock().await;
    writer.write_all(&line).await?;
    writer.flush().await?;
    Ok(())
}

fn observability_summary(
    cwd: Option<PathBuf>,
) -> Result<crate::daemon::proto::ObservabilitySummaryResponse> {
    use crate::daemon::proto::{
        ObservabilitySummaryResponse, RecentTokenExecution, TokenSummaryEntry,
    };

    let store = ObservabilityStore::default_path().and_then(|path| ObservabilityStore::open(&path));
    match store {
        Ok(store) => {
            let since_ts = crate::utils::now_ts() - 7 * 24 * 60 * 60;
            let summaries = store.token_summary_since(since_ts)?;
            let recent = store.recent_token_executions(10)?;
            let token_summary = summaries
                .into_iter()
                .map(|s| TokenSummaryEntry {
                    backend: s.backend,
                    count: s.count,
                    input_tokens_mean: s.input_tokens_mean,
                    output_tokens_mean: s.output_tokens_mean,
                    normalized_total_mean: s.normalized_total_mean,
                })
                .collect();
            let recent_token_executions = recent
                .into_iter()
                .map(|r| RecentTokenExecution {
                    id: r.id,
                    ts: r.ts,
                    execution_id: r.execution_id,
                    backend: r.backend,
                    model: r.model,
                    input_tokens: r.input_tokens,
                    output_tokens: r.output_tokens,
                    normalized_total_tokens: r.normalized_total_tokens,
                })
                .collect();
            Ok(ObservabilitySummaryResponse {
                cwd,
                window_secs: Some(7 * 24 * 60 * 60),
                token_summary,
                recent_token_executions,
                write_latency: None,
                stream_throughput: None,
                error: None,
            })
        }
        Err(err) => Ok(ObservabilitySummaryResponse {
            cwd,
            window_secs: Some(7 * 24 * 60 * 60),
            token_summary: Vec::new(),
            recent_token_executions: Vec::new(),
            write_latency: None,
            stream_throughput: None,
            error: Some(err.to_string()),
        }),
    }
}

async fn memory_context_snapshot(
    cwd: PathBuf,
    scope_mode: DesktopMemoryScopeMode,
    engine_pool: Arc<Mutex<EnginePool>>,
) -> DesktopMemoryContextSnapshot {
    let mut errors = Vec::new();
    let checkout = engine_pool.lock().await.engine_for(cwd.clone());
    checkout.shutdown_evicted().await;
    let engine = checkout.engine;
    let engine = engine.lock().await;

    let memory = match engine.memory_store() {
        Some(store) => {
            match memory_buckets_for_scope(
                store,
                &scope_mode,
                &cwd,
                engine.engine_session_id(),
                *engine.effective_config().recall_thresholds(),
            ) {
                Ok(buckets) => buckets,
                Err(err) => {
                    errors.push(DesktopSnapshotError {
                        area: "memory".to_string(),
                        message: err.to_string(),
                    });
                    DesktopMemoryBuckets::default()
                }
            }
        }
        None => {
            errors.push(DesktopSnapshotError {
                area: "memory".to_string(),
                message: "memory store is unavailable".to_string(),
            });
            DesktopMemoryBuckets::default()
        }
    };

    let context_engine = DesktopContextEngineSnapshot {
        enabled: engine.context_engine_enabled(),
        memory_db: engine
            .effective_config()
            .memory_db_path()
            .map(PathBuf::from),
        budgets: engine.context_engine_budgets().into(),
    };

    let memory_summary = memory_summary(&memory);
    DesktopMemoryContextSnapshot {
        cwd,
        scope_mode,
        memory,
        memory_summary,
        runtime_context: engine.recent_runtime_context_snapshot(),
        context_engine,
        errors,
    }
}

fn memory_buckets_for_scope(
    store: &MemoryStore,
    scope_mode: &DesktopMemoryScopeMode,
    cwd: &Path,
    session_id: &str,
    thresholds: RecallThresholdsConfig,
) -> Result<DesktopMemoryBuckets> {
    let buckets = match scope_mode {
        DesktopMemoryScopeMode::Workspace => store.recall_buckets_with_thresholds(
            "local-user",
            &cwd.display().to_string(),
            session_id,
            thresholds,
        )?,
        DesktopMemoryScopeMode::All => store.all_scope_buckets(100)?,
    };
    Ok(DesktopMemoryBuckets::from(buckets))
}

fn memory_summary(memory: &DesktopMemoryBuckets) -> DesktopMemorySummary {
    DesktopMemorySummary {
        identity: memory.identity.len(),
        preference: memory.preference.len(),
        strategic: memory.strategic.len(),
        domain: memory.domain.len(),
        procedural: memory.procedural.len(),
        episodic: memory.episodic.len(),
    }
}

impl From<RecallBuckets> for DesktopMemoryBuckets {
    fn from(value: RecallBuckets) -> Self {
        Self {
            identity: value
                .identity
                .into_iter()
                .map(DesktopMemoryRecord::from)
                .collect(),
            preference: value
                .preference
                .into_iter()
                .map(DesktopMemoryRecord::from)
                .collect(),
            strategic: value
                .strategic
                .into_iter()
                .map(DesktopMemoryRecord::from)
                .collect(),
            domain: value
                .domain
                .into_iter()
                .map(DesktopMemoryRecord::from)
                .collect(),
            procedural: value
                .procedural
                .into_iter()
                .map(DesktopMemoryRecord::from)
                .collect(),
            episodic: value
                .episodic
                .into_iter()
                .map(DesktopMemoryRecord::from)
                .collect(),
        }
    }
}

impl From<MemoryRecord> for DesktopMemoryRecord {
    fn from(value: MemoryRecord) -> Self {
        Self {
            id: value.id,
            memory_type: value.memory_type.as_str().to_string(),
            facet: value.facet.map(|facet| facet.as_str().to_string()),
            scope: value.scope.as_str().to_string(),
            scope_id: value.scope_id,
            content: value.content,
            confidence: value.confidence,
            created_at: value.created_at,
            updated_at: value.updated_at,
            expires_at: value.expires_at,
        }
    }
}

#[cfg(test)]
#[path = "desktop_tests.rs"]
mod desktop_tests;
