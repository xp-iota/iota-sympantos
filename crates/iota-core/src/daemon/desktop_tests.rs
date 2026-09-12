#![allow(clippy::await_holding_lock)]

use crate::config::{ContextEngineConfig, ContextInjection, NimiaConfig, RecallThresholdsConfig};
use crate::daemon::desktop::*;
use crate::memory::{
    MemoryFacet, MemoryInsert, MemoryMergeMode, MemoryScope, MemoryStore, MemoryType,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

#[tokio::test]
async fn approval_registry_delivers_decision_once() {
    let registry = ApprovalRegistry::default();
    let (tx, rx) = oneshot::channel();
    registry
        .insert("turn-1".to_string(), "approval-1".to_string(), tx)
        .await;

    assert!(registry.respond("approval-1", true).await);
    assert!(rx.await.unwrap());
    assert!(!registry.respond("approval-1", false).await);
}

#[tokio::test]
async fn approval_registry_returns_false_for_missing_id() {
    let registry = ApprovalRegistry::default();
    assert!(!registry.respond("missing", true).await);
}

#[tokio::test]
async fn approval_registry_denies_all_pending_for_turn() {
    let registry = ApprovalRegistry::default();
    let (tx1, rx1) = oneshot::channel();
    let (tx2, rx2) = oneshot::channel();
    let (other_tx, other_rx) = oneshot::channel();

    registry
        .insert("turn-1".to_string(), "approval-1".to_string(), tx1)
        .await;
    registry
        .insert("turn-1".to_string(), "approval-2".to_string(), tx2)
        .await;
    registry
        .insert("turn-2".to_string(), "approval-3".to_string(), other_tx)
        .await;

    assert_eq!(registry.deny_for_turn("turn-1").await, 2);
    assert!(!rx1.await.unwrap());
    assert!(!rx2.await.unwrap());
    assert!(registry.respond("approval-3", true).await);
    assert!(other_rx.await.unwrap());
}

#[tokio::test]
async fn turn_registry_cancel_reports_whether_turn_existed() {
    let registry = TurnRegistry::default();
    let handle = tokio::spawn(async {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
    });

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let accept = tokio::spawn(async move { listener.accept().await.unwrap().0.into_split().1 });
    let _client = TcpStream::connect(addr).await.unwrap();
    let write_half = accept.await.unwrap();

    registry
        .insert(
            "turn-1".to_string(),
            handle,
            Arc::new(Mutex::new(write_half)),
            tokio_util::sync::CancellationToken::new(),
        )
        .await;

    assert!(registry.abort("turn-1").await.is_some());
    assert!(registry.abort("turn-1").await.is_none());
}

/// Builds a write half plus a registry entry whose task ends only when its
/// cancellation token fires — mirroring how a real turn unwinds cooperatively.
async fn registry_with_cancellable_turn(
    turn_id: &str,
    grace_behaviour: CancellableTurn,
) -> (
    TurnRegistry,
    tokio_util::sync::CancellationToken,
    std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    let registry = TurnRegistry::default();
    let cancel = tokio_util::sync::CancellationToken::new();
    let finished = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let accept = tokio::spawn(async move { listener.accept().await.unwrap().0.into_split().1 });
    let _client = TcpStream::connect(addr).await.unwrap();
    let write_half = accept.await.unwrap();

    let task_cancel = cancel.clone();
    let task_finished = std::sync::Arc::clone(&finished);
    let handle = tokio::spawn(async move {
        match grace_behaviour {
            // Cooperative: stops as soon as it is told to.
            CancellableTurn::Cooperative => task_cancel.cancelled().await,
            // Wedged: ignores cancellation and would run far past any grace.
            CancellableTurn::Wedged => {
                tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
            }
        }
        task_finished.store(true, std::sync::atomic::Ordering::SeqCst);
    });

    registry
        .insert(
            turn_id.to_string(),
            handle,
            Arc::new(Mutex::new(write_half)),
            cancel.clone(),
        )
        .await;
    (registry, cancel, finished)
}

#[derive(Clone, Copy)]
enum CancellableTurn {
    Cooperative,
    Wedged,
}

#[tokio::test]
async fn shutdown_cancels_and_awaits_cooperative_turns() {
    let (registry, cancel, finished) =
        registry_with_cancellable_turn("turn-1", CancellableTurn::Cooperative).await;

    let cancelled = registry
        .cancel_all_and_wait(std::time::Duration::from_secs(5))
        .await;

    assert_eq!(cancelled, 1, "every in-flight turn must be counted");
    assert!(cancel.is_cancelled(), "shutdown must fire the cancel token");
    assert!(
        finished.load(std::sync::atomic::Ordering::SeqCst),
        "shutdown must wait for a cooperative turn to finish unwinding, so its ACP \
         backend is told to stop before the client pool is drained"
    );
}

#[tokio::test]
async fn shutdown_aborts_turns_that_ignore_the_grace_period() {
    let (registry, _cancel, finished) =
        registry_with_cancellable_turn("turn-1", CancellableTurn::Wedged).await;

    let started = std::time::Instant::now();
    let cancelled = registry
        .cancel_all_and_wait(std::time::Duration::from_millis(200))
        .await;

    assert_eq!(cancelled, 1);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "shutdown must not block indefinitely on a wedged turn"
    );
    // The task was aborted, so it never reached its completion marker.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(!finished.load(std::sync::atomic::Ordering::SeqCst));
}

