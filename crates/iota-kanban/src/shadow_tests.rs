use crate::shadow::{SHADOW_SCHEMA, ShadowEvent, ShadowMaterializer, ShadowWatcher};
use crate::sqlite_store::SqliteKanbanStore;
use crate::store::KanbanStore;
use crate::{BoardId, CreateTaskRequest, LinkKind, TaskId};
use rusqlite::{Connection, params};
use std::path::Path;

fn test_tmp_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("iota-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn open_store(path: &Path) -> SqliteKanbanStore {
    SqliteKanbanStore::open(path).unwrap()
}

fn make_board(store: &dyn KanbanStore) -> BoardId {
    store.create_board("test", "Test Board").unwrap()
}

fn make_task(store: &dyn KanbanStore, board_id: BoardId) -> TaskId {
    store
        .create_task(CreateTaskRequest {
            board_id,
            title: "test task".to_string(),
            body: None,
            status: None,
            assignee: None,
            priority: None,
            tags: vec![],
            workspace_kind: None,
            workspace_path: None,
        })
        .unwrap()
}

fn init_shadow_db(db_path: &Path) -> Connection {
    let conn = Connection::open(db_path).unwrap();
    conn.execute_batch(SHADOW_SCHEMA).unwrap();
    conn
}

#[test]
fn materialize_creates_shadow_db() {
    let tmp = test_tmp_dir();
    let store = open_store(&tmp.join("store.db"));

    let board_id = make_board(&store);
    let task_id = make_task(&store, board_id);
    store.add_comment(task_id, "alice", "hello world").unwrap();

    let task = store.get_task(task_id).unwrap();
    let board = store.get_board("test").unwrap();

    let materializer = ShadowMaterializer::new(tmp.join("shadows"));
    let shadow_db = materializer.materialize(&task, &board, &store).unwrap();

    assert!(shadow_db.path.exists());
    assert_eq!(shadow_db.task_id, task_id);

    let conn = Connection::open(&shadow_db.path).unwrap();
    let task_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM tasks WHERE id = ?1",
            params![task_id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(task_count, 1);

    let comment_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM task_comments WHERE task_id = ?1",
            params![task_id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(comment_count, 1);
}

#[test]
fn materialize_includes_linked_tasks() {
    let tmp = test_tmp_dir();
    let store = open_store(&tmp.join("store.db"));

    let board_id = make_board(&store);
    let parent_id = make_task(&store, board_id);
    let child_id = make_task(&store, board_id);
    store
        .create_link(child_id, parent_id, LinkKind::Parent)
        .unwrap();

    let child_task = store.get_task(child_id).unwrap();
    let board = store.get_board("test").unwrap();

    let materializer = ShadowMaterializer::new(tmp.join("shadows"));
    let shadow_db = materializer
        .materialize(&child_task, &board, &store)
        .unwrap();

    let conn = Connection::open(&shadow_db.path).unwrap();
    let task_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get(0))
        .unwrap();
    assert_eq!(task_count, 2);
}

#[test]
fn cleanup_removes_shadow_dir() {
    let tmp = test_tmp_dir();
    let store = open_store(&tmp.join("store.db"));

    let board_id = make_board(&store);
    let task_id = make_task(&store, board_id);
    let task = store.get_task(task_id).unwrap();
    let board = store.get_board("test").unwrap();

    let materializer = ShadowMaterializer::new(tmp.join("shadows"));
    let shadow_db = materializer.materialize(&task, &board, &store).unwrap();

    let shadow_dir = shadow_db.path.parent().unwrap().to_path_buf();
    assert!(shadow_dir.exists());

    materializer.cleanup(task_id).unwrap();
    assert!(!shadow_dir.exists());
}

#[test]
fn watcher_polls_new_events() {
    let tmp = test_tmp_dir();
    let db_path = tmp.join("kanban.db");
    let task_id: TaskId = 42;
    let now = 1_000_000i64;

    let conn = init_shadow_db(&db_path);
    conn.execute(
        "INSERT INTO tasks (id, board_id, title, status, tags,
             created_at, updated_at)
             VALUES (?1, 1, 'test', 'triage', '[]', ?2, ?2)",
        params![task_id.to_string(), now],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO task_events (task_id, kind, payload, created_at)
             VALUES (?1, 'heartbeat', '{}', ?2)",
        params![task_id.to_string(), now],
    )
    .unwrap();
    conn.execute(
        r#"INSERT INTO task_events (task_id, kind, payload, created_at)
             VALUES (?1, 'comment', '{"author":"bot","body":"done"}', ?2)"#,
        params![task_id.to_string(), now],
    )
    .unwrap();
    drop(conn);

    let mut watcher = ShadowWatcher::new(db_path, task_id);

    let (events, terminal) = watcher.poll().unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].event_type, "heartbeat");
    assert_eq!(events[1].event_type, "comment");
    assert!(terminal.is_none());

    watcher.mark_events_synced(&events);
    let (events2, _) = watcher.poll().unwrap();
    assert_eq!(events2.len(), 0);
}

