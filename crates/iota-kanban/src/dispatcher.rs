use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use super::shadow::{ShadowMaterializer, ShadowWatcher};
use super::store::KanbanStore;
use super::types::*;
use super::worker::{WorkerConfig, WorkerEnv, WorkerHandle};
use crate::paths::default_shadows_dir;

// ---------------------------------------------------------------------------
// DispatcherConfig
// ---------------------------------------------------------------------------

pub struct DispatcherConfig {
    /// How often tick() is expected to be called (informational).
    pub tick_interval: Duration,
    /// Maximum number of concurrent workers.
    pub max_concurrent: usize,
    /// Kill a worker that has been running for this long regardless of heartbeats.
    pub claim_ttl: Duration,
    /// Kill a worker whose run has not sent a heartbeat within this window.
    pub heartbeat_timeout: Duration,
    /// Path to the hermes binary.
    pub hermes_bin: PathBuf,
    /// Directory where shadow databases are materialised.
    pub shadows_dir: PathBuf,
    /// Extra env vars forwarded to every hermes worker (e.g. inference-provider config).
    pub extra_env: std::collections::BTreeMap<String, String>,
    /// When set, only spawn workers for this task ID (used by `iota kanban dispatch <id>`).
    pub task_id_filter: Option<TaskId>,
    /// Move a task to Blocked after this many failed/timed-out runs.
    pub max_failures: usize,
}

impl Default for DispatcherConfig {
    fn default() -> Self {
        Self {
            tick_interval: Duration::from_secs(30),
            max_concurrent: 4,
            claim_ttl: Duration::from_secs(900),         // 15 min
            heartbeat_timeout: Duration::from_secs(300), // 5 min — hermes -z runs can take >90s
            hermes_bin: PathBuf::from("hermes"),
            shadows_dir: default_shadows_dir(),
            extra_env: std::collections::BTreeMap::new(),
            task_id_filter: None,
            max_failures: 3,
        }
    }
}

// ---------------------------------------------------------------------------
// TickReport
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct TickReport {
    pub spawned: usize,
    pub completed: usize,
    pub timed_out: usize,
    pub spawn_failures: usize,
    pub reclaimed: usize,
}

// ---------------------------------------------------------------------------
// Dispatcher
// ---------------------------------------------------------------------------

pub struct Dispatcher {
    config: DispatcherConfig,
    workers: HashMap<TaskId, (WorkerHandle, ShadowWatcher)>,
    materializer: ShadowMaterializer,
    worker_config: WorkerConfig,
}

impl Dispatcher {
    pub fn new(config: DispatcherConfig) -> Self {
        let materializer = ShadowMaterializer::new(config.shadows_dir.clone());
        let worker_config = WorkerConfig {
            hermes_bin: config.hermes_bin.clone(),
            extra_env: config.extra_env.clone(),
        };
        Self {
            config,
            workers: HashMap::new(),
            materializer,
            worker_config,
        }
    }

    pub fn tick_interval(&self) -> Duration {
        self.config.tick_interval
    }

    #[cfg(test)]
    pub fn set_hermes_bin_for_tests(&mut self, hermes_bin: PathBuf) {
        self.config.hermes_bin = hermes_bin.clone();
        self.worker_config.hermes_bin = hermes_bin;
    }

    /// Number of workers currently active.
    pub fn active_worker_count(&self) -> usize {
        self.workers.len()
    }

    // -----------------------------------------------------------------------
    // tick
    // -----------------------------------------------------------------------

