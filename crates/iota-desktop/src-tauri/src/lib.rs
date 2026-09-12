mod daemon_client;

use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::Emitter;
use tauri::Manager;
use tokio::sync::Mutex;

use iota_core::acp::AcpBackend;
use iota_core::config::{self, backend_config, backend_process_env_with_context};
use iota_kanban::{Dispatcher, DispatcherConfig, KanbanStore, SqliteKanbanStore, types::*};

pub struct AppState {
    pub kanban_store: Arc<Mutex<SqliteKanbanStore>>,
    pub kanban_dispatcher: Arc<Mutex<Dispatcher>>,
    pub shadows_dir: PathBuf,
}

/// Structured startup failures for the pre-Tauri-runtime bootstrap sequence
/// in [`run`] (result.md S-06). Each variant carries enough context to
/// print an actionable diagnostic; callers must not `.expect()`/panic on
/// these — [`run`] classifies every recoverable startup failure into one of
/// these variants and exits cleanly via [`StartupError::report_and_exit`]
/// instead of unwinding with an opaque panic message.
#[derive(Debug)]
enum StartupError {
    /// The user's home directory (and therefore `~/.i6/...`) could not be
    /// resolved — typically a misconfigured or minimal environment (e.g. a
    /// container without `HOME` set).
    HomeDirectoryUnresolved { detail: String },
    /// A required local directory (kanban db dir, shadows dir) could not be
    /// created — usually a permissions or disk-space issue.
    DirectoryCreationFailed { path: PathBuf, source: String },
    /// The Kanban SQLite store could not be opened — could be permissions,
    /// disk corruption, or another process holding an incompatible lock.
    StoreOpenFailed { path: PathBuf, source: String },
}

impl StartupError {
    fn category(&self) -> &'static str {
        match self {
            StartupError::HomeDirectoryUnresolved { .. } => "configuration",
            StartupError::DirectoryCreationFailed { .. } => "permission_or_filesystem",
            StartupError::StoreOpenFailed { .. } => "data_corruption_or_lock_conflict",
        }
    }

    fn diagnostic_message(&self) -> String {
        match self {
            StartupError::HomeDirectoryUnresolved { detail } => format!(
                "Could not resolve the user's home directory, so iota-desktop cannot locate its \
                 local data under ~/.i6. Detail: {detail}"
            ),
            StartupError::DirectoryCreationFailed { path, source } => format!(
                "Failed to create required directory '{}'. This is usually a filesystem \
                 permission problem or insufficient disk space. Detail: {source}",
                path.display()
            ),
            StartupError::StoreOpenFailed { path, source } => format!(
                "Failed to open the local Kanban database at '{}'. This can happen if the file \
                 is corrupted, another process is holding an incompatible lock on it, or \
                 permissions were changed unexpectedly. Detail: {source}",
                path.display()
            ),
        }
    }

    /// Prints a structured, categorized diagnostic to stderr and exits the
    /// process with a non-zero status. This is the only place [`run`]'s
    /// startup sequence terminates the process on error — every other
    /// startup failure path returns a `StartupError` up to [`run`] instead
    /// of calling `.expect()`/`panic!` directly, so the process always
    /// exits through this single, consistent, non-panicking path.
    fn report_and_exit(self) -> ! {
        eprintln!(
            "iota-desktop failed to start [{}]: {}",
            self.category(),
            self.diagnostic_message()
        );
        std::process::exit(1);
    }
}

#[derive(Debug, serde::Serialize, Clone)]
struct DesktopKanbanDispatchReport {
    spawned: usize,
    completed: usize,
    timed_out: usize,
    spawn_failures: usize,
    reclaimed: usize,
    active_workers: usize,
}

#[derive(Debug, serde::Serialize)]
struct DesktopKanbanTaskDetail {
    task: Task,
    board: Option<Board>,
    comments: Vec<Comment>,
    runs: Vec<Run>,
    links: Vec<Link>,
    events: Vec<KanbanEvent>,
    logs: DesktopKanbanTaskLogs,
}

#[derive(Debug, serde::Serialize)]
struct DesktopKanbanTaskLogs {
    stdout_path: String,
    stderr_path: String,
    stdout: String,
    stderr: String,
}

