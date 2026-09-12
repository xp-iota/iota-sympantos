use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};

use iota_core::acp::AcpBackend;
use iota_core::config::{self, backend_config, backend_process_env_with_context};
use iota_kanban::types::Status;
use iota_kanban::{
    AdvancedBridge, Dispatcher, DispatcherConfig, KanbanStore, SqliteKanbanStore,
    default_pull_source, export_event_bundle, import_event_bundle, migrate_v1_bundle,
    pull_event_bundle, push_event_bundle, read_event_bundle, serve_event_sync, write_event_bundle,
};

pub(super) fn run_kanban_command(args: &[String]) -> Result<()> {
    let store_path = default_kanban_dir().join("iota.db");
    let shadows_dir = default_kanban_dir().join("shadows");
    let store = Arc::new(SqliteKanbanStore::open(&store_path)?);
    let bridge = AdvancedBridge::new(PathBuf::from("hermes"), shadows_dir);

    for line in execute_kanban_command(args, store.as_ref(), &bridge)? {
        println!("{line}");
    }
    Ok(())
}

fn execute_kanban_command(
    args: &[String],
    store: &dyn KanbanStore,
    bridge: &AdvancedBridge,
) -> Result<Vec<String>> {
    match args.first().map(String::as_str) {
        Some("create-board") => {
            let slug = args
                .get(1)
                .context("Usage: iota kanban create-board <slug> <name>")?
                .clone();
            let name = args
                .get(2)
                .context("Usage: iota kanban create-board <slug> <name>")?
                .clone();
            let board_id = store.create_board(&slug, &name)?;
            Ok(vec![format!("Created board #{board_id} ({slug})")])
        }
        Some("create-task") => {
            let board_id: u64 = args
                .get(1)
                .context("Usage: iota kanban create-task <board-id> <title>")?
                .parse()
                .context("invalid board id")?;
            let title = args
                .get(2..)
                .filter(|s| !s.is_empty())
                .context("Usage: iota kanban create-task <board-id> <title>")?
                .join(" ");
            use iota_kanban::types::{CreateTaskRequest, Status as KStatus};
            let req = CreateTaskRequest {
                board_id,
                title: title.clone(),
                body: None,
                status: Some(KStatus::Triage),
                assignee: None,
                priority: None,
                tags: vec![],
                workspace_kind: None,
                workspace_path: None,
            };
            let task_id = store.create_task(req)?;
            Ok(vec![format!(
                "Created task #{task_id} on board #{board_id}: {title}"
            )])
        }
        Some("move") => {
            let task_id = parse_task_id(args.get(1), "Usage: iota kanban move <id> <status>")?;
            let status_str = args
                .get(2)
                .context("Usage: iota kanban move <id> <status>")?;
            let to: Status = status_str
                .parse()
                .with_context(|| format!("invalid status: {status_str}"))?;
            let from = store.get_task(task_id)?.status;
            store.transition(task_id, to)?;
            Ok(vec![format!("Task #{task_id} moved: {from} -> {to}",)])
        }
        Some("specify") => {
            let task_id = parse_task_id(args.get(1), "Usage: iota kanban specify <id>")?;
            ensure_bridge_available(bridge)?;
            let result = bridge
                .specify(task_id, store)
                .with_context(|| format!("failed to specify task #{task_id}"))?;
            Ok(vec![format!(
                "Specified task #{} ({} chars).",
                result.task_id,
                result.spec_body.chars().count()
            )])
        }
        Some("decompose") => {
            let task_id = parse_task_id(args.get(1), "Usage: iota kanban decompose <id>")?;
            ensure_bridge_available(bridge)?;
            let result = bridge
                .decompose(task_id, store)
                .with_context(|| format!("failed to decompose task #{task_id}"))?;
            Ok(vec![format!(
                "Decomposed task #{} into subtask(s): {}",
                result.parent_id,
                result
                    .child_ids
                    .iter()
                    .map(|id| format!("#{id}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )])
        }
        Some("export") | Some("export-events") => {
            let path = parse_path_arg(args.get(1), "Usage: iota kanban export <path> [cursor]")?;
            let cursor = args
                .get(2)
                .map(|value| value.parse::<u64>())
                .transpose()
                .context("invalid event cursor")?
                .unwrap_or(0);
            let source = hostname::get()
                .ok()
                .and_then(|value| value.into_string().ok())
                .unwrap_or_else(|| "local".to_string());
            let bundle = export_event_bundle(store, cursor, source)?;
            write_event_bundle(&path, &bundle)?;
            Ok(vec![format!(
                "Exported {} kanban event(s) to {} (source {}, sequence {}).",
                bundle.events.len(),
                path.display(),
                bundle.source_id,
                bundle.source_sequence
            )])
        }
        Some("import") | Some("import-events") => {
            let path = parse_path_arg(args.get(1), "Usage: iota kanban import <path>")?;
            let bundle = read_event_bundle(&path)?;
            let concrete = store
                .as_any()
                .downcast_ref::<SqliteKanbanStore>()
                .context("kanban event import requires SqliteKanbanStore")?;
            let report = import_event_bundle(concrete, &bundle)?;
            Ok(vec![format!(
                "Imported {}/{} kanban event(s) from {} ({} skipped, cursor {}).",
                report.events_applied,
                report.events_seen,
                report.source,
                report.events_skipped,
                report.cursor
            )])
        }
        Some("migrate-events") => {
            let path = parse_path_arg(
                args.get(1),
                "Usage: iota kanban migrate-events <v1-bundle-path>",
            )?;
            // v1 events carry no uuid, so migration assigns fresh ones. That
            // makes it non-idempotent: importing the same export twice would
            // import its events twice. The converted bundle is written to a v2
            // file and imported in the same call, so this should be run once
            // per v1 export.
            let bundle = migrate_v1_bundle(&path)?;
            let concrete = store
                .as_any()
                .downcast_ref::<SqliteKanbanStore>()
                .context("kanban event migration requires SqliteKanbanStore")?;
            let report = import_event_bundle(concrete, &bundle)?;
            Ok(vec![format!(
                "Migrated {}/{} event(s) from legacy bundle {} (source {}, cursor {}).",
                report.events_applied,
                report.events_seen,
                path.display(),
                report.source,
                report.cursor
            )])
        }
        Some("serve-sync") => {
            let addr = args.get(1).map(String::as_str).unwrap_or("127.0.0.1:47662");
            let concrete = store
                .as_any()
                .downcast_ref::<SqliteKanbanStore>()
                .context("kanban sync server requires SqliteKanbanStore")?;
            println!("Serving kanban sync on {addr}");
            serve_event_sync(Arc::new(concrete.clone()), addr)?;
            Ok(Vec::new())
        }
        Some("pull") => {
            let addr = args
                .get(1)
                .map(String::as_str)
                .context("Usage: iota kanban pull <addr> [cursor]")?;
            let cursor = args
                .get(2)
                .map(|value| value.parse::<u64>())
                .transpose()
                .context("invalid event cursor")?
                .unwrap_or(0);
            let source = default_pull_source(addr);
            let bundle = pull_event_bundle(addr, cursor, source)?;
            let concrete = store
                .as_any()
                .downcast_ref::<SqliteKanbanStore>()
                .context("kanban sync pull requires SqliteKanbanStore")?;
            let report = import_event_bundle(concrete, &bundle)?;
            Ok(vec![format!(
                "Pulled and imported {}/{} kanban event(s) from {} ({} skipped, cursor {}).",
                report.events_applied,
                report.events_seen,
                report.source,
                report.events_skipped,
                report.cursor
            )])
        }
        Some("push") => {
            let addr = args
                .get(1)
                .map(String::as_str)
                .context("Usage: iota kanban push <addr> [cursor]")?;
            let cursor = args
                .get(2)
                .map(|value| value.parse::<u64>())
                .transpose()
                .context("invalid event cursor")?
                .unwrap_or(0);
            let source = hostname::get()
                .ok()
                .and_then(|value| value.into_string().ok())
                .unwrap_or_else(|| "local".to_string());
            let bundle = export_event_bundle(store, cursor, source)?;
            let report = push_event_bundle(addr, bundle)?;
            Ok(vec![format!(
                "Pushed {}/{} kanban event(s) to {} ({} skipped, cursor {}).",
                report.events_applied,
                report.events_seen,
                addr,
                report.events_skipped,
                report.cursor
            )])
        }
        Some("dispatch") => {
            let task_id = parse_task_id(
                args.get(1),
                "Usage: iota kanban dispatch <id> [--timeout <secs>]",
            )?;

            // Optional --timeout <secs> (default 600)
            let timeout_secs: u64 = args[2..]
                .windows(2)
                .find(|w| w[0] == "--timeout")
                .and_then(|w| w[1].parse().ok())
                .unwrap_or(600);

            let task = store
                .get_task(task_id)
                .with_context(|| format!("task #{task_id} not found"))?;
            if task.status != Status::Ready {
                bail!(
                    "Task #{task_id} is in state '{}', not 'ready'. \
                     Move it first: iota kanban move {task_id} ready",
                    task.status
                );
            }

            let shadows_dir = default_kanban_dir().join("shadows");
            // Load hermes inference-provider config from nimia.yaml so the worker
            // has HERMES_INFERENCE_PROVIDER, HERMES_MODEL, etc. available.
            let hermes_env = config::read_config()
                .ok()
                .map(|cfg| {
                    let default_section = iota_core::config::BackendConfig::default();
                    let section = backend_config(&cfg, AcpBackend::Hermes);
                    let section_ref = section.unwrap_or(&default_section);
                    backend_process_env_with_context(AcpBackend::Hermes, section_ref, None)
                })
                .unwrap_or_default();
            let mut dispatcher = Dispatcher::new(DispatcherConfig {
                shadows_dir,
                tick_interval: std::time::Duration::from_millis(500),
                claim_ttl: std::time::Duration::from_secs(timeout_secs.max(60)),
                heartbeat_timeout: std::time::Duration::from_secs(timeout_secs.max(60)),
                max_concurrent: 1,
                hermes_bin: PathBuf::from("hermes"),
                extra_env: hermes_env,
                task_id_filter: Some(task_id),
                max_failures: 3,
            });

            println!("Dispatching task #{task_id}: {} ...", task.title);

            let poll = std::time::Duration::from_millis(500);
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
            let mut prev_status = Status::Ready;

            loop {
                let report = dispatcher.tick(store)?;
                if report.spawn_failures > 0 {
                    bail!("Failed to spawn hermes worker for task #{task_id}");
                }

                let current = store.get_task(task_id)?;
                if current.status != prev_status {
                    match current.status {
                        Status::Running => {
                            println!("  [dispatch] worker spawned (ready -> running)");
                        }
                        Status::Done => {
                            println!("  [dispatch] worker finished: done");
                            return Ok(vec![format!("Task #{task_id} dispatch complete: done")]);
                        }
                        Status::Blocked => {
                            println!("  [dispatch] worker finished: blocked");
                            return Ok(vec![format!("Task #{task_id} dispatch complete: blocked")]);
                        }
                        Status::Archived => {
                            return Ok(vec![format!(
                                "Task #{task_id} dispatch complete: archived"
                            )]);
                        }
                        Status::Ready if prev_status == Status::Running => {
                            // Worker exited non-zero — task reset to ready.
                            println!("  [dispatch] worker exited non-zero (task reset to ready)");
                        }
                        _ => {}
                    }
                    prev_status = current.status;
                }

                if std::time::Instant::now() >= deadline {
                    bail!(
                        "Dispatch timed out after {timeout_secs}s. \
                         Task #{task_id} is currently '{}'.",
                        store.get_task(task_id)?.status
                    );
                }

                std::thread::sleep(poll);
            }
        }
        Some("help") | Some("-h") | Some("--help") | None => Ok(vec![usage().to_string()]),
        Some(other) => bail!("unknown kanban command: {other}\n{}", usage()),
    }
}

fn ensure_bridge_available(bridge: &AdvancedBridge) -> Result<()> {
    if bridge.is_available() {
        Ok(())
    } else {
        bail!("advanced kanban bridge is not available: hermes command failed")
    }
}

fn parse_task_id(value: Option<&String>, usage: &str) -> Result<u64> {
    let Some(value) = value else {
        bail!("{usage}");
    };
    let value = value.strip_prefix('#').unwrap_or(value);
    value
        .parse::<u64>()
        .with_context(|| format!("invalid task id: {value}"))
}

fn parse_path_arg(value: Option<&String>, usage: &str) -> Result<PathBuf> {
    let Some(value) = value else {
        bail!("{usage}");
    };
    Ok(PathBuf::from(value))
}

fn default_kanban_dir() -> PathBuf {
    // Same resolution path as `TuiApp::new`, so `IOTA_KANBAN_DIR` relocates
    // both consistently.
    iota_core::config::paths::kanban_db_path()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| Path::new(".").to_path_buf())
}

fn usage() -> &'static str {
    "Usage:\n  iota kanban create-board <slug> <name>\n  iota kanban create-task <board-id> <title>\n  iota kanban move <id> <status>\n  iota kanban delete-task <id>\n  iota kanban remove-link <from> <to> <kind>\n  iota kanban dispatch <id> [--timeout <secs>]\n  iota kanban specify <id>\n  iota kanban decompose <id>\n  iota kanban export <path> [cursor]\n  iota kanban import <path>\n  iota kanban serve-sync [addr]\n  iota kanban pull <addr> [cursor]\n  iota kanban push <addr> [cursor]\n\nStatuses: triage -> todo -> ready -> running -> done -> archived (also: blocked)\n\ndispatch: spawns a hermes worker for a ready task, polls until done/blocked."
}

#[cfg(test)]
#[path = "kanban_cmd_tests.rs"]
mod tests;
