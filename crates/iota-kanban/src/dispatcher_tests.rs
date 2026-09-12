use crate::shadow::ShadowWatcher;
use crate::sqlite_store::SqliteKanbanStore;
use crate::store::KanbanStore;
use crate::worker::WorkerHandle;
use crate::{CreateTaskRequest, Dispatcher, DispatcherConfig, LinkKind, RunStatus, Status};
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

fn tmp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("iota-disp-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn dispatcher_config_defaults() {
    let cfg = DispatcherConfig::default();
    assert_eq!(cfg.tick_interval, Duration::from_secs(30));
    assert_eq!(cfg.max_concurrent, 4);
    assert_eq!(cfg.claim_ttl, Duration::from_secs(900));
    assert_eq!(cfg.heartbeat_timeout, Duration::from_secs(300));
    assert_eq!(cfg.hermes_bin, PathBuf::from("hermes"));
}

#[test]
fn dispatcher_new_has_no_workers() {
    let cfg = DispatcherConfig {
        shadows_dir: std::env::temp_dir().join("iota-disp-unused"),
        ..Default::default()
    };
    let d = Dispatcher::new(cfg);
    assert_eq!(d.active_worker_count(), 0);
}

#[test]
fn recompute_ready_unblocks_tasks() {
    let tmp = tmp_dir();
    let store = SqliteKanbanStore::open(&tmp.join("store.db")).unwrap();

    let board_id = store.create_board("test", "Test Board").unwrap();

    // Create blocker task and advance to Done
    let blocker_id = store
        .create_task(CreateTaskRequest {
            board_id,
            title: "Blocker".to_string(),
            body: None,
            status: None,
            assignee: None,
            priority: None,
            tags: vec![],
            workspace_kind: None,
            workspace_path: None,
        })
        .unwrap();
    store.transition(blocker_id, Status::Todo).unwrap();
    store.transition(blocker_id, Status::Ready).unwrap();
    store.transition(blocker_id, Status::Running).unwrap();
    store.transition(blocker_id, Status::Done).unwrap();

    // Create blocked task and advance to Blocked
    let blocked_id = store
        .create_task(CreateTaskRequest {
            board_id,
            title: "Blocked".to_string(),
            body: None,
            status: None,
            assignee: None,
            priority: None,
            tags: vec![],
            workspace_kind: None,
            workspace_path: None,
        })
        .unwrap();
    store.transition(blocked_id, Status::Todo).unwrap();
    store.transition(blocked_id, Status::Ready).unwrap();
    store.transition(blocked_id, Status::Running).unwrap();
    store.transition(blocked_id, Status::Blocked).unwrap();

    // Create a Blocks link: blocker_id blocks blocked_id
    store
        .create_link(blocker_id, blocked_id, LinkKind::Blocks)
        .unwrap();

    let cfg = DispatcherConfig {
        shadows_dir: tmp.join("shadows"),
        ..Default::default()
    };
    let mut dispatcher = Dispatcher::new(cfg);
    dispatcher.recompute_ready(&store).unwrap();

    let task = store.get_task(blocked_id).unwrap();
    assert_eq!(task.status, Status::Ready);
}

#[test]
fn spawn_failure_rolls_task_back_to_ready_and_fails_run() {
    let tmp = tmp_dir();
    let store = SqliteKanbanStore::open(&tmp.join("store.db")).unwrap();
    let board_id = store.create_board("test", "Test Board").unwrap();
    let task_id = store
        .create_task(CreateTaskRequest {
            board_id,
            title: "Ready task".to_string(),
            body: None,
            status: Some(Status::Ready),
            assignee: None,
            priority: None,
            tags: vec![],
            workspace_kind: None,
            workspace_path: None,
        })
        .unwrap();

    let cfg = DispatcherConfig {
        max_concurrent: 1,
        hermes_bin: PathBuf::from("/missing/hermes-for-iota-test"),
        shadows_dir: tmp.join("shadows"),
        ..Default::default()
    };
    let mut dispatcher = Dispatcher::new(cfg);
    let report = dispatcher.tick(&store).unwrap();

    assert_eq!(report.spawn_failures, 1);
    assert_eq!(dispatcher.active_worker_count(), 0);
    assert_eq!(store.get_task(task_id).unwrap().status, Status::Ready);
    let runs = store.get_runs(task_id).unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].status, RunStatus::Failed);
}

#[test]
fn tick_blocks_task_after_failure_limit() {
    let tmp = tmp_dir();
    let store = SqliteKanbanStore::open(&tmp.join("store.db")).unwrap();
    let board_id = store.create_board("test", "Test Board").unwrap();
    let task_id = store
        .create_task(CreateTaskRequest {
            board_id,
            title: "Ready task".to_string(),
            body: None,
            status: Some(Status::Ready),
            assignee: None,
            priority: None,
            tags: vec![],
            workspace_kind: None,
            workspace_path: None,
        })
        .unwrap();

    let cfg = DispatcherConfig {
        max_concurrent: 1,
        max_failures: 1,
        hermes_bin: PathBuf::from("/missing/hermes-for-iota-test"),
        shadows_dir: tmp.join("shadows"),
        ..Default::default()
    };
    let mut dispatcher = Dispatcher::new(cfg);
    let report = dispatcher.tick(&store).unwrap();

    assert_eq!(report.spawn_failures, 1);
    assert_eq!(store.get_task(task_id).unwrap().status, Status::Blocked);
    let runs = store.get_runs(task_id).unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].status, RunStatus::Failed);
}

