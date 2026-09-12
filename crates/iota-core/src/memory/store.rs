use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use super::embedding::{self, EmbeddingEngine};
use crate::config::EmbeddingConfig;
use crate::config::RecallThresholdsConfig;
use crate::utils::now_ts;

/// Alias so the store layer uses a shorter name without duplicating the struct.
pub type RecallThresholds = RecallThresholdsConfig;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum MemoryType {
    Semantic,
    Episodic,
    Procedural,
}

impl MemoryType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Semantic => "semantic",
            Self::Episodic => "episodic",
            Self::Procedural => "procedural",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "semantic" => Some(Self::Semantic),
            "episodic" => Some(Self::Episodic),
            "procedural" => Some(Self::Procedural),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum MemoryFacet {
    Identity,
    Preference,
    Strategic,
    Domain,
}

impl MemoryFacet {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Identity => "identity",
            Self::Preference => "preference",
            Self::Strategic => "strategic",
            Self::Domain => "domain",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "identity" => Some(Self::Identity),
            "preference" => Some(Self::Preference),
            "strategic" => Some(Self::Strategic),
            "domain" => Some(Self::Domain),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum MemoryScope {
    Session,
    Project,
    User,
    Global,
}

impl MemoryScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Session => "session",
            Self::Project => "project",
            Self::User => "user",
            Self::Global => "global",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "session" => Some(Self::Session),
            "project" => Some(Self::Project),
            "user" => Some(Self::User),
            "global" => Some(Self::Global),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRecord {
    pub id: String,
    #[serde(rename = "type")]
    pub memory_type: MemoryType,
    pub facet: Option<MemoryFacet>,
    pub scope: MemoryScope,
    pub scope_id: String,
    pub content: String,
    pub confidence: f64,
    pub created_at: i64,
    pub updated_at: i64,
    pub expires_at: i64,
}

#[derive(Clone)]
pub struct MemoryStore {
    conn: Arc<Mutex<Connection>>,
    fts_available: bool,
    embedding: EmbeddingEngine,
}

#[derive(Debug, Clone)]
pub struct MemoryInsert {
    pub memory_type: MemoryType,
    pub facet: Option<MemoryFacet>,
    pub scope: MemoryScope,
    pub scope_id: String,
    pub content: String,
    pub confidence: f64,
    pub source_backend: Option<String>,
    pub source_session_id: Option<String>,
    pub source_execution_id: Option<String>,
    pub metadata_json: Option<String>,
    pub ttl_days: i64,
    pub supersedes: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct RecallBuckets {
    pub identity: Vec<MemoryRecord>,
    pub preference: Vec<MemoryRecord>,
    pub strategic: Vec<MemoryRecord>,
    pub domain: Vec<MemoryRecord>,
    pub procedural: Vec<MemoryRecord>,
    pub episodic: Vec<MemoryRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MemoryMergeMode {
    Auto,
    Add,
    Update,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MemorySearchMode {
    Keyword,
    Vector,
    Hybrid,
}

impl MemoryStore {
    pub fn open(path: &Path) -> Result<Self> {
        Self::open_with_embedding(path, None)
    }

    pub fn open_with_embedding(path: &Path, config: Option<EmbeddingConfig>) -> Result<Self> {
        let conn = crate::store::db::open_db(path)?;
        let fts_available = init_schema(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            fts_available,
            embedding: EmbeddingEngine::from_config(config),
        })
    }

    pub fn default_path() -> Result<PathBuf> {
        Ok(crate::config::paths::StorePaths::resolve()?.memory_db())
    }

    pub fn insert(&self, input: MemoryInsert) -> Result<String> {
        self.insert_with_merge(input, MemoryMergeMode::Auto)?
            .context("memory insert with auto-merge unexpectedly skipped")
    }

    pub fn insert_with_merge(
        &self,
        input: MemoryInsert,
        merge_mode: MemoryMergeMode,
    ) -> Result<Option<String>> {
        validate_taxonomy(&input)?;
        let vector = self.embedding.embed(&input.content);
        let now = now_ts();
        let ttl_days = input.ttl_days.max(1);
        let expires_at = now + ttl_days * 86_400;
        let content_hash = content_hash(&input.content);
        let conn = crate::utils::lock_or_recover(&self.conn);
        if let Some(existing_id) = conn
            .query_row(
                "SELECT id FROM memory WHERE scope = ?1 AND scope_id = ?2 AND type = ?3 AND facet IS ?4 AND content_hash = ?5 LIMIT 1",
                params![
                    input.scope.as_str(),
                    input.scope_id,
                    input.memory_type.as_str(),
                    input.facet.as_ref().map(MemoryFacet::as_str),
                    content_hash,
                ],
                |row| row.get::<_, String>(0),
            )
            .optional()? {
            if merge_mode == MemoryMergeMode::None {
                return Ok(None);
            }
            conn.execute(
                "UPDATE memory SET updated_at = ?2, expires_at = ?3, confidence = MAX(confidence, ?4) WHERE id = ?1",
                params![existing_id, now, expires_at, input.confidence],
            )?;
            self.upsert_embedding(&conn, &existing_id, &vector, now)?;
            return Ok(Some(existing_id));
        }

        let related_id = latest_related_memory_id(
            &conn,
            input.scope.as_str(),
            &input.scope_id,
            input.memory_type.as_str(),
            input.facet.as_ref().map(MemoryFacet::as_str),
        )?;
        if merge_mode == MemoryMergeMode::None && related_id.is_some() {
            return Ok(None);
        }

        let should_update_related = merge_mode == MemoryMergeMode::Update && related_id.is_some();
        if should_update_related {
            let target_id = related_id.context("related memory id missing while updating")?;
            conn.execute(
                "UPDATE memory SET content = ?2, content_hash = ?3, confidence = MAX(confidence, ?4), updated_at = ?5, expires_at = ?6,
                 source_backend = COALESCE(?7, source_backend),
                 source_session_id = COALESCE(?8, source_session_id),
                 source_execution_id = COALESCE(?9, source_execution_id),
                 metadata_json = COALESCE(?10, metadata_json)
                 WHERE id = ?1",
                params![
                    target_id,
                    input.content,
                    content_hash,
                    input.confidence,
                    now,
                    expires_at,
                    input.source_backend,
                    input.source_session_id,
                    input.source_execution_id,
                    input.metadata_json,
                ],
            )?;
            self.upsert_embedding(&conn, &target_id, &vector, now)?;
            return Ok(Some(target_id));
        }

        let id = Uuid::new_v4().to_string();
        let supersedes = input.supersedes.clone().or(related_id);
        conn.execute(
            "INSERT INTO memory (id, type, facet, scope, scope_id, content, content_hash, confidence, source_backend, source_session_id, source_execution_id, metadata_json, ttl_days, created_at, updated_at, expires_at, supersedes, owner, visibility)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?14, ?15, ?16, 'local', 'private')
             ON CONFLICT(scope, scope_id, type, facet, content_hash) DO UPDATE SET updated_at=excluded.updated_at, expires_at=excluded.expires_at, confidence=MAX(memory.confidence, excluded.confidence)",
            params![
                id,
                input.memory_type.as_str(),
                input.facet.as_ref().map(MemoryFacet::as_str),
                input.scope.as_str(),
                input.scope_id,
                input.content,
                content_hash,
                input.confidence,
                input.source_backend,
                input.source_session_id,
                input.source_execution_id,
                input.metadata_json,
                ttl_days,
                now,
                expires_at,
                supersedes,
            ],
        )?;
        self.upsert_embedding(&conn, &id, &vector, now)?;
        Ok(Some(id))
    }

    #[allow(dead_code)]
    pub fn recall_buckets(
        &self,
        user_id: &str,
        project_id: &str,
        session_id: &str,
    ) -> Result<RecallBuckets> {
        self.recall_buckets_with_thresholds(
            user_id,
            project_id,
            session_id,
            RecallThresholds::default(),
        )
    }

    pub fn recall_buckets_with_thresholds(
        &self,
        user_id: &str,
        project_id: &str,
        session_id: &str,
        thresholds: RecallThresholds,
    ) -> Result<RecallBuckets> {
        let user_ids = user_scope_candidates(user_id);
        let project_ids = project_scope_candidates(project_id);
        let episodic =
            self.recall_episodic_bucket(session_id, &project_ids, thresholds.episodic)?;
        Ok(RecallBuckets {
            identity: self.recall_semantic_bucket(
                &MemoryScope::User,
                &user_ids,
                &MemoryFacet::Identity,
                thresholds.identity,
                20,
            )?,
            preference: self.recall_semantic_bucket(
                &MemoryScope::User,
                &user_ids,
                &MemoryFacet::Preference,
                thresholds.preference,
                30,
            )?,
            strategic: self.recall_semantic_bucket(
                &MemoryScope::Project,
                &project_ids,
                &MemoryFacet::Strategic,
                thresholds.strategic,
                30,
            )?,
            domain: self.recall_semantic_bucket(
                &MemoryScope::Project,
                &project_ids,
                &MemoryFacet::Domain,
                thresholds.domain,
                50,
            )?,
            procedural: self.query_many(
                &MemoryScope::Project,
                &project_ids,
                None,
                Some(&MemoryType::Procedural),
                thresholds.procedural,
                10,
            )?,
            episodic,
        })
    }

    fn recall_semantic_bucket(
        &self,
        scope: &MemoryScope,
        scope_ids: &[String],
        facet: &MemoryFacet,
        min_confidence: f64,
        limit: usize,
    ) -> Result<Vec<MemoryRecord>> {
        self.query_many(
            scope,
            scope_ids,
            Some(facet),
            Some(&MemoryType::Semantic),
            min_confidence,
            limit,
        )
    }

    fn recall_episodic_bucket(
        &self,
        session_id: &str,
        project_ids: &[String],
        min_confidence: f64,
    ) -> Result<Vec<MemoryRecord>> {
        let mut episodic = self.query(
            &MemoryScope::Session,
            session_id,
            None,
            Some(&MemoryType::Episodic),
            min_confidence,
            20,
        )?;
        episodic.extend(self.query_many(
            &MemoryScope::Project,
            project_ids,
            None,
            Some(&MemoryType::Episodic),
            min_confidence,
            20,
        )?);
        episodic.truncate(20);
        Ok(episodic)
    }

    pub fn compact_episodic_scope(
        &self,
        scope: MemoryScope,
        scope_id: &str,
        keep_latest: usize,
    ) -> Result<usize> {
        let keep_latest = keep_latest.max(1) as i64;
        let conn = crate::utils::lock_or_recover(&self.conn);
        let deleted = conn.execute(
            "DELETE FROM memory WHERE id IN (
                SELECT id FROM memory
                WHERE scope = ?1 AND scope_id = ?2 AND type = 'episodic'
                ORDER BY updated_at DESC, created_at DESC
                LIMIT -1 OFFSET ?3
            )",
            params![scope.as_str(), scope_id, keep_latest],
        )?;
        Ok(deleted)
    }

    pub fn get_session_turns(&self, session_id: &str) -> Result<Vec<(String, String)>> {
        let conn = crate::utils::lock_or_recover(&self.conn);
        let mut stmt = conn.prepare(
            "SELECT source_backend, content FROM memory
             WHERE scope = 'session' AND scope_id = ?1 AND type = 'episodic' AND expires_at > ?2
             ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map(params![session_id, now_ts()], |row| {
            let backend: Option<String> = row.get(0)?;
            let content: String = row.get(1)?;
            Ok((backend.unwrap_or_default(), content))
        })?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<MemoryRecord>> {
        self.search_with_mode(query, limit, MemorySearchMode::Keyword)
    }

    pub fn search_with_mode(
        &self,
        query: &str,
        limit: usize,
        mode: MemorySearchMode,
    ) -> Result<Vec<MemoryRecord>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let trimmed = query.trim();
        match mode {
            MemorySearchMode::Keyword => self.search_keyword(trimmed, limit),
            MemorySearchMode::Vector => self.search_vector(trimmed, limit),
            MemorySearchMode::Hybrid => self.search_hybrid(trimmed, limit),
        }
    }

    pub fn all_scope_buckets(&self, limit_per_bucket: usize) -> Result<RecallBuckets> {
        Ok(RecallBuckets {
            identity: self.query_all(
                Some(&MemoryFacet::Identity),
                Some(&MemoryType::Semantic),
                limit_per_bucket,
            )?,
            preference: self.query_all(
                Some(&MemoryFacet::Preference),
                Some(&MemoryType::Semantic),
                limit_per_bucket,
            )?,
            strategic: self.query_all(
                Some(&MemoryFacet::Strategic),
                Some(&MemoryType::Semantic),
                limit_per_bucket,
            )?,
            domain: self.query_all(
                Some(&MemoryFacet::Domain),
                Some(&MemoryType::Semantic),
                limit_per_bucket,
            )?,
            procedural: self.query_all(None, Some(&MemoryType::Procedural), limit_per_bucket)?,
            episodic: self.query_all(None, Some(&MemoryType::Episodic), limit_per_bucket)?,
        })
    }

    fn query_all(
        &self,
        facet: Option<&MemoryFacet>,
        memory_type: Option<&MemoryType>,
        limit: usize,
    ) -> Result<Vec<MemoryRecord>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let facet_value = facet.map(MemoryFacet::as_str);
        let type_value = memory_type.map(MemoryType::as_str);
        let conn = crate::utils::lock_or_recover(&self.conn);
        let mut stmt = conn.prepare(
            "SELECT id, type, facet, scope, scope_id, content, confidence, created_at, updated_at, expires_at FROM memory
             WHERE (?1 IS NULL OR facet = ?1) AND (?2 IS NULL OR type = ?2) AND expires_at > ?3
             ORDER BY confidence DESC, updated_at DESC, created_at DESC
             LIMIT ?4",
        )?;
        rows_to_records(stmt.query_map(
            params![facet_value, type_value, now_ts(), limit as i64],
            row_to_memory_record,
        )?)
    }

    fn search_keyword(&self, trimmed: &str, limit: usize) -> Result<Vec<MemoryRecord>> {
        if self.fts_available && !trimmed.is_empty() {
            match self.search_fts(trimmed, limit) {
                Ok(records) => return Ok(records),
                Err(err) => {
                    tracing::warn!(
                        error = %err,
                        query = trimmed,
                        limit,
                        "memory.fts.fallback"
                    );
                }
            }
        }
        self.search_like(trimmed, limit)
    }

    fn search_vector(&self, query: &str, limit: usize) -> Result<Vec<MemoryRecord>> {
        if query.trim().is_empty() {
            return self.search_keyword(query, limit);
        }
        let query_vec = self.embedding.embed(query);
        if query_vec.is_empty() {
            return Ok(Vec::new());
        }
        let query_canonical = embedding::canonicalize(query);
        let conn = crate::utils::lock_or_recover(&self.conn);
        let mut stmt = conn.prepare(
            "SELECT m.id, m.type, m.facet, m.scope, m.scope_id, m.content, m.confidence, m.created_at, m.updated_at, m.expires_at, e.vector_blob
             FROM memory m
             JOIN memory_embedding e ON e.memory_id = m.id
             WHERE m.expires_at > ?1
             ORDER BY m.updated_at DESC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![now_ts(), 800i64], |row| {
            Ok((row_to_memory_record(row)?, row.get::<_, Vec<u8>>(10)?))
        })?;

        let mut scored = Vec::new();
        for row in rows {
            let (record, blob) = match row {
                Ok(value) => value,
                Err(err) if is_invalid_memory_taxonomy_error(&err) => {
                    warn_invalid_memory_taxonomy(&err);
                    continue;
                }
                Err(err) => return Err(err.into()),
            };
            let vector = embedding::from_blob(&blob);
            let similarity = embedding::cosine(&query_vec, &vector);
            let overlap = token_overlap_score(&query_canonical, &record.content);
            let score = 0.65 * similarity + 0.20 * overlap + 0.15 * record.confidence;
            scored.push((score, record));
        }
        sort_scored_records(&mut scored);
        Ok(scored
            .into_iter()
            .filter(|(score, _)| *score > 0.05)
            .take(limit)
            .map(|(_, record)| record)
            .collect())
    }

    fn search_hybrid(&self, query: &str, limit: usize) -> Result<Vec<MemoryRecord>> {
        let expanded_limit = expanded_search_limit(limit);
        let keyword = self.search_keyword(query, expanded_limit)?;
        let vector = self.search_vector(query, expanded_limit)?;
        let mut ranking: HashMap<String, (f64, MemoryRecord)> = HashMap::new();

        for (index, record) in keyword.into_iter().enumerate() {
            add_ranked_record(&mut ranking, index, record, 1.0);
        }
        for (index, record) in vector.into_iter().enumerate() {
            add_ranked_record(&mut ranking, index, record, 1.2);
        }

        let mut merged = ranking.into_values().collect::<Vec<_>>();
        sort_scored_records(&mut merged);
        Ok(merged
            .into_iter()
            .take(limit)
            .map(|(_, record)| record)
            .collect())
    }

    fn search_fts(&self, query: &str, limit: usize) -> Result<Vec<MemoryRecord>> {
        // Wrap the query in double-quotes to make FTS5 treat it as a phrase
        // rather than a boolean expression.  Internal double-quotes are escaped
        // by doubling them (FTS5 phrase-quoting rules).
        let safe_query = format!("\"{}\"", query.replace('"', "\"\""));
        let conn = crate::utils::lock_or_recover(&self.conn);
        let mut stmt = conn.prepare(
            "SELECT m.id, m.type, m.facet, m.scope, m.scope_id, m.content, m.confidence, m.created_at, m.updated_at, m.expires_at
             FROM memory m JOIN memory_fts f ON m.rowid = f.rowid
             WHERE memory_fts MATCH ?1 AND m.expires_at > ?2
             ORDER BY rank, m.confidence DESC, m.updated_at DESC LIMIT ?3",
        )?;
        rows_to_records(stmt.query_map(
            params![safe_query, now_ts(), limit as i64],
            row_to_memory_record,
        )?)
    }

    fn search_like(&self, query: &str, limit: usize) -> Result<Vec<MemoryRecord>> {
        let needle = format!("%{}%", query.replace('%', "\\%").replace('_', "\\_"));
        let conn = crate::utils::lock_or_recover(&self.conn);
        let mut stmt = conn.prepare(
            "SELECT id, type, facet, scope, scope_id, content, confidence, created_at, updated_at, expires_at FROM memory
             WHERE expires_at > ?1 AND content LIKE ?2 ESCAPE ?4
             ORDER BY confidence DESC, updated_at DESC LIMIT ?3",
        )?;
        rows_to_records(stmt.query_map(
            params![now_ts(), needle, limit as i64, "\\"],
            row_to_memory_record,
        )?)
    }

    fn query(
        &self,
        scope: &MemoryScope,
        scope_id: &str,
        facet: Option<&MemoryFacet>,
        memory_type: Option<&MemoryType>,
        min_confidence: f64,
        limit: usize,
    ) -> Result<Vec<MemoryRecord>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let facet_value = facet.map(MemoryFacet::as_str);
        let type_value = memory_type.map(MemoryType::as_str);
        let conn = crate::utils::lock_or_recover(&self.conn);
        let mut stmt = conn.prepare(
            "SELECT id, type, facet, scope, scope_id, content, confidence, created_at, updated_at, expires_at FROM memory\n             WHERE scope = ?1 AND scope_id = ?2 AND (?3 IS NULL OR facet = ?3) AND (?4 IS NULL OR type = ?4) AND confidence >= ?5 AND expires_at > ?6\n             ORDER BY confidence DESC, content_hash ASC LIMIT ?7",
        )?;
        rows_to_records(stmt.query_map(
            params![
                scope.as_str(),
                scope_id,
                facet_value,
                type_value,
                min_confidence,
                now_ts(),
                limit as i64
            ],
            row_to_memory_record,
        )?)
    }

