use crate::store::ledger::*;
use std::path::Path;

#[test]
fn session_created_and_summary_readable() {
    let ledger = SessionLedger::open(Path::new(":memory:")).unwrap();
    let cwd = Path::new("/tmp/test-project");
    ledger
        .ensure_session("sess-1", cwd, Some("codex"), Some("gpt-4o"))
        .unwrap();

    let summary = ledger.summary("sess-1").unwrap().unwrap();
    assert_eq!(summary.iota_session_id, "sess-1");
    assert_eq!(summary.active_backend.as_deref(), Some("codex"));
    assert_eq!(summary.turn_count, 0);
    assert!(summary.last_output_summary.is_none());
}

#[test]
fn turn_increments_turn_count() {
    let ledger = SessionLedger::open(Path::new(":memory:")).unwrap();
    let cwd = Path::new("/tmp/test-project");
    ledger
        .ensure_session("sess-2", cwd, Some("codex"), None)
        .unwrap();

    ledger
        .record_turn("sess-2", "codex", None, "hash1", "output1", "completed")
        .unwrap();
    ledger
        .record_turn("sess-2", "codex", None, "hash2", "output2", "completed")
        .unwrap();

    let summary = ledger.summary("sess-2").unwrap().unwrap();
    assert_eq!(summary.turn_count, 2);
    // Both turns may share the same second-granularity timestamp; just assert
    // that some output summary was recorded.
    assert!(summary.last_output_summary.is_some());
}

#[test]
fn handoff_publish_and_read() {
    let ledger = SessionLedger::open(Path::new(":memory:")).unwrap();
    let cwd = Path::new("/tmp/test-project");
    ledger
        .ensure_session("sess-3", cwd, Some("claude"), None)
        .unwrap();

    ledger
        .publish_handoff(
            "sess-3",
            Some("claude"),
            Some("codex"),
            cwd,
            "context summary",
        )
        .unwrap();

    let handoff = ledger.read_handoff("sess-3", Some("codex"), cwd).unwrap();
    assert_eq!(handoff.as_deref(), Some("context summary"));
}

#[test]
fn handoff_read_returns_none_for_unknown_session() {
    let ledger = SessionLedger::open(Path::new(":memory:")).unwrap();
    let cwd = Path::new("/tmp/test-project");
    let result = ledger.read_handoff("no-such-session", None, cwd).unwrap();
    assert!(result.is_none());
}

#[test]
fn latest_session_for_cwd_returns_some_for_known_cwd() {
    let ledger = SessionLedger::open(Path::new(":memory:")).unwrap();
    let cwd = Path::new("/tmp/proj");
    let other_cwd = Path::new("/tmp/other");

    ledger
        .ensure_session("sess-known", cwd, Some("codex"), None)
        .unwrap();

    let latest = ledger.latest_session_for_cwd(cwd).unwrap();
    assert_eq!(latest.as_deref(), Some("sess-known"));

    let none = ledger.latest_session_for_cwd(other_cwd).unwrap();
    assert!(none.is_none());
}

#[test]
fn default_path_ends_with_store_sqlite() {
    let path = SessionLedger::default_path().unwrap();
    assert_eq!(
        path.file_name().and_then(|n| n.to_str()),
        Some("store.sqlite"),
        "SessionLedger::default_path() should point to store.sqlite, got: {}",
        path.display()
    );
}

#[test]
fn record_backend_session_is_idempotent() {
    let ledger = SessionLedger::open(Path::new(":memory:")).unwrap();
    let cwd = Path::new("/tmp/proj");
    ledger
        .ensure_session("sess-b", cwd, Some("codex"), None)
        .unwrap();

    ledger
        .record_backend_session("sess-b", "codex", Some("bsess-1"), cwd)
        .unwrap();
    // Second call should not error (ON CONFLICT upsert).
    ledger
        .record_backend_session("sess-b", "codex", Some("bsess-2"), cwd)
        .unwrap();
}