#[test]
fn tick_reclaims_expired_running_tasks_without_worker_handle() {
    let tmp = tmp_dir();
    let store = SqliteKanbanStore::open(&tmp.join("store.db")).unwrap();
    let board_id = store.create_board("test", "Test Board").unwrap();
    let task_id = store
        .create_task(CreateTaskRequest {
            board_id,
            title: "Stale running task".to_string(),
            body: None,
            status: Some(Status::Ready),
            assignee: None,
            priority: None,
            tags: vec![],
            workspace_kind: None,
            workspace_path: None,
        })
        .unwrap();
    store.transition(task_id, Status::Running).unwrap();

    let cfg = DispatcherConfig {
        max_concurrent: 0,
        claim_ttl: Duration::from_secs(0),
        shadows_dir: tmp.join("shadows"),
        ..Default::default()
    };
    let mut dispatcher = Dispatcher::new(cfg);
    let report = dispatcher.tick(&store).unwrap();

    assert_eq!(report.reclaimed, 1);
    assert_eq!(store.get_task(task_id).unwrap().status, Status::Ready);
}

#[test]
fn reclaim_expired_running_task_closes_stale_running_runs() {
    let tmp = tmp_dir();
    let store = SqliteKanbanStore::open(&tmp.join("store.db")).unwrap();
    let board_id = store.create_board("test", "Test Board").unwrap();
    let task_id = store
        .create_task(CreateTaskRequest {
            board_id,
            title: "Stale running task".to_string(),
            body: None,
            status: Some(Status::Ready),
            assignee: None,
            priority: None,
            tags: vec![],
            workspace_kind: None,
            workspace_path: None,
        })
        .unwrap();
    store.transition(task_id, Status::Running).unwrap();
    let run_id = store.create_run(task_id, "default").unwrap();

    let cfg = DispatcherConfig {
        max_concurrent: 0,
        claim_ttl: Duration::from_secs(0),
        shadows_dir: tmp.join("shadows"),
        ..Default::default()
    };
    let mut dispatcher = Dispatcher::new(cfg);
    let report = dispatcher.tick(&store).unwrap();

    assert_eq!(report.reclaimed, 1);
    let runs = store.get_runs(task_id).unwrap();
    let run = runs.iter().find(|run| run.id == run_id).unwrap();
    assert_eq!(run.status, RunStatus::TimedOut);
    assert_eq!(store.get_task(task_id).unwrap().status, Status::Ready);
}

#[test]
fn terminal_shadow_status_updates_task_and_stops_live_worker() {
    let tmp = tmp_dir();
    let store = SqliteKanbanStore::open(&tmp.join("store.db")).unwrap();
    let board_id = store.create_board("test", "Test Board").unwrap();
    let task_id = store
        .create_task(CreateTaskRequest {
            board_id,
            title: "Running task".to_string(),
            body: None,
            status: Some(Status::Ready),
            assignee: None,
            priority: None,
            tags: vec![],
            workspace_kind: None,
            workspace_path: None,
        })
        .unwrap();
    store.transition(task_id, Status::Running).unwrap();
    let run_id = store.create_run(task_id, "test-profile").unwrap();

    let shadow_dir = tmp.join("shadows").join(task_id.to_string());
    std::fs::create_dir_all(&shadow_dir).unwrap();
    let shadow_path = shadow_dir.join("kanban.db");
    let conn = rusqlite::Connection::open(&shadow_path).unwrap();
    conn.execute_batch(
        "CREATE TABLE task_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id TEXT NOT NULL,
                run_id INTEGER,
                kind TEXT NOT NULL,
                payload TEXT,
                created_at INTEGER NOT NULL
             );
             CREATE TABLE tasks (
                id TEXT PRIMARY KEY,
                board_id INTEGER NOT NULL,
                title TEXT NOT NULL,
                status TEXT NOT NULL,
                tags TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
             );",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO tasks (id, board_id, title, status, tags,
             created_at, updated_at)
             VALUES (?1, ?2, 'test', 'done', '[]', 1, 1)",
        rusqlite::params![task_id.to_string(), board_id as i64],
    )
    .unwrap();
    drop(conn);

    let child = if cfg!(windows) {
        std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-WindowStyle",
                "Hidden",
                "-Command",
                "Start-Sleep -Seconds 30",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap()
    } else {
        std::process::Command::new("sh")
            .args(["-c", "sleep 30"])
            .spawn()
            .unwrap()
    };

    let cfg = DispatcherConfig {
        max_concurrent: 1,
        shadows_dir: tmp.join("shadows"),
        ..Default::default()
    };
    let mut dispatcher = Dispatcher::new(cfg);
    dispatcher.workers.insert(
        task_id,
        (
            WorkerHandle {
                run_id: run_id.clone(),
                child,
                started_at: std::time::Instant::now(),
            },
            ShadowWatcher::new(shadow_path, task_id),
        ),
    );

    let report = dispatcher.tick(&store).unwrap();

    assert_eq!(report.completed, 1);
    assert_eq!(dispatcher.active_worker_count(), 0);
    assert_eq!(store.get_task(task_id).unwrap().status, Status::Done);
    assert_eq!(
        store.get_runs(task_id).unwrap()[0].status,
        RunStatus::Completed
    );
}