    /// One iteration of the dispatcher loop.
    pub fn tick(&mut self, store: &dyn KanbanStore) -> Result<TickReport> {
        let mut report = self.health_check(store)?;

        report.reclaimed += self.reclaim_expired_running(store)?;

        self.recompute_ready(store)?;

        // Spawn new workers for Ready tasks up to the concurrency limit
        let available = self
            .config
            .max_concurrent
            .saturating_sub(self.workers.len());
        if available > 0 {
            if let Some(filter_id) = self.config.task_id_filter {
                // Single-task mode: query the target task directly to avoid
                // limit() cutting it off when lower-id tasks are also ready.
                if let Ok(task) = store.get_task(filter_id)
                    && task.status == Status::Ready
                    && !self.workers.contains_key(&task.id)
                {
                    match self.spawn_worker(&task, store) {
                        Ok(()) => report.spawned += 1,
                        Err(e) => {
                            tracing::warn!(
                                task_id = task.id,
                                error = %e,
                                "failed to spawn worker"
                            );
                            report.spawn_failures += 1;
                        }
                    }
                }
            } else {
                let ready_tasks = store.list_tasks(TaskFilter {
                    status: Some(Status::Ready),
                    limit: Some(available),
                    ..Default::default()
                })?;

                for task in ready_tasks {
                    if self.workers.len() >= self.config.max_concurrent {
                        break;
                    }
                    if self.workers.contains_key(&task.id) {
                        continue;
                    }
                    match self.spawn_worker(&task, store) {
                        Ok(()) => report.spawned += 1,
                        Err(e) => {
                            tracing::warn!(
                                task_id = task.id,
                                error = %e,
                                "failed to spawn worker"
                            );
                            report.spawn_failures += 1;
                        }
                    }
                }
            }
        }

        Ok(report)
    }

    // -----------------------------------------------------------------------
    // health_check
    // -----------------------------------------------------------------------

