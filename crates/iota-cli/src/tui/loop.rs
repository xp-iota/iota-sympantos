use std::path::PathBuf;
use std::sync::atomic::Ordering;

use anyhow::Result;
use crossterm::event::{Event as CEvent, EventStream, KeyEventKind};
use futures_util::StreamExt;
use tokio::sync::{mpsc, oneshot};

use iota_core::acp::{AcpBackend, AcpPromptOutput};
use iota_core::utils::lock_or_recover;
use iota_kanban::TickReport;

use super::events::LoopSignal;
use super::state::{ConversationEntry, ObservabilityMeta};
use super::{ApprovalRequest, Terminal, TuiApp, TurnMessage, scrollback};

pub(super) async fn run_loop(
    terminal: &mut Terminal,
    app: &mut TuiApp,
    mut approval_rx: mpsc::Receiver<ApprovalRequest>,
) -> Result<()> {
    let mut tick = tokio::time::interval(std::time::Duration::from_millis(80));
    let mut events = EventStream::new();

    // Frame rate limiter - skip redraw if we drew less than MIN_FRAME_MS ago.
    const MIN_FRAME_MS: u64 = 8; // ~120 fps cap
    let mut last_draw = std::time::Instant::now()
        .checked_sub(std::time::Duration::from_millis(MIN_FRAME_MS))
        .unwrap_or(std::time::Instant::now());

    // engine_result carries the completed engine response back to the loop.
    // We use a one-shot channel so the engine future can be driven without
    // holding a mutable borrow on `app` across the terminal.draw() call.
    let (engine_tx, mut engine_rx) = mpsc::channel::<Result<(AcpBackend, AcpPromptOutput)>>(1);

    // pending_prompt is set by the submit path.
    let mut pending_prompt: Option<(AcpBackend, PathBuf, String)> = None;
    let (kanban_tx, mut kanban_rx) = mpsc::channel::<Result<TickReport>>(4);
    let (kanban_shutdown_tx, kanban_shutdown_rx) = oneshot::channel();
    let kanban_task = spawn_kanban_dispatcher_task(app, kanban_tx, kanban_shutdown_rx);

    loop {
        app.run_loop_flush_pending_scrollback(terminal);
        run_loop_draw_if_due(terminal, app, &mut last_draw, MIN_FRAME_MS)?;
        app.run_loop_spawn_pending_prompt_if_any(&mut pending_prompt, &engine_tx);

        tokio::select! {
            _ = tick.tick() => {
                app.tick_count += 1;
            }

            // Streaming output chunks - drain all available, then redraw.
            Some(chunk) = app.stream_rx.recv(), if app.running_turn => {
                app.run_loop_handle_stream_chunk(chunk, &mut last_draw, MIN_FRAME_MS);
            }

            // Incoming approval requests from the ACP layer.
            Some(req) = approval_rx.recv() => {
                app.run_loop_handle_approval_request(req);
            }

            // Collect engine result.
            Some(result) = engine_rx.recv() => {
                app.run_loop_handle_engine_result(result);
            }

            Some(result) = kanban_rx.recv() => {
                app.run_loop_handle_kanban_dispatch_result(result);
            }

            Ok(event) = app.kanban_event_rx.recv() => {
                app.run_loop_handle_kanban_ui_event(event);
            }

            // Pick up the internal submit signal from the channel.
            Some(msg) = app.turn_rx.recv() => {
                let TurnMessage::Prompt { backend, cwd, text } = msg;
                pending_prompt = Some((backend, cwd, text));
            }

            maybe_event = events.next() => {
                let Some(Ok(event)) = maybe_event else { break };
                if matches!(app.run_loop_handle_terminal_event(event).await, LoopSignal::Break) {
                    break;
                }
            }
        }
    }

    let _ = kanban_shutdown_tx.send(());
    let _ = kanban_task.await;
    app.run_loop_teardown_turn_and_engine().await;
    Ok(())
}

