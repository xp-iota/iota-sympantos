use anyhow::{Context, Result};
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;

use crate::acp::{AcpBackend, AcpPromptOutput, TurnCancelled};
use crate::config::configured_model;
use crate::context::ComposeInput;
use crate::runtime_event::{ErrorEvent, MemoryEvent, OutputEvent, RuntimeEvent, StateEvent};
use crate::skill::SkillRegistry;
use crate::store::cache::{ExecutionStatus, request_hash};

use super::IotaEngine;
use super::memory_ops::{
    deterministic_memory_answer, has_successful_memory_write, is_explicit_memory_tool_prompt,
    is_memory_persistence_intent, memory_inject_payload,
};
use super::telemetry::event_payload;

impl IotaEngine {
    /// Run a prompt and return only the final assistant text.
    pub async fn run_prompt_text(
        &mut self,
        backend: AcpBackend,
        cwd: PathBuf,
        prompt: &str,
    ) -> Result<String> {
        Ok(self.run_with_timing(backend, cwd, prompt).await?.text)
    }

    /// Run a prompt and return text, runtime events, backend session id, and timing data.
    pub async fn run_with_timing(
        &mut self,
        backend: AcpBackend,
        cwd: PathBuf,
        prompt: &str,
    ) -> Result<AcpPromptOutput> {
        self.run(backend, cwd, prompt, None).await
    }

    /// Run a prompt with an optional externally supplied execution id.
    ///
    /// The daemon uses `requested_execution_id` so callers can correlate persisted cache/events
    /// with their own request id. When it is `None`, the cache layer allocates the id.
    pub async fn run(
        &mut self,
        backend: AcpBackend,
        cwd: PathBuf,
        prompt: &str,
        requested_execution_id: Option<&str>,
    ) -> Result<AcpPromptOutput> {
        self.run_cancellable(backend, cwd, prompt, requested_execution_id, None)
            .await
    }

