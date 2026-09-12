use anyhow::Result;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use uuid::Uuid;

use crate::runtime_event::TokenUsageEvent;
use crate::utils::now_ts;

#[derive(Clone)]
pub struct ObservabilityStore {
    pool: Arc<super::db::DbPool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredTokenUsage {
    pub id: String,
    pub ts: i64,
    pub execution_id: Option<String>,
    pub session_id: Option<String>,
    pub backend: String,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub source: String,
    pub input_tokens: Option<u64>,
    pub cache_read_input_tokens: Option<u64>,
    pub cache_creation_input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub thinking_tokens: Option<u64>,
    pub tool_use_prompt_tokens: Option<u64>,
    pub provider_reported_total_tokens: Option<u64>,
    pub normalized_total_tokens: Option<u64>,
    pub raw_payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsageSummary {
    pub backend: String,
    pub count: u64,
    pub input_tokens_mean: Option<f64>,
    pub input_tokens_stddev: Option<f64>,
    pub input_tokens_cv: Option<f64>,
    pub cache_read_input_tokens_mean: Option<f64>,
    pub cache_read_input_tokens_stddev: Option<f64>,
    pub cache_read_input_tokens_cv: Option<f64>,
    pub cache_creation_input_tokens_mean: Option<f64>,
    pub cache_creation_input_tokens_stddev: Option<f64>,
    pub cache_creation_input_tokens_cv: Option<f64>,
    pub output_tokens_mean: Option<f64>,
    pub output_tokens_stddev: Option<f64>,
    pub output_tokens_cv: Option<f64>,
    pub thinking_tokens_mean: Option<f64>,
    pub thinking_tokens_stddev: Option<f64>,
    pub thinking_tokens_cv: Option<f64>,
    pub provider_reported_total_mean: Option<f64>,
    pub provider_reported_total_stddev: Option<f64>,
    pub provider_reported_total_cv: Option<f64>,
    pub normalized_total_mean: Option<f64>,
    pub normalized_total_stddev: Option<f64>,
    pub normalized_total_cv: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenPercentiles {
    pub backend: String,
    pub count: usize,
    pub p50: Option<u64>,
    pub p95: Option<u64>,
    pub p99: Option<u64>,
}

impl ObservabilityStore {
    pub fn open(path: &Path) -> Result<Self> {
        let pool = super::db::DbPool::shared(path)?;
        let store = Self { pool };
        store.init()?;
        Ok(store)
    }

    pub fn default_path() -> Result<PathBuf> {
        Ok(crate::config::paths::StorePaths::resolve()?.events_db())
    }

    pub fn record_token_usage(
        &self,
        execution_id: Option<&str>,
        session_id: Option<&str>,
        backend: &str,
        usage: &TokenUsageEvent,
    ) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        let raw_payload_json = serde_json::to_string(&usage.raw_payload)?;
        // Validate token count consistency before persisting.
        let provider_total = usage.provider_reported_total_tokens.unwrap_or(0);
        let computed = usage
            .input_tokens
            .unwrap_or(0)
            .saturating_add(usage.output_tokens.unwrap_or(0))
            .saturating_add(usage.thinking_tokens.unwrap_or(0));
        if provider_total > 0 && computed > 0 && computed > provider_total {
            tracing::warn!(
                execution_id = execution_id.unwrap_or("none"),
                backend,
                computed,
                provider_total,
                "token count inconsistency: computed > provider_total"
            );
        }
        let conn = self.pool.write("observability.record_token_usage");
        conn.execute(
            "INSERT INTO token_usage_events (
                id, ts, execution_id, session_id, backend, model, provider, source,
                input_tokens, cache_read_input_tokens, cache_creation_input_tokens,
                output_tokens, thinking_tokens, tool_use_prompt_tokens,
                provider_reported_total_tokens, normalized_total_tokens, raw_payload_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
            params![
                &id,
                now_ts(),
                execution_id,
                session_id.or(usage.session_id.as_deref()),
                backend,
                usage.model.as_deref(),
                usage.provider.as_deref(),
                usage.source.as_deref().unwrap_or("unknown"),
                opt_i64(usage.input_tokens),
                opt_i64(usage.cache_read_input_tokens),
                opt_i64(usage.cache_creation_input_tokens),
                opt_i64(usage.output_tokens),
                opt_i64(usage.thinking_tokens),
                opt_i64(usage.tool_use_prompt_tokens),
                opt_i64(usage.provider_reported_total_tokens),
                opt_i64(usage.normalized_total_tokens),
                raw_payload_json,
            ],
        )?;
        Ok(id)
    }

    pub fn recent_token_usage(&self, limit: usize) -> Result<Vec<StoredTokenUsage>> {
        let conn = self.pool.read("observability.recent_token_usage");
        let mut stmt = conn.prepare(
            "SELECT id, ts, execution_id, session_id, backend, model, provider, source,
                    input_tokens, cache_read_input_tokens, cache_creation_input_tokens,
                    output_tokens, thinking_tokens, tool_use_prompt_tokens,
                    provider_reported_total_tokens, normalized_total_tokens, raw_payload_json
             FROM token_usage_events
             ORDER BY ts DESC, id DESC
             LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], row_to_token_usage)?;
        collect_rows(rows)
    }

    pub fn recent_token_executions(&self, limit: usize) -> Result<Vec<StoredTokenUsage>> {
        let events = self.recent_token_usage(limit.saturating_mul(20).max(limit))?;
        let mut best_by_execution: BTreeMap<String, StoredTokenUsage> = BTreeMap::new();
        for event in events {
            let key = event
                .execution_id
                .clone()
                .unwrap_or_else(|| event.id.clone());
            match best_by_execution.get(&key) {
                Some(existing) if token_event_score(existing) >= token_event_score(&event) => {}
                _ => {
                    best_by_execution.insert(key, event);
                }
            }
        }
        let mut records = best_by_execution.into_values().collect::<Vec<_>>();
        records.sort_by(|a, b| b.ts.cmp(&a.ts).then_with(|| b.id.cmp(&a.id)));
        records.truncate(limit);
        Ok(records)
    }

    pub fn token_usage_for_execution(&self, execution_id: &str) -> Result<Vec<StoredTokenUsage>> {
        let conn = self.pool.read("observability.token_usage_for_execution");
        let mut stmt = conn.prepare(
            "SELECT id, ts, execution_id, session_id, backend, model, provider, source,
                    input_tokens, cache_read_input_tokens, cache_creation_input_tokens,
                    output_tokens, thinking_tokens, tool_use_prompt_tokens,
                    provider_reported_total_tokens, normalized_total_tokens, raw_payload_json
             FROM token_usage_events
             WHERE execution_id = ?1
             ORDER BY ts ASC, id ASC",
        )?;
        let rows = stmt.query_map(params![execution_id], row_to_token_usage)?;
        collect_rows(rows)
    }

    pub fn token_summary_since(&self, since_ts: i64) -> Result<Vec<TokenUsageSummary>> {
        let events = self.token_usage_since(since_ts)?;
        let mut best_by_execution: BTreeMap<String, StoredTokenUsage> = BTreeMap::new();
        for event in events {
            let key = event
                .execution_id
                .clone()
                .unwrap_or_else(|| event.id.clone());
            match best_by_execution.get(&key) {
                Some(existing) if token_event_score(existing) >= token_event_score(&event) => {}
                _ => {
                    best_by_execution.insert(key, event);
                }
            }
        }
        let mut by_backend: BTreeMap<String, SummaryAccumulator> = BTreeMap::new();
        for event in best_by_execution.values() {
            by_backend
                .entry(event.backend.clone())
                .or_default()
                .add(event);
        }
        Ok(by_backend
            .into_iter()
            .map(|(backend, acc)| acc.finish(backend))
            .collect())
    }

    #[allow(dead_code)]
    pub fn token_usage_between(&self, from_ts: i64, to_ts: i64) -> Result<Vec<StoredTokenUsage>> {
        let conn = self.pool.read("observability.token_usage_between");
        let mut stmt = conn.prepare(
            "SELECT id, ts, execution_id, session_id, backend, model, provider, source,
                    input_tokens, cache_read_input_tokens, cache_creation_input_tokens,
                    output_tokens, thinking_tokens, tool_use_prompt_tokens,
                    provider_reported_total_tokens, normalized_total_tokens, raw_payload_json
             FROM token_usage_events
             WHERE ts >= ?1 AND ts <= ?2
             ORDER BY ts DESC, id DESC",
        )?;
        let rows = stmt.query_map(params![from_ts, to_ts], row_to_token_usage)?;
        collect_rows(rows)
    }

    #[allow(dead_code)]
    pub fn token_percentiles(&self, backend: &str) -> Result<TokenPercentiles> {
        let conn = self.pool.read("observability.token_percentiles");
        let mut stmt = conn.prepare(
            "SELECT normalized_total_tokens
             FROM token_usage_events
             WHERE backend = ?1 AND normalized_total_tokens IS NOT NULL
             ORDER BY normalized_total_tokens ASC",
        )?;
        let totals: Vec<u64> = stmt
            .query_map(params![backend], |row| row.get::<_, i64>(0))?
            .filter_map(|r| r.ok())
            .filter_map(|v| u64::try_from(v).ok())
            .collect();
        Ok(TokenPercentiles {
            backend: backend.to_string(),
            count: totals.len(),
            p50: percentile(&totals, 50),
            p95: percentile(&totals, 95),
            p99: percentile(&totals, 99),
        })
    }

    fn token_usage_since(&self, since_ts: i64) -> Result<Vec<StoredTokenUsage>> {
        let conn = self.pool.read("observability.token_usage_since");
        let mut stmt = conn.prepare(
            "SELECT id, ts, execution_id, session_id, backend, model, provider, source,
                    input_tokens, cache_read_input_tokens, cache_creation_input_tokens,
                    output_tokens, thinking_tokens, tool_use_prompt_tokens,
                    provider_reported_total_tokens, normalized_total_tokens, raw_payload_json
             FROM token_usage_events
             WHERE ts >= ?1
             ORDER BY ts DESC, id DESC",
        )?;
        let rows = stmt.query_map(params![since_ts], row_to_token_usage)?;
        collect_rows(rows)
    }

    fn init(&self) -> Result<()> {
        let conn = self.pool.write("observability.init");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS token_usage_events (
              id TEXT PRIMARY KEY,
              ts INTEGER NOT NULL,
              execution_id TEXT,
              session_id TEXT,
              backend TEXT NOT NULL,
              model TEXT,
              provider TEXT,
              source TEXT NOT NULL,
              input_tokens INTEGER,
              cache_read_input_tokens INTEGER,
              cache_creation_input_tokens INTEGER,
              output_tokens INTEGER,
              thinking_tokens INTEGER,
              tool_use_prompt_tokens INTEGER,
              provider_reported_total_tokens INTEGER,
              normalized_total_tokens INTEGER,
              raw_payload_json TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_token_usage_execution ON token_usage_events(execution_id, ts);
            CREATE INDEX IF NOT EXISTS idx_token_usage_backend ON token_usage_events(backend, ts);",
        )?;
        Ok(())
    }
}

#[derive(Default)]
struct SummaryAccumulator {
    count: u64,
    input: MeanAccumulator,
    cache_read: MeanAccumulator,
    cache_creation: MeanAccumulator,
    output: MeanAccumulator,
    thinking: MeanAccumulator,
    provider_total: MeanAccumulator,
    normalized_total: MeanAccumulator,
}

impl SummaryAccumulator {
    fn add(&mut self, event: &StoredTokenUsage) {
        self.count += 1;
        self.input.add(event.input_tokens);
        self.cache_read.add(event.cache_read_input_tokens);
        self.cache_creation.add(event.cache_creation_input_tokens);
        self.output.add(event.output_tokens);
        self.thinking.add(event.thinking_tokens);
        self.provider_total
            .add(event.provider_reported_total_tokens);
        self.normalized_total.add(event.normalized_total_tokens);
    }