fn spawn_kanban_dispatcher_task(
    app: &TuiApp,
    kanban_tx: mpsc::Sender<Result<TickReport>>,
    mut shutdown_rx: oneshot::Receiver<()>,
) -> tokio::task::JoinHandle<()> {
    let store = app.kanban_store.clone();
    let dispatcher = app.kanban_dispatcher.clone();
    let daemon_active = app.kanban_daemon_active.clone();
    let interval = lock_or_recover(&dispatcher).tick_interval();

    tokio::spawn(async move {
        let mut tick = tokio::time::interval(interval);
        loop {
            tokio::select! {
                _ = tick.tick() => {}
                _ = &mut shutdown_rx => break,
            }
            // Skip the tick if daemon is paused
            if !daemon_active.load(Ordering::Relaxed) {
                continue;
            }
            let store = store.clone();
            let dispatcher = dispatcher.clone();
            let result = tokio::task::spawn_blocking(move || {
                lock_or_recover(&dispatcher).tick(store.as_ref())
            })
            .await
            .unwrap_or_else(|err| Err(anyhow::anyhow!("kanban dispatcher task failed: {}", err)));
            match shutdown_rx.try_recv() {
                Ok(()) | Err(oneshot::error::TryRecvError::Closed) => break,
                Err(oneshot::error::TryRecvError::Empty) => {}
            }
            if kanban_tx.send(result).await.is_err() {
                break;
            }
        }
    })
}

fn run_loop_draw_if_due(
    terminal: &mut Terminal,
    app: &TuiApp,
    last_draw: &mut std::time::Instant,
    min_frame_ms: u64,
) -> Result<()> {
    let now = std::time::Instant::now();
    if now.duration_since(*last_draw).as_millis() as u64 >= min_frame_ms {
        terminal.draw(|f| app.render(f))?;
        *last_draw = now;
    }
    Ok(())
}

impl TuiApp {
    fn run_loop_handle_kanban_dispatch_result(&mut self, result: Result<TickReport>) {
        match result {
            Ok(report)
                if report.spawned > 0
                    || report.completed > 0
                    || report.timed_out > 0
                    || report.spawn_failures > 0
                    || report.reclaimed > 0 =>
            {
                self.record_entry(ConversationEntry::SystemNotice {
                    text: format!(
                        "Kanban dispatch: spawned: {}, completed: {}, timed out: {}, spawn failures: {}, reclaimed: {}",
                        report.spawned,
                        report.completed,
                        report.timed_out,
                        report.spawn_failures,
                        report.reclaimed
                    ),
                });
            }
            Ok(_) => {}
            Err(err) => {
                self.record_entry(ConversationEntry::SystemNotice {
                    text: format!("Kanban dispatch failed: {}", err),
                });
            }
        }
    }

    fn run_loop_handle_kanban_ui_event(&mut self, event: iota_kanban::KanbanUiEvent) {
        use iota_kanban::KanbanUiEvent;
        let text = match event {
            KanbanUiEvent::TaskCreated { id, title } => {
                format!("Task #{} created: {}", id, title)
            }
            KanbanUiEvent::TaskStatusChanged { id, from, to } => {
                format!("Task #{}: {} -> {}", id, from, to)
            }
            KanbanUiEvent::RunStarted { task_id, .. } => {
                format!("Worker started for task #{}", task_id)
            }
            KanbanUiEvent::RunCompleted {
                task_id, status, ..
            } => {
                format!("Worker completed for task #{} ({})", task_id, status)
            }
            _ => return, // Don't spam for minor events
        };
        self.record_entry(ConversationEntry::SystemNotice { text });
    }

    fn run_loop_spawn_pending_prompt_if_any(
        &mut self,
        pending_prompt: &mut Option<(AcpBackend, PathBuf, String)>,
        engine_tx: &mpsc::Sender<Result<(AcpBackend, AcpPromptOutput)>>,
    ) {
        let Some((backend, cwd, prompt)) = pending_prompt.take() else {
            return;
        };
        self.streaming_text.clear();
        self.streaming_version
            .set(self.streaming_version.get().wrapping_add(1));
        self.streaming_backend = Some(backend);
        let engine_arc = self.engine.clone();
        let stream_tx = self.stream_tx.clone();
        let engine_tx2 = engine_tx.clone();
        self.turn_task = Some(tokio::spawn(async move {
            let mut engine = engine_arc.lock().await;
            engine.set_stream_output_sender(Some(stream_tx));
            let result = engine.run_with_timing(backend, cwd, &prompt).await;
            engine.set_stream_output_sender(None);
            let _ = engine_tx2
                .send(result.map(|output| (backend, output)))
                .await;
        }));
    }

