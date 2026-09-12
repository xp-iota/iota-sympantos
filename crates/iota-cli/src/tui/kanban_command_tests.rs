use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use iota_kanban::{
    AdvancedBridge, CreateTaskRequest, Dispatcher, DispatcherConfig, KanbanStore,
    SqliteKanbanStore, Status, TaskFilter,
};

fn test_store() -> Arc<dyn KanbanStore> {
    Arc::new(SqliteKanbanStore::open(Path::new(":memory:")).unwrap())
}

fn exec(args: &str, store: &Arc<dyn KanbanStore>) -> Vec<String> {
    super::execute(args, store, None)
}

/// Entity ids are collision-resistant random values, not 1-based counters, so
/// tests must read the real id back instead of assuming `1`. Asserting on
/// `"#1"` was also flaky: it is a substring check, and a 19-digit id like
/// `#1234567890…` matches it by accident roughly one run in nine.
fn only_board_id(store: &Arc<dyn KanbanStore>) -> u64 {
    let boards = store.list_boards().unwrap();
    assert_eq!(boards.len(), 1, "expected exactly one board");
    boards[0].id
}

fn only_task_id(store: &Arc<dyn KanbanStore>) -> u64 {
    let tasks = store.list_tasks(TaskFilter::default()).unwrap();
    assert_eq!(tasks.len(), 1, "expected exactly one task");
    tasks[0].id
}