    fn finish(self, backend: String) -> TokenUsageSummary {
        TokenUsageSummary {
            backend,
            count: self.count,
            input_tokens_mean: self.input.mean(),
            input_tokens_stddev: self.input.stddev(),
            input_tokens_cv: self.input.cv(),
            cache_read_input_tokens_mean: self.cache_read.mean(),
            cache_read_input_tokens_stddev: self.cache_read.stddev(),
            cache_read_input_tokens_cv: self.cache_read.cv(),
            cache_creation_input_tokens_mean: self.cache_creation.mean(),
            cache_creation_input_tokens_stddev: self.cache_creation.stddev(),
            cache_creation_input_tokens_cv: self.cache_creation.cv(),
            output_tokens_mean: self.output.mean(),
            output_tokens_stddev: self.output.stddev(),
            output_tokens_cv: self.output.cv(),
            thinking_tokens_mean: self.thinking.mean(),
            thinking_tokens_stddev: self.thinking.stddev(),
            thinking_tokens_cv: self.thinking.cv(),
            provider_reported_total_mean: self.provider_total.mean(),
            provider_reported_total_stddev: self.provider_total.stddev(),
            provider_reported_total_cv: self.provider_total.cv(),
            normalized_total_mean: self.normalized_total.mean(),
            normalized_total_stddev: self.normalized_total.stddev(),
            normalized_total_cv: self.normalized_total.cv(),
        }
    }
}

#[derive(Default)]
struct MeanAccumulator {
    sum: u64,
    sum_squares: f64,
    count: u64,
}

impl MeanAccumulator {
    fn add(&mut self, value: Option<u64>) {
        if let Some(value) = value {
            self.sum += value;
            self.sum_squares += (value as f64) * (value as f64);
            self.count += 1;
        }
    }

