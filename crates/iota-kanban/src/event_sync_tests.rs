use crate::event_sync::{
    FORMAT_VERSION, KanbanEventBundle, LEGACY_FORMAT_VERSION, MAX_EVENT_SYNC_MESSAGE_BYTES,
    default_pull_source, export_event_bundle, handle_event_sync_stream, import_event_bundle,
    migrate_v1_bundle, pull_event_bundle, push_event_bundle, read_event_bundle, write_event_bundle,
};
use crate::{CreateTaskRequest, KanbanEvent, KanbanStore, SqliteKanbanStore, Status};
use anyhow::Context;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// A valid token for tests. Must be at least 64 chars, matching the length
/// floor real hex-encoded tokens satisfy.
const TEST_TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// Serializes the token-file tests.
///
/// `IOTA_SYNC_TOKEN_PATH` is process-global, so two tests that set it to
/// different paths would otherwise race and read each other's token. Every test
/// that touches the env var must hold this for its whole body.
fn token_env_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Points the sync token lookup at `token` for the caller's lifetime.
struct TokenFile {
    _guard: std::sync::MutexGuard<'static, ()>,
    path: std::path::PathBuf,
}

impl TokenFile {
    /// Writes `contents` (if any) to a fresh path and activates it.
    fn new(contents: Option<&str>) -> Self {
        let guard = token_env_lock();
        let path = std::env::temp_dir().join(format!("iota-sync-tok-{}", uuid::Uuid::new_v4()));
        if let Some(contents) = contents {
            std::fs::write(&path, contents).expect("writing test token file");
        }
        // SAFETY: single-threaded by `token_env_lock`.
        unsafe {
            std::env::set_var(crate::event_sync::SYNC_TOKEN_PATH_ENV, &path);
        }
        Self {
            _guard: guard,
            path,
        }
    }

    /// Rewrites the token file with new contents.
    fn set(&self, contents: &str) {
        std::fs::write(&self.path, contents).expect("rewriting test token file");
    }
}