fn fake_hermes_echo_spec(tmp: &Path) -> PathBuf {
    if cfg!(windows) {
        let path = tmp.join("fake-hermes.cmd");
        std::fs::write(&path, "@echo off\r\necho {\"spec\":\"expanded spec\"}\r\n").unwrap();
        path
    } else {
        let path = tmp.join("fake-hermes.sh");
        std::fs::write(&path, "#!/bin/sh\necho '{\"spec\":\"expanded spec\"}'\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&path).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&path, perms).unwrap();
        }
        path
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn test_help() {
    let store = test_store();
    let out = exec("help", &store);
    assert!(
        out.iter().any(|l| l.contains("Kanban commands")),
        "help output should contain 'Kanban commands', got: {:?}",
        out
    );
    assert!(
        out.iter()
            .any(|l| l.contains("dispatch") && l.contains("Tick dispatcher")),
        "help output should describe dispatch, got: {:?}",
        out
    );
    assert!(
        out.iter().any(|l| l.contains("specify")),
        "help output should describe specify, got: {:?}",
        out
    );
    assert!(
        out.iter().any(|l| l.contains("decompose")),
        "help output should describe decompose, got: {:?}",
        out
    );
    assert!(
        !out.iter().any(|l| l.contains("sync")),
        "help output should not expose unfinished distributed sync, got: {:?}",
        out
    );
}

#[test]
fn test_boards_empty() {
    let store = test_store();
    let out = exec("boards", &store);
    assert!(
        out.iter().any(|l| l.contains("No boards")),
        "expected 'No boards' message, got: {:?}",
        out
    );
}

#[test]
fn test_board_create_and_list() {
    let store = test_store();
    let create_out = exec("board create myboard My Board", &store);
    let board_id = only_board_id(&store);
    assert!(
        create_out[0].contains(&format!("Created board #{board_id}")),
        "expected board creation message, got: {:?}",
        create_out
    );

    let list_out = exec("boards", &store);
    assert!(
        list_out.iter().any(|l| l.contains("myboard")),
        "boards listing should contain 'myboard', got: {:?}",
        list_out
    );
}

#[test]
fn test_create_task_no_board() {
    let store = test_store();
    let out = exec("create Fix bug", &store);
    assert!(
        out.iter().any(|l| l.contains("No boards")),
        "expected error about no boards, got: {:?}",
        out
    );
}

#[test]
fn test_create_and_show_task() {
    let store = test_store();
    exec("board create dev Development", &store);
    let create_out = exec("create Fix the login page", &store);
    let task_id = only_task_id(&store);
    assert!(
        create_out[0].contains(&format!("Created task #{task_id}: Fix the login page")),
        "expected task creation message, got: {:?}",
        create_out
    );

    let show_out = exec(&format!("show {task_id}"), &store);
    assert!(
        show_out.iter().any(|l| l.contains("Fix the login page")),
        "show should include the task title, got: {:?}",
        show_out
    );
    assert!(
        show_out.iter().any(|l| l.contains("triage")),
        "new task should be in triage status, got: {:?}",
        show_out
    );
}

#[test]
fn test_move_task() {
    let store = test_store();
    exec("board create dev Development", &store);
    exec("create Implement feature", &store);
    let task_id = only_task_id(&store);

    let move_out = exec(&format!("move {task_id} todo"), &store);
    assert!(
        move_out[0].contains(&format!("Task #{task_id} -> todo")),
        "expected successful move message, got: {:?}",
        move_out
    );
}

#[test]
fn test_move_invalid_transition() {
    let store = test_store();
    exec("board create dev Development", &store);
    exec("create Some task", &store);

    // Task starts at triage; triage->running is invalid
    let move_out = exec("move 1 running", &store);
    assert!(
        move_out[0].contains("Error:"),
        "expected error for invalid transition, got: {:?}",
        move_out
    );
}

#[test]
fn test_assign_task() {
    let store = test_store();
    exec("board create dev Development", &store);
    exec("create Build dashboard", &store);
    let task_id = only_task_id(&store);

    let assign_out = exec(&format!("assign {task_id} alice"), &store);
    assert!(
        assign_out[0].contains("@alice"),
        "expected assignment confirmation with @alice, got: {:?}",
        assign_out
    );

    let show_out = exec(&format!("show {task_id}"), &store);
    assert!(
        show_out.iter().any(|l| l.contains("@alice")),
        "show should display @alice as assignee, got: {:?}",
        show_out
    );
}

#[test]
fn test_comment_task() {
    let store = test_store();
    exec("board create dev Development", &store);
    exec("create Review PR", &store);

    let comment_out = exec("comment 1 Looks good to me", &store);
    assert!(
        comment_out[0].contains("Comment added"),
        "expected comment confirmation, got: {:?}",
        comment_out
    );
}

#[test]
fn test_unknown_subcommand() {
    let store = test_store();
    let out = exec("foobar", &store);
    assert!(
        out[0].contains("Unknown kanban subcommand"),
        "expected unknown subcommand message, got: {:?}",
        out
    );
}

#[test]
fn test_list_empty() {
    let store = test_store();
    let out = exec("list", &store);
    assert!(
        out.iter().any(|l| l.contains("No tasks found")),
        "expected 'No tasks found' message, got: {:?}",
        out
    );
}

#[test]
fn test_list_with_status_filter() {
    let store = test_store();
    exec("board create dev Development", &store);
    exec("create Task A", &store);
    exec("create Task B", &store);

    // Move the first task to todo (valid: triage -> todo)
    let tasks = store.list_tasks(TaskFilter::default()).unwrap();
    let task_a = tasks
        .iter()
        .find(|t| t.title == "Task A")
        .expect("Task A should exist");
    exec(&format!("move {} todo", task_a.id), &store);

    // List only todo tasks
    let out = exec("list todo", &store);
    assert!(
        out.iter().any(|l| l.contains("Task A")),
        "list todo should include Task A, got: {:?}",
        out
    );
    assert!(
        !out.iter().any(|l| l.contains("Task B")),
        "list todo should NOT include Task B (still in triage), got: {:?}",
        out
    );
}

#[test]
fn test_dispatch_no_ready() {
    let store = test_store();
    let out = exec("dispatch", &store);
    assert!(
        out.iter().any(|l| l.contains("No ready tasks")),
        "expected 'No ready tasks' message, got: {:?}",
        out
    );
}

#[test]
fn test_dispatch_ticks_dispatcher() {
    let concrete = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
    let board_id = concrete.create_board("dev", "Development").unwrap();
    let task_id = concrete
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
    let store: Arc<dyn KanbanStore> = Arc::new(concrete);
    let tmp = std::env::temp_dir().join(format!("iota-kb-cmd-{}", uuid::Uuid::new_v4()));
    let dispatcher = Arc::new(Mutex::new(Dispatcher::new(DispatcherConfig {
        max_concurrent: 1,
        hermes_bin: Path::new("/missing/hermes-for-iota-test").to_path_buf(),
        shadows_dir: tmp.clone(),
        ..Default::default()
    })));

    let out = super::execute_with_dispatcher("dispatch", &store, None, Some(&dispatcher), None);

    assert!(
        out.iter().any(|l| l.contains("spawn failure")),
        "expected dispatcher report with spawn failure, got: {:?}",
        out
    );
    assert_eq!(store.get_task(task_id).unwrap().status, Status::Ready);
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_dispatch_reports_busy_instead_of_blocking_on_dispatcher_lock() {
    let store = test_store();
    let tmp = std::env::temp_dir().join(format!("iota-kb-cmd-{}", uuid::Uuid::new_v4()));
    let dispatcher = Arc::new(Mutex::new(Dispatcher::new(DispatcherConfig {
        shadows_dir: tmp.clone(),
        ..Default::default()
    })));
    let guard = dispatcher.lock().unwrap();

    let out = super::execute_with_dispatcher("dispatch", &store, None, Some(&dispatcher), None);

    drop(guard);
    assert!(
        out.iter()
            .any(|line| line.contains("already running in the background")),
        "expected non-blocking busy message, got: {:?}",
        out
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_daemon_toggle() {
    let store = test_store();
    let daemon_active = Arc::new(AtomicBool::new(true));

    let out = super::execute_with_dispatcher("daemon", &store, None, None, Some(&daemon_active));
    assert!(
        out.iter().any(|l| l.contains("daemon stopped")),
        "expected daemon stopped message, got: {:?}",
        out
    );
    assert!(!daemon_active.load(std::sync::atomic::Ordering::Relaxed));

    let out = super::execute_with_dispatcher("daemon", &store, None, None, Some(&daemon_active));
    assert!(
        out.iter().any(|l| l.contains("daemon started")),
        "expected daemon started message, got: {:?}",
        out
    );
    assert!(daemon_active.load(std::sync::atomic::Ordering::Relaxed));
}

#[test]
fn test_dispatch_with_task_id_readies_todo_task() {
    let concrete = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
    let board_id = concrete.create_board("dev", "Development").unwrap();
    let task_id = concrete
        .create_task(CreateTaskRequest {
            board_id,
            title: "Todo task".to_string(),
            body: None,
            status: Some(Status::Todo),
            assignee: None,
            priority: None,
            tags: vec![],
            workspace_kind: None,
            workspace_path: None,
        })
        .unwrap();
    let store: Arc<dyn KanbanStore> = Arc::new(concrete);
    let tmp = std::env::temp_dir().join(format!("iota-kb-cmd-{}", uuid::Uuid::new_v4()));
    let dispatcher = Arc::new(Mutex::new(Dispatcher::new(DispatcherConfig {
        max_concurrent: 1,
        hermes_bin: Path::new("/missing/hermes-for-iota-test").to_path_buf(),
        shadows_dir: tmp.clone(),
        ..Default::default()
    })));

    let out = super::execute_with_dispatcher(
        &format!("dispatch #{}", task_id),
        &store,
        None,
        Some(&dispatcher),
        None,
    );

    // Task should have been transitioned to ready, then dispatch attempted (spawn failure)
    assert!(
        out.iter().any(|l| l.contains("spawn failure")),
        "expected spawn failure (hermes not found), got: {:?}",
        out
    );
    // Task stays ready after failed spawn
    assert_eq!(store.get_task(task_id).unwrap().status, Status::Ready);
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_dispatch_with_task_id_readies_triage_task() {
    let concrete = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
    let board_id = concrete.create_board("dev", "Development").unwrap();
    let task_id = concrete
        .create_task(CreateTaskRequest {
            board_id,
            title: "Triage task".to_string(),
            body: None,
            status: Some(Status::Triage),
            assignee: None,
            priority: None,
            tags: vec![],
            workspace_kind: None,
            workspace_path: None,
        })
        .unwrap();
    let store: Arc<dyn KanbanStore> = Arc::new(concrete);
    let tmp = std::env::temp_dir().join(format!("iota-kb-cmd-{}", uuid::Uuid::new_v4()));
    let dispatcher = Arc::new(Mutex::new(Dispatcher::new(DispatcherConfig {
        max_concurrent: 1,
        hermes_bin: Path::new("/missing/hermes-for-iota-test").to_path_buf(),
        shadows_dir: tmp.clone(),
        ..Default::default()
    })));

    let out = super::execute_with_dispatcher(
        &format!("dispatch #{}", task_id),
        &store,
        None,
        Some(&dispatcher),
        None,
    );

    assert!(
        out.iter().any(|line| line.contains("spawn failure")),
        "expected spawn failure (hermes not found), got: {:?}",
        out
    );
    assert_eq!(store.get_task(task_id).unwrap().status, Status::Ready);
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_dispatch_rejects_invalid_status_task() {
    let concrete = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
    let board_id = concrete.create_board("dev", "Development").unwrap();
    let task_id = concrete
        .create_task(CreateTaskRequest {
            board_id,
            title: "Done task".to_string(),
            body: None,
            status: Some(Status::Done),
            assignee: None,
            priority: None,
            tags: vec![],
            workspace_kind: None,
            workspace_path: None,
        })
        .unwrap();
    let store: Arc<dyn KanbanStore> = Arc::new(concrete);

    let out =
        super::execute_with_dispatcher(&format!("dispatch {}", task_id), &store, None, None, None);

    assert!(
        out.iter().any(|l| l.contains("must be")),
        "expected rejection message for done task, got: {:?}",
        out
    );
}

#[test]
fn test_specify_updates_task_body_with_bridge() {
    let tmp = std::env::temp_dir().join(format!("iota-kb-bridge-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();
    let concrete = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
    let board_id = concrete.create_board("dev", "Development").unwrap();
    let task_id = concrete
        .create_task(CreateTaskRequest {
            board_id,
            title: "Vague task".to_string(),
            body: None,
            status: None,
            assignee: None,
            priority: None,
            tags: vec![],
            workspace_kind: None,
            workspace_path: None,
        })
        .unwrap();
    let store: Arc<dyn KanbanStore> = Arc::new(concrete);
    let bridge = AdvancedBridge::new(fake_hermes_echo_spec(&tmp), tmp.join("shadows"));

    let out = super::execute_with_services(
        &format!("specify {}", task_id),
        &store,
        None,
        None,
        None,
        Some(&bridge),
    );

    assert!(
        out.iter().any(|line| line.contains("Specified task")),
        "expected specify success, got: {:?}",
        out
    );
    assert_eq!(
        store.get_task(task_id).unwrap().body.as_deref(),
        Some("expanded spec")
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

// ---------------------------------------------------------------------------
// Board column view tests
// ---------------------------------------------------------------------------

#[test]
fn test_view_no_board() {
    let store = test_store();
    let result = exec("view", &store);
    assert!(
        result[0].contains("No boards found"),
        "expected 'No boards found' message, got: {:?}",
        result
    );
}

#[test]
fn test_view_empty_board() {
    let store = test_store();
    exec("board create dev Development", &store);
    let result = exec("view dev", &store);
    assert!(
        result.iter().any(|l| l.contains("Development")),
        "view should show board name, got: {:?}",
        result
    );
    assert!(
        result.iter().any(|l| l.contains("TRIAGE")),
        "view should show TRIAGE column header, got: {:?}",
        result
    );
    assert!(
        result.iter().any(|l| l.contains("TODO")),
        "view should show TODO column header, got: {:?}",
        result
    );
    assert!(
        result.iter().any(|l| l.contains("RUNNING")),
        "view should show RUNNING column header, got: {:?}",
        result
    );
}

#[test]
fn test_view_with_tasks() {
    let store = test_store();
    exec("board create dev Development", &store);
    exec("create My Task", &store);
    let task_id = only_task_id(&store);
    let result = exec("view dev", &store);
    assert!(
        result.iter().any(|l| l.contains(&format!("#{task_id}"))),
        "view should show task #{task_id}, got: {:?}",
        result
    );
    assert!(
        result.iter().any(|l| l.contains("TRIAGE(1)")),
        "view should show TRIAGE(1), got: {:?}",
        result
    );
}

#[test]
fn test_view_default_board() {
    let store = test_store();
    exec("board create dev Development", &store);
    exec("create Some Task", &store);
    // Call view with no slug - should pick first board
    let result = exec("view", &store);
    assert!(
        result.iter().any(|l| l.contains("Development")),
        "view without slug should show first board, got: {:?}",
        result
    );
}

#[test]
fn test_view_columns_alias() {
    let store = test_store();
    exec("board create dev Development", &store);
    let result = exec("columns", &store);
    assert!(
        result.iter().any(|l| l.contains("Development")),
        "columns alias should work like view, got: {:?}",
        result
    );
}

#[test]
fn test_view_running_marker() {
    let concrete = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
    let board_id = concrete.create_board("dev", "Development").unwrap();
    let task_id = concrete
        .create_task(CreateTaskRequest {
            board_id,
            title: "Active task".to_string(),
            body: None,
            status: Some(Status::Running),
            assignee: None,
            priority: None,
            tags: vec![],
            workspace_kind: None,
            workspace_path: None,
        })
        .unwrap();
    let store: Arc<dyn KanbanStore> = Arc::new(concrete);

    let result = exec("view dev", &store);
    assert!(
        result.iter().any(|l| l.contains(&format!("#{task_id}*"))),
        "running tasks should have * marker, got: {:?}",
        result
    );
    assert!(
        result.iter().any(|l| l.contains("RUNNING(1)")),
        "view should show RUNNING(1), got: {:?}",
        result
    );
}

#[test]
fn test_board_view_subdispatch() {
    let store = test_store();
    exec("board create dev Development", &store);
    // "/kanban board view dev" should also work
    let result = exec("board view dev", &store);
    assert!(
        result.iter().any(|l| l.contains("Development")),
        "board view subcommand should work, got: {:?}",
        result
    );
}
