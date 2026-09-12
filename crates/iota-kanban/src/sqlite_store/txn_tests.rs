use crate::sqlite_store::SqliteKanbanStore;
use crate::store::KanbanStore;
use crate::types::{CreateTaskRequest, Status};
use std::path::Path;

fn new_store() -> SqliteKanbanStore {
    SqliteKanbanStore::open(Path::new(":memory:")).expect("open in-memory store")
}

#[test]
fn with_transaction_commits_on_ok() {
    let store = new_store();
    let board_id = store.create_board("board-1", "Board One").unwrap();
    let result = store.with_transaction(|conn| {
        SqliteKanbanStore::create_task_on_conn(
            conn,
            CreateTaskRequest {
                board_id,
                title: "Task A".to_string(),
                body: None,
                status: None,
                assignee: None,
                priority: None,
                tags: Vec::new(),
                workspace_kind: None,
                workspace_path: None,
            },
        )
    });
    assert!(result.is_ok());
    let task = store.get_task_impl(result.unwrap()).unwrap();
    assert_eq!(task.title, "Task A");
}

#[test]
fn with_transaction_rolls_back_on_err() {
    let store = new_store();
    let board_id = store.create_board("board-1", "Board One").unwrap();

    // Deliberately fail the second write inside the same transaction as the
    // first: the first write must not be visible afterwards.
    let result: anyhow::Result<()> = store.with_transaction(|conn| {
        SqliteKanbanStore::create_task_on_conn(
            conn,
            CreateTaskRequest {
                board_id,
                title: "Should not persist".to_string(),
                body: None,
                status: None,
                assignee: None,
                priority: None,
                tags: Vec::new(),
                workspace_kind: None,
                workspace_path: None,
            },
        )?;
        anyhow::bail!("simulated failure after domain write");
    });
    assert!(result.is_err());

    let tasks = store
        .list_tasks_impl(crate::types::TaskFilter {
            board_id: Some(board_id),
            status: None,
            assignee: None,
            limit: None,
        })
        .unwrap();
    assert!(
        tasks.is_empty(),
        "task inserted before the simulated failure must have been rolled back"
    );
}

#[test]
fn create_task_and_event_are_atomic() {
    let store = new_store();
    let board_id = store.create_board("board-1", "Board One").unwrap();
    let id = store
        .create_task(CreateTaskRequest {
            board_id,
            title: "Atomic task".to_string(),
            body: None,
            status: None,
            assignee: None,
            priority: None,
            tags: Vec::new(),
            workspace_kind: None,
            workspace_path: None,
        })
        .unwrap();

    // The event log must contain exactly one TASK_CREATED event referencing
    // this task, proving the domain write and event append committed
    // together.
    let events = store.events_since_impl(0).unwrap();
    let matching = events
        .iter()
        .filter(|e| e.event_type == crate::event_sourcing::EVT_TASK_CREATED)
        .count();
    assert_eq!(matching, 1);
    assert!(store.get_task_impl(id).is_ok());
}

#[test]
fn transition_rejects_invalid_state_change_and_leaves_no_event() {
    let store = new_store();
    let board_id = store.create_board("board-1", "Board One").unwrap();
    let id = store
        .create_task(CreateTaskRequest {
            board_id,
            title: "State task".to_string(),
            body: None,
            status: None,
            assignee: None,
            priority: None,
            tags: Vec::new(),
            workspace_kind: None,
            workspace_path: None,
        })
        .unwrap();

    let events_before = store.events_since_impl(0).unwrap().len();
    // Triage -> Done is not a valid direct transition in the state machine;
    // this must fail without leaving a dangling TASK_TRANSITIONED event.
    let result = store.transition(id, Status::Done);
    assert!(result.is_err());
    let events_after = store.events_since_impl(0).unwrap().len();
    assert_eq!(
        events_before, events_after,
        "a rejected transition must not append an event"
    );
}