async fn tick_kanban_dispatcher(state: &AppState) -> Result<DesktopKanbanDispatchReport, String> {
    let store = state.kanban_store.clone();
    let dispatcher = state.kanban_dispatcher.clone();
    tokio::task::spawn_blocking(move || {
        let store = store.blocking_lock();
        let mut dispatcher = dispatcher.blocking_lock();
        let report = dispatcher.tick(&*store).map_err(|e| e.to_string())?;
        Ok(DesktopKanbanDispatchReport {
            spawned: report.spawned,
            completed: report.completed,
            timed_out: report.timed_out,
            spawn_failures: report.spawn_failures,
            reclaimed: report.reclaimed,
            active_workers: dispatcher.active_worker_count(),
        })
    })
    .await
    .map_err(|e| format!("kanban dispatcher task failed: {e}"))?
}

fn hermes_worker_env() -> std::collections::BTreeMap<String, String> {
    config::read_config()
        .ok()
        .map(|cfg| {
            let default_section = iota_core::config::BackendConfig::default();
            let section = backend_config(&cfg, AcpBackend::Hermes);
            let section_ref = section.unwrap_or(&default_section);
            backend_process_env_with_context(AcpBackend::Hermes, section_ref, None)
        })
        .unwrap_or_default()
}

fn event_mentions_task(event: &KanbanEvent, task_id: TaskId) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&event.payload) else {
        return false;
    };
    value_mentions_task(&value, task_id)
}

fn value_mentions_task(value: &serde_json::Value, task_id: TaskId) -> bool {
    match value {
        serde_json::Value::Object(object) => object.iter().any(|(key, value)| {
            matches!(key.as_str(), "task_id" | "from_id" | "to_id")
                && value.as_u64() == Some(task_id)
                || value_mentions_task(value, task_id)
        }),
        serde_json::Value::Array(items) => items
            .iter()
            .any(|value| value_mentions_task(value, task_id)),
        _ => false,
    }
}

fn read_tail(path: &std::path::Path, max_bytes: u64) -> Result<String, String> {
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(String::new()),
        Err(err) => return Err(format!("failed to open {}: {err}", path.display())),
    };
    let len = file
        .metadata()
        .map_err(|err| format!("failed to stat {}: {err}", path.display()))?
        .len();
    let start = len.saturating_sub(max_bytes);
    file.seek(SeekFrom::Start(start))
        .map_err(|err| format!("failed to seek {}: {err}", path.display()))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

fn task_logs(
    task_id: TaskId,
    shadows_dir: &std::path::Path,
) -> Result<DesktopKanbanTaskLogs, String> {
    let stdout_path = find_task_log_path(task_id, shadows_dir, "stdout");
    let stderr_path = find_task_log_path(task_id, shadows_dir, "stderr");
    Ok(DesktopKanbanTaskLogs {
        stdout_path: stdout_path.display().to_string(),
        stderr_path: stderr_path.display().to_string(),
        stdout: read_tail(&stdout_path, 64 * 1024)?,
        stderr: read_tail(&stderr_path, 64 * 1024)?,
    })
}

fn find_task_log_path(task_id: TaskId, shadows_dir: &std::path::Path, stream: &str) -> PathBuf {
    let file_name = format!("{task_id}.{stream}.log");
    let candidates = [
        shadows_dir.join(&file_name),
        shadows_dir.parent().unwrap_or(shadows_dir).join(&file_name),
    ];
    candidates
        .iter()
        .find(|path| path.exists())
        .cloned()
        .unwrap_or_else(|| candidates[0].clone())
}

#[tauri::command]
async fn get_config() -> Result<iota_core::daemon::DesktopConfigSnapshot, String> {
    let messages =
        daemon_client::send_or_queue(iota_core::daemon::DaemonClientMessage::GetConfig {
            request_id: None,
        })
        .await
        .map_err(|e| e.to_string())?;
    messages
        .into_iter()
        .find_map(|message| match message {
            iota_core::daemon::DaemonServerMessage::ConfigSnapshot { config } => Some(config),
            _ => None,
        })
        .ok_or_else(|| "daemon did not return config snapshot".to_string())
}

