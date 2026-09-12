use anyhow::{Result, bail};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::io::BufReader;
use tokio::process::ChildStdin;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::mcp::router;
use crate::runtime_event::{self, RuntimeEvent, ToolCallEvent, ToolResultEvent};

use super::AcpBackend;
use super::message::{
    TurnCancelled, acp_tool_call_parts, extract_final_text, is_terminal_result,
    permission_request_id, text_from_session_update, text_from_terminal_result,
};
use super::permission as acp_permission;
use super::wire::{format_acp_error, is_response_id, parse_message_line, read_next_line};

pub(super) struct PromptReadOptions<'a> {
    pub(super) backend: AcpBackend,
    pub(super) tool_whitelist: &'a [String],
    pub(super) show_native: bool,
    pub(super) timeout_ms: u64,
    pub(super) expected_prompt_id: &'a str,
    pub(super) stream_tx: Option<&'a mpsc::Sender<String>>,
    pub(super) event_tx: Option<&'a mpsc::Sender<RuntimeEvent>>,
    pub(super) execution_id: Option<&'a str>,
    pub(super) cwd: &'a std::path::Path,
    /// When set and fired, the read loop sends `session/cancel` and returns [`TurnCancelled`].
    pub(super) cancel: Option<&'a CancellationToken>,
    /// The backend-native session id, needed for the `session/cancel` notification.
    pub(super) session_id: &'a str,
    pub(super) progress: Arc<Mutex<PromptProgress>>,
}

#[derive(Debug)]
pub(super) struct PromptProgress {
    started: Instant,
    phase: &'static str,
    line_count: usize,
    update_count: usize,
    tool_call_count: usize,
    first_update_ms: Option<u64>,
    first_tool_call_ms: Option<u64>,
}

#[derive(Debug)]
pub(super) struct PromptProgressSnapshot {
    pub(super) elapsed_ms: u64,
    pub(super) phase: &'static str,
    pub(super) line_count: usize,
    pub(super) update_count: usize,
    pub(super) tool_call_count: usize,
    pub(super) first_update_ms: Option<u64>,
    pub(super) first_tool_call_ms: Option<u64>,
}

impl PromptProgress {
    pub(super) fn shared(started: Instant) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Self {
            started,
            phase: "starting",
            line_count: 0,
            update_count: 0,
            tool_call_count: 0,
            first_update_ms: None,
            first_tool_call_ms: None,
        }))
    }

    pub(super) fn set_phase(progress: &Arc<Mutex<Self>>, phase: &'static str) {
        if let Ok(mut progress) = progress.lock() {
            progress.phase = phase;
        }
    }

    pub(super) fn snapshot(progress: &Arc<Mutex<Self>>) -> PromptProgressSnapshot {
        let progress = progress.lock().unwrap_or_else(|error| error.into_inner());
        PromptProgressSnapshot {
            elapsed_ms: progress.started.elapsed().as_millis() as u64,
            phase: progress.phase,
            line_count: progress.line_count,
            update_count: progress.update_count,
            tool_call_count: progress.tool_call_count,
            first_update_ms: progress.first_update_ms,
            first_tool_call_ms: progress.first_tool_call_ms,
        }
    }
}

/// Outcome of one iteration of the read loop's race between the next backend line
/// and the caller's cancellation signal.
enum ReadStep {
    /// The next line (or `None` at end of stream) arrived before cancellation.
    Line(Option<String>),
    /// The caller's cancellation token fired first.
    Cancelled,
}