    /// Run a prompt that can be cancelled mid-turn via `cancel`.
    ///
    /// This is the cancellation entry point downstream consumers previously lacked.
    /// When `cancel` fires while the backend is producing a turn, the live ACP
    /// process is told to stop (a cooperative `session/cancel` notification), the
    /// turn is recorded as [`ExecutionStatus::Cancelled`] for durable evidence, and
    /// the call returns `Err` carrying [`TurnCancelled`]. Passing `None` is
    /// identical to [`run`](Self::run).
    ///
    /// [`ExecutionStatus::Cancelled`]: crate::store::cache::ExecutionStatus::Cancelled
    #[tracing::instrument(
        skip(self, prompt, cancel),
        fields(
            acp.backend = %backend,
            cwd = %cwd.display(),
            session.id = %self.engine_session_id,
            acp.model = tracing::field::Empty,
            execution.id = tracing::field::Empty,
            request.hash = tracing::field::Empty,
        )
    )]
    pub async fn run_cancellable(
        &mut self,
        backend: AcpBackend,
        cwd: PathBuf,
        prompt: &str,
        requested_execution_id: Option<&str>,
        cancel: Option<&CancellationToken>,
    ) -> Result<AcpPromptOutput> {
        let request_hash = request_hash(&backend.to_string(), &cwd, prompt);
        tracing::Span::current().record("request.hash", &request_hash);
        tracing::info!("prompt.requested");

        let mut skill_roots = self.effective_config.skill_roots().to_vec();
        for root in &self.resource_skill_roots {
            if !skill_roots.contains(root) {
                skill_roots.push(root.clone());
            }
        }
        let skills = SkillRegistry::load_cached(&cwd, &skill_roots, &mut self.skill_registry_cache);
        // Native MCP turns must reach the ACP backend. A matching local skill
        // is an offline engine handler and would otherwise short-circuit the
        // backend, attempting to start the skill's MCP server itself.
        let native_mcp_available = !self
            .effective_config
            .context_mcp_servers(backend)
            .is_empty();
        // Cockpit's textual compatibility transport still carries the local
        // skill instructions in the user prompt. It must reach Hermes rather
        // than be mistaken for an iota-core engine skill and echoed locally.
        let cockpit_text_transport =
            backend == AcpBackend::Hermes && prompt.contains("cockpit world simulation");
        let matched_skill = (!native_mcp_available && !cockpit_text_transport)
            .then(|| skills.match_skill(backend, prompt))
            .flatten();
        let model = self
            .effective_config
            .backend_config(backend)
            .and_then(configured_model);
        if let Some(ref m) = model {
            tracing::Span::current().record("acp.model", m);
        }

        // The ledger records the logical session first, then later records turns and backend ids.
        self.ensure_ledger_session(backend, &cwd, model.as_deref())
            .await;
        // When switching from one backend to another, inject recent dialogue as handoff text.
        let handoff = self.prepare_backend_handoff(backend, &cwd);
        let execution_id = self
            .cache_store
            .as_ref()
            .map(|store| {
                store.begin_execution_with_id(
                    &backend.to_string(),
                    &self.engine_session_id,
                    &request_hash,
                    requested_execution_id,
                )
            })
            .transpose()?;

        if let Some(ref eid) = execution_id {
            tracing::Span::current().record("execution.id", eid);
            tracing::info!("execution.started");
        }
        self.record_runtime_event(
            &execution_id,
            backend,
            RuntimeEvent::State(StateEvent {
                state: "started".to_string(),
                detail: None,
            }),
        );

        if let Some(skill) = matched_skill {
            // Engine-run skills are local deterministic handlers. When they match, they replace
            // the external ACP backend call.
            if let Some(skill_output) =
                crate::skill::runner::run_engine_skill(skill, prompt).await?
            {
                let mut events = Vec::new();
                for event in skill_output.events {
                    self.record_runtime_event(&execution_id, backend, event.clone());
                    events.push(event);
                }
                let output_event = RuntimeEvent::Output(OutputEvent {
                    text: skill_output.text.clone(),
                    role: Some("engine".to_string()),
                });
                self.record_runtime_event(&execution_id, backend, output_event.clone());
                events.push(output_event);
                self.persist_turn_as_episodic_memory(
                    backend,
                    prompt,
                    &skill_output.text,
                    execution_id.as_deref(),
                );
                return Ok(self
                    .finalize_local_turn(
                        backend,
                        &execution_id,
                        &request_hash,
                        prompt,
                        skill_output.text,
                        events,
                    )
                    .await);
            }
        }

        // Run memory recall and git status concurrently — both are blocking I/O
        // operations that are independent of each other.
        let memory_store_c = self.memory_store.clone();
        let thresholds = *self.effective_config.recall_thresholds();
        let project_id = cwd.display().to_string();
        let session_id_for_recall = self.engine_session_id.clone();
        let cwd_for_git = cwd.clone();

        let memory_task = tokio::task::spawn_blocking(move || {
            memory_store_c.as_ref().and_then(|store| {
                store
                    .recall_buckets_with_thresholds(
                        "local-user",
                        &project_id,
                        &session_id_for_recall,
                        thresholds,
                    )
                    .ok()
            })
        });
        let workspace_task =
            tokio::task::spawn_blocking(move || crate::context::render_workspace(&cwd_for_git));

        self.log_engine_event(
            execution_id.as_deref(),
            backend,
            "info",
            "memory.recall.started",
            serde_json::json!({
                "user_id": "local-user",
                "project_id": cwd.display().to_string(),
            }),
        );
        tracing::info!(
            user_id = "local-user",
            project_id = %cwd.display(),
            "memory.recall.started"
        );

        // Await both concurrently.
        let (memory_result, workspace_result) = tokio::join!(memory_task, workspace_task);
        let memory = memory_result.unwrap_or(None);
        let workspace_str = workspace_result.unwrap_or_default();

        if let Some(ref buckets) = memory {
            self.log_engine_event(
                execution_id.as_deref(),
                backend,
                "info",
                "memory.recall.completed",
                serde_json::json!({
                    "identity_count": buckets.identity.len(),
                    "preference_count": buckets.preference.len(),
                    "strategic_count": buckets.strategic.len(),
                    "domain_count": buckets.domain.len(),
                    "procedural_count": buckets.procedural.len(),
                    "episodic_count": buckets.episodic.len(),
                }),
            );
            tracing::info!(
                identity_count = buckets.identity.len(),
                preference_count = buckets.preference.len(),
                strategic_count = buckets.strategic.len(),
                domain_count = buckets.domain.len(),
                procedural_count = buckets.procedural.len(),
                episodic_count = buckets.episodic.len(),
                "memory.recall.completed"
            );
        }

        let memory_event = memory.as_ref().map(|buckets| {
            // Keep a structured event showing which memories were injected into the prompt.
            RuntimeEvent::Memory(MemoryEvent {
                action: "inject".to_string(),
                memory_id: None,
                payload: memory_inject_payload(buckets, self.context_engine.budgets().memory_chars),
            })
        });
        if let Some(event) = memory_event.clone() {
            self.log_engine_event(
                execution_id.as_deref(),
                backend,
                "info",
                "memory.inject",
                event_payload(&event),
            );
            tracing::info!(
                payload = %event_payload(&event),
                "memory.inject"
            );
            self.record_runtime_event(&execution_id, backend, event);
        }
        if let Some((buckets, text)) = memory.as_ref().and_then(|buckets| {
            deterministic_memory_answer(prompt, buckets).map(|text| (buckets, text))
        }) {
            // Memory queries can be answered from recall buckets without calling a backend.
            let mut events = Vec::new();
            if let Some(event) = memory_event.clone() {
                events.push(event);
            }
            let output_event = RuntimeEvent::Output(OutputEvent {
                text: text.clone(),
                role: Some("engine".to_string()),
            });
            self.record_runtime_event(&execution_id, backend, output_event.clone());
            events.push(output_event);
            let _ = buckets;
            return Ok(self
                .finalize_local_turn(backend, &execution_id, &request_hash, prompt, text, events)
                .await);
        }
        // Compose the effective prompt — git status is already pre-computed so this
        // is now a pure CPU operation (no blocking I/O).
        let context_engine = self.context_engine.clone();
        let session_id_c = self.engine_session_id.clone();
        let model_c = model.clone();
        let skills_c = skills.clone();
        let working_memory_c = self.working_memory.clone();
        let handoff_c = handoff.clone();
        let prompt_c = prompt.to_string();
        let cwd_c = cwd.clone();
        let mcp_tools_available = !self
            .effective_config
            .context_mcp_servers(backend)
            .is_empty();
        let effective_prompt = context_engine.compose_effective_prompt(ComposeInput {
            backend,
            cwd: &cwd_c,
            session_id: &session_id_c,
            model: model_c.as_deref(),
            prompt: &prompt_c,
            memory: memory.as_ref(),
            skills: Some(&skills_c),
            working_memory: &working_memory_c,
            handoff: handoff_c.as_deref(),
            mcp_tools_available,
            workspace: Some(&workspace_str),
        });
        let context_injected = effective_prompt.contains("<iota-context>");
        let context_config = self.effective_config.context_engine();
        tracing::info!(
            backend = %backend,
            raw_prompt_bytes = prompt.len(),
            effective_prompt_bytes = effective_prompt.len(),
            context_injected,
            context_injection = context_config.injection.as_str(),
            mcp_server_count = self.effective_config.context_mcp_servers(backend).len(),
            "prompt.composed"
        );

        if context_injected {
            self.capture_runtime_context_snapshot(
                execution_id
                    .clone()
                    .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                backend,
                cwd.clone(),
                model.clone(),
                effective_prompt.clone(),
            );
        }

        let client_started = self.ensure_acp_client(backend, cwd.clone()).await?;
        let key = super::AcpClientKey {
            backend,
            cwd: cwd.clone(),
        };
        let client = self
            .acp_clients
            .get_mut(&key)
            .context("ACP client missing after warm")?;
        let startup_timing = client.startup_timing();
        match client
            .execute_cancellable(&cwd, &effective_prompt, execution_id.as_deref(), cancel)
            .await
        {
            Ok(mut output) => {
                // Normalize ACP output with engine-owned metadata before persisting events.
                output.execution_id = execution_id.clone();
                if let Some(event) = memory_event.clone() {
                    output.events.insert(0, event);
                }
                output.timing.client_started = client_started;
                output.timing.process_spawned = client_started;
                if client_started {
                    output.timing.process_spawn_ms = Some(startup_timing.process_spawn_ms);
                    output.timing.init_ms = Some(startup_timing.init_ms);
                }
                let has_output_event = output
                    .events
                    .iter()
                    .any(|event| matches!(event, RuntimeEvent::Output(_)));
                for event in output.events.iter().cloned() {
                    self.record_runtime_event(&execution_id, backend, event);
                }
                if is_memory_persistence_intent(prompt)
                    && !has_successful_memory_write(&output.events)
                {
                    let message = "memory persistence requested, but no successful iota_memory_write tool result was observed";
                    self.record_runtime_event(
                        &execution_id,
                        backend,
                        RuntimeEvent::Error(ErrorEvent {
                            message: message.to_string(),
                            code: None,
                            data: None,
                        }),
                    );
                    self.mark_execution_finished(&execution_id, ExecutionStatus::Failed);
                    self.record_ledger_turn(
                        backend,
                        execution_id.as_deref(),
                        &request_hash,
                        message,
                        ExecutionStatus::Failed.as_str(),
                    )
                    .await;
                    return Err(anyhow::anyhow!(message));
                }
                if !has_output_event {
                    // Some backends only return final text. Synthesize an Output event so
                    // event consumers see a consistent shape.
                    let output_text = &output.text;
                    tracing::info!(
                        execution_id = execution_id.as_deref(),
                        output_len = output_text.len(),
                        "output.final"
                    );
                    self.record_runtime_event(
                        &execution_id,
                        backend,
                        RuntimeEvent::Output(OutputEvent {
                            text: output_text.clone(),
                            role: Some("assistant".to_string()),
                        }),
                    );
                }
                if let Some(session_id) = output.backend_session_id.as_deref() {
                    // Preserve backend-native session ids for future backend-specific continuity.
                    self.persist_backend_session_id(backend, &cwd, session_id)
                        .await;
                }
                self.mark_execution_finished_with_timing(
                    &execution_id,
                    backend,
                    ExecutionStatus::Completed,
                    &output.timing,
                );
                tracing::info!(
                    total_ms = output.timing.total_ms,
                    prompt_ms = output.timing.prompt_ms,
                    "execution.completed"
                );
                self.record_ledger_turn(
                    backend,
                    execution_id.as_deref(),
                    &request_hash,
                    &output.text,
                    ExecutionStatus::Completed.as_str(),
                )
                .await;
                self.last_used_backend = Some(backend);
                self.working_memory.push_turn(backend, prompt, &output.text);
                if !is_explicit_memory_tool_prompt(prompt) {
                    self.persist_turn_as_episodic_memory(
                        backend,
                        prompt,
                        &output.text,
                        execution_id.as_deref(),
                    );
                }
                Ok(output)
            }
            Err(err) => {
                // A cancelled turn is not a failure: the backend was deliberately told
                // to stop. Record it as Cancelled so the ledger reflects what happened.
                let cancelled = err.downcast_ref::<TurnCancelled>().is_some();
                let status = if cancelled {
                    ExecutionStatus::Cancelled
                } else {
                    ExecutionStatus::Failed
                };
                self.record_runtime_event(
                    &execution_id,
                    backend,
                    RuntimeEvent::Error(ErrorEvent {
                        message: err.to_string(),
                        code: None,
                        data: None,
                    }),
                );
                self.mark_execution_finished(&execution_id, status.clone());
                if cancelled {
                    tracing::info!("execution.cancelled");
                } else {
                    tracing::warn!(error = %err, "execution.failed");
                }
                self.record_ledger_turn(
                    backend,
                    execution_id.as_deref(),
                    &request_hash,
                    &err.to_string(),
                    status.as_str(),
                )
                .await;
                Err(err)
            }
        }
    }

    async fn finalize_local_turn(
        &mut self,
        backend: AcpBackend,
        execution_id: &Option<String>,
        request_hash: &str,
        prompt: &str,
        text: String,
        events: Vec<RuntimeEvent>,
    ) -> AcpPromptOutput {
        self.mark_execution_finished(execution_id, ExecutionStatus::Completed);
        self.record_ledger_turn(
            backend,
            execution_id.as_deref(),
            request_hash,
            &text,
            ExecutionStatus::Completed.as_str(),
        )
        .await;
        self.last_used_backend = Some(backend);
        self.working_memory.push_turn(backend, prompt, &text);
        let mut output = AcpPromptOutput::synthetic(text);
        output.execution_id = execution_id.clone();
        output.events = events;
        output
    }
}

