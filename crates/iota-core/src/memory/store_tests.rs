use crate::memory::store::*;

#[test]
fn deduplicates_memory_content() {
    let store = MemoryStore::open(Path::new(":memory:")).unwrap();
    let input = test_memory_insert(
        MemoryType::Semantic,
        Some(MemoryFacet::Identity),
        MemoryScope::User,
        "local",
        "User prefers concise answers",
    );
    store.insert(input.clone()).unwrap();
    store.insert(input).unwrap();
    let buckets = store.recall_buckets("local", "project", "session").unwrap();
    assert_eq!(buckets.identity.len(), 1);
}

#[test]
fn search_finds_records_and_empty_query_lists_records() {
    let store = MemoryStore::open(Path::new(":memory:")).unwrap();
    store
        .insert(test_memory_insert(
            MemoryType::Semantic,
            Some(MemoryFacet::Domain),
            MemoryScope::Project,
            "project",
            "ACP uses JSON-RPC over newline-delimited stdio",
        ))
        .unwrap();
    store
        .insert(test_memory_insert(
            MemoryType::Procedural,
            None,
            MemoryScope::Project,
            "project",
            "Run cargo test before submitting changes",
        ))
        .unwrap();

    let records = store.search("JSON-RPC", 10).unwrap();
    assert!(
        records
            .iter()
            .any(|record| record.content.contains("JSON-RPC"))
    );

    let all = store.search("", 10).unwrap();
    assert_eq!(all.len(), 2);
    assert!(store.search("cargo", 0).unwrap().is_empty());
}

#[test]
fn search_skips_records_with_unknown_taxonomy_values() {
    let store = MemoryStore::open(Path::new(":memory:")).unwrap();
    store
        .insert(test_memory_insert(
            MemoryType::Semantic,
            Some(MemoryFacet::Domain),
            MemoryScope::Project,
            "project",
            "valid memory survives bad legacy rows",
        ))
        .unwrap();

    {
        let conn = crate::utils::lock_or_recover(&store.conn);
        conn.execute("PRAGMA ignore_check_constraints = ON", [])
            .unwrap();
        let now = now_ts();
        conn.execute(
            "INSERT INTO memory (id, type, facet, scope, scope_id, content, content_hash, confidence, ttl_days, created_at, updated_at, expires_at)
             VALUES (?1, 'legacy-type', 'domain', 'project', 'project', ?2, ?3, 1.0, 365, ?4, ?4, ?5)",
            rusqlite::params![
                uuid::Uuid::new_v4().to_string(),
                "bad legacy taxonomy row",
                content_hash("bad legacy taxonomy row"),
                now,
                now + 86_400,
            ],
        )
        .unwrap();
    }

    let records = store.search("", 10).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].content, "valid memory survives bad legacy rows");
}

#[test]
fn vector_search_matches_semantic_similarity() {
    let store = MemoryStore::open(Path::new(":memory:")).unwrap();
    store
        .insert(test_memory_insert(
            MemoryType::Semantic,
            Some(MemoryFacet::Domain),
            MemoryScope::Project,
            "project",
            "Rust engine stores memory in sqlite",
        ))
        .unwrap();
    store
        .insert(test_memory_insert(
            MemoryType::Semantic,
            Some(MemoryFacet::Domain),
            MemoryScope::Project,
            "project",
            "Weather forecast for tomorrow",
        ))
        .unwrap();

    let result = store
        .search_with_mode("sqlite memory engine", 2, MemorySearchMode::Vector)
        .unwrap();
    assert!(!result.is_empty());
    assert!(
        result
            .iter()
            .any(|record| record.content.contains("sqlite"))
    );
}

fn test_memory_insert(
    memory_type: MemoryType,
    facet: Option<MemoryFacet>,
    scope: MemoryScope,
    scope_id: &str,
    content: &str,
) -> MemoryInsert {
    MemoryInsert {
        memory_type,
        facet,
        scope,
        scope_id: scope_id.to_string(),
        content: content.to_string(),
        confidence: 1.0,
        source_backend: None,
        source_session_id: None,
        source_execution_id: None,
        metadata_json: None,
        ttl_days: 365,
        supersedes: None,
    }
}

