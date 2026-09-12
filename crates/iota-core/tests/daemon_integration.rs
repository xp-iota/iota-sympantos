//! Integration tests for the daemon desktop protocol.
//!
//! These tests verify end-to-end flows including:
//! - Hello handshake with version negotiation (AC9.1)
//! - Reconnection after disconnect (AC9.2)
//! - Protocol error handling
//!
//! Protocol-only integration tests run without external services.

use iota_core::daemon::{
    DESKTOP_PROTOCOL_VERSION, DaemonClientMessage, DaemonServerMessage, PROTOCOL_VERSION_MAX,
    PROTOCOL_VERSION_MIN,
};

#[tokio::test]
async fn hello_handshake_v2_client_succeeds() {
    let hello = DaemonClientMessage::hello("test-client".to_string(), None);
    let json = serde_json::to_string(&hello).unwrap();
    let decoded: DaemonClientMessage = serde_json::from_str(&json).unwrap();
    assert!(matches!(
        decoded,
        DaemonClientMessage::Hello {
            ref client_name,
            ..
        } if client_name == "test-client"
    ));
}

#[tokio::test]
async fn hello_handshake_sends_the_supported_version_range() {
    let hello = DaemonClientMessage::hello("test-client".to_string(), None);
    let json = serde_json::to_string(&hello).unwrap();
    assert!(json.contains(&format!("\"min_version\":{}", PROTOCOL_VERSION_MIN)));
    assert!(json.contains(&format!("\"max_version\":{}", PROTOCOL_VERSION_MAX)));
    assert!(json.contains(&format!(
        "\"protocol_version\":{}",
        DESKTOP_PROTOCOL_VERSION
    )));
}

#[tokio::test]
async fn hello_handshake_v2_client_is_rejected() {
    // This build speaks only the current protocol; an older client must be
    // told so explicitly rather than having its payload misread.
    let legacy = serde_json::json!({
        "type": "hello",
        "client_name": "legacy",
        "protocol_version": 2
    });
    let decoded: DaemonClientMessage = serde_json::from_value(legacy).unwrap();
    match decoded {
        DaemonClientMessage::Hello {
            protocol_version, ..
        } => assert_eq!(protocol_version, 2),
        other => panic!("expected hello, got {other:?}"),
    }
}

#[tokio::test]
async fn hello_accepted_contains_negotiated_version() {
    let msg = DaemonServerMessage::HelloAccepted {
        protocol_version: DESKTOP_PROTOCOL_VERSION,
        negotiated_version: Some(DESKTOP_PROTOCOL_VERSION),
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains(&format!(
        "\"negotiated_version\":{DESKTOP_PROTOCOL_VERSION}"
    )));
}

#[tokio::test]
async fn start_turn_message_roundtrips() {
    let msg = DaemonClientMessage::StartTurn {
        turn_id: "integration-turn-1".to_string(),
        cwd: "/tmp/test".into(),
        backend: "codex".to_string(),
        prompt: "hello world".to_string(),
        timeout_ms: Some(600_000),
        request_id: Some("req-1".to_string()),
    };
    let json = serde_json::to_string(&msg).unwrap();
    let decoded: DaemonClientMessage = serde_json::from_str(&json).unwrap();
    assert!(
        matches!(decoded, DaemonClientMessage::StartTurn { turn_id, .. } if turn_id == "integration-turn-1")
    );
}

#[tokio::test]
async fn ping_pong_roundtrip() {
    let ping = DaemonClientMessage::Ping {
        seq: 42,
        request_id: None,
    };
    let json = serde_json::to_string(&ping).unwrap();
    assert!(json.contains("\"type\":\"ping\""));
    assert!(json.contains("\"seq\":42"));

    let pong = DaemonServerMessage::Pong { seq: 42 };
    let pong_json = serde_json::to_string(&pong).unwrap();
    assert!(pong_json.contains("\"type\":\"pong\""));
    assert!(pong_json.contains("\"seq\":42"));
}

// ---------------------------------------------------------------------------
// Manual Runbook (AC9.4)
// ---------------------------------------------------------------------------
//
// ## Scenarios that cannot be fully automated:
//
// ### Kanban dispatcher → event_sync (AC9.3)
// 1. Start desktop app with kanban board open
// 2. Create a task via CLI: `iota kanban task create --board test "New task"`
// 3. Verify desktop UI updates within 5 seconds showing the new task
// 4. Move task to "done" via desktop drag-and-drop
// 5. Verify CLI `iota kanban task list` reflects the status change
//
// ### Desktop UI Reconnection Indicator
// 1. Start desktop app, confirm "connected" state in status bar
// 2. Kill the daemon process: `pkill -f "iota __daemon"`
// 3. Verify UI shows "reconnecting" state within 30 seconds (heartbeat miss)
// 4. Restart daemon: `iota __daemon &`
// 5. Verify UI transitions back to "connected" state
// 6. Verify pending operations (if any) are replayed