    fn query_many(
        &self,
        scope: &MemoryScope,
        scope_ids: &[String],
        facet: Option<&MemoryFacet>,
        memory_type: Option<&MemoryType>,
        min_confidence: f64,
        limit: usize,
    ) -> Result<Vec<MemoryRecord>> {
        if limit == 0 || scope_ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut records = Vec::new();
        for scope_id in scope_ids {
            records.extend(self.query(
                scope,
                scope_id,
                facet,
                memory_type,
                min_confidence,
                limit,
            )?);
        }
        Ok(finalize_query_records(records, limit))
    }

    fn upsert_embedding(
        &self,
        conn: &Connection,
        memory_id: &str,
        vector: &[f32],
        updated_at: i64,
    ) -> Result<()> {
        if vector.is_empty() {
            return Ok(());
        }
        let blob = embedding::to_blob(vector);
        conn.execute(
            "INSERT INTO memory_embedding (memory_id, vector_blob, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(memory_id) DO UPDATE SET
               vector_blob = excluded.vector_blob,
               updated_at = excluded.updated_at",
            params![memory_id, blob, updated_at],
        )?;
        Ok(())
    }
}

fn init_schema(conn: &Connection) -> Result<bool> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS memory (
  id TEXT PRIMARY KEY,
  type TEXT NOT NULL CHECK(type IN ('semantic','episodic','procedural')),
  facet TEXT CHECK(facet IN ('identity','preference','strategic','domain')),
  scope TEXT NOT NULL CHECK(scope IN ('session','project','user','global')),
  scope_id TEXT NOT NULL,
  content TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  confidence REAL NOT NULL DEFAULT 1.0,
  source_backend TEXT,
  source_session_id TEXT,
  source_execution_id TEXT,
  metadata_json TEXT,
  ttl_days INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  expires_at INTEGER NOT NULL,
  supersedes TEXT,
  owner TEXT NOT NULL DEFAULT 'local',
  visibility TEXT NOT NULL DEFAULT 'private'
);
CREATE TABLE IF NOT EXISTS memory_embedding (
    memory_id TEXT PRIMARY KEY,
    vector_blob BLOB NOT NULL,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY(memory_id) REFERENCES memory(id) ON DELETE CASCADE
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_memory_dedup ON memory(scope, scope_id, type, facet, content_hash);
CREATE INDEX IF NOT EXISTS idx_memory_recall_semantic ON memory(scope, scope_id, facet, confidence DESC, updated_at DESC) WHERE type = 'semantic';
CREATE INDEX IF NOT EXISTS idx_memory_recall_procedural ON memory(scope, scope_id, confidence DESC, updated_at DESC) WHERE type = 'procedural';
CREATE INDEX IF NOT EXISTS idx_memory_recall_episodic ON memory(scope, scope_id, created_at DESC) WHERE type = 'episodic';
CREATE INDEX IF NOT EXISTS idx_memory_embedding_updated ON memory_embedding(updated_at DESC);
CREATE VIEW IF NOT EXISTS memories AS SELECT * FROM memory;
CREATE TRIGGER IF NOT EXISTS memories_delete INSTEAD OF DELETE ON memories BEGIN
  DELETE FROM memory WHERE id = old.id;
END;",
    )?;
    Ok(init_fts(conn).is_ok())
}