#[test]
fn merge_mode_add_always_inserts_new_record() {
    let store = MemoryStore::open(Path::new(":memory:")).unwrap();
    let input = test_memory_insert(
        MemoryType::Semantic,
        Some(MemoryFacet::Domain),
        MemoryScope::Project,
        "proj",
        "The sky is blue",
    );
    let id1 = store
        .insert_with_merge(input.clone(), MemoryMergeMode::Add)
        .unwrap();
    let id2 = store
        .insert_with_merge(input, MemoryMergeMode::Add)
        .unwrap();
    // Both inserts succeed but they are the same content → ON CONFLICT updates the existing row,
    // so both return the same id via the dedup path.
    assert!(id1.is_some());
    assert!(id2.is_some());
    // A second *different* content record is always added.
    let other = test_memory_insert(
        MemoryType::Semantic,
        Some(MemoryFacet::Domain),
        MemoryScope::Project,
        "proj",
        "The grass is green",
    );
    let id3 = store
        .insert_with_merge(other, MemoryMergeMode::Add)
        .unwrap();
    assert_ne!(id1.unwrap(), id3.unwrap());
}

#[test]
fn merge_mode_update_replaces_content() {
    let store = MemoryStore::open(Path::new(":memory:")).unwrap();
    let first = test_memory_insert(
        MemoryType::Semantic,
        Some(MemoryFacet::Strategic),
        MemoryScope::Project,
        "proj",
        "Initial goal: ship v1",
    );
    let original_id = store
        .insert_with_merge(first, MemoryMergeMode::Add)
        .unwrap()
        .unwrap();

    let updated = test_memory_insert(
        MemoryType::Semantic,
        Some(MemoryFacet::Strategic),
        MemoryScope::Project,
        "proj",
        "Updated goal: ship v2",
    );
    let returned_id = store
        .insert_with_merge(updated, MemoryMergeMode::Update)
        .unwrap()
        .unwrap();

    assert_eq!(original_id, returned_id);
    let records = store.search("v2", 10).unwrap();
    assert!(records.iter().any(|r| r.content.contains("v2")));
}

#[test]
fn merge_mode_none_skips_exact_duplicate() {
    let store = MemoryStore::open(Path::new(":memory:")).unwrap();
    let input = test_memory_insert(
        MemoryType::Episodic,
        None,
        MemoryScope::Session,
        "sess",
        "User logged in",
    );
    store
        .insert_with_merge(input.clone(), MemoryMergeMode::Add)
        .unwrap();
    // None mode must return None for a content-hash duplicate.
    let result = store
        .insert_with_merge(input, MemoryMergeMode::None)
        .unwrap();
    assert!(result.is_none());
}

#[test]
fn compact_episodic_removes_oldest_records() {
    let store = MemoryStore::open(Path::new(":memory:")).unwrap();
    for i in 0..5u32 {
        store
            .insert(test_memory_insert(
                MemoryType::Episodic,
                None,
                MemoryScope::Session,
                "sess",
                &format!("event number {}", i),
            ))
            .unwrap();
    }
    let deleted = store
        .compact_episodic_scope(MemoryScope::Session, "sess", 2)
        .unwrap();
    assert_eq!(deleted, 3);
    let remaining = store.search("event", 10).unwrap();
    assert_eq!(remaining.len(), 2);
}

#[test]
fn hybrid_search_returns_relevant_results() {
    let store = MemoryStore::open(Path::new(":memory:")).unwrap();
    store
        .insert(test_memory_insert(
            MemoryType::Procedural,
            None,
            MemoryScope::Project,
            "proj",
            "Run cargo test to execute unit tests",
        ))
        .unwrap();
    store
        .insert(test_memory_insert(
            MemoryType::Procedural,
            None,
            MemoryScope::Project,
            "proj",
            "Deploy with docker compose up",
        ))
        .unwrap();

    let results = store
        .search_with_mode("cargo unit tests", 5, MemorySearchMode::Hybrid)
        .unwrap();
    assert!(!results.is_empty());
    assert!(results.iter().any(|r| r.content.contains("cargo")));
}

