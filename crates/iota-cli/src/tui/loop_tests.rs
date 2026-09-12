use crate::tui::r#loop::*;
use std::sync::{Arc, Mutex};

use tokio::sync::oneshot;

use crate::tui::Overlay;
use iota_core::config::NimiaConfig;
use iota_kanban::{CreateTaskRequest, Dispatcher, DispatcherConfig, SqliteKanbanStore, Status};

#[test]
fn approval_request_closes_existing_overlay_so_prompt_is_visible() {
    let mut app = TuiApp::new_for_test(NimiaConfig::default()).unwrap();
    let (reply, _rx) = oneshot::channel();
    app.overlay = Overlay::QuitConfirm;

    app.run_loop_handle_approval_request(ApprovalRequest {
        tool_name: "shell".to_string(),
        params: serde_json::Value::Null,
        reply,
    });

    assert_eq!(app.overlay, Overlay::None);
    assert!(app.pending_approval.is_some());
}

#[test]
fn kanban_daemon_starts_disabled() {
    let app = TuiApp::new_for_test(NimiaConfig::default()).unwrap();

    assert!(
        !app.kanban_daemon_active
            .load(std::sync::atomic::Ordering::Relaxed),
        "auto-dispatch should require explicit user opt-in"
    );
}

#[test]
fn tick_drives_kanban_dispatcher_lifecycle() {
    let mut app = TuiApp::new_for_test(NimiaConfig::default()).unwrap();
    let tmp = std::env::temp_dir().join(format!("iota-tui-kanban-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();
    app.kanban_store = Arc::new(SqliteKanbanStore::open(&tmp.join("store.db")).unwrap());
    app.kanban_dispatcher = Arc::new(Mutex::new(Dispatcher::new(DispatcherConfig {
        hermes_bin: PathBuf::from("/missing/hermes-for-iota-test"),
        shadows_dir: tmp.join("shadows"),
        ..Default::default()
    })));
    let slug = format!("tick-test-{}", uuid::Uuid::new_v4());
    let board_id = app.kanban_store.create_board(&slug, "Tick Test").unwrap();
    let task_id = app
        .kanban_store
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
    let result = {
        let store = app.kanban_store.clone();
        lock_or_recover(&app.kanban_dispatcher).tick(store.as_ref())
    };
    app.run_loop_handle_kanban_dispatch_result(result);

    let runs = app.kanban_store.get_runs(task_id).unwrap();
    assert_eq!(runs.len(), 1, "tick should dispatch ready task");
    assert_eq!(runs[0].status, iota_kanban::RunStatus::Failed);
    assert_eq!(
        app.kanban_store.get_task(task_id).unwrap().status,
        Status::Ready
    );
}