fn user_scope_candidates(user_id: &str) -> Vec<String> {
    unique_strings(vec![
        user_id.to_string(),
        "user-sympantos".to_string(),
        "local-user".to_string(),
    ])
}

fn project_scope_candidates(project_id: &str) -> Vec<String> {
    let mut values = vec![project_id.to_string(), "iota-sympantos".to_string()];
    if let Some(name) = Path::new(project_id)
        .file_name()
        .and_then(|value| value.to_str())
    {
        values.push(name.to_string());
    }
    unique_strings(values)
}

fn unique_strings(values: Vec<String>) -> Vec<String> {
    let mut output = Vec::new();
    for value in values {
        let trimmed = value.trim();
        if !trimmed.is_empty() && !output.iter().any(|existing| existing == trimmed) {
            output.push(trimmed.to_string());
        }
    }
    output
}

fn sort_scored_records(records: &mut [(f64, MemoryRecord)]) {
    records.sort_by(|left, right| {
        right
            .0
            .partial_cmp(&left.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| right.1.updated_at.cmp(&left.1.updated_at))
    });
}

fn finalize_query_records(mut records: Vec<MemoryRecord>, limit: usize) -> Vec<MemoryRecord> {
    records.sort_by(|left, right| {
        right
            .confidence
            .partial_cmp(&left.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.id.cmp(&right.id))
    });
    records.dedup_by(|left, right| left.id == right.id);
    records.truncate(limit);
    records
}

