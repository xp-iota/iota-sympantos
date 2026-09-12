use crate::tui::events::*;
use std::path::PathBuf;
use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use iota_core::config::NimiaConfig;
use iota_kanban::{CreateTaskRequest, SqliteKanbanStore, Status, store::KanbanStore};

fn app_with_task() -> (TuiApp, u64) {
    let mut app = TuiApp::new_for_test(NimiaConfig::default()).unwrap();
    let tmp = std::env::temp_dir().join(format!("iota-kanban-view-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();
    let store = SqliteKanbanStore::open(&tmp.join("store.db")).unwrap();
    let board_id = store.create_board("dev", "Dev").unwrap();
    let task_id = store
        .create_task(CreateTaskRequest {
            board_id,
            title: "Implement tab".to_string(),
            body: None,
            status: Some(Status::Todo),
            assignee: None,
            priority: Some(1),
            tags: vec![],
            workspace_kind: None,
            workspace_path: Some(PathBuf::from(".")),
        })
        .unwrap();
    app.kanban_store = Arc::new(store);
    (app, task_id)
}

#[test]
fn kanban_tab_slash_command_opens_view() {
    let (mut app, _) = app_with_task();

    assert!(app.handle_slash_command("/kanban tab dev"));

    assert!(app.kanban_view.active);
    assert_eq!(app.kanban_view.board_slug.as_deref(), Some("dev"));
}

#[tokio::test]
async fn kanban_tab_keys_switch_mode_and_prefill_task_command() {
    let (mut app, task_id) = app_with_task();
    app.handle_slash_command("/kanban tab dev");

    app.on_key_event(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE))
        .await;
    assert_eq!(app.kanban_view.mode, KanbanViewMode::List);

    app.on_key_event(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE))
        .await;
    assert_eq!(app.composer.text, format!("/kanban move #{task_id} "));
}