impl IotaEngine {
    /// Persist the backend-native session id after ACP returns it.
    ///
    /// The ledger write is a blocking SQLite call, so it runs on the blocking
    /// pool rather than stalling a runtime worker thread.
    pub(super) async fn persist_backend_session_id(
        &self,
        backend: AcpBackend,
        cwd: &std::path::Path,
        backend_session_id: &str,
    ) {
        let Some(ledger) = self.session_ledger_store.clone() else {
            return;
        };
        let session_id = self.engine_session_id.clone();
        let backend_name = backend.to_string();
        let cwd = cwd.to_path_buf();
        let backend_session_id = backend_session_id.to_string();
        let result = tokio::task::spawn_blocking(move || {
            ledger.record_backend_session(
                &session_id,
                &backend_name,
                Some(&backend_session_id),
                &cwd,
            )
        })
        .await;
        warn_on_ledger_failure("record_backend_session", result, &self.engine_session_id);
    }

    /// Ensure the durable ledger has a session row and a backend-session row for this turn.
    pub(super) async fn ensure_ledger_session(
        &self,
        backend: AcpBackend,
        cwd: &std::path::Path,
        model: Option<&str>,
    ) {
        let Some(ledger) = self.session_ledger_store.clone() else {
            return;
        };
        let session_id = self.engine_session_id.clone();
        let backend_name = backend.to_string();
        let cwd = cwd.to_path_buf();
        let model = model.map(str::to_string);
        let result = tokio::task::spawn_blocking(move || {
            ledger.ensure_session(&session_id, &cwd, Some(&backend_name), model.as_deref())?;
            ledger.record_backend_session(&session_id, &backend_name, None, &cwd)
        })
        .await;
        warn_on_ledger_failure("ensure_ledger_session", result, &self.engine_session_id);
    }