#[tauri::command]
async fn save_backend_model(
    backend_str: String,
    model: iota_core::daemon::DesktopModelConfig,
) -> Result<iota_core::daemon::DesktopConfigSnapshot, String> {
    let messages =
        daemon_client::send_or_queue(iota_core::daemon::DaemonClientMessage::SaveBackendModel {
            request_id: None,
            backend: backend_str,
            model,
        })
        .await
        .map_err(|e| e.to_string())?;
    messages
        .into_iter()
        .find_map(|message| match message {
            iota_core::daemon::DaemonServerMessage::ConfigSnapshot { config } => Some(config),
            _ => None,
        })
        .ok_or_else(|| "daemon did not return config snapshot".to_string())
}

#[tauri::command]
async fn submit_prompt(
    prompt: String,
    backend_str: String,
    turn_id: Option<String>,
    window: tauri::Window,
) -> Result<String, String> {
    let home = dirs::home_dir().ok_or_else(|| "Could not find home directory".to_string())?;
    let cwd = std::env::current_dir().unwrap_or(home);
    let turn_id = turn_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    daemon_client::start_turn(window, turn_id.clone(), cwd, backend_str, prompt)
        .await
        .map_err(|e| e.to_string())?;
    Ok(turn_id)
}

#[tauri::command]
async fn handle_approval(req_id: String, approved: bool) -> Result<(), String> {
    let messages =
        daemon_client::send_or_queue(iota_core::daemon::DaemonClientMessage::RespondApproval {
            request_id: None,
            approval_id: req_id,
            approved,
        })
        .await
        .map_err(|e| e.to_string())?;

    let accepted = messages.into_iter().find_map(|message| match message {
        iota_core::daemon::DaemonServerMessage::ApprovalResponded { accepted, .. } => {
            Some(accepted)
        }
        _ => None,
    });
    match accepted {
        Some(true) => Ok(()),
        Some(false) => Err("approval request was not pending".to_string()),
        None => Err("daemon did not acknowledge approval response".to_string()),
    }
}

#[tauri::command]
async fn cancel_turn(turn_id: String, window: tauri::Window) -> Result<(), String> {
    let messages =
        daemon_client::send_or_queue(iota_core::daemon::DaemonClientMessage::CancelTurn {
            request_id: None,
            turn_id: turn_id.clone(),
        })
        .await
        .map_err(|e| e.to_string())?;

    let mut accepted = None;
    for message in messages {
        if let iota_core::daemon::DaemonServerMessage::TurnCancelled {
            accepted: value, ..
        } = &message
        {
            accepted = Some(*value);
            let _ = window.emit("daemon-message", message);
        }
    }
    match accepted {
        Some(true) => Ok(()),
        Some(false) => Err(format!("turn {} is not active", turn_id)),
        None => Err("daemon did not acknowledge turn cancellation".to_string()),
    }
}

#[tauri::command]
async fn check_backend(
    backend_str: String,
) -> Result<iota_core::daemon::DaemonServerMessage, String> {
    daemon_client::send_or_queue(iota_core::daemon::DaemonClientMessage::CheckBackend {
        request_id: None,
        backend: backend_str,
    })
    .await
    .map_err(|e| e.to_string())?
    .into_iter()
    .find(|message| {
        matches!(
            message,
            iota_core::daemon::DaemonServerMessage::BackendCheckResult { .. }
        )
    })
    .ok_or_else(|| "daemon did not return backend check result".to_string())
}

#[tauri::command]
async fn get_observability_summary() -> Result<serde_json::Value, String> {
    let home = dirs::home_dir().ok_or_else(|| "Could not find home directory".to_string())?;
    let cwd = std::env::current_dir().unwrap_or(home);
    daemon_client::send_or_queue(
        iota_core::daemon::DaemonClientMessage::GetObservabilitySummary {
            cwd: Some(cwd),
            request_id: None,
        },
    )
    .await
    .map_err(|e| e.to_string())?
    .into_iter()
    .find_map(|message| match message {
        iota_core::daemon::DaemonServerMessage::ObservabilitySummary { summary } => Some(summary),
        _ => None,
    })
    .map(|summary| serde_json::to_value(summary).map_err(|e| e.to_string()))
    .transpose()?
    .ok_or_else(|| "daemon did not return observability summary".to_string())
}