    fn health_check(&mut self, store: &dyn KanbanStore) -> Result<TickReport> {
        let mut report = TickReport::default();
        let task_ids: Vec<TaskId> = self.workers.keys().copied().collect();
        let mut to_remove: Vec<TaskId> = Vec::new();

        for task_id in task_ids {
            // Collect handle info and poll watcher in a single borrow, then
            // release before calling store methods (which may re-lock internals).
            let Some(entry) = self.workers.get_mut(&task_id) else {
                continue; // entry was removed by a previous iteration (shouldn't happen)
            };
            let run_id = entry.0.run_id.clone();
            let elapsed = entry.0.elapsed_secs();
            let exit_code = entry.0.is_finished();
            let (events, terminal_status) = entry.1.poll().unwrap_or_default();

            // Sync events into the store
            if !events.is_empty()
                && let Some(entry) = self.workers.get_mut(&task_id)
            {
                match entry.1.sync_events(&events, store, &run_id) {
                    Ok(()) => entry.1.mark_events_synced(&events),
                    Err(e) => {
                        tracing::warn!(
                            task_id,
                            run_id = %run_id,
                            error = %e,
                            "failed to sync shadow events"
                        );
                    }
                }
            }

            // --- Claim TTL exceeded ---
            if elapsed > self.config.claim_ttl.as_secs() {
                if let Some(entry) = self.workers.get_mut(&task_id)
                    && let Err(e) = entry.0.kill()
                {
                    tracing::warn!(task_id, error = %e, "Failed to kill expired worker process");
                }
                if let Err(e) = store.complete_run(&run_id, RunStatus::TimedOut, None) {
                    tracing::error!(task_id, run_id = %run_id, error = %e, "Failed to complete expired run in store");
                }
                self.transition_after_failure(store, task_id);
                if let Err(e) = self.materializer.cleanup(task_id) {
                    tracing::warn!(task_id, error = %e, "Failed to cleanup shadow materializer after claim TTL expiration");
                }
                report.timed_out += 1;
                to_remove.push(task_id);
                continue;
            }

            // --- Heartbeat timeout ---
            let heartbeat_expired = store
                .get_runs(task_id)
                .ok()
                .and_then(|runs| runs.into_iter().find(|r| r.id == run_id))
                .map(|run| {
                    let now = crate::utils::now_ts();
                    (now - run.last_heartbeat) > self.config.heartbeat_timeout.as_secs() as i64
                })
                .unwrap_or(false);

            if heartbeat_expired {
                if let Some(entry) = self.workers.get_mut(&task_id)
                    && let Err(e) = entry.0.kill()
                {
                    tracing::warn!(task_id, error = %e, "Failed to kill heartbeat-timeout worker process");
                }
                if let Err(e) = store.complete_run(&run_id, RunStatus::TimedOut, None) {
                    tracing::error!(task_id, run_id = %run_id, error = %e, "Failed to complete heartbeat-timeout run in store");
                }
                self.transition_after_failure(store, task_id);
                if let Err(e) = self.materializer.cleanup(task_id) {
                    tracing::warn!(task_id, error = %e, "Failed to cleanup shadow materializer after heartbeat timeout");
                }
                report.timed_out += 1;
                to_remove.push(task_id);
                continue;
            }

            // --- Terminal status from shadow or process has exited ---
            if terminal_status.is_some() || exit_code.is_some() {
                if terminal_status.is_some()
                    && exit_code.is_none()
                    && let Some(entry) = self.workers.get_mut(&task_id)
                    && let Err(e) = entry.0.kill()
                {
                    tracing::warn!(task_id, error = %e, "Failed to kill active worker process after terminal status");
                }
                let failed = exit_code.map(|c| c != 0).unwrap_or(false);
                let rs = if failed {
                    RunStatus::Failed
                } else {
                    RunStatus::Completed
                };
                if let Err(e) = store.complete_run(&run_id, rs, exit_code) {
                    tracing::error!(task_id, run_id = %run_id, error = %e, "Failed to complete run in store");
                }
                if failed {
                    self.transition_after_failure(store, task_id);
                } else if let Some(status) = terminal_status.as_deref() {
                    match status {
                        "done" => {
                            if let Err(e) = store.transition(task_id, Status::Done) {
                                tracing::error!(task_id, error = %e, "Failed to transition completed task to Done");
                            }
                        }
                        "blocked" => {
                            if let Err(e) = store.transition(task_id, Status::Blocked) {
                                tracing::error!(task_id, error = %e, "Failed to transition blocked task to Blocked");
                            }
                        }
                        _ => {}
                    }
                } else {
                    // Process exited with code 0 but hermes never wrote a terminal status
                    // to the shadow DB (e.g. it completed internal work without calling
                    // kanban_complete).  Treat as Done rather than leaving the task
                    // Running with no active worker.
                    tracing::debug!(
                        task_id,
                        run_id = %run_id,
                        "worker exited 0 without shadow terminal status; marking Done"
                    );
                    if let Err(e) = store.transition(task_id, Status::Done) {
                        tracing::error!(task_id, error = %e, "Failed to transition task to Done after zero exit");
                    }
                }
                if let Err(e) = self.materializer.cleanup(task_id) {
                    tracing::warn!(task_id, error = %e, "Failed to cleanup shadow materializer");
                }
                report.completed += 1;
                to_remove.push(task_id);
            }
        }

        for task_id in to_remove {
            self.workers.remove(&task_id);
        }

        Ok(report)
    }

    // -----------------------------------------------------------------------
    // recompute_ready
    // -----------------------------------------------------------------------

    fn recompute_ready(&mut self, store: &dyn KanbanStore) -> Result<()> {
        let blocked = store.list_tasks(TaskFilter {
            status: Some(Status::Blocked),
            ..Default::default()
        })?;

        for task in blocked {
            let links = store.get_links(task.id)?;

            // Collect all task IDs that are blocking this one
            let blockers: Vec<TaskId> = links
                .iter()
                .filter(|l| l.to_id == task.id && l.kind == LinkKind::Blocks)
                .map(|l| l.from_id)
                .collect();

            // If there are no blockers, nothing to check
            if blockers.is_empty() {
                continue;
            }

            // Unblock when every blocker has reached a terminal status
            let all_done = blockers.iter().all(|&blocker_id| {
                store
                    .get_task(blocker_id)
                    .map(|t| t.status == Status::Done || t.status == Status::Archived)
                    .unwrap_or(false)
            });

            if all_done && let Err(e) = store.transition(task.id, Status::Ready) {
                tracing::error!(task_id = task.id, error = %e, "Failed to transition unblocked task to Ready");
            }
        }

        Ok(())
    }