#[tokio::test]
async fn shutdown_with_no_active_turns_is_a_noop() {
    let registry = TurnRegistry::default();
    let cancelled = registry
        .cancel_all_and_wait(std::time::Duration::from_secs(1))
        .await;
    assert_eq!(cancelled, 0);
}

#[tokio::test]
async fn desktop_connection_rejects_message_before_hello() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let stream = listener.accept().await.unwrap().0;
        let pool = Arc::new(Mutex::new(EnginePool::new(
            NimiaConfig::default(),
            false,
            1000,
        )));
        let (read_half, write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        let message: DaemonClientMessage = serde_json::from_str(line.trim()).unwrap();
        handle_desktop_connection(
            message,
            reader,
            write_half,
            pool,
            ApprovalRegistry::default(),
            TurnRegistry::default(),
        )
        .await
        .unwrap();
    });

    let mut client = TcpStream::connect(addr).await.unwrap();
    let message = DaemonClientMessage::GetConfig { request_id: None };
    let mut line = serde_json::to_vec(&message).unwrap();
    line.push(b'\n');
    client.write_all(&line).await.unwrap();
    let mut reader = BufReader::new(client);
    let mut response = String::new();
    reader.read_line(&mut response).await.unwrap();
    let response: DaemonServerMessage = serde_json::from_str(response.trim()).unwrap();

    assert!(matches!(
        response,
        DaemonServerMessage::ProtocolError { .. }
    ));
    server.await.unwrap();
}