pub(super) async fn read_prompt_events_for_id<R>(
    lines: &mut tokio::io::Lines<BufReader<R>>,
    stdin: &mut ChildStdin,
    options: PromptReadOptions<'_>,
) -> Result<(String, Vec<RuntimeEvent>)>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let PromptReadOptions {
        backend,
        tool_whitelist,
        show_native,
        timeout_ms,
        expected_prompt_id,
        stream_tx,
        event_tx,
        execution_id,
        cwd,
        cancel,
        session_id,
        progress,
    } = options;
    let mut output = String::new();
    let mut events = Vec::new();
    let mut streamed = false;
    let timeout_message = format!("ACP prompt timed out after {}ms", timeout_ms);
    loop {
        // Race the next backend line against the caller's cancellation signal. When
        // cancellation wins, tell the backend to stop its current turn cooperatively
        // (a `session/cancel` notification) rather than dropping the future or killing
        // the process — the latter leaves the backend running or destroys the session.
        // The `read_line` future borrows `lines`, so it must be fully dropped (i.e.
        // the `select!` block must end) before we can touch `lines` again in the
        // cancel path. We therefore only decide *whether* we were cancelled inside
        // the select, then act on it below.
        let read_line = read_next_line(lines, timeout_ms, &timeout_message);
        let step = match cancel {
            Some(token) => {
                tokio::select! {
                    biased;
                    _ = token.cancelled() => ReadStep::Cancelled,
                    line = read_line => ReadStep::Line(line?),
                }
            }
            None => ReadStep::Line(read_line.await?),
        };
        let line = match step {
            ReadStep::Cancelled => {
                tracing::info!(
                    backend = %backend,
                    execution_id = execution_id.unwrap_or("-"),
                    "acp.turn.cancel_requested"
                );
                super::client::send_notification(
                    stdin,
                    "session/cancel",
                    json!({ "sessionId": session_id }),
                )
                .await?;
                // Drain the backend's acknowledgment so the pipe is clean for session
                // reuse: after `session/cancel` the agent stops and replies to the
                // pending `session/prompt` with a `cancelled` stop reason. Best effort —
                // never block cancellation on it.
                drain_after_cancel(lines, expected_prompt_id, show_native, timeout_ms).await;
                return Err(TurnCancelled.into());
            }
            ReadStep::Line(line) => line,
        };
        let Some(line) = line else {
            break;
        };
        if let Ok(mut progress) = progress.lock() {
            progress.line_count += 1;
        }
        let message = parse_message_line(&line, show_native)?;

        if let Some(error) = &message.error {
            events.push(runtime_event::map_acp_error(
                error.message.clone(),
                error.code,
                error.data.clone(),
            ));
            bail!(format_acp_error(error));
        }

        if is_response_id(&message, expected_prompt_id)
            && let Some(result) = &message.result
        {
            if let Some(text) = text_from_terminal_result(result, streamed) {
                output.push_str(&text);
            }
            if let Some(usage) = runtime_event::token_usage_from_value(result) {
                push_event(&mut events, event_tx, RuntimeEvent::TokenUsage(usage));
            }
            if is_terminal_result(result) {
                PromptProgress::set_phase(&progress, "terminal prompt response");
                break;
            }
        }

        let Some(method) = message.method.as_deref() else {
            continue;
        };

        for event in runtime_event::map_acp_events(method, message.params.as_ref()) {
            push_event(&mut events, event_tx, event);
        }

        match method {
            "session/update" | "session_update" => {
                let (first_update, first_update_ms) = if let Ok(mut progress) = progress.lock() {
                    progress.update_count += 1;
                    progress.phase = "receiving session/update";
                    let first_update = progress.first_update_ms.is_none();
                    if first_update {
                        progress.first_update_ms =
                            Some(progress.started.elapsed().as_millis() as u64);
                    }
                    (first_update, progress.first_update_ms)
                } else {
                    (false, None)
                };
                if first_update {
                    tracing::info!(
                        backend = %backend,
                        execution_id = execution_id.unwrap_or("-"),
                        first_update_ms,
                        "acp.prompt.first_update"
                    );
                }
                if let Some(text) = text_from_session_update(message.params.as_ref()) {
                    streamed = true;
                    output.push_str(&text);
                    if let Some(tx) = stream_tx {
                        let _ = tx.try_send(text);
                    }
                }
            }
            "session/complete" | "session_complete" => {
                PromptProgress::set_phase(&progress, "session/complete");
                if !streamed
                    && let Some(text) = message.params.as_ref().and_then(extract_final_text)
                {
                    output.push_str(&text);
                }
                break;
            }
            "session/request_permission" | "request_permission" | "permission/request" => {
                let id = permission_request_id(&message)?;
                let params = message.params.clone().unwrap_or(Value::Null);
                let decision = acp_permission::answer_permission_request(
                    stdin,
                    id,
                    params,
                    execution_id,
                    backend,
                    tool_whitelist,
                    Some(cwd),
                )
                .await?;
                push_event(
                    &mut events,
                    event_tx,
                    RuntimeEvent::ApprovalDecision(decision),
                );
            }
            _ => {
                if let (Some(id), Some(intercepted)) = (
                    message.id.clone(),
                    router::try_intercept_tool_call(method, message.params.as_ref()),
                ) {
                    let (tool_name, tool_arguments) = acp_tool_call_parts(message.params.as_ref());
                    let call_id = id.as_str().unwrap_or("tool-call").to_string();
                    let first_tool_call_ms = if let Ok(mut progress) = progress.lock() {
                        progress.tool_call_count += 1;
                        progress.phase = "executing MCP tool";
                        if progress.first_tool_call_ms.is_none() {
                            progress.first_tool_call_ms =
                                Some(progress.started.elapsed().as_millis() as u64);
                        }
                        progress.first_tool_call_ms
                    } else {
                        None
                    };
                    tracing::info!(
                        backend = %backend,
                        execution_id = execution_id.unwrap_or("-"),
                        tool_call_id = %call_id,
                        tool_name = %tool_name,
                        first_tool_call_ms,
                        "acp.tool.call"
                    );
                    push_event(
                        &mut events,
                        event_tx,
                        RuntimeEvent::ToolCall(ToolCallEvent {
                            id: call_id.clone(),
                            name: tool_name.clone(),
                            arguments: tool_arguments.clone(),
                        }),
                    );
                    let result = intercepted.unwrap_or_else(|err| json!({"content":[{"type":"text","text":err.to_string()}],"isError":true}));
                    let ok = !result
                        .get("isError")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    tracing::info!(
                        backend = %backend,
                        execution_id = execution_id.unwrap_or("-"),
                        tool_call_id = %call_id,
                        tool_name = %tool_name,
                        ok,
                        "acp.tool.result"
                    );
                    push_event(
                        &mut events,
                        event_tx,
                        RuntimeEvent::ToolResult(ToolResultEvent {
                            id: call_id,
                            name: tool_name,
                            ok,
                            result: result.clone(),
                        }),
                    );
                    super::client::send_response(stdin, id, result).await?;
                    PromptProgress::set_phase(&progress, "awaiting backend after MCP tool");
                    continue;
                }

                if show_native {
                    eprintln!("[acp native] {}", line);
                }
            }
        }
    }
    Ok((output, events))
}