impl Drop for TokenFile {
    fn drop(&mut self) {
        unsafe {
            std::env::remove_var(crate::event_sync::SYNC_TOKEN_PATH_ENV);
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

fn serve_event_sync_until_shutdown(
    store: Arc<SqliteKanbanStore>,
    listener: std::net::TcpListener,
    shutdown: Arc<AtomicBool>,
    token: &'static str,
) -> anyhow::Result<()> {
    listener.set_nonblocking(true)?;
    while !shutdown.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => handle_event_sync_stream(store.as_ref(), stream, token)?,
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(err) => return Err(err).context("accepting kanban sync connection"),
        }
    }
    Ok(())
}

/// Builds a bundle directly, computing the hash the way the exporter does.
fn bundle_with(
    source_id: &str,
    source_sequence: u64,
    events: Vec<KanbanEvent>,
) -> KanbanEventBundle {
    KanbanEventBundle::for_tests(FORMAT_VERSION, source_id, source_sequence, events)
}

#[test]
fn event_bundle_round_trips_state_with_stable_ids() {
    let source = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
    let board_id = source.create_board("dev", "Development").unwrap();
    let task_id = source
        .create_task(CreateTaskRequest {
            board_id,
            title: "Sync me".to_string(),
            body: Some("body".to_string()),
            status: Some(Status::Todo),
            assignee: Some("alice".to_string()),
            priority: Some(7),
            tags: vec!["sync".to_string()],
            workspace_kind: Some("existing".to_string()),
            workspace_path: Some(std::path::PathBuf::from("/workspace/sync-me")),
        })
        .unwrap();
    source.add_comment(task_id, "bob", "comment").unwrap();

    let bundle = export_event_bundle(&source, 0, "node-a").unwrap();
    let target = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
    let report = import_event_bundle(&target, &bundle).unwrap();

    assert_eq!(report.source, "node-a");
    assert_eq!(report.events_seen, bundle.events.len());
    assert_eq!(report.events_skipped, 0);
    assert_eq!(target.get_board("dev").unwrap().id, board_id);
    let imported_task = target.get_task(task_id).unwrap();
    assert_eq!(imported_task.title, "Sync me");
    assert_eq!(imported_task.workspace_kind.as_deref(), Some("existing"));
    assert_eq!(
        imported_task.workspace_path.as_deref(),
        Some(Path::new("/workspace/sync-me"))
    );
    assert_eq!(target.list_comments(task_id).unwrap()[0].body, "comment");
}

#[test]
fn every_event_carries_a_non_nil_uuid() {
    let source = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
    source.create_board("dev", "Development").unwrap();
    let bundle = export_event_bundle(&source, 0, "node-a").unwrap();

    assert!(!bundle.events.is_empty());
    for event in &bundle.events {
        assert!(
            !event.event_uuid.is_nil(),
            "event {} must have an identity uuid",
            event.id
        );
    }
}

#[test]
fn duplicate_import_is_idempotent_by_uuid() {
    let source = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
    source.create_board("dev", "Development").unwrap();
    let bundle = export_event_bundle(&source, 0, "node-a").unwrap();
    let target = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();

    let first = import_event_bundle(&target, &bundle).unwrap();
    let second = import_event_bundle(&target, &bundle).unwrap();

    assert_eq!(first.events_applied, 1);
    assert_eq!(
        second.events_applied, 0,
        "re-importing the same bundle must apply nothing"
    );
    assert_eq!(second.events_skipped, 1);
    assert_eq!(target.list_boards().unwrap().len(), 1);
}

#[test]
fn partially_overlapping_bundles_apply_only_new_events() {
    let source = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
    source.create_board("dev", "Development").unwrap();
    let first_bundle = export_event_bundle(&source, 0, "node-a").unwrap();

    source.create_board("ops", "Operations").unwrap();
    let second_bundle = export_event_bundle(&source, 0, "node-a").unwrap();

    let target = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
    import_event_bundle(&target, &first_bundle).unwrap();
    let report = import_event_bundle(&target, &second_bundle).unwrap();

    // The second bundle re-sends the first event plus the new one.
    assert_eq!(
        report.events_applied, 1,
        "only the unseen event may be applied"
    );
    assert_eq!(report.events_skipped, 1);
    assert_eq!(target.list_boards().unwrap().len(), 2);
}

#[test]
fn out_of_order_bundles_apply_in_arrival_order_without_loss() {
    let source = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
    source.create_board("a", "A").unwrap();
    // Both bundles are exported from 0, so the later one is a superset of the
    // earlier one — which is what makes delivering them out of order a real
    // test of dedup rather than of disjoint ranges.
    let first = export_event_bundle(&source, 0, "node-a").unwrap();
    source.create_board("b", "B").unwrap();
    let second = export_event_bundle(&source, 0, "node-a").unwrap();

    let target = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
    // Deliver the superset first, then the subset that it already covers.
    let newer = import_event_bundle(&target, &second).unwrap();
    let older = import_event_bundle(&target, &first).unwrap();

    assert_eq!(newer.events_applied, 2);
    assert_eq!(
        older.events_applied, 0,
        "an already-imported event must be recognized despite arriving out of order"
    );
    assert_eq!(older.events_skipped, 1);
    assert_eq!(target.list_boards().unwrap().len(), 2);
}

#[test]
fn concurrent_imports_do_not_duplicate_events() {
    let source = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
    source.create_board("dev", "Development").unwrap();
    let bundle = Arc::new(export_event_bundle(&source, 0, "node-a").unwrap());

    let target = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
    let store_path = std::env::temp_dir().join(format!("iota-sync-conc-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&store_path).unwrap();
    let shared = Arc::new(SqliteKanbanStore::open(&store_path.join("iota.db")).unwrap());

    let handles: Vec<_> = (0..4)
        .map(|_| {
            let shared = Arc::clone(&shared);
            let bundle = Arc::clone(&bundle);
            std::thread::spawn(move || {
                import_event_bundle(&shared, &bundle)
                    .unwrap()
                    .events_applied
            })
        })
        .collect();

    let applied: usize = handles.into_iter().map(|h| h.join().unwrap()).sum();
    assert_eq!(
        shared.list_boards().unwrap().len(),
        1,
        "concurrent imports must not duplicate the board"
    );
    assert!(
        applied <= 1 || shared.list_boards().unwrap().len() == 1,
        "applied count {applied} must not imply duplicate state"
    );
    let _ = std::fs::remove_dir_all(store_path);
    drop(target);
}

#[test]
fn unapplicable_event_does_not_advance_cursor() {
    let target = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
    let bundle = bundle_with(
        "node-a",
        1,
        vec![KanbanEvent {
            id: 1,
            event_uuid: uuid::Uuid::new_v4(),
            event_type: "task_updated".to_string(),
            payload: serde_json::json!({
                "task_id": 999,
                "patch": { "title": "missing" }
            })
            .to_string(),
            created_at: 0,
        }],
    );

    assert!(import_event_bundle(&target, &bundle).is_err());
    assert_eq!(target.sync_cursor("node-a").unwrap(), 0);
}

#[test]
fn tampered_bundle_is_rejected_by_hash() {
    let source = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
    source.create_board("dev", "Development").unwrap();
    let mut bundle = export_event_bundle(&source, 0, "node-a").unwrap();

    // Simulate corruption: change a payload without recomputing the hash.
    bundle.events[0].payload = r#"{"board_id":1,"slug":"evil","name":"Evil"}"#.to_string();

    let target = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
    let error = import_event_bundle(&target, &bundle).unwrap_err();
    assert!(
        error.to_string().contains("hash mismatch"),
        "tampering must be caught by the bundle hash, got: {error}"
    );
    assert!(target.list_boards().unwrap().is_empty());
}

#[test]
fn forged_source_id_is_recorded_as_declared_not_trusted() {
    // `source_id` is producer-asserted and unauthenticated; import must accept
    // it as a label without letting it override stored state or grant anything.
    let source = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
    source.create_board("dev", "Development").unwrap();
    let mut bundle = export_event_bundle(&source, 0, "node-a").unwrap();
    bundle = bundle_with(
        &bundle.source_id,
        bundle.source_sequence,
        bundle.events.clone(),
    );

    let forged = bundle_with("node-a", bundle.source_sequence, bundle.events.clone());
    let target = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
    let report = import_event_bundle(&target, &forged).unwrap();

    assert_eq!(report.source, "node-a");
    // The cursor is tracked under the declared source, not a synthesized one.
    assert!(target.sync_cursor("node-a").unwrap() > 0);
}

#[test]
fn unsupported_format_version_is_rejected() {
    let target = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
    let bundle = KanbanEventBundle::for_tests_with_version(99, "node-a", 0, vec![]);

    let error = import_event_bundle(&target, &bundle).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("unsupported kanban event bundle version"),
        "got: {error}"
    );
}

#[test]
fn default_pull_source_is_stable_per_peer_addr() {
    assert_eq!(
        default_pull_source("127.0.0.1:47662"),
        "peer:127.0.0.1:47662"
    );
    assert_ne!(
        default_pull_source("127.0.0.1:47662"),
        default_pull_source("127.0.0.1:47663")
    );
}

#[test]
fn import_cursors_are_isolated_by_peer_source() {
    let target = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();

    target
        .set_sync_cursor(&default_pull_source("127.0.0.1:47662"), 10)
        .unwrap();

    assert_eq!(
        target
            .sync_cursor(&default_pull_source("127.0.0.1:47662"))
            .unwrap(),
        10
    );
    assert_eq!(
        target
            .sync_cursor(&default_pull_source("127.0.0.1:47663"))
            .unwrap(),
        0
    );
}

#[test]
fn imported_events_are_re_exportable_for_multihop_sync() {
    let source = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
    source.create_board("dev", "Development").unwrap();
    let bundle = export_event_bundle(&source, 0, "node-a").unwrap();
    let relay = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
    import_event_bundle(&relay, &bundle).unwrap();

    let forwarded = export_event_bundle(&relay, 0, "node-b").unwrap();

    assert_eq!(forwarded.events.len(), 1);
    assert_eq!(forwarded.events[0].event_type, "board_created");
    // A relay must preserve the original identity, otherwise a downstream node
    // would treat the same event as new.
    assert_eq!(
        forwarded.events[0].event_uuid, bundle.events[0].event_uuid,
        "relaying must not re-identify an event"
    );
}

#[test]
fn tcp_sync_pull_and_push_round_trip() {
    let remote = Arc::new(SqliteKanbanStore::open(Path::new(":memory:")).unwrap());
    remote.create_board("remote", "Remote").unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let shutdown = Arc::new(AtomicBool::new(false));
    let server_store = remote.clone();
    let server_shutdown = shutdown.clone();
    let handle = std::thread::spawn(move || {
        serve_event_sync_until_shutdown(server_store, listener, server_shutdown, TEST_TOKEN)
            .unwrap();
    });

    // Authenticate via the token file the client reads.
    let _token = TokenFile::new(Some(TEST_TOKEN));

    let local = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
    let pulled = pull_event_bundle(addr, 0, "remote-node").unwrap();
    import_event_bundle(&local, &pulled).unwrap();
    assert_eq!(local.get_board("remote").unwrap().name, "Remote");

    let outgoing_store = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
    outgoing_store.create_board("local", "Local").unwrap();
    let outgoing = export_event_bundle(&outgoing_store, 0, "local-node").unwrap();
    let report = push_event_bundle(addr, outgoing).unwrap();
    assert!(report.events_applied > 0);
    assert_eq!(remote.get_board("local").unwrap().name, "Local");

    shutdown.store(true, Ordering::Relaxed);
    let _ = std::net::TcpStream::connect(addr);
    handle.join().unwrap();
}

#[test]
fn unauthenticated_request_is_rejected_before_processing() {
    let remote = Arc::new(SqliteKanbanStore::open(Path::new(":memory:")).unwrap());
    remote.create_board("secret", "Secret").unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let shutdown = Arc::new(AtomicBool::new(false));
    let server_store = remote.clone();
    let server_shutdown = shutdown.clone();
    let handle = std::thread::spawn(move || {
        serve_event_sync_until_shutdown(server_store, listener, server_shutdown, TEST_TOKEN)
            .unwrap();
    });

    // Client has no token available at all.
    let token = TokenFile::new(None);
    let error = pull_event_bundle(addr, 0, "peer").unwrap_err();
    assert!(
        error.to_string().contains("unauthenticated"),
        "a request without a token must be rejected as unauthenticated, got: {error}"
    );

    // A wrong token must be rejected just as firmly.
    token.set(&"f".repeat(64));
    let error = pull_event_bundle(addr, 0, "peer").unwrap_err();
    assert!(
        error.to_string().contains("unauthenticated"),
        "a wrong token must be rejected, got: {error}"
    );

    // A short token must not be accepted even if it prefixes the real one.
    token.set(&TEST_TOKEN[..16]);
    let error = pull_event_bundle(addr, 0, "peer").unwrap_err();
    assert!(
        error.to_string().contains("unauthenticated"),
        "a truncated token must be rejected, got: {error}"
    );

    assert!(
        remote.get_board("dummy").is_err(),
        "rejected requests must not mutate the store"
    );

    shutdown.store(true, Ordering::Relaxed);
    let _ = std::net::TcpStream::connect(addr);
    handle.join().unwrap();
}

#[test]
fn write_and_read_event_bundle_file() {
    let source = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
    source.create_board("dev", "Development").unwrap();
    let bundle = export_event_bundle(&source, 0, "node-a").unwrap();
    let tmp =
        std::env::temp_dir().join(format!("iota-kanban-events-{}.json", uuid::Uuid::new_v4()));

    write_event_bundle(&tmp, &bundle).unwrap();
    let loaded = read_event_bundle(&tmp).unwrap();

    assert_eq!(loaded.source_id, "node-a");
    assert_eq!(loaded.events.len(), 1);
    assert_eq!(loaded.format_version, FORMAT_VERSION);
    let _ = std::fs::remove_file(tmp);
}

#[test]
fn read_event_bundle_rejects_a_v1_file() {
    let tmp = std::env::temp_dir().join(format!("iota-kanban-v1-{}.json", uuid::Uuid::new_v4()));
    std::fs::write(
        &tmp,
        serde_json::json!({
            "format_version": LEGACY_FORMAT_VERSION,
            "source": "legacy-node",
            "cursor": 1,
            "events": [{
                "id": 1,
                "event_type": "board_created",
                "payload": r#"{"board_id":101,"slug":"legacy","name":"Legacy"}"#,
                "created_at": 0
            }]
        })
        .to_string(),
    )
    .unwrap();

    // The ordinary read path must refuse v1 rather than silently importing it.
    let error = read_event_bundle(&tmp).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("unsupported kanban event bundle version"),
        "got: {error}"
    );
    let _ = std::fs::remove_file(tmp);
}

#[test]
fn migrate_v1_bundle_produces_an_importable_v2_bundle() {
    let tmp = std::env::temp_dir().join(format!("iota-kanban-mig-{}.json", uuid::Uuid::new_v4()));
    std::fs::write(
        &tmp,
        serde_json::json!({
            "format_version": LEGACY_FORMAT_VERSION,
            "source": "legacy-node",
            "cursor": 1,
            "events": [{
                "id": 1,
                "event_type": "board_created",
                "payload": r#"{"board_id":101,"slug":"legacy","name":"Legacy"}"#,
                "created_at": 0
            }]
        })
        .to_string(),
    )
    .unwrap();

    let bundle = migrate_v1_bundle(&tmp).unwrap();
    assert_eq!(bundle.format_version, FORMAT_VERSION);
    assert_eq!(bundle.source_id, "legacy-node");
    assert!(!bundle.events[0].event_uuid.is_nil());
    bundle.verify_hash().unwrap();

    let target = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
    let report = import_event_bundle(&target, &bundle).unwrap();
    assert_eq!(report.events_applied, 1);
    assert_eq!(target.get_board("legacy").unwrap().name, "Legacy");
    let _ = std::fs::remove_file(tmp);
}

#[test]
fn invalid_task_status_rolls_back_bundle_and_cursor() {
    let target = SqliteKanbanStore::open(Path::new(":memory:")).unwrap();
    let bundle = bundle_with(
        "invalid-status",
        2,
        vec![
            KanbanEvent {
                id: 1,
                event_uuid: uuid::Uuid::new_v4(),
                event_type: "board_created".to_string(),
                payload: r#"{"board_id":101,"slug":"invalid","name":"Invalid"}"#.to_string(),
                created_at: 0,
            },
            KanbanEvent {
                id: 2,
                event_uuid: uuid::Uuid::new_v4(),
                event_type: "task_created".to_string(),
                payload: serde_json::json!({
                    "task_id": 201,
                    "board_id": 101,
                    "title": "bad",
                    "body": null,
                    "status": "not-a-status",
                    "assignee": null,
                    "priority": 0,
                    "tags": []
                })
                .to_string(),
                created_at: 0,
            },
        ],
    );

    assert!(import_event_bundle(&target, &bundle).is_err());
    assert!(target.list_boards().unwrap().is_empty());
    assert_eq!(target.sync_cursor("invalid-status").unwrap(), 0);
}

#[test]
fn read_event_bundle_rejects_oversized_file() {
    let path = std::env::temp_dir().join(format!(
        "iota-kanban-oversized-{}.json",
        uuid::Uuid::new_v4()
    ));
    let file = std::fs::File::create(&path).unwrap();
    file.set_len(MAX_EVENT_SYNC_MESSAGE_BYTES as u64 + 1)
        .unwrap();
    assert!(read_event_bundle(&path).is_err());
    let _ = std::fs::remove_file(path);
}