#[test]
fn recall_buckets_respects_confidence_threshold() {
    let store = MemoryStore::open(Path::new(":memory:")).unwrap();
    // Insert a low-confidence identity record.
    let mut low = test_memory_insert(
        MemoryType::Semantic,
        Some(MemoryFacet::Identity),
        MemoryScope::User,
        "local-user",
        "Name: Alice",
    );
    low.confidence = 0.50;
    store.insert_with_merge(low, MemoryMergeMode::Add).unwrap();

    // Default threshold for identity is 0.85 → record should be excluded.
    let buckets = store.recall_buckets("local-user", "proj", "sess").unwrap();
    assert!(buckets.identity.is_empty());

    // Lower the threshold below 0.50 → record should appear.
    let thresholds = RecallThresholds {
        identity: 0.40,
        ..Default::default()
    };
    let buckets_low = store
        .recall_buckets_with_thresholds("local-user", "proj", "sess", thresholds)
        .unwrap();
    assert!(!buckets_low.identity.is_empty());
}

#[allow(clippy::too_many_arguments)]
fn insert_memory(
    store: &MemoryStore,
    memory_type: MemoryType,
    facet: Option<MemoryFacet>,
    scope: MemoryScope,
    scope_id: &str,
    content: &str,
    confidence: f64,
    ttl_days: i64,
) -> String {
    let mut insert = test_memory_insert(memory_type, facet, scope, scope_id, content);
    insert.confidence = confidence;
    insert.ttl_days = ttl_days;
    store
        .insert_with_merge(insert, MemoryMergeMode::Add)
        .unwrap()
        .unwrap()
}

#[test]
fn all_scope_buckets_group_unexpired_records() {
    let store = MemoryStore::open(Path::new(":memory:")).unwrap();
    let now = crate::utils::now_ts();

    insert_memory(
        &store,
        MemoryType::Semantic,
        Some(MemoryFacet::Identity),
        MemoryScope::User,
        "local-user",
        "User is Han",
        0.9,
        30,
    );
    insert_memory(
        &store,
        MemoryType::Semantic,
        Some(MemoryFacet::Preference),
        MemoryScope::User,
        "local-user",
        "Prefers concise answers",
        0.8,
        30,
    );
    insert_memory(
        &store,
        MemoryType::Semantic,
        Some(MemoryFacet::Strategic),
        MemoryScope::Project,
        "/tmp/project",
        "Ship desktop viewer",
        0.7,
        30,
    );
    insert_memory(
        &store,
        MemoryType::Semantic,
        Some(MemoryFacet::Domain),
        MemoryScope::Project,
        "/tmp/project",
        "Uses daemon-first Tauri",
        0.7,
        30,
    );
    insert_memory(
        &store,
        MemoryType::Procedural,
        None,
        MemoryScope::Project,
        "/tmp/project",
        "Run npm test before build",
        0.6,
        30,
    );
    insert_memory(
        &store,
        MemoryType::Episodic,
        None,
        MemoryScope::Session,
        "session-1",
        "Prompt: hi\nOutput: hello",
        0.6,
        30,
    );

    let buckets = store.all_scope_buckets(50).unwrap();
    assert_eq!(buckets.identity.len(), 1);
    assert_eq!(buckets.preference.len(), 1);
    assert_eq!(buckets.strategic.len(), 1);
    assert_eq!(buckets.domain.len(), 1);
    assert_eq!(buckets.procedural.len(), 1);
    assert_eq!(buckets.episodic.len(), 1);
    assert!(buckets.identity[0].expires_at > now);
}

#[test]
fn all_scope_buckets_exclude_expired_records() {
    let store = MemoryStore::open(Path::new(":memory:")).unwrap();
    let id = insert_memory(
        &store,
        MemoryType::Semantic,
        Some(MemoryFacet::Identity),
        MemoryScope::User,
        "local-user",
        "Expired identity",
        0.9,
        1,
    );
    {
        let conn = crate::utils::lock_or_recover(&store.conn);
        conn.execute(
            "UPDATE memory SET expires_at = ?2 WHERE id = ?1",
            rusqlite::params![id, crate::utils::now_ts() - 1],
        )
        .unwrap();
    }

    let buckets = store.all_scope_buckets(50).unwrap();
    assert!(buckets.identity.is_empty());
}
