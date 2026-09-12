use crate::store::approvals::*;
use rusqlite::OptionalExtension;
use serde_json::json;

#[test]
fn default_path_ends_with_store_sqlite() {
    let path = ApprovalStore::default_path().unwrap();
    assert_eq!(
        path.file_name().and_then(|n| n.to_str()),
        Some("store.sqlite"),
        "ApprovalStore::default_path() should point to store.sqlite, got: {}",
        path.display()
    );
}

#[test]
fn records_request_with_execution_id_before_decision() {
    let store = ApprovalStore::open(Path::new(":memory:")).unwrap();
    let request_id = store
        .record_request(
            Some("exec-1"),
            "codex",
            "shell",
            &json!({"command":"echo hi"}),
        )
        .unwrap();
    store
        .record_decision(&request_id, true, "test decision")
        .unwrap();

    let conn = store.pool.read("approvals_tests.verify");
    let execution_id = conn
        .query_row(
            "SELECT execution_id FROM approval_requests WHERE request_id = ?1",
            params![request_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .unwrap()
        .flatten();
    assert_eq!(execution_id.as_deref(), Some("exec-1"));

    let approved = conn
        .query_row(
            "SELECT approved FROM approval_decisions WHERE request_id = ?1",
            params![request_id],
            |row| row.get::<_, i64>(0),
        )
        .unwrap();
    assert_eq!(approved, 1);
}

#[test]
fn classify_shell_operation() {
    let dims = classify_operation("bash_exec", &json!({}), None);
    assert!(dims.contains(&ApprovalDimension::Shell));
}

#[test]
fn classify_network_operation_from_url_in_payload() {
    let dims = classify_operation(
        "file_write",
        &json!({"url": "https://example.com/data"}),
        None,
    );
    assert!(dims.contains(&ApprovalDimension::Network));
}

#[test]
fn classify_privilege_escalation_from_sudo() {
    let dims = classify_operation(
        "run_command",
        &json!({"command": "sudo apt-get update"}),
        None,
    );
    assert!(dims.contains(&ApprovalDimension::PrivilegeEscalation));
}

#[test]
fn classify_workspace_boundary_path() {
    let (cwd, inside_path, outside_path) = if cfg!(windows) {
        (
            Path::new("C:\\Users\\han\\coding\\creative\\iota-sympantos"),
            "C:\\Users\\han\\coding\\creative\\iota-sympantos\\src\\main.rs",
            "C:\\etc\\passwd",
        )
    } else {
        (
            Path::new("/Users/han/coding/creative/iota-sympantos"),
            "/Users/han/coding/creative/iota-sympantos/src/main.rs",
            "/etc/passwd",
        )
    };

    // Relative path, inside workspace
    let dims = classify_operation("file_write", &json!({"path": "src/main.rs"}), Some(cwd));
    assert!(!dims.contains(&ApprovalDimension::FileOutsideWorkspace));

    // Relative path with .., inside workspace
    let dims = classify_operation(
        "file_write",
        &json!({"path": "src/../src/main.rs"}),
        Some(cwd),
    );
    assert!(!dims.contains(&ApprovalDimension::FileOutsideWorkspace));

    // Relative path, outside workspace
    let dims = classify_operation("file_write", &json!({"path": "../external.txt"}), Some(cwd));
    assert!(dims.contains(&ApprovalDimension::FileOutsideWorkspace));

    // Absolute path, outside workspace
    let dims = classify_operation("file_write", &json!({"path": outside_path}), Some(cwd));
    assert!(dims.contains(&ApprovalDimension::FileOutsideWorkspace));

    // Absolute path, inside workspace
    let dims = classify_operation("file_write", &json!({"path": inside_path}), Some(cwd));
    assert!(!dims.contains(&ApprovalDimension::FileOutsideWorkspace));
}

#[test]
fn default_decision_requires_manual_approval() {
    let dims = vec![ApprovalDimension::Shell];
    let decision = default_decision(&dims);
    assert!(!decision.approved);

    let empty_dims: Vec<ApprovalDimension> = vec![];
    let empty_decision = default_decision(&empty_dims);
    assert!(!empty_decision.approved);
    assert!(empty_decision.reason.contains("manual"));
}
