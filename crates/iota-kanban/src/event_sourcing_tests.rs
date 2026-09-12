use crate::event_sourcing::TaskCreatedPayload;
use crate::{CreateTaskRequest, KanbanEvent, KanbanStore, LinkKind, SqliteKanbanStore, Status};
use std::path::Path;

fn test_store() -> SqliteKanbanStore {
    SqliteKanbanStore::open(Path::new(":memory:")).unwrap()
}

#[test]
fn events_recorded_on_mutations() {
    let store = test_store();
    let board_id = store.create_board("test", "Test Board").unwrap();
    let task_id = store
        .create_task(CreateTaskRequest {
            board_id,
            title: "My task".to_string(),
            body: None,
            status: None,
            assignee: None,
            priority: None,
            tags: vec![],
            workspace_kind: None,
            workspace_path: None,
        })
        .unwrap();
    store.transition(task_id, Status::Todo).unwrap();

    let events = store.events_since(0).unwrap();
    assert!(
        events.len() >= 3,
        "expected at least 3 events, got {}",
        events.len()
    );
    assert_eq!(events[0].event_type, "board_created");
    assert_eq!(events[1].event_type, "task_created");
    assert_eq!(events[2].event_type, "task_transitioned");
}

#[test]
fn replay_rebuilds_state() {
    let store1 = test_store();
    let board_id = store1.create_board("dev", "Development").unwrap();
    let task_id = store1
        .create_task(CreateTaskRequest {
            board_id,
            title: "Build feature".to_string(),
            body: Some("Details here".to_string()),
            status: None,
            assignee: Some("alice".to_string()),
            priority: Some(5),
            tags: vec!["urgent".to_string()],
            workspace_kind: None,
            workspace_path: None,
        })
        .unwrap();
    store1.transition(task_id, Status::Todo).unwrap();
    store1.add_comment(task_id, "bob", "Looks good").unwrap();

    // Collect events from store1
    let events = store1.events_since(0).unwrap();

    // Replay into a fresh store
    let store2 = test_store();
    let applied = store2.replay_events(&events).unwrap();
    assert_eq!(applied, events.len());

    // Verify state matches
    let boards = store2.list_boards().unwrap();
    assert_eq!(boards.len(), 1);
    assert_eq!(boards[0].slug, "dev");

    let task = store2.get_task(task_id).unwrap();
    assert_eq!(task.title, "Build feature");
    assert_eq!(task.status, Status::Todo);
    assert_eq!(task.assignee.as_deref(), Some("alice"));

    let comments = store2.list_comments(task_id).unwrap();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].body, "Looks good");
}

#[test]
fn replay_rejects_unknown_events_atomically() {
    let store = test_store();
    let events = vec![
        KanbanEvent {
            id: 1,
            event_uuid: uuid::Uuid::new_v4(),
            event_type: "unknown_type".to_string(),
            payload: "{}".to_string(),
            created_at: 0,
        },
        KanbanEvent {
            id: 2,
            event_uuid: uuid::Uuid::new_v4(),
            event_type: "board_created".to_string(),
            payload: r#"{"board_id":1,"slug":"x","name":"X"}"#.to_string(),
            created_at: 0,
        },
    ];
    assert!(store.replay_events(&events).is_err());
    assert!(store.list_boards().unwrap().is_empty());
}

#[test]
fn event_payload_round_trips() {
    let payload = TaskCreatedPayload {
        task_id: 42,
        board_id: 1,
        title: "Test task".to_string(),
        body: Some("Body content".to_string()),
        status: "triage".to_string(),
        assignee: Some("alice".to_string()),
        priority: 5,
        tags: vec!["tag1".to_string(), "tag2".to_string()],
        workspace_kind: Some("existing".to_string()),
        workspace_path: Some("/workspace/task".to_string()),
    };
    let json = serde_json::to_string(&payload).unwrap();
    let deserialized: TaskCreatedPayload = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.task_id, 42);
    assert_eq!(deserialized.title, "Test task");
    assert_eq!(
        deserialized.tags,
        vec!["tag1".to_string(), "tag2".to_string()]
    );
}

#[test]
fn replay_link_events() {
    let store1 = test_store();
    let board_id = store1.create_board("b", "Board").unwrap();
    let first_id = store1
        .create_task(CreateTaskRequest {
            board_id,
            title: "T1".to_string(),
            body: None,
            status: None,
            assignee: None,
            priority: None,
            tags: vec![],
            workspace_kind: None,
            workspace_path: None,
        })
        .unwrap();
    let second_id = store1
        .create_task(CreateTaskRequest {
            board_id,
            title: "T2".to_string(),
            body: None,
            status: None,
            assignee: None,
            priority: None,
            tags: vec![],
            workspace_kind: None,
            workspace_path: None,
        })
        .unwrap();
    store1
        .create_link(first_id, second_id, LinkKind::Blocks)
        .unwrap();

    let events = store1.events_since(0).unwrap();
    let store2 = test_store();
    store2.replay_events(&events).unwrap();

    let links = store2.get_links(first_id).unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].kind, LinkKind::Blocks);
}