#[test]
fn watcher_detects_terminal_status() {
    let tmp = test_tmp_dir();
    let db_path = tmp.join("kanban.db");
    let task_id: TaskId = 7;
    let now = 1_000_000i64;

    let conn = init_shadow_db(&db_path);
    conn.execute(
        "INSERT INTO tasks (id, board_id, title, status, tags,
             created_at, updated_at)
             VALUES (?1, 1, 'test', 'done', '[]', ?2, ?2)",
        params![task_id.to_string(), now],
    )
    .unwrap();
    drop(conn);

    let mut watcher = ShadowWatcher::new(db_path, task_id);
    let (_, terminal) = watcher.poll().unwrap();
    assert_eq!(terminal, Some("done".to_string()));
}

#[test]
fn sync_events_applies_to_store() {
    let tmp = test_tmp_dir();
    let store = open_store(&tmp.join("store.db"));

    let board_id = make_board(&store);
    let task_id = make_task(&store, board_id);
    store.transition(task_id, crate::Status::Todo).unwrap();
    store.transition(task_id, crate::Status::Ready).unwrap();
    store.transition(task_id, crate::Status::Running).unwrap();
    let run_id = store.create_run(task_id, "test-profile").unwrap();

    let watcher = ShadowWatcher::new(tmp.join("unused.db"), task_id);

    let events = vec![
        ShadowEvent {
            id: 1,
            task_id,
            event_type: "heartbeat".to_string(),
            payload: "{}".to_string(),
        },
        ShadowEvent {
            id: 2,
            task_id,
            event_type: "comment".to_string(),
            payload: r#"{"author":"bot","body":"task done"}"#.to_string(),
        },
    ];

    watcher.sync_events(&events, &store, &run_id).unwrap();

    let comments = store.list_comments(task_id).unwrap();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].author, "bot");
    assert_eq!(comments[0].body, "task done");
}

#[test]
fn failed_sync_does_not_advance_event_cursor() {
    let tmp = test_tmp_dir();
    let db_path = tmp.join("kanban.db");
    let task_id: TaskId = 1;
    let now = 1_000_000i64;

    let conn = init_shadow_db(&db_path);
    conn.execute(
        "INSERT INTO tasks (id, board_id, title, status, tags,
             created_at, updated_at)
             VALUES (?1, 1, 'test', 'running', '[]', ?2, ?2)",
        params![task_id.to_string(), now],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO task_events (task_id, kind, payload, created_at)
             VALUES (?1, 'comment', '{bad-json', ?2)",
        params![task_id.to_string(), now],
    )
    .unwrap();
    drop(conn);

    let store = open_store(&tmp.join("store.db"));
    let board_id = make_board(&store);
    let main_task_id = make_task(&store, board_id);
    let run_id = store.create_run(main_task_id, "test-profile").unwrap();
    let mut watcher = ShadowWatcher::new(db_path, main_task_id);

    let (events, _) = watcher.poll().unwrap();
    assert_eq!(events.len(), 1);
    assert!(watcher.sync_events(&events, &store, &run_id).is_err());

    let (events_again, _) = watcher.poll().unwrap();
    assert_eq!(events_again.len(), 1);
    assert_eq!(events_again[0].id, events[0].id);
}

#[test]
fn sync_events_routes_comment_to_event_task_id() {
    let tmp = test_tmp_dir();
    let store = open_store(&tmp.join("store.db"));
    let board_id = make_board(&store);
    let main_task_id = make_task(&store, board_id);
    let linked_task_id = make_task(&store, board_id);
    let run_id = store.create_run(main_task_id, "test-profile").unwrap();
    let watcher = ShadowWatcher::new(tmp.join("unused.db"), main_task_id);

    let events = vec![ShadowEvent {
        id: 1,
        task_id: linked_task_id,
        event_type: "comment".to_string(),
        payload: r#"{"author":"bot","body":"linked note"}"#.to_string(),
    }];

    watcher.sync_events(&events, &store, &run_id).unwrap();

    assert!(store.list_comments(main_task_id).unwrap().is_empty());
    let comments = store.list_comments(linked_task_id).unwrap();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].body, "linked note");
}

#[test]
fn sync_events_defers_main_task_status_until_worker_exit() {
    let tmp = test_tmp_dir();
    let store = open_store(&tmp.join("store.db"));
    let board_id = make_board(&store);
    let task_id = make_task(&store, board_id);
    store.transition(task_id, crate::Status::Todo).unwrap();
    store.transition(task_id, crate::Status::Ready).unwrap();
    store.transition(task_id, crate::Status::Running).unwrap();
    let run_id = store.create_run(task_id, "test-profile").unwrap();
    let watcher = ShadowWatcher::new(tmp.join("unused.db"), task_id);

    let events = vec![ShadowEvent {
        id: 1,
        task_id,
        event_type: "status_change".to_string(),
        payload: r#"{"to":"done"}"#.to_string(),
    }];

    watcher.sync_events(&events, &store, &run_id).unwrap();

    assert_eq!(
        store.get_task(task_id).unwrap().status,
        crate::Status::Running
    );
}