    /// Build and persist a summary when the active conversation switches backend.
    pub(super) fn prepare_backend_handoff(
        &mut self,
        backend: AcpBackend,
        cwd: &std::path::Path,
    ) -> Option<String> {
        let previous = self
            .last_used_backend
            .filter(|previous| *previous != backend)?;
        // Budget in tokens, matching the capsule's `<handoff>` section rather
        // than the old character count.
        let summary = self
            .working_memory
            .render(crate::context::tokens::chars_budget_to_tokens(800));
        if summary.trim().is_empty() {
            return None;
        }
        if let Some(ledger) = &self.session_ledger_store
            && let Err(error) = ledger.publish_handoff(
                &self.engine_session_id,
                Some(&previous.to_string()),
                Some(&backend.to_string()),
                cwd,
                &summary,
            )
        {
            // The ledger is authoritative for session continuity: losing the
            // handoff means the next turn on the new backend starts without the
            // previous backend's context, so it must be visible.
            let category = crate::store::ErrorCategory::classify(&error);
            tracing::warn!(
                session_id = %self.engine_session_id,
                from_backend = %previous,
                to_backend = %backend,
                store = "ledger",
                category = category.as_str(),
                error = %error,
                "failed to record backend handoff; continuity for this switch is lost"
            );
            crate::telemetry::metrics::get().record_storage_degraded("ledger", category.as_str());
        }
        if let Some(store) = &self.memory_store {
            let _ = store.insert(crate::memory::MemoryInsert {
                memory_type: crate::memory::MemoryType::Episodic,
                facet: None,
                scope: crate::memory::MemoryScope::Session,
                scope_id: self.engine_session_id.clone(),
                content: format!(
                    "Backend handoff from {} to {}:\n{}",
                    previous, backend, summary
                ),
                confidence: 0.85,
                source_backend: Some(previous.to_string()),
                source_session_id: Some(self.engine_session_id.clone()),
                source_execution_id: None,
                metadata_json: Some("{\"kind\":\"handoff\"}".to_string()),
                ttl_days: 7,
                supersedes: None,
            });
        }
        Some(summary)
    }