    fn mean(&self) -> Option<f64> {
        (self.count > 0).then(|| self.sum as f64 / self.count as f64)
    }

    fn stddev(&self) -> Option<f64> {
        if self.count < 2 {
            return None;
        }
        let count = self.count as f64;
        let sum = self.sum as f64;
        let variance = (self.sum_squares - (sum * sum / count)) / (count - 1.0);
        Some(variance.max(0.0).sqrt())
    }

    fn cv(&self) -> Option<f64> {
        let mean = self.mean()?;
        if mean == 0.0 {
            return None;
        }
        Some(self.stddev()? / mean)
    }
}

// ---------------------------------------------------------------------------
// Performance Metrics
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencyPercentiles {
    pub p50_ms: Option<f64>,
    pub p99_ms: Option<f64>,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThroughputSummary {
    pub mean_tokens_per_sec: Option<f64>,
    pub count: usize,
}

#[derive(Debug, Clone, Default)]
pub struct ObservabilityMetrics {
    write_latencies_ms: Arc<Mutex<Vec<f64>>>,
    stream_throughput_tokens_per_sec: Arc<Mutex<Vec<f64>>>,
}

impl ObservabilityMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_write_latency(&self, latency_ms: f64) {
        if let Ok(mut data) = self.write_latencies_ms.lock() {
            data.push(latency_ms);
        }
    }

    pub fn record_stream_throughput(&self, tokens_per_sec: f64) {
        if let Ok(mut data) = self.stream_throughput_tokens_per_sec.lock() {
            data.push(tokens_per_sec);
        }
    }

    pub fn write_latency_percentiles(&self) -> LatencyPercentiles {
        let data = self
            .write_latencies_ms
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        compute_percentiles(&data)
    }

    pub fn stream_throughput_summary(&self) -> ThroughputSummary {
        let data = self
            .stream_throughput_tokens_per_sec
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let count = data.len();
        let mean = if count > 0 {
            Some(data.iter().sum::<f64>() / count as f64)
        } else {
            None
        };
        ThroughputSummary {
            mean_tokens_per_sec: mean,
            count,
        }
    }
}

impl ObservabilityStore {
    pub fn record_token_usage_with_metrics(
        &self,
        metrics: &ObservabilityMetrics,
        execution_id: Option<&str>,
        session_id: Option<&str>,
        backend: &str,
        usage: &TokenUsageEvent,
    ) -> Result<String> {
        let start = Instant::now();
        let result = self.record_token_usage(execution_id, session_id, backend, usage);
        let latency_ms = start.elapsed().as_secs_f64() * 1000.0;
        metrics.record_write_latency(latency_ms);
        result
    }
}

fn compute_percentiles(data: &[f64]) -> LatencyPercentiles {
    let count = data.len();
    if count == 0 {
        return LatencyPercentiles {
            p50_ms: None,
            p99_ms: None,
            count: 0,
        };
    }
    let mut sorted = data.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let p50_idx = (count as f64 * 0.50).ceil() as usize - 1;
    let p99_idx = (count as f64 * 0.99).ceil() as usize - 1;
    LatencyPercentiles {
        p50_ms: Some(sorted[p50_idx.min(count - 1)]),
        p99_ms: Some(sorted[p99_idx.min(count - 1)]),
        count,
    }
}

fn token_event_score(event: &StoredTokenUsage) -> u8 {
    let mut score = 0;
    // Prefer official backend-reported totals over computed
    if event.provider_reported_total_tokens.is_some() {
        score += 5;
    }
    // Normalized/computed totals are more authoritative
    if event.normalized_total_tokens.is_some() {
        score += 4;
    }
    // Prefer sources other than partial session updates
    if event.source != "session_update.usage_update" {
        score += 2;
    }
    // Individual token counts
    if event.input_tokens.is_some() {
        score += 1;
    }
    if event.output_tokens.is_some() {
        score += 1;
    }
    score
}

fn opt_i64(value: Option<u64>) -> Option<i64> {
    value.and_then(|value| value.try_into().ok())
}

fn opt_u64(value: Option<i64>) -> Option<u64> {
    value.and_then(|value| value.try_into().ok())
}

fn row_to_token_usage(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredTokenUsage> {
    let raw_payload_json: String = row.get(16)?;
    let raw_payload = serde_json::from_str(&raw_payload_json).unwrap_or(Value::Null);
    Ok(StoredTokenUsage {
        id: row.get(0)?,
        ts: row.get(1)?,
        execution_id: row.get(2)?,
        session_id: row.get(3)?,
        backend: row.get(4)?,
        model: row.get(5)?,
        provider: row.get(6)?,
        source: row.get(7)?,
        input_tokens: opt_u64(row.get(8)?),
        cache_read_input_tokens: opt_u64(row.get(9)?),
        cache_creation_input_tokens: opt_u64(row.get(10)?),
        output_tokens: opt_u64(row.get(11)?),
        thinking_tokens: opt_u64(row.get(12)?),
        tool_use_prompt_tokens: opt_u64(row.get(13)?),
        provider_reported_total_tokens: opt_u64(row.get(14)?),
        normalized_total_tokens: opt_u64(row.get(15)?),
        raw_payload,
    })
}

fn collect_rows<T>(rows: impl Iterator<Item = rusqlite::Result<T>>) -> Result<Vec<T>> {
    let mut items = Vec::new();
    for row in rows {
        items.push(row?);
    }
    Ok(items)
}

fn percentile(sorted_values: &[u64], p: usize) -> Option<u64> {
    if sorted_values.is_empty() || p == 0 || p > 100 {
        return None;
    }
    let index = ((sorted_values.len() as f64) * (p as f64 / 100.0)).ceil() as usize;
    let index = index.saturating_sub(1).min(sorted_values.len() - 1);
    Some(sorted_values[index])
}