    fn run_loop_handle_stream_chunk(
        &mut self,
        chunk: String,
        last_draw: &mut std::time::Instant,
        min_frame_ms: u64,
    ) {
        self.streaming_text.push_str(&chunk);
        while let Ok(c) = self.stream_rx.try_recv() {
            self.streaming_text.push_str(&c);
        }
        self.streaming_version
            .set(self.streaming_version.get().wrapping_add(1));
        *last_draw = std::time::Instant::now()
            .checked_sub(std::time::Duration::from_millis(min_frame_ms))
            .unwrap_or(std::time::Instant::now());
    }

    fn run_loop_handle_approval_request(&mut self, req: ApprovalRequest) {
        let tool_name = req.tool_name.clone();
        self.overlay = super::Overlay::None;
        self.pending_approval = Some(req);
        self.record_entry(ConversationEntry::SystemNotice {
            text: format!("Approval requested: {}", tool_name),
        });
    }

    fn run_loop_handle_engine_result(&mut self, result: Result<(AcpBackend, AcpPromptOutput)>) {
        self.turn_task = None;
        match result {
            Ok((backend, output)) => {
                let observability = observability_from_output(&output);
                self.latest_observability = Some(observability.clone());
                self.record_entry(ConversationEntry::AssistantMessage {
                    backend,
                    text: output.text,
                    observability: Some(observability),
                });
            }
            Err(err) => {
                self.record_entry(ConversationEntry::SystemNotice {
                    text: format!("Error: {}", err),
                });
            }
        }
        self.run_loop_reset_turn_state();

        if let Some(queued) = self.queued_prompt.take() {
            self.record_queued_prompt_delta(-1);
            self.record_entry(ConversationEntry::UserMessage {
                text: queued.clone(),
                backend: Some(self.active_backend),
            });
            self.running_turn = true;
            self.turn_started_at = Some(std::time::Instant::now());
            self.send_turn_prompt(self.active_backend, self.cwd.clone(), queued);
        }
    }

    fn run_loop_reset_turn_state(&mut self) {
        self.running_turn = false;
        self.streaming_text.clear();
        self.streaming_version
            .set(self.streaming_version.get().wrapping_add(1));
        self.streaming_backend = None;
        self.turn_started_at = None;
    }

    async fn run_loop_teardown_turn_and_engine(&mut self) {
        if let Some(handle) = self.turn_task.take() {
            handle.abort();
        }
        self.engine.lock().await.shutdown_open_clients().await;
    }

    async fn run_loop_handle_terminal_event(&mut self, event: CEvent) -> LoopSignal {
        match event {
            CEvent::Key(key) if key.kind == KeyEventKind::Press => self.on_key_event(key).await,
            CEvent::Key(_) => LoopSignal::Continue,
            CEvent::Resize(_, _) => LoopSignal::Continue,
            _ => LoopSignal::Continue,
        }
    }

    fn run_loop_flush_pending_scrollback(&mut self, terminal: &mut Terminal) {
        if self.help_requested {
            let _ = scrollback::insert_help(terminal);
            self.help_requested = false;
        }
        for entry in self.pending_scrollback_entries.drain(..) {
            let _ = scrollback::insert_entry(terminal, &entry);
        }
    }
}

fn observability_from_output(output: &AcpPromptOutput) -> ObservabilityMeta {
    let token_usage = output.events.iter().rev().find_map(|event| match event {
        iota_core::runtime_event::RuntimeEvent::TokenUsage(usage) => Some(usage),
        _ => None,
    });
    ObservabilityMeta {
        execution_id: output.execution_id.clone(),
        total_ms: Some(output.timing.total_ms),
        prompt_ms: Some(output.timing.prompt_ms),
        input_tokens: token_usage.and_then(|usage| usage.input_tokens),
        cache_tokens: token_usage.and_then(|usage| usage.cache_tokens),
        cache_read_input_tokens: token_usage.and_then(|usage| usage.cache_read_input_tokens),
        cache_creation_input_tokens: token_usage
            .and_then(|usage| usage.cache_creation_input_tokens),
        output_tokens: token_usage.and_then(|usage| usage.output_tokens),
        thinking_tokens: token_usage.and_then(|usage| usage.thinking_tokens),
        total_tokens: token_usage.and_then(|usage| usage.total_tokens),
        provider_reported_total_tokens: token_usage
            .and_then(|usage| usage.provider_reported_total_tokens),
        normalized_total_tokens: token_usage.and_then(|usage| usage.normalized_total_tokens),
    }
}

#[cfg(test)]
#[path = "loop_tests.rs"]
mod loop_tests;