    /// Store the final result of a turn in the session ledger.
    ///
    /// The ledger is authoritative for session/turn history, so unlike the
    /// auxiliary stores a failure here is logged as an error — but it still
    /// must not fail a turn whose model output already succeeded.
    pub(super) async fn record_ledger_turn(
        &self,
        backend: AcpBackend,
        execution_id: Option<&str>,
        request_hash: &str,
        output: &str,
        status: &str,
    ) {
        let Some(ledger) = self.session_ledger_store.clone() else {
            return;
        };
        let session_id = self.engine_session_id.clone();
        let backend_name = backend.to_string();
        let execution_id = execution_id.map(str::to_string);
        let request_hash = request_hash.to_string();
        let summary = crate::utils::summarize(output, 500);
        let status = status.to_string();
        let result = tokio::task::spawn_blocking(move || {
            ledger.record_turn(
                &session_id,
                &backend_name,
                execution_id.as_deref(),
                &request_hash,
                &summary,
                &status,
            )
        })
        .await;
        warn_on_ledger_failure("record_turn", result, &self.engine_session_id);
    }
}

/// Logs a ledger write failure without propagating it.
///
/// Ledger writes record history *about* a turn that has already produced its
/// answer; failing the turn at this point would discard output the user
/// already has. Every failure is logged with its session and store so the gap
/// is diagnosable.
fn warn_on_ledger_failure<T>(
    operation: &'static str,
    result: Result<Result<T>, tokio::task::JoinError>,
    session_id: &str,
) {
    let outcome = match result {
        Ok(inner) => inner,
        Err(join_error) => {
            tracing::error!(
                store = "ledger",
                operation,
                session_id,
                error = %join_error,
                "ledger write task failed"
            );
            return;
        }
    };
    if let Err(error) = outcome {
        tracing::error!(
            store = "ledger",
            operation,
            session_id,
            error = %error,
            "ledger write failed"
        );
    }
}
