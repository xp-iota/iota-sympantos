use crate::sqlite_store::SqliteKanbanStore;
use crate::store::KanbanStore;
use crate::types::*;
use std::path::Path;

fn open_memory() -> SqliteKanbanStore {
    SqliteKanbanStore::open(Path::new(":memory:")).unwrap()
}

fn make_board(store: &SqliteKanbanStore) -> BoardId {
    store.create_board("b", "Board").unwrap()
}

fn make_task(store: &SqliteKanbanStore, board_id: BoardId, title: &str) -> TaskId {
    store
        .create_task(CreateTaskRequest {
            board_id,
            title: title.to_string(),
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

// ---------------------------------------------------------------------------
// Task 10: Board & Task CRUD
// ---------------------------------------------------------------------------

#[test]
fn board_crud() {
    let store = open_memory();
    let id = store.create_board("test-board", "Test Board").unwrap();
    assert!(id > 0);

    let board = store.get_board("test-board").unwrap();
    assert_eq!(board.id, id);
    assert_eq!(board.slug, "test-board");
    assert_eq!(board.name, "Test Board");

    let boards = store.list_boards().unwrap();
    assert_eq!(boards.len(), 1);
}

#[test]
fn board_duplicate_slug_fails() {
    let store = open_memory();
    store.create_board("dup", "Dup Board").unwrap();
    assert!(store.create_board("dup", "Dup Again").is_err());
}

#[test]
fn task_create_and_get() {
    let store = open_memory();
    let board_id = make_board(&store);
    let req = CreateTaskRequest {
        board_id,
        title: "My Task".to_string(),
        body: Some("some body".to_string()),
        status: None,
        assignee: Some("alice".to_string()),
        priority: Some(5),
        tags: vec!["alpha".to_string(), "beta".to_string()],
        workspace_kind: None,
        workspace_path: None,
    };
    let task_id = store.create_task(req).unwrap();
    let task = store.get_task(task_id).unwrap();
    assert_eq!(task.id, task_id);
    assert_eq!(task.board_id, board_id);
    assert_eq!(task.title, "My Task");
    assert_eq!(task.body.as_deref(), Some("some body"));
    assert_eq!(task.status, Status::Triage);
    assert_eq!(task.assignee.as_deref(), Some("alice"));
    assert_eq!(task.priority, 5);
    assert_eq!(task.tags, vec!["alpha", "beta"]);
}

#[test]
fn task_update_partial() {
    let store = open_memory();
    let board_id = make_board(&store);
    let req = CreateTaskRequest {
        board_id,
        title: "Original".to_string(),
        body: Some("body text".to_string()),
        status: None,
        assignee: Some("carol".to_string()),
        priority: Some(1),
        tags: vec![],
        workspace_kind: None,
        workspace_path: None,
    };
    let task_id = store.create_task(req).unwrap();

    let patch = TaskPatch {
        title: Some("Updated".to_string()),
        priority: Some(10),
        ..Default::default()
    };
    store.update_task(task_id, patch).unwrap();

    let task = store.get_task(task_id).unwrap();
    assert_eq!(task.title, "Updated");
    assert_eq!(task.priority, 10);
    assert_eq!(task.body.as_deref(), Some("body text"));
    assert_eq!(task.assignee.as_deref(), Some("carol"));
}

#[test]
fn task_list_with_filter() {
    let store = open_memory();
    let board_id = make_board(&store);

    let _id1 = store
        .create_task(CreateTaskRequest {
            board_id,
            title: "T1".to_string(),
            body: None,
            status: Some(Status::Todo),
            assignee: Some("bob".to_string()),
            priority: None,
            tags: vec![],
            workspace_kind: None,
            workspace_path: None,
        })
        .unwrap();
    let id2 = store
        .create_task(CreateTaskRequest {
            board_id,
            title: "T2".to_string(),
            body: None,
            status: Some(Status::Ready),
            assignee: Some("bob".to_string()),
            priority: None,
            tags: vec![],
            workspace_kind: None,
            workspace_path: None,
        })
        .unwrap();

    let ready_tasks = store
        .list_tasks(TaskFilter {
            board_id: None,
            status: Some(Status::Ready),
            assignee: None,
            limit: None,
        })
        .unwrap();
    assert_eq!(ready_tasks.len(), 1);
    assert_eq!(ready_tasks[0].id, id2);

    let bob_tasks = store
        .list_tasks(TaskFilter {
            board_id: None,
            status: None,
            assignee: Some("bob".to_string()),
            limit: None,
        })
        .unwrap();
    assert_eq!(bob_tasks.len(), 2);
}

#[test]
fn task_delete() {
    let store = open_memory();
    let board_id = make_board(&store);
    let task_id = make_task(&store, board_id, "To Delete");
    store.delete_task(task_id).unwrap();
    assert!(store.get_task(task_id).is_err());
}

// ---------------------------------------------------------------------------
// Task 11: State Transition tests
// ---------------------------------------------------------------------------

#[test]
fn transition_valid_triage_to_todo() {
    let store = open_memory();
    let board_id = make_board(&store);
    let task_id = make_task(&store, board_id, "T");
    store.transition(task_id, Status::Todo).unwrap();
    let task = store.get_task(task_id).unwrap();
    assert_eq!(task.status, Status::Todo);
}

#[test]
fn transition_full_lifecycle() {
    let store = open_memory();
    let board_id = make_board(&store);
    let task_id = make_task(&store, board_id, "T");

    store.transition(task_id, Status::Todo).unwrap();
    store.transition(task_id, Status::Ready).unwrap();
    store.transition(task_id, Status::Running).unwrap();

    let task = store.get_task(task_id).unwrap();
    assert_eq!(task.status, Status::Running);
    assert!(task.claimed_at.is_some());

    store.transition(task_id, Status::Done).unwrap();
    store.transition(task_id, Status::Archived).unwrap();

    let task = store.get_task(task_id).unwrap();
    assert_eq!(task.status, Status::Archived);
}

#[test]
fn transition_invalid_rejected() {
    let store = open_memory();
    let board_id = make_board(&store);
    let task_id = make_task(&store, board_id, "T");
    let err = store.transition(task_id, Status::Running).unwrap_err();
    assert!(
        err.to_string().contains("invalid status transition"),
        "expected 'invalid status transition' in error, got: {}",
        err
    );
}

#[test]
fn update_task_status_rejects_invalid_transition() {
    let store = open_memory();
    let board_id = make_board(&store);
    let task_id = make_task(&store, board_id, "T");

    let err = store
        .update_task(
            task_id,
            TaskPatch {
                status: Some(Status::Running),
                ..Default::default()
            },
        )
        .unwrap_err();

    assert!(
        err.to_string().contains("invalid status transition"),
        "expected transition validation error, got: {}",
        err
    );
    let task = store.get_task(task_id).unwrap();
    assert_eq!(task.status, Status::Triage);
}

#[test]
fn transition_blocked_and_unblock() {
    let store = open_memory();
    let board_id = make_board(&store);
    let task_id = store
        .create_task(CreateTaskRequest {
            board_id,
            title: "T".to_string(),
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
    store.transition(task_id, Status::Blocked).unwrap();
    let task = store.get_task(task_id).unwrap();
    assert_eq!(task.status, Status::Blocked);

    store.transition(task_id, Status::Ready).unwrap();
    let task = store.get_task(task_id).unwrap();
    assert_eq!(task.status, Status::Ready);
}

// ---------------------------------------------------------------------------
// Task 12: Links, Comments, Runs, Events
// ---------------------------------------------------------------------------

#[test]
fn links_create_and_query() {
    let store = open_memory();
    let board_id = make_board(&store);
    let t1 = make_task(&store, board_id, "T1");
    let t2 = make_task(&store, board_id, "T2");

    store.create_link(t1, t2, LinkKind::Parent).unwrap();
    store.create_link(t1, t2, LinkKind::Blocks).unwrap();

    assert_eq!(store.get_links(t1).unwrap().len(), 2);
    assert_eq!(store.get_links(t2).unwrap().len(), 2);

    store.remove_link(t1, t2, LinkKind::Blocks).unwrap();

    let remaining = store.get_links(t1).unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].kind, LinkKind::Parent);
}

#[test]
fn comments_add_and_list() {
    let store = open_memory();
    let board_id = make_board(&store);
    let task_id = make_task(&store, board_id, "T");

    store
        .add_comment(task_id, "alice", "first comment")
        .unwrap();
    store.add_comment(task_id, "bob", "second comment").unwrap();

    let comments = store.list_comments(task_id).unwrap();
    assert_eq!(comments.len(), 2);
    assert_eq!(comments[0].author, "alice");
    assert_eq!(comments[0].body, "first comment");
    assert_eq!(comments[1].author, "bob");
    assert_eq!(comments[1].body, "second comment");
}

#[test]
fn runs_lifecycle() {
    let store = open_memory();
    let board_id = make_board(&store);
    let task_id = make_task(&store, board_id, "T");

    let run_id = store.create_run(task_id, "default").unwrap();
    assert!(!run_id.is_empty());

    store.heartbeat(&run_id).unwrap();
    store
        .complete_run(&run_id, RunStatus::Completed, Some(0))
        .unwrap();

    let runs = store.get_runs(task_id).unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].status, RunStatus::Completed);
    assert_eq!(runs[0].exit_code, Some(0));
    assert!(runs[0].finished_at.is_some());
}

#[test]
fn heartbeat_on_finished_run_fails() {
    let store = open_memory();
    let board_id = make_board(&store);
    let task_id = make_task(&store, board_id, "T");

    let run_id = store.create_run(task_id, "default").unwrap();
    store
        .complete_run(&run_id, RunStatus::Completed, Some(0))
        .unwrap();

    assert!(store.heartbeat(&run_id).is_err());
}

#[test]
fn events_append_and_read() {
    let store = open_memory();
    let id1 = store.append_event("task.created", r#"{"id":1}"#).unwrap();
    let _id2 = store.append_event("task.updated", r#"{"id":2}"#).unwrap();
    let _id3 = store.append_event("task.deleted", r#"{"id":3}"#).unwrap();

    let all = store.events_since(0).unwrap();
    assert_eq!(all.len(), 3);

    let rest = store.events_since(id1).unwrap();
    assert_eq!(rest.len(), 2);
}

#[test]
fn independently_created_entities_use_non_sequential_ids() {
    let first = open_memory();
    let second = open_memory();
    let first_board = make_board(&first);
    let second_board = make_board(&second);

    assert_ne!(first_board, second_board);
    assert!(first_board > 1_000 && second_board > 1_000);

    let first_task = make_task(&first, first_board, "first");
    let second_task = make_task(&second, second_board, "second");
    assert_ne!(first_task, second_task);
    assert!(first_task > 1_000 && second_task > 1_000);

    let first_comment = first.add_comment(first_task, "alice", "first").unwrap();
    let second_comment = second.add_comment(second_task, "bob", "second").unwrap();
    assert_ne!(first_comment, second_comment);
    assert!(first_comment > 1_000 && second_comment > 1_000);
}