fn add_ranked_record(
    ranking: &mut HashMap<String, (f64, MemoryRecord)>,
    index: usize,
    record: MemoryRecord,
    weight_multiplier: f64,
) {
    let weight = (1.0 / ((index + 1) as f64)) * weight_multiplier;
    let entry = ranking
        .entry(record.id.clone())
        .or_insert_with(|| (0.0, record.clone()));
    entry.0 += weight;
    entry.1 = record;
}

fn expanded_search_limit(limit: usize) -> usize {
    limit.saturating_mul(3).max(20)
}

fn init_fts(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(
  content,
  content='memory',
  content_rowid='rowid',
  tokenize='unicode61'
);
CREATE TRIGGER IF NOT EXISTS memory_ai AFTER INSERT ON memory BEGIN
  INSERT INTO memory_fts(rowid, content) VALUES (new.rowid, new.content);
END;
CREATE TRIGGER IF NOT EXISTS memory_ad AFTER DELETE ON memory BEGIN
  INSERT INTO memory_fts(memory_fts, rowid, content) VALUES('delete', old.rowid, old.content);
END;
CREATE TRIGGER IF NOT EXISTS memory_au AFTER UPDATE ON memory BEGIN
  INSERT INTO memory_fts(memory_fts, rowid, content) VALUES('delete', old.rowid, old.content);
  INSERT INTO memory_fts(rowid, content) VALUES (new.rowid, new.content);
END;",
    )?;
    let missing_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM memory m LEFT JOIN memory_fts f ON m.rowid = f.rowid WHERE f.rowid IS NULL",
        [],
        |row| row.get(0),
    )?;
    if missing_count > 0 {
        conn.execute("INSERT INTO memory_fts(memory_fts) VALUES('rebuild')", [])?;
    }
    Ok(())
}

