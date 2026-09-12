use crate::*;
use iota_core::daemon::{DESKTOP_PROTOCOL_VERSION, DaemonClientMessage};

#[test]
fn desktop_hello_uses_current_protocol_version() {
    let message = daemon_client::hello_message();
    assert!(matches!(
        message,
        DaemonClientMessage::Hello {
            protocol_version: DESKTOP_PROTOCOL_VERSION,
            ..
        }
    ));
}

#[test]
fn test_get_memory_context_snapshot_message_building() {
    let cwd = PathBuf::from("/tmp/workspace");
    let message = DaemonClientMessage::GetMemoryContextSnapshot {
        cwd: cwd.clone(),
        scope_mode: iota_core::daemon::DesktopMemoryScopeMode::Workspace,
        request_id: None,
    };
    if let DaemonClientMessage::GetMemoryContextSnapshot {
        cwd: path,
        scope_mode,
        ..
    } = message
    {
        assert_eq!(path, cwd);
        assert_eq!(
            scope_mode,
            iota_core::daemon::DesktopMemoryScopeMode::Workspace
        );
    } else {
        panic!("expected GetMemoryContextSnapshot message");
    }
}

#[test]
fn event_mentions_task_only_matches_task_link_fields() {
    let unrelated_same_board = KanbanEvent {
        id: 1,
        event_uuid: uuid::Uuid::new_v4(),
        event_type: "task_created".to_string(),
        payload: serde_json::json!({
            "task_id": 42,
            "board_id": 7,
            "id": 7
        })
        .to_string(),
        created_at: 0,
    };
    let related_link = KanbanEvent {
        id: 2,
        event_uuid: uuid::Uuid::new_v4(),
        event_type: "link_created".to_string(),
        payload: serde_json::json!({
            "from_id": 42,
            "to_id": 7,
            "kind": "blocks"
        })
        .to_string(),
        created_at: 0,
    };

    assert!(!event_mentions_task(&unrelated_same_board, 7));
    assert!(event_mentions_task(&related_link, 7));
}

#[test]
fn task_logs_reads_logs_from_shadows_dir() {
    let tmp = std::env::temp_dir().join(format!("iota-desktop-logs-{}", uuid::Uuid::new_v4()));
    let shadows = tmp.join("shadows");
    std::fs::create_dir_all(&shadows).unwrap();
    std::fs::write(shadows.join("7.stdout.log"), "stdout tail").unwrap();
    std::fs::write(shadows.join("7.stderr.log"), "stderr tail").unwrap();

    let logs = task_logs(7, &shadows).unwrap();

    assert!(
        std::path::Path::new(&logs.stdout_path)
            .ends_with(std::path::Path::new("shadows").join("7.stdout.log"))
    );
    assert!(
        std::path::Path::new(&logs.stderr_path)
            .ends_with(std::path::Path::new("shadows").join("7.stderr.log"))
    );
    assert_eq!(logs.stdout, "stdout tail");
    assert_eq!(logs.stderr, "stderr tail");
    let _ = std::fs::remove_dir_all(&tmp);
}