/// After sending `session/cancel`, read and discard lines until the backend
/// finishes the pending prompt (its terminal response, or a cancelled stop
/// reason), so a reused session does not inherit stray output from the cancelled
/// turn. Bounded by a short timeout and best-effort: any read error just stops.
async fn drain_after_cancel<R>(
    lines: &mut tokio::io::Lines<BufReader<R>>,
    expected_prompt_id: &str,
    show_native: bool,
    timeout_ms: u64,
) where
    R: tokio::io::AsyncRead + Unpin,
{
    // Cap the drain so a misbehaving backend cannot make cancellation hang; the
    // acknowledgment normally arrives promptly once the turn is told to stop.
    let drain_budget_ms = timeout_ms.min(2_000);
    let deadline = "cancel drain reached its time budget";
    while let Ok(Some(line)) = read_next_line(lines, drain_budget_ms, deadline).await {
        let Ok(message) = parse_message_line(&line, show_native) else {
            continue;
        };
        if is_response_id(&message, expected_prompt_id) {
            return;
        }
        if matches!(
            message.method.as_deref(),
            Some("session/complete") | Some("session_complete")
        ) {
            return;
        }
    }
}

fn push_event(
    events: &mut Vec<RuntimeEvent>,
    event_tx: Option<&mpsc::Sender<RuntimeEvent>>,
    event: RuntimeEvent,
) {
    if let Some(tx) = event_tx
        && let Err(e) = tx.try_send(event.clone())
    {
        tracing::error!(
            error = %e,
            event = ?event,
            "Failed to send RuntimeEvent to TUI; event may have been dropped"
        );
    }
    events.push(event);
}