fn validate_taxonomy(input: &MemoryInsert) -> Result<()> {
    if input.memory_type == MemoryType::Semantic && input.facet.is_none() {
        bail!("semantic memory requires a facet");
    }
    if input.memory_type != MemoryType::Semantic && input.facet.is_some() {
        bail!("only semantic memory may set facet");
    }
    Ok(())
}

fn latest_related_memory_id(
    conn: &Connection,
    scope: &str,
    scope_id: &str,
    memory_type: &str,
    facet: Option<&str>,
) -> Result<Option<String>> {
    conn.query_row(
        "SELECT id FROM memory WHERE scope = ?1 AND scope_id = ?2 AND type = ?3 AND facet IS ?4 ORDER BY updated_at DESC LIMIT 1",
        params![scope, scope_id, memory_type, facet],
        |row| row.get(0),
    )
    .optional()
    .context("Failed to find related memory")
}

fn row_to_memory_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryRecord> {
    let memory_type = row.get::<_, String>(1)?;
    let facet = row.get::<_, Option<String>>(2)?;
    let scope = row.get::<_, String>(3)?;
    Ok(MemoryRecord {
        id: row.get(0)?,
        memory_type: parse_memory_type_column(memory_type, 1)?,
        facet: facet
            .map(|value| parse_memory_facet_column(value, 2))
            .transpose()?,
        scope: parse_memory_scope_column(scope, 3)?,
        scope_id: row.get(4)?,
        content: row.get(5)?,
        confidence: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
        expires_at: row.get(9)?,
    })
}