    fn reclaim_expired_running(&mut self, store: &dyn KanbanStore) -> Result<usize> {
        let running = store.list_tasks(TaskFilter {
            status: Some(Status::Running),
            ..Default::default()
        })?;
        let now = crate::utils::now_ts();
        let mut reclaimed = 0;

        for task in running {
            if self.workers.contains_key(&task.id) {
                continue;
            }
            let Some(claimed_at) = task.claimed_at else {
                continue;
            };
            let config_ttl = self.config.claim_ttl.as_secs() as i64;
            // 0 means "not configured on this task" — fall back to the dispatcher's
            // global config TTL rather than treating it as "expire immediately".
            let ttl = if task.claim_ttl_secs > 0 {
                std::cmp::min(task.claim_ttl_secs, config_ttl)
            } else {
                config_ttl
            };
            if now - claimed_at >= ttl {
                for run in store
                    .get_runs(task.id)?
                    .into_iter()
                    .filter(|run| run.status == RunStatus::Running)
                {
                    store.complete_run(&run.id, RunStatus::TimedOut, None)?;
                }
                store.transition(task.id, Status::Ready)?;
                reclaimed += 1;
            }
        }

        Ok(reclaimed)
    }

    // -----------------------------------------------------------------------
    // spawn_worker
    // -----------------------------------------------------------------------

    fn spawn_worker(&mut self, task: &Task, store: &dyn KanbanStore) -> Result<()> {
        let profile = task.assignee.as_deref().unwrap_or("default").to_string();

        // Transition to Running
        store.transition(task.id, Status::Running)?;

        // Create a run record
        let run_id = match store.create_run(task.id, &profile) {
            Ok(run_id) => run_id,
            Err(e) => {
                if let Err(te) = store.transition(task.id, Status::Ready) {
                    tracing::error!(task_id = task.id, error = %te, "Failed to transition task back to Ready after run creation failure");
                }
                return Err(e);
            }
        };

        let spawn_result = (|| -> Result<(WorkerHandle, ShadowWatcher)> {
            // Locate the board
            let boards = store.list_boards()?;
            let board = boards
                .into_iter()
                .find(|b| b.id == task.board_id)
                .ok_or_else(|| {
                    anyhow::anyhow!("board {} not found for task {}", task.board_id, task.id)
                })?;

            // Materialise the shadow DB (includes a task_runs entry)
            let shadow_db = self.materializer.materialize(task, &board, store)?;

            // Spawn the worker process (hermes reads context via kanban_show)
            let env = WorkerEnv {
                task_id: task.id,
                run_id: run_id.clone(),
                shadow_run_id: shadow_db.run_id,
                shadow_path: shadow_db.path.clone(),
                board_slug: board.slug,
                profile,
            };
            let handle = WorkerHandle::spawn(&self.worker_config, env)?;

            // Create a watcher for the shadow DB
            let watcher = ShadowWatcher::new(shadow_db.path, task.id);
            Ok((handle, watcher))
        })();

        let (handle, watcher) = match spawn_result {
            Ok(result) => result,
            Err(e) => {
                let _ = store.complete_run(&run_id, RunStatus::Failed, None);
                self.transition_after_failure(store, task.id);
                let _ = self.materializer.cleanup(task.id);
                return Err(e);
            }
        };

        self.workers.insert(task.id, (handle, watcher));
        Ok(())
    }

    fn transition_after_failure(&self, store: &dyn KanbanStore, task_id: TaskId) {
        let failure_count = store
            .get_runs(task_id)
            .map(|runs| {
                runs.into_iter()
                    .filter(|run| matches!(run.status, RunStatus::Failed | RunStatus::TimedOut))
                    .count()
            })
            .unwrap_or(0);
        let next_status = if failure_count >= self.config.max_failures {
            Status::Blocked
        } else {
            Status::Ready
        };
        if let Err(e) = store.transition(task_id, next_status) {
            tracing::error!(task_id, status = %next_status, error = %e, "Failed to transition task after worker failure");
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "dispatcher_tests.rs"]
mod tests;