#[tokio::test]
async fn desktop_connection_rejects_hello_without_auth_token() {
    let _env_lock = crate::daemon::auth::test_token_path_env_lock();
    let dir = std::env::temp_dir().join(format!("iota-daemon-auth-test-{}", uuid::Uuid::new_v4()));
    let token_path = dir.join("daemon.token");
    unsafe {
        std::env::set_var("IOTA_DAEMON_TOKEN_PATH", &token_path);
    }

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let stream = listener.accept().await.unwrap().0;
        let pool = Arc::new(Mutex::new(EnginePool::new(
            NimiaConfig::default(),
            false,
            1000,
        )));
        let (read_half, write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        let message: DaemonClientMessage = serde_json::from_str(line.trim()).unwrap();
        handle_desktop_connection(
            message,
            reader,
            write_half,
            pool,
            ApprovalRegistry::default(),
            TurnRegistry::default(),
        )
        .await
        .unwrap();
    });

    let mut client = TcpStream::connect(addr).await.unwrap();
    let message = DaemonClientMessage::hello("test".to_string(), None);
    let mut line = serde_json::to_vec(&message).unwrap();
    line.push(b'\n');
    client.write_all(&line).await.unwrap();
    let mut reader = BufReader::new(client);
    let mut response = String::new();
    reader.read_line(&mut response).await.unwrap();
    let response: DaemonServerMessage = serde_json::from_str(response.trim()).unwrap();

    assert!(matches!(
        response,
        DaemonServerMessage::ProtocolError { .. }
    ));
    server.await.unwrap();
    unsafe {
        std::env::remove_var("IOTA_DAEMON_TOKEN_PATH");
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn desktop_connection_accepts_hello_with_valid_auth_token() {
    let _env_lock = crate::daemon::auth::test_token_path_env_lock();
    let dir = std::env::temp_dir().join(format!("iota-daemon-auth-test-{}", uuid::Uuid::new_v4()));
    let token_path = dir.join("daemon.token");
    unsafe {
        std::env::set_var("IOTA_DAEMON_TOKEN_PATH", &token_path);
    }
    let token = crate::daemon::auth::load_or_create_token().unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let stream = listener.accept().await.unwrap().0;
        let pool = Arc::new(Mutex::new(EnginePool::new(
            NimiaConfig::default(),
            false,
            1000,
        )));
        let (read_half, write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        let message: DaemonClientMessage = serde_json::from_str(line.trim()).unwrap();
        handle_desktop_connection(
            message,
            reader,
            write_half,
            pool,
            ApprovalRegistry::default(),
            TurnRegistry::default(),
        )
        .await
        .unwrap();
    });

    let mut client = TcpStream::connect(addr).await.unwrap();
    let message = DaemonClientMessage::hello("test".to_string(), Some(token));
    let mut line = serde_json::to_vec(&message).unwrap();
    line.push(b'\n');
    client.write_all(&line).await.unwrap();
    let mut reader = BufReader::new(client);
    let mut response = String::new();
    reader.read_line(&mut response).await.unwrap();
    let response: DaemonServerMessage = serde_json::from_str(response.trim()).unwrap();

    assert!(matches!(
        response,
        DaemonServerMessage::HelloAccepted { .. }
    ));
    // An accepted connection stays open for further messages, so the server
    // task only returns once it observes EOF. Drop the client's read half to
    // close the connection, then await the server with a timeout so a
    // regression that leaves the reader loop spinning fails the test instead
    // of hanging the whole test binary (which would also block every other
    // test waiting on `test_token_path_env_lock`).
    drop(reader);
    tokio::time::timeout(std::time::Duration::from_secs(10), server)
        .await
        .expect("server task did not observe EOF after the client disconnected")
        .unwrap();
    unsafe {
        std::env::remove_var("IOTA_DAEMON_TOKEN_PATH");
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn memory_summary_counts_bucket_lengths() {
    let mut buckets = DesktopMemoryBuckets::default();
    buckets
        .identity
        .push(desktop_record("id-1", "semantic", Some("identity")));
    buckets
        .episodic
        .push(desktop_record("id-2", "episodic", None));

    let summary = memory_summary(&buckets);
    assert_eq!(summary.identity, 1);
    assert_eq!(summary.episodic, 1);
    assert_eq!(summary.preference, 0);
}

#[tokio::test]
async fn memory_context_snapshot_workspace_uses_configured_recall_thresholds() {
    let memory_path = std::env::temp_dir().join(format!(
        "iota-desktop-memory-thresholds-{}.sqlite",
        uuid::Uuid::new_v4()
    ));
    let cwd = std::env::current_dir().unwrap();
    let store = MemoryStore::open(&memory_path).unwrap();
    store
        .insert_with_merge(
            MemoryInsert {
                memory_type: MemoryType::Semantic,
                facet: Some(MemoryFacet::Identity),
                scope: MemoryScope::User,
                scope_id: "local-user".to_string(),
                content: "Configured threshold identity should stay hidden".to_string(),
                confidence: 0.88,
                source_backend: None,
                source_session_id: None,
                source_execution_id: None,
                metadata_json: None,
                ttl_days: 30,
                supersedes: None,
            },
            MemoryMergeMode::Add,
        )
        .unwrap();

    let pool = Arc::new(Mutex::new(EnginePool::new(
        NimiaConfig {
            context_engine: Some(ContextEngineConfig {
                memory_db: Some(memory_path.display().to_string()),
                recall_thresholds: Some(RecallThresholdsConfig {
                    identity: 0.9,
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
        false,
        1000,
    )));

    let snapshot = memory_context_snapshot(cwd, DesktopMemoryScopeMode::Workspace, pool).await;
    assert!(snapshot.memory.identity.is_empty());
}

#[tokio::test]
async fn memory_context_snapshot_reports_injection_off_as_disabled() {
    let cwd = std::env::current_dir().unwrap();
    let pool = Arc::new(Mutex::new(EnginePool::new(
        NimiaConfig {
            context_engine: Some(ContextEngineConfig {
                injection: ContextInjection::Off,
                ..Default::default()
            }),
            ..Default::default()
        },
        false,
        1000,
    )));

    let snapshot = memory_context_snapshot(cwd, DesktopMemoryScopeMode::Workspace, pool).await;
    assert!(!snapshot.context_engine.enabled);
}

#[test]
fn negotiate_version_accepts_current_client() {
    let hello = DaemonClientMessage::hello("current-client".to_string(), None);
    let result = negotiate_version(&hello).unwrap();
    assert_eq!(result, crate::daemon::DESKTOP_PROTOCOL_VERSION);
}

/// Builds a Hello at an arbitrary protocol version, as a client of that era
/// would send it.
fn hello_at_version(
    client_name: &str,
    version: u32,
    min: Option<u32>,
    max: Option<u32>,
) -> DaemonClientMessage {
    DaemonClientMessage::Hello {
        client_name: client_name.to_string(),
        protocol_version: version,
        min_version: min,
        max_version: max,
        schema_version: None,
        capabilities: Vec::new(),
        auth_token: None,
    }
}

#[test]
fn negotiate_version_rejects_a_legacy_client_with_an_explicit_error() {
    // A client from before the current protocol must be refused outright —
    // its payloads no longer match this server's schema.
    let hello = hello_at_version("legacy-client", 1, None, None);
    let error = negotiate_version(&hello).unwrap_err();
    assert!(
        error.contains("version"),
        "the rejection must name the version problem, got: {error}"
    );
}

#[test]
fn negotiate_version_rejects_a_future_client() {
    let hello = hello_at_version("future-client", 99, None, None);
    let error = negotiate_version(&hello).unwrap_err();
    assert!(
        error.contains("version"),
        "the rejection must name the version problem, got: {error}"
    );
}

#[test]
fn negotiate_version_accepts_a_client_whose_range_covers_the_server() {
    let hello = hello_at_version(
        "ranged-client",
        crate::daemon::PROTOCOL_VERSION_MIN,
        Some(crate::daemon::PROTOCOL_VERSION_MIN),
        Some(crate::daemon::PROTOCOL_VERSION_MAX + 1),
    );
    assert_eq!(
        negotiate_version(&hello).unwrap(),
        crate::daemon::DESKTOP_PROTOCOL_VERSION
    );
}

#[test]
fn negotiate_version_rejects_a_client_range_that_excludes_the_server() {
    let hello = hello_at_version("disjoint-client", 90, Some(90), Some(99));
    assert!(negotiate_version(&hello).is_err());
}

#[test]
fn negotiate_version_rejects_non_hello() {
    let msg = DaemonClientMessage::GetConfig { request_id: None };
    let result = negotiate_version(&msg);
    assert!(result.is_err());
}

fn desktop_record(id: &str, memory_type: &str, facet: Option<&str>) -> DesktopMemoryRecord {
    DesktopMemoryRecord {
        id: id.to_string(),
        memory_type: memory_type.to_string(),
        facet: facet.map(str::to_string),
        scope: "user".to_string(),
        scope_id: "local-user".to_string(),
        content: "content".to_string(),
        confidence: 1.0,
        created_at: 1,
        updated_at: 2,
        expires_at: 3,
    }
}

/// Exercises the connection-level duplicate-request guard over a real socket.
///
/// A client that retries an in-flight request after losing the reply must not
/// cause the side effect to happen twice; the second attempt is answered with
/// an explicit duplicate notice instead of re-running.
#[tokio::test]
async fn duplicate_request_id_on_one_connection_is_rejected_not_rerun() {
    let _env_lock = crate::daemon::auth::test_token_path_env_lock();
    let dir = std::env::temp_dir().join(format!("iota-daemon-auth-test-{}", uuid::Uuid::new_v4()));
    let token_path = dir.join("daemon.token");
    unsafe {
        std::env::set_var("IOTA_DAEMON_TOKEN_PATH", &token_path);
    }
    let token = crate::daemon::auth::load_or_create_token().unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let stream = listener.accept().await.unwrap().0;
        let pool = Arc::new(Mutex::new(EnginePool::new(
            NimiaConfig::default(),
            false,
            1000,
        )));
        let (read_half, write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        let message: DaemonClientMessage = serde_json::from_str(line.trim()).unwrap();
        handle_desktop_connection(
            message,
            reader,
            write_half,
            pool,
            ApprovalRegistry::default(),
            TurnRegistry::default(),
        )
        .await
        .unwrap();
    });

    let client = TcpStream::connect(addr).await.unwrap();
    // One reader for the whole connection. Rebuilding a `BufReader` per reply
    // would discard bytes the previous one had already buffered, so two
    // replies arriving in a single read would strand the second forever.
    let (client_read, mut client_write) = client.into_split();
    let mut reader = BufReader::new(client_read);
    let hello = DaemonClientMessage::hello("test", Some(token));
    write_line_half(&mut client_write, &hello).await;
    let line = read_line_before_timeout(&mut reader).await;
    assert!(matches!(
        serde_json::from_str::<DaemonServerMessage>(line.trim()).unwrap(),
        DaemonServerMessage::HelloAccepted { .. }
    ));

    // Same correlation id twice, with a request that is cheap but observable:
    // an unknown approval id is reported as NotFound on the first attempt.
    let first = DaemonClientMessage::RespondApproval {
        approval_id: "approval-x".to_string(),
        approved: true,
        request_id: Some("dup-1".to_string()),
    };
    write_line_half(&mut client_write, &first).await;
    // The first attempt is executed: an `approval_responded` acknowledgement,
    // then a notice that no such approval was pending.
    let ack_line = read_line_before_timeout(&mut reader).await;
    assert!(
        ack_line.contains("approval_responded"),
        "first attempt should be acknowledged, got: {ack_line}"
    );
    let notice_line = read_line_before_timeout(&mut reader).await;
    assert!(
        notice_line.contains("not pending"),
        "first attempt should report the approval as not pending, got: {notice_line}"
    );

    // Replaying the identical request id must be refused as a duplicate rather
    // than re-executed.
    write_line_half(&mut client_write, &first).await;
    let third_line = read_line_before_timeout(&mut reader).await;
    let decoded: DaemonServerMessage = serde_json::from_str(third_line.trim()).unwrap();
    match decoded {
        DaemonServerMessage::ProtocolError { message, code, .. } => {
            assert!(
                message.contains("duplicate request_id"),
                "expected a duplicate-request notice, got: {message}"
            );
            assert_eq!(code, Some(DaemonErrorCode::InvalidRequest));
        }
        other => panic!("expected a duplicate rejection, got {other:?}"),
    }

    drop(reader);
    drop(client_write);
    tokio::time::timeout(std::time::Duration::from_secs(10), server)
        .await
        .expect("server task did not observe EOF after the client disconnected")
        .unwrap();
    unsafe {
        std::env::remove_var("IOTA_DAEMON_TOKEN_PATH");
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// Writes one JSON line to the write half of a split stream.
async fn write_line_half(
    stream: &mut tokio::net::tcp::OwnedWriteHalf,
    message: &DaemonClientMessage,
) {
    let mut line = serde_json::to_vec(message).unwrap();
    line.push(b'\n');
    stream.write_all(&line).await.unwrap();
}

/// Reads one line, failing instead of hanging when the reply never arrives.
///
/// An unbounded read here would stall the whole test binary — including every
/// other test waiting on `test_token_path_env_lock` — so a protocol regression
/// must surface as a failure.
async fn read_line_before_timeout(
    reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>,
) -> String {
    let mut line = String::new();
    let read = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        reader.read_line(&mut line),
    )
    .await
    .expect("timed out waiting for a server reply");
    let bytes = read.expect("failed to read a server reply");
    assert!(bytes > 0, "server closed the connection without replying");
    line
}
