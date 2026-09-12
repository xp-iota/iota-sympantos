use crate::cli::kanban_cmd::*;
use iota_kanban::{CreateTaskRequest, Status};

fn fake_hermes_echo_spec(tmp: &Path) -> PathBuf {
    if cfg!(windows) {
        let path = tmp.join("fake-hermes.cmd");
        std::fs::write(&path, "@echo off\r\necho {\"spec\":\"cli spec\"}\r\n").unwrap();
        path
    } else {
        let path = tmp.join("fake-hermes.sh");
        std::fs::write(&path, "#!/bin/sh\necho '{\"spec\":\"cli spec\"}'\n").unwrap();
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

#[test]
fn specify_updates_task_body() {
    let tmp = std::env::temp_dir().join(format!("iota-cli-kanban-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();
    let store = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
    let board_id = store.create_board("dev", "Development").unwrap();
    let task_id = store
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
    let bridge = AdvancedBridge::new(fake_hermes_echo_spec(&tmp), tmp.join("shadows"));
    let args = vec!["specify".to_string(), task_id.to_string()];

    let out = execute_kanban_command(&args, &store, &bridge).unwrap();

    assert!(out[0].contains("Specified task"));
    assert_eq!(
        store.get_task(task_id).unwrap().body.as_deref(),
        Some("cli spec")
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn export_and_import_events_round_trip() {
    let tmp = std::env::temp_dir().join(format!("iota-cli-kanban-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();
    let source = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
    let board_id = source.create_board("dev", "Development").unwrap();
    let task_id = source
        .create_task(CreateTaskRequest {
            board_id,
            title: "Exported task".to_string(),
            body: None,
            status: Some(Status::Todo),
            assignee: None,
            priority: None,
            tags: vec![],
            workspace_kind: None,
            workspace_path: None,
        })
        .unwrap();
    let bridge = AdvancedBridge::new(fake_hermes_echo_spec(&tmp), tmp.join("shadows"));
    let bundle_path = tmp.join("events.json");

    let export_out = execute_kanban_command(
        &["export".to_string(), bundle_path.display().to_string()],
        &source,
        &bridge,
    )
    .unwrap();
    assert!(export_out[0].contains("Exported"));

    let target = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
    let import_out = execute_kanban_command(
        &["import".to_string(), bundle_path.display().to_string()],
        &target,
        &bridge,
    )
    .unwrap();

    assert!(import_out[0].contains("Imported"));
    assert_eq!(target.get_task(task_id).unwrap().title, "Exported task");
    let _ = std::fs::remove_dir_all(&tmp);
}