fn parse_memory_type_column(value: String, column: usize) -> rusqlite::Result<MemoryType> {
    MemoryType::parse(&value).ok_or_else(|| invalid_memory_taxonomy_value(column, value))
}

fn parse_memory_facet_column(value: String, column: usize) -> rusqlite::Result<MemoryFacet> {
    MemoryFacet::parse(&value).ok_or_else(|| invalid_memory_taxonomy_value(column, value))
}

fn parse_memory_scope_column(value: String, column: usize) -> rusqlite::Result<MemoryScope> {
    MemoryScope::parse(&value).ok_or_else(|| invalid_memory_taxonomy_value(column, value))
}

fn invalid_memory_taxonomy_value(column: usize, value: String) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        column,
        rusqlite::types::Type::Text,
        format!("invalid memory taxonomy value: {}", value).into(),
    )
}

fn is_invalid_memory_taxonomy_error(err: &rusqlite::Error) -> bool {
    matches!(
        err,
        rusqlite::Error::FromSqlConversionFailure(_, rusqlite::types::Type::Text, source)
            if source
                .to_string()
                .starts_with("invalid memory taxonomy value:")
    )
}

fn warn_invalid_memory_taxonomy(err: &rusqlite::Error) {
    tracing::warn!(error = %err, "skipping memory record with unknown taxonomy value");
}

fn rows_to_records(
    rows: impl Iterator<Item = rusqlite::Result<MemoryRecord>>,
) -> Result<Vec<MemoryRecord>> {
    let mut records = Vec::new();
    for row in rows {
        match row {
            Ok(record) => records.push(record),
            Err(err) if is_invalid_memory_taxonomy_error(&err) => {
                warn_invalid_memory_taxonomy(&err);
            }
            Err(err) => return Err(err.into()),
        }
    }
    Ok(records)
}

fn token_overlap_score(query_canonical: &str, content: &str) -> f64 {
    let content_canonical = embedding::canonicalize(content);
    let query_tokens = query_canonical
        .split_whitespace()
        .collect::<std::collections::HashSet<_>>();
    if query_tokens.is_empty() {
        return 0.0;
    }
    let content_tokens = content_canonical
        .split_whitespace()
        .collect::<std::collections::HashSet<_>>();
    let overlap = query_tokens
        .iter()
        .filter(|token| content_tokens.contains(**token))
        .count();
    overlap as f64 / query_tokens.len() as f64
}

fn content_hash(content: &str) -> String {
    let canonical = embedding::canonicalize(content);
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod store_tests;