#[tauri::command]
async fn get_memory_context_snapshot(
    scope_mode: String,
) -> Result<iota_core::daemon::DesktopMemoryContextSnapshot, String> {
    let scope_mode = match scope_mode.as_str() {
        "all" => iota_core::daemon::DesktopMemoryScopeMode::All,
        _ => iota_core::daemon::DesktopMemoryScopeMode::Workspace,
    };
    let home = dirs::home_dir().ok_or_else(|| "Could not find home directory".to_string())?;
    let cwd = std::env::current_dir().unwrap_or(home);

    let messages = daemon_client::send_or_queue(
        iota_core::daemon::DaemonClientMessage::GetMemoryContextSnapshot {
            cwd,
            scope_mode,
            request_id: None,
        },
    )
    .await
    .map_err(|e| e.to_string())?;

    for message in messages {
        match message {
            iota_core::daemon::DaemonServerMessage::MemoryContextSnapshot { snapshot } => {
                return Ok(snapshot);
            }
            iota_core::daemon::DaemonServerMessage::ProtocolError { message, .. } => {
                return Err(message);
            }
            _ => {}
        }
    }
    Err("daemon did not return memory context snapshot".to_string())
}

#[tauri::command]
fn current_workspace() -> Result<String, String> {
    std::env::current_dir()
        .map(|cwd| cwd.display().to_string())
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_boards(state: tauri::State<'_, AppState>) -> Result<Vec<Board>, String> {
    let store = state.kanban_store.lock().await;
    store.list_boards().map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_tasks(
    filter: TaskFilter,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<Task>, String> {
    let store = state.kanban_store.lock().await;
    store.list_tasks(filter).map_err(|e| e.to_string())
}

#[tauri::command]
async fn create_task(
    req: CreateTaskRequest,
    state: tauri::State<'_, AppState>,
) -> Result<TaskId, String> {
    let task_id = {
        let store = state.kanban_store.lock().await;
        store.create_task(req).map_err(|e| e.to_string())?
    };
    let _ = tick_kanban_dispatcher(state.inner()).await;
    Ok(task_id)
}

#[tauri::command]
async fn transition_task(
    task_id: TaskId,
    to_status: Status,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    {
        let store = state.kanban_store.lock().await;
        store
            .transition(task_id, to_status)
            .map_err(|e| e.to_string())?;
    }
    let _ = tick_kanban_dispatcher(state.inner()).await;
    Ok(())
}

#[tauri::command]
async fn update_kanban_task(
    task_id: TaskId,
    patch: TaskPatch,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    {
        let store = state.kanban_store.lock().await;
        store
            .update_task(task_id, patch)
            .map_err(|e| e.to_string())?;
    }
    let _ = tick_kanban_dispatcher(state.inner()).await;
    Ok(())
}

#[derive(Debug, serde::Deserialize)]
struct DesktopKanbanLinkRequest {
    from_id: TaskId,
    to_id: TaskId,
    kind: LinkKind,
}

#[tauri::command]
async fn create_kanban_link(
    req: DesktopKanbanLinkRequest,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let store = state.kanban_store.lock().await;
    store
        .create_link(req.from_id, req.to_id, req.kind)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn remove_kanban_link(
    req: DesktopKanbanLinkRequest,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let store = state.kanban_store.lock().await;
    store
        .remove_link(req.from_id, req.to_id, req.kind)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn dispatch_kanban(
    window: tauri::Window,
    state: tauri::State<'_, AppState>,
) -> Result<DesktopKanbanDispatchReport, String> {
    let report = tick_kanban_dispatcher(state.inner()).await?;
    let _ = window.emit("kanban-updated", report.clone());
    Ok(report)
}

#[tauri::command]
async fn get_kanban_task_detail(
    task_id: TaskId,
    state: tauri::State<'_, AppState>,
) -> Result<DesktopKanbanTaskDetail, String> {
    let store = state.kanban_store.lock().await;
    let task = store.get_task(task_id).map_err(|e| e.to_string())?;
    let board = store
        .list_boards()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|board| board.id == task.board_id);
    let comments = store.list_comments(task_id).map_err(|e| e.to_string())?;
    let runs = store.get_runs(task_id).map_err(|e| e.to_string())?;
    let links = store.get_links(task_id).map_err(|e| e.to_string())?;
    let mut events: Vec<KanbanEvent> = store
        .events_since(0)
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|event| event_mentions_task(event, task_id))
        .collect();
    events.sort_by_key(|event| std::cmp::Reverse(event.id));
    events.truncate(30);
    let logs = task_logs(task_id, &state.shadows_dir)?;
    Ok(DesktopKanbanTaskDetail {
        task,
        board,
        comments,
        runs,
        links,
        events,
        logs,
    })
}

#[tauri::command]
async fn list_comments(
    task_id: TaskId,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<Comment>, String> {
    let store = state.kanban_store.lock().await;
    store.list_comments(task_id).map_err(|e| e.to_string())
}

#[tauri::command]
async fn add_comment(
    task_id: TaskId,
    author: String,
    body: String,
    state: tauri::State<'_, AppState>,
) -> Result<CommentId, String> {
    let store = state.kanban_store.lock().await;
    store
        .add_comment(task_id, &author, &body)
        .map_err(|e| e.to_string())
}

/// Resolves and prepares the local Kanban store/dispatcher/shadows
/// directory, classifying every recoverable failure into a
/// [`StartupError`] instead of panicking (result.md S-06 / AC10.1-AC10.3).
/// Split out of [`run`] so the fallible bootstrap sequence can use `?`
/// internally and `run` only has one place (`report_and_exit`) that ever
/// terminates the process on a startup error.
fn bootstrap_kanban_state() -> Result<(SqliteKanbanStore, Dispatcher, PathBuf), StartupError> {
    let store_path = iota_core::config::paths::kanban_db_path().ok_or_else(|| {
        StartupError::HomeDirectoryUnresolved {
            detail: "kanban_db_path() returned None (dirs::home_dir() failed)".to_string(),
        }
    })?;
    let kanban_dir = store_path
        .parent()
        .ok_or_else(|| StartupError::HomeDirectoryUnresolved {
            detail: format!(
                "resolved kanban db path '{}' unexpectedly has no parent directory",
                store_path.display()
            ),
        })?
        .to_path_buf();
    std::fs::create_dir_all(&kanban_dir).map_err(|err| StartupError::DirectoryCreationFailed {
        path: kanban_dir.clone(),
        source: err.to_string(),
    })?;

    let shadows_dir = iota_core::config::paths::kanban_shadows_dir().ok_or_else(|| {
        StartupError::HomeDirectoryUnresolved {
            detail: "kanban_shadows_dir() returned None (dirs::home_dir() failed)".to_string(),
        }
    })?;
    std::fs::create_dir_all(&shadows_dir).map_err(|err| StartupError::DirectoryCreationFailed {
        path: shadows_dir.clone(),
        source: err.to_string(),
    })?;

    let store =
        SqliteKanbanStore::open(&store_path).map_err(|err| StartupError::StoreOpenFailed {
            path: store_path.clone(),
            source: err.to_string(),
        })?;
    let dispatcher = Dispatcher::new(DispatcherConfig {
        shadows_dir: shadows_dir.clone(),
        extra_env: hermes_worker_env(),
        ..Default::default()
    });

    Ok((store, dispatcher, shadows_dir))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
#[allow(clippy::collapsible_if)]
pub fn run() {
    let (store, dispatcher, shadows_dir) = match bootstrap_kanban_state() {
        Ok(state) => state,
        Err(err) => err.report_and_exit(),
    };

    // Seed initial board and tasks if database is empty
    if let Ok(true) = store.list_boards().map(|b| b.is_empty()) {
        if let Ok(board_id) = store.create_board("iota-proj", "Iota Sympantos Core") {
            let _ = store.create_task(CreateTaskRequest {
                board_id,
                title: "配置 ACP 代理后端 (Gemini / Claude)".to_string(),
                body: Some("配置 nimia.yaml，验证各 AI 后端的连接与自动权限机制。".to_string()),
                status: Some(Status::Todo),
                assignee: Some("Developer".to_string()),
                priority: Some(1),
                tags: vec!["configuration".to_string(), "backend".to_string()],
                workspace_kind: None,
                workspace_path: None,
            });
            let _ = store.create_task(CreateTaskRequest {
                board_id,
                title: "开发 Tauri 桌面应用大盘".to_string(),
                body: Some(
                    "使用 React + Tailwind CSS v4 开发 iota-desktop 前端视图与交互组件。"
                        .to_string(),
                ),
                status: Some(Status::Ready),
                assignee: Some("Developer".to_string()),
                priority: Some(2),
                tags: vec!["frontend".to_string(), "gui".to_string()],
                workspace_kind: None,
                workspace_path: None,
            });
            let _ = store.create_task(CreateTaskRequest {
                board_id,
                title: "实现多节点事件同步协议".to_string(),
                body: Some(
                    "编写基于 TCP / HTTP 的 Kanban 变更事件同步 implementation。".to_string(),
                ),
                status: Some(Status::Triage),
                assignee: None,
                priority: Some(3),
                tags: vec!["sync".to_string(), "networking".to_string()],
                workspace_kind: None,
                workspace_path: None,
            });
            let _ = store.create_task(CreateTaskRequest {
                board_id,
                title: "完成 CLI/TUI 的 Rust 模块拆分".to_string(),
                body: Some(
                    "将单一包拆分为 iota-core 与 iota-cli 并通过 Cargo 统一编排。".to_string(),
                ),
                status: Some(Status::Done),
                assignee: Some("System".to_string()),
                priority: Some(0),
                tags: vec!["refactor".to_string(), "core".to_string()],
                workspace_kind: None,
                workspace_path: None,
            });
        }
    }

    if let Err(error) = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            kanban_store: Arc::new(Mutex::new(store)),
            kanban_dispatcher: Arc::new(Mutex::new(dispatcher)),
            shadows_dir,
        })
        .setup(|app| {
            let store = app.state::<AppState>().kanban_store.clone();
            let dispatcher = app.state::<AppState>().kanban_dispatcher.clone();
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(daemon_client::start_heartbeat_loop(app_handle.clone()));
            tauri::async_runtime::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(3));
                loop {
                    interval.tick().await;
                    let store = store.clone();
                    let dispatcher = dispatcher.clone();
                    let result = tokio::task::spawn_blocking(move || {
                        let store = store.blocking_lock();
                        let mut dispatcher = dispatcher.blocking_lock();
                        let report = dispatcher.tick(&*store)?;
                        Ok::<_, anyhow::Error>((report, dispatcher.active_worker_count()))
                    })
                    .await;
                    if let Ok(Ok((report, active_workers))) = result {
                        if report.spawned > 0
                            || report.completed > 0
                            || report.timed_out > 0
                            || report.spawn_failures > 0
                            || report.reclaimed > 0
                            || active_workers > 0
                        {
                            let _ = app_handle.emit(
                                "kanban-updated",
                                DesktopKanbanDispatchReport {
                                    spawned: report.spawned,
                                    completed: report.completed,
                                    timed_out: report.timed_out,
                                    spawn_failures: report.spawn_failures,
                                    reclaimed: report.reclaimed,
                                    active_workers,
                                },
                            );
                        }
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_backend_model,
            submit_prompt,
            handle_approval,
            cancel_turn,
            check_backend,
            get_observability_summary,
            get_memory_context_snapshot,
            current_workspace,
            list_boards,
            list_tasks,
            create_task,
            transition_task,
            update_kanban_task,
            create_kanban_link,
            remove_kanban_link,
            dispatch_kanban,
            get_kanban_task_detail,
            list_comments,
            add_comment,
        ])
        .run(tauri::generate_context!())
    {
        eprintln!("iota-desktop runtime terminated with an initialization error: {error}");
    }
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
