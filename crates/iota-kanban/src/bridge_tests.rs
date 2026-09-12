use crate::CreateTaskRequest;
use crate::bridge::{AdvancedBridge, read_new_shadow_tasks};
use crate::sqlite_store::SqliteKanbanStore;
use crate::store::KanbanStore;
use std::path::{Path, PathBuf};

#[test]
fn bridge_is_available_false_for_missing_binary() {
    let tmp = std::env::temp_dir().join(format!("iota-bridge-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();
    let bridge = AdvancedBridge::new(PathBuf::from("/nonexistent/hermes-binary-xyz"), tmp.clone());
    assert!(!bridge.is_available());
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn specify_fails_gracefully_when_hermes_missing() {
    let store = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
    let board_id = store.create_board("b", "B").unwrap();
    let task_id = store
        .create_task(CreateTaskRequest {
            board_id,
            title: "Vague task".into(),
            body: Some("do stuff".into()),
            status: None,
            assignee: None,
            priority: None,
            tags: vec![],
            workspace_kind: None,
            workspace_path: None,
        })
        .unwrap();

    let tmp = std::env::temp_dir().join(format!("iota-bridge-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();
    let bridge = AdvancedBridge::new(PathBuf::from("/nonexistent/hermes-xyz"), tmp.clone());

    let result = bridge.specify(task_id, &store);
    assert!(result.is_err());
    assert!(
        !tmp.join(task_id.to_string()).exists(),
        "shadow directory should be cleaned after specify spawn failure"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn decompose_fails_gracefully_when_hermes_missing() {
    let store = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
    let board_id = store.create_board("b", "B").unwrap();
    let task_id = store
        .create_task(CreateTaskRequest {
            board_id,
            title: "Big task".into(),
            body: None,
            status: None,
            assignee: None,
            priority: None,
            tags: vec![],
            workspace_kind: None,
            workspace_path: None,
        })
        .unwrap();

    let tmp = std::env::temp_dir().join(format!("iota-bridge-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();
    let bridge = AdvancedBridge::new(PathBuf::from("/nonexistent/hermes-xyz"), tmp.clone());

    let result = bridge.decompose(task_id, &store);
    assert!(result.is_err());
    assert!(
        !tmp.join(task_id.to_string()).exists(),
        "shadow directory should be cleaned after decompose spawn failure"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn with_timeout_configures_bridge() {
    let tmp = std::env::temp_dir().join(format!("iota-bridge-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();
    let bridge = AdvancedBridge::new(PathBuf::from("hermes"), tmp.clone())
        .with_timeout(std::time::Duration::from_secs(60));
    assert_eq!(bridge.timeout, std::time::Duration::from_secs(60));
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn specify_respects_timeout() {
    let store = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
    let board_id = store.create_board("b", "B").unwrap();
    let task_id = store
        .create_task(CreateTaskRequest {
            board_id,
            title: "Slow task".into(),
            body: None,
            status: None,
            assignee: None,
            priority: None,
            tags: vec![],
            workspace_kind: None,
            workspace_path: None,
        })
        .unwrap();

    let tmp = std::env::temp_dir().join(format!("iota-bridge-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();
    let hermes_path = if cfg!(windows) {
        let path = tmp.join("slow-hermes.cmd");
        std::fs::write(
                &path,
                "@echo off\r\npowershell -NoProfile -WindowStyle Hidden -Command Start-Sleep -Seconds 10\r\n",
            )
            .unwrap();
        path
    } else {
        let path = tmp.join("slow-hermes.sh");
        std::fs::write(&path, "#!/bin/sh\nsleep 10\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&path).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&path, perms).unwrap();
        }
        path
    };

    let bridge = AdvancedBridge::new(hermes_path, tmp.join("shadows"))
        .with_timeout(std::time::Duration::from_millis(50));
    let started = std::time::Instant::now();
    let result = bridge.specify(task_id, &store);

    let err = match result {
        Ok(_) => panic!("slow hermes command should time out"),
        Err(err) => err,
    };
    assert!(
        format!("{err:#}").contains("timed out"),
        "expected timeout error, got {err:#}"
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "timeout should stop the command promptly"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn read_new_shadow_tasks_excludes_existing_materialized_tasks() {
    let tmp = std::env::temp_dir().join(format!("iota-bridge-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();
    let db_path = tmp.join("kanban.db");
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute_batch(
        "CREATE TABLE tasks (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                body TEXT,
                assignee TEXT
             );",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO tasks (id, title, body, assignee) VALUES ('1', 'parent', NULL, NULL)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO tasks (id, title, body, assignee) VALUES ('2', 'linked', NULL, NULL)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO tasks (id, title, body, assignee) VALUES ('3', 'new child', 'body', 'alice')",
        [],
    )
    .unwrap();
    drop(conn);

    let tasks = read_new_shadow_tasks(&db_path, &["1".to_string(), "2".to_string()]).unwrap();

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].title, "new child");
    assert_eq!(tasks[0].body.as_deref(), Some("body"));
    assert_eq!(tasks[0].assignee.as_deref(), Some("alice"));
    let _ = std::fs::remove_dir_all(&tmp);
}

// ── ensure_bridge_available tests ────────────────────────────────────────────

#[test]
fn ensure_bridge_available_returns_err_when_hermes_missing() {
    let tmp = std::env::temp_dir().join(format!("iota-bridge-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();
    let bridge = AdvancedBridge::new(PathBuf::from("/nonexistent/hermes-xyz"), tmp.clone());

    let result = crate::bridge::ensure_bridge_available(&bridge);
    assert!(
        result.is_err(),
        "expected Err when hermes binary is missing"
    );

    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("/nonexistent/hermes-xyz"),
        "error message must contain the hermes_bin path, got: {msg}"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn specify_error_message_contains_hermes_binary_not_found() {
    let tmp = std::env::temp_dir().join(format!("iota-bridge-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();
    let store = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
    let board_id = store.create_board("b", "B").unwrap();
    let task_id = store
        .create_task(CreateTaskRequest {
            board_id,
            title: "Test task".into(),
            body: None,
            status: None,
            assignee: None,
            priority: None,
            tags: vec![],
            workspace_kind: None,
            workspace_path: None,
        })
        .unwrap();

    let missing_bin = PathBuf::from("/nonexistent/hermes-not-found-xyz");
    let bridge = AdvancedBridge::new(missing_bin.clone(), tmp.clone());

    let err = bridge.specify(task_id, &store).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("hermes binary not found"),
        "specify error must contain 'hermes binary not found', got: {msg}"
    );
    assert!(
        msg.contains(missing_bin.to_str().unwrap()),
        "specify error must contain the binary path, got: {msg}"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn decompose_error_message_contains_hermes_binary_not_found() {
    let tmp = std::env::temp_dir().join(format!("iota-bridge-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();
    let store = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
    let board_id = store.create_board("b", "B").unwrap();
    let task_id = store
        .create_task(CreateTaskRequest {
            board_id,
            title: "Big task".into(),
            body: None,
            status: None,
            assignee: None,
            priority: None,
            tags: vec![],
            workspace_kind: None,
            workspace_path: None,
        })
        .unwrap();

    let missing_bin = PathBuf::from("/nonexistent/hermes-not-found-xyz");
    let bridge = AdvancedBridge::new(missing_bin.clone(), tmp.clone());

    let err = bridge.decompose(task_id, &store).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("hermes binary not found"),
        "decompose error must contain 'hermes binary not found', got: {msg}"
    );
    assert!(
        msg.contains(missing_bin.to_str().unwrap()),
        "decompose error must contain the binary path, got: {msg}"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}
