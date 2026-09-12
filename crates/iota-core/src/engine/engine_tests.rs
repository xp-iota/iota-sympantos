use crate::config::{ContextEngineConfig, NimiaConfig};
use crate::engine::*;
use crate::memory::{MemoryFacet, MemoryRecord, MemoryScope, MemoryType};
use crate::runtime_event::{RuntimeEvent, ToolResultEvent};
use crate::store::cache::request_hash;

fn unique_test_path(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("iota-{}-{}.sqlite", name, uuid::Uuid::new_v4()))
}

#[test]
fn memory_inject_payload_uses_configured_budget() {
    // The capsule enforces the budget in tokens, so the trace reports tokens as
    // well as the configured character count: two 20-character records cost 5
    // tokens each, and a 20-character budget is 5 tokens, so exactly one fits.
    let buckets = RecallBuckets {
        identity: vec![memory_record(&"a".repeat(20))],
        preference: vec![memory_record(&"b".repeat(20))],
        ..Default::default()
    };
    let payload = memory_inject_payload(&buckets, 20);
    let budget = payload.get("budget").unwrap();

    assert_eq!(
        budget.get("memory_chars").and_then(|v| v.as_u64()),
        Some(20)
    );
    assert_eq!(
        budget.get("memory_tokens").and_then(|v| v.as_u64()),
        Some(5)
    );
    assert_eq!(budget.get("total_chars").and_then(|v| v.as_u64()), Some(40));
    assert_eq!(
        budget.get("total_tokens").and_then(|v| v.as_u64()),
        Some(10)
    );
    assert_eq!(
        budget.get("truncated").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(
        budget.get("excluded_count").and_then(|v| v.as_u64()),
        Some(1)
    );
}

#[test]
fn memory_inject_payload_reports_cjk_cost_in_tokens() {
    // Ten Chinese characters are ten tokens, not two: the char-based trace used
    // to report `truncated: false` for recall the capsule had actually dropped.
    let buckets = RecallBuckets {
        identity: vec![memory_record(&"字".repeat(10))],
        ..Default::default()
    };
    let payload = memory_inject_payload(&buckets, 20);
    let budget = payload.get("budget").unwrap();

    assert_eq!(budget.get("total_chars").and_then(|v| v.as_u64()), Some(10));
    assert_eq!(
        budget.get("total_tokens").and_then(|v| v.as_u64()),
        Some(10)
    );
    assert_eq!(
        budget.get("truncated").and_then(|v| v.as_bool()),
        Some(true),
        "10 CJK tokens must not be reported as fitting a 5-token budget"
    );
}

fn memory_record(content: &str) -> MemoryRecord {
    MemoryRecord {
        id: uuid::Uuid::new_v4().to_string(),
        memory_type: MemoryType::Semantic,
        facet: Some(MemoryFacet::Identity),
        scope: MemoryScope::User,
        scope_id: "local-user".to_string(),
        content: content.to_string(),
        confidence: 1.0,
        created_at: 1,
        updated_at: 1,
        expires_at: 999,
    }
}

#[test]
fn memory_inject_payload_within_budget_no_truncation() {
    let record = memory_record("short");
    let buckets = RecallBuckets {
        identity: vec![record],
        ..Default::default()
    };
    let payload = memory_inject_payload(&buckets, 10_000);
    let budget = payload.get("budget").unwrap();
    assert_eq!(
        budget.get("truncated").and_then(|v| v.as_bool()),
        Some(false)
    );
    assert_eq!(
        budget.get("excluded_count").and_then(|v| v.as_u64()),
        Some(0)
    );
}

#[test]
fn memory_inject_payload_empty_buckets_returns_zero_total() {
    let buckets = RecallBuckets::default();
    let payload = memory_inject_payload(&buckets, 1000);
    let budget = payload.get("budget").unwrap();
    assert_eq!(budget.get("total_chars").and_then(|v| v.as_u64()), Some(0));
    assert_eq!(
        budget.get("truncated").and_then(|v| v.as_bool()),
        Some(false)
    );
}

#[test]
fn memory_persistence_intent_requires_successful_memory_write_result() {
    assert!(memory_ops::is_memory_persistence_intent(
        "请把这些信息写入持久化记忆"
    ));
    assert!(!memory_ops::has_successful_memory_write(&[]));
    assert!(memory_ops::has_successful_memory_write(&[
        RuntimeEvent::ToolResult(ToolResultEvent {
            id: "tool-1".to_string(),
            name: "mcp__iota-context__iota_memory_write".to_string(),
            ok: true,
            result: serde_json::json!({"id": "memory-1"}),
        })
    ]));
    assert!(!memory_ops::has_successful_memory_write(&[
        RuntimeEvent::ToolResult(ToolResultEvent {
            id: "tool-1".to_string(),
            name: "mcp__iota-context__iota_memory_write".to_string(),
            ok: false,
            result: serde_json::json!({"error": "invalid taxonomy"}),
        })
    ]));
}

#[tokio::test]
async fn run_returns_cache_begin_conflict_instead_of_continuing_without_execution_id() {
    let memory_path = unique_test_path("engine-memory");
    let cache_path = unique_test_path("engine-cache");
    let cwd = std::env::current_dir().unwrap();
    let prompt = "my name is cache conflict";
    let execution_id = "fixed-execution-id";
    let cache = CacheStore::open(&cache_path).unwrap();
    cache
        .begin_execution_with_id(
            "codex",
            "session",
            "different-request-hash",
            Some(execution_id),
        )
        .unwrap();

    let config = NimiaConfig {
        context_engine: Some(ContextEngineConfig {
            memory_db: Some(memory_path.display().to_string()),
            ..Default::default()
        }),
        ..Default::default()
    };
    let mut engine = IotaEngine::create_session(config, false, 1_000, Some(&cwd));
    engine.cache_store = Some(cache);
    engine.memory_store = Some(MemoryStore::open(&memory_path).unwrap());

    let err = match engine
        .run(AcpBackend::Codex, cwd.clone(), prompt, Some(execution_id))
        .await
    {
        Ok(_) => panic!("cache begin conflict must stop the turn"),
        Err(err) => err,
    };

    assert!(err.to_string().contains("execution_id conflict"));
    assert_eq!(request_hash("codex", &cwd, prompt).len(), 64);
}

#[test]
fn test_resume_session_restores_backend_and_working_memory() {
    let ledger_path = unique_test_path("engine-ledger");
    let memory_path = unique_test_path("engine-memory");
    let cwd = std::env::current_dir().unwrap();
    let session_id = "test-session-123";

    // Initialize stores
    let ledger = SessionLedger::open(&ledger_path).unwrap();
    let memory = MemoryStore::open(&memory_path).unwrap();

    // 1. Record session and active backend in ledger
    ledger
        .ensure_session(session_id, &cwd, Some("claude-code"), None)
        .unwrap();

    // 2. Insert a turn into memory store as episodic memory
    let turn_content = "Prompt: write a rust script\nOutput: fn main() {}";
    memory
        .insert(crate::memory::MemoryInsert {
            memory_type: MemoryType::Episodic,
            facet: None,
            scope: MemoryScope::Session,
            scope_id: session_id.to_string(),
            content: turn_content.to_string(),
            confidence: 0.8,
            source_backend: Some("claude-code".to_string()),
            source_session_id: Some(session_id.to_string()),
            source_execution_id: None,
            metadata_json: None,
            ttl_days: 7,
            supersedes: None,
        })
        .unwrap();

    // Verify latest session query works
    let latest = ledger.latest_session_for_cwd(&cwd).unwrap().unwrap();
    assert_eq!(latest, session_id);

    // Create a config
    let config = NimiaConfig::default();

    // Instantiate engine without session_cwd
    let mut engine = IotaEngine::create_session(config, false, 1_000, None);

    // Inject our in-memory databases
    engine.session_ledger_store = Some(ledger);
    engine.memory_store = Some(memory);

    // Assert that initially engine has no session state
    assert!(engine.last_used_backend.is_none());
    assert_eq!(engine.working_memory.render(800), "");

    // Now resume session state with cwd
    engine.resume_session_state(Some(&cwd));

    // Verify state was correctly restored
    assert_eq!(engine.engine_session_id, session_id);
    assert_eq!(engine.last_used_backend, Some(AcpBackend::ClaudeCode));

    // Reconstruct the expected rendered working memory summary
    let wm_summary = engine.working_memory.render(800);
    assert!(
        wm_summary.contains("[claude-code] user: write a rust script; assistant: fn main() {}")
    );
}

fn test_engine() -> IotaEngine {
    IotaEngine::create_session(NimiaConfig::default(), false, 30_000, None)
}

/// A fresh engine has never warmed a backend, so `has_warm_client` must report
/// no warm client for any `(backend, cwd)`. This also pins the self-healing
/// signature: the probe takes `&mut self` so it can evict a client whose
/// process has exited rather than reporting a dead pool entry as warm.
#[test]
fn has_warm_client_is_false_on_a_cold_engine() {
    let mut engine = test_engine();
    let cwd = std::env::current_dir().unwrap();
    for backend in acp::ALL_BACKENDS {
        assert!(
            !engine.has_warm_client(backend, &cwd),
            "a cold engine must not report a warm client for {backend}"
        );
    }
}

// ── prepare_backend_handoff tests ────────────────────────────────────────────

/// When the active backend switches, `prepare_backend_handoff` must call
/// `SessionLedger::publish_handoff` with `from_backend = last_used_backend`
/// and `to_backend = new backend`.
///
/// Requirements: 2.3
#[test]
fn prepare_backend_handoff_publishes_handoff_on_backend_switch() {
    let ledger_path = unique_test_path("engine-handoff-switch");
    let cwd = std::env::current_dir().unwrap();

    let ledger = SessionLedger::open(&ledger_path).unwrap();

    let mut engine = test_engine();
    // Inject a real ledger so publish_handoff writes to SQLite.
    engine.session_ledger_store = Some(ledger.clone());

    // Seed the session row so the ledger FK constraints are satisfied.
    ledger
        .ensure_session(&engine.engine_session_id, &cwd, Some("codex"), None)
        .unwrap();

    // Simulate a previous turn on Codex.
    engine.last_used_backend = Some(AcpBackend::Codex);

    // Add working-memory content so the handoff summary is non-empty.
    engine
        .working_memory
        .push_turn(AcpBackend::Codex, "write a hello world", "fn main() {}");

    // Switch to ClaudeCode — this should trigger publish_handoff.
    let result = engine.prepare_backend_handoff(AcpBackend::ClaudeCode, &cwd);

    // The method must return a non-empty summary string.
    assert!(
        result.is_some(),
        "expected a handoff summary when switching backends"
    );

    // Verify the handoff row was written to the ledger.
    let history = ledger
        .get_handoff_history(&engine.engine_session_id)
        .unwrap();
    assert_eq!(
        history.len(),
        1,
        "exactly one handoff row should be written"
    );

    let (from_backend, to_backend, summary) = &history[0];
    assert_eq!(
        from_backend, "codex",
        "from_backend must equal last_used_backend"
    );
    assert_eq!(
        to_backend, "claude-code",
        "to_backend must equal the new backend"
    );
    assert!(!summary.is_empty(), "handoff summary must not be empty");
}

/// When the requested backend is the same as `last_used_backend`,
/// `prepare_backend_handoff` must NOT call `SessionLedger::publish_handoff`.
///
/// Requirements: 2.4
#[test]
fn prepare_backend_handoff_skips_handoff_when_backend_unchanged() {
    let ledger_path = unique_test_path("engine-handoff-same");
    let cwd = std::env::current_dir().unwrap();

    let ledger = SessionLedger::open(&ledger_path).unwrap();

    let mut engine = test_engine();
    engine.session_ledger_store = Some(ledger.clone());

    ledger
        .ensure_session(&engine.engine_session_id, &cwd, Some("codex"), None)
        .unwrap();

    // Both last_used_backend and the requested backend are Codex.
    engine.last_used_backend = Some(AcpBackend::Codex);
    engine
        .working_memory
        .push_turn(AcpBackend::Codex, "write a hello world", "fn main() {}");

    // Call with the same backend — no handoff should be published.
    let result = engine.prepare_backend_handoff(AcpBackend::Codex, &cwd);

    // The method must return None (no handoff needed).
    assert!(
        result.is_none(),
        "expected no handoff summary when backend is unchanged"
    );

    // Verify no handoff row was written to the ledger.
    let history = ledger
        .get_handoff_history(&engine.engine_session_id)
        .unwrap();
    assert!(
        history.is_empty(),
        "no handoff row should be written when backend is unchanged"
    );
}

#[test]
fn recent_context_snapshot_starts_empty() {
    let engine = test_engine();
    assert!(engine.recent_runtime_context_snapshot().is_none());
}

#[test]
fn recent_context_snapshot_is_in_memory_only() {
    let mut engine = test_engine();
    let cwd = std::env::current_dir().unwrap();
    engine.capture_runtime_context_snapshot(
        "turn-1".to_string(),
        AcpBackend::Codex,
        cwd.clone(),
        Some("model-a".to_string()),
        "<iota-context>\n<session>\nbackend: codex\n</session>\n</iota-context>\n\nUser request:\nhello".to_string(),
    );

    let snapshot = engine.recent_runtime_context_snapshot().unwrap();
    assert_eq!(snapshot.turn_id, "turn-1");
    assert_eq!(snapshot.backend, "codex");
    assert_eq!(snapshot.cwd, cwd);
    assert!(snapshot.capsule_text.contains("<iota-context>"));
    assert!(
        snapshot
            .sections
            .iter()
            .any(|section| section.name == "session")
    );
}

#[test]
fn recent_context_snapshot_parses_memory_sections_with_attributes() {
    let mut engine = test_engine();
    let cwd = std::env::current_dir().unwrap();
    engine.capture_runtime_context_snapshot(
        "turn-memory".to_string(),
        AcpBackend::Codex,
        cwd,
        None,
        "<iota-context>\n<memory type=\"identity\">\n- User is Han\n</memory>\n</iota-context>\n\nUser request:\nhello".to_string(),
    );

    let snapshot = engine.recent_runtime_context_snapshot().unwrap();
    assert!(
        snapshot
            .sections
            .iter()
            .any(|section| section.name == "memory" && section.preview.contains("User is Han"))
    );
}

#[test]
fn recent_context_snapshot_ignores_memory_tools_when_parsing_memory_sections() {
    let mut engine = test_engine();
    let cwd = std::env::current_dir().unwrap();
    engine.capture_runtime_context_snapshot(
        "turn-memory-tools".to_string(),
        AcpBackend::Codex,
        cwd,
        None,
        "<iota-context>\n<memory-tools>\nUse iota_memory_write.\n</memory-tools>\n\n<memory type=\"identity\">\n- User is Han\n</memory>\n</iota-context>\n\nUser request:\nhello".to_string(),
    );

    let snapshot = engine.recent_runtime_context_snapshot().unwrap();
    let memory_section = snapshot
        .sections
        .iter()
        .find(|section| section.name == "memory")
        .expect("memory section should be parsed");
    assert!(memory_section.preview.contains("User is Han"));
    assert!(!memory_section.preview.contains("memory-tools"));
    assert!(!memory_section.preview.contains("iota_memory_write"));
}

#[test]
fn ephemeral_session_disables_every_durable_store() {
    let engine = IotaEngine::create_ephemeral_session(NimiaConfig::default(), false, 1_000);
    assert!(engine.memory_store.is_none());
    assert!(engine.cache_store.is_none());
    assert!(engine.observability_store.is_none());
    assert!(engine.session_ledger_store.is_none());
}
