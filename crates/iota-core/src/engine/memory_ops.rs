use crate::acp::AcpBackend;
use crate::memory::{MemoryInsert, MemoryScope, MemoryType, RecallBuckets};
use crate::runtime_event::RuntimeEvent;
use crate::utils::summarize;

use super::IotaEngine;

impl IotaEngine {
    /// Persist a summarized prompt/output pair as session-scoped episodic memory.
    pub(super) fn persist_turn_as_episodic_memory(
        &self,
        backend: AcpBackend,
        prompt: &str,
        output: &str,
        execution_id: Option<&str>,
    ) {
        let Some(store) = &self.memory_store else {
            return;
        };
        let content = format!(
            "Prompt: {}\nOutput: {}",
            summarize(prompt, 300),
            summarize(output, 500)
        );
        let content_chars = content.chars().count();
        self.log_engine_event(
            execution_id,
            backend,
            "info",
            "memory.write.call",
            serde_json::json!({
                "source": "engine-episodic",
                "type": "episodic",
                "scope": "session",
                "scope_id": self.engine_session_id.clone(),
                "confidence": 0.8,
                "content_chars": content_chars,
            }),
        );
        tracing::info!(
            backend = %backend,
            execution_id = execution_id.unwrap_or("-"),
            session_id = %self.engine_session_id,
            content_chars,
            source = "engine-episodic",
            "engine episodic memory write started"
        );
        match store.insert(MemoryInsert {
            memory_type: MemoryType::Episodic,
            facet: None,
            scope: MemoryScope::Session,
            scope_id: self.engine_session_id.clone(),
            content,
            confidence: 0.8,
            source_backend: Some(backend.to_string()),
            source_session_id: Some(self.engine_session_id.clone()),
            source_execution_id: execution_id.map(str::to_string),
            metadata_json: None,
            ttl_days: 7,
            supersedes: None,
        }) {
            Ok(id) => {
                self.log_memory_write_ok(execution_id, backend, "engine-episodic", &id);
            }
            Err(err) => {
                self.log_memory_write_err(execution_id, backend, "engine-episodic", &err);
            }
        }
        let keep = self.effective_config.episodic_compaction_keep();
        match store.compact_episodic_scope(MemoryScope::Session, &self.engine_session_id, keep) {
            Ok(deleted) => {
                self.log_engine_event(
                    execution_id,
                    backend,
                    "info",
                    "memory.compaction",
                    serde_json::json!({
                        "scope": "session",
                        "scope_id": self.engine_session_id.clone(),
                        "keep_latest": keep,
                        "deleted": deleted,
                        "ok": true,
                    }),
                );
                tracing::info!(
                    backend = %backend,
                    execution_id = execution_id.unwrap_or("-"),
                    session_id = %self.engine_session_id,
                    keep_latest = keep,
                    deleted,
                    "engine episodic memory compaction completed"
                );
            }
            Err(err) => {
                self.log_engine_event(
                    execution_id,
                    backend,
                    "warn",
                    "memory.compaction",
                    serde_json::json!({
                        "scope": "session",
                        "scope_id": self.engine_session_id.clone(),
                        "keep_latest": keep,
                        "ok": false,
                        "error": err.to_string(),
                    }),
                );
                tracing::warn!(
                    backend = %backend,
                    execution_id = execution_id.unwrap_or("-"),
                    session_id = %self.engine_session_id,
                    keep_latest = keep,
                    error = %err,
                    "engine episodic memory compaction failed"
                );
            }
        }
    }

    pub(super) fn log_memory_write_ok(
        &self,
        execution_id: Option<&str>,
        backend: AcpBackend,
        source: &str,
        memory_id: &str,
    ) {
        self.log_engine_event(
            execution_id,
            backend,
            "info",
            "memory.write.result",
            serde_json::json!({
                "source": source,
                "memory_id": memory_id,
                "ok": true,
            }),
        );
        tracing::info!(
            backend = %backend,
            execution_id = execution_id.unwrap_or("-"),
            session_id = %self.engine_session_id,
            memory_id = %memory_id,
            source = source,
            "engine memory write completed"
        );
    }

    pub(super) fn log_memory_write_err(
        &self,
        execution_id: Option<&str>,
        backend: AcpBackend,
        source: &str,
        err: &anyhow::Error,
    ) {
        self.log_engine_event(
            execution_id,
            backend,
            "warn",
            "memory.write.result",
            serde_json::json!({
                "source": source,
                "ok": false,
                "error": err.to_string(),
            }),
        );
        tracing::warn!(
            backend = %backend,
            execution_id = execution_id.unwrap_or("-"),
            session_id = %self.engine_session_id,
            error = %err,
            source = source,
            "engine memory write failed"
        );
    }
}

pub(super) fn is_explicit_memory_tool_prompt(prompt: &str) -> bool {
    prompt.contains("iota_memory_write")
}

pub(super) fn is_memory_persistence_intent(prompt: &str) -> bool {
    let lower = prompt.to_lowercase();
    let chinese_write_memory =
        (prompt.contains("保存") || prompt.contains("写入") || prompt.contains("记录"))
            && (prompt.contains("记忆") || prompt.contains("信息") || prompt.contains("这些"));
    prompt.contains("记住")
        || prompt.contains("持久化")
        || chinese_write_memory
        || lower.contains("remember")
        || lower.contains("save to memory")
        || lower.contains("persist")
        || lower.contains("store this")
}

pub(super) fn has_successful_memory_write(events: &[RuntimeEvent]) -> bool {
    events.iter().any(|event| match event {
        RuntimeEvent::ToolResult(result) => {
            result.ok
                && (result.name == "iota_memory_write"
                    || result.name.ends_with("__iota_memory_write"))
        }
        _ => false,
    })
}

pub(super) fn deterministic_memory_answer(prompt: &str, buckets: &RecallBuckets) -> Option<String> {
    if !is_memory_query(prompt) {
        return None;
    }
    let lower = prompt.to_lowercase();
    let mut lines = Vec::new();
    let all_info = prompt.contains("所有信息");
    if all_info {
        push_memory_lines(&mut lines, "身份", &buckets.identity);
        push_memory_lines(&mut lines, "偏好", &buckets.preference);
        push_memory_lines(&mut lines, "项目目标", &buckets.strategic);
        push_memory_lines(&mut lines, "技术事实", &buckets.domain);
        push_memory_lines(&mut lines, "实验步骤", &buckets.procedural);
        push_memory_lines(&mut lines, "历史经历", &buckets.episodic);
        return (!lines.is_empty()).then(|| lines.join("\n"));
    }
    if prompt.contains("谁") || lower.contains("who") || prompt.contains("了解") {
        push_memory_lines(&mut lines, "身份", &buckets.identity);
    }
    if prompt.contains("偏好") || prompt.contains("报告格式") || prompt.contains("语言") {
        push_memory_lines(&mut lines, "偏好", &buckets.preference);
    }
    if prompt.contains("目标") || prompt.contains("技术") || prompt.contains("实现") {
        push_memory_lines(&mut lines, "项目目标", &buckets.strategic);
        push_memory_lines(&mut lines, "技术事实", &buckets.domain);
    }
    if prompt.contains("步骤") || prompt.contains("发生") || prompt.contains("回顾") {
        push_memory_lines(&mut lines, "实验步骤", &buckets.procedural);
        push_memory_lines(&mut lines, "历史经历", &buckets.episodic);
    }
    (!lines.is_empty()).then(|| lines.join("\n"))
}

pub(super) fn is_memory_query(prompt: &str) -> bool {
    prompt.contains('？')
        || prompt.contains('?')
        || prompt.contains("请介绍")
        || prompt.contains("你知道")
        || prompt.contains("告诉我")
        || prompt.contains("回顾")
        || prompt.contains("列出")
        || prompt.contains("发生了什么")
}

fn push_memory_lines(
    lines: &mut Vec<String>,
    label: &str,
    records: &[crate::memory::MemoryRecord],
) {
    if records.is_empty() {
        return;
    }
    lines.push(format!("{}：", label));
    for record in records {
        lines.push(format!("- {}", record.content.trim()));
    }
}

pub(super) fn memory_inject_payload(
    buckets: &RecallBuckets,
    memory_chars: usize,
) -> serde_json::Value {
    // The capsule enforces this budget in tokens (see `context::tokens`), so the
    // trace has to report tokens too — a char-based `truncated` flag disagreed
    // with what was actually injected for any CJK recall.
    let memory_tokens = crate::context::tokens::chars_budget_to_tokens(memory_chars);
    let total_chars = memory_total_chars(buckets);
    let total_tokens = memory_total_tokens(buckets);
    serde_json::json!({
        "identity": memory_bucket_summary(&buckets.identity),
        "preference": memory_bucket_summary(&buckets.preference),
        "strategic": memory_bucket_summary(&buckets.strategic),
        "domain": memory_bucket_summary(&buckets.domain),
        "procedural": memory_bucket_summary(&buckets.procedural),
        "episodic": memory_bucket_summary(&buckets.episodic),
        "budget": {
            "memory_chars": memory_chars,
            "memory_tokens": memory_tokens,
            "total_chars": total_chars,
            "total_tokens": total_tokens,
            "truncated": total_tokens > memory_tokens,
            "excluded_count": excluded_memory_count(buckets, memory_tokens),
        }
    })
}

fn all_records(buckets: &RecallBuckets) -> impl Iterator<Item = &crate::memory::MemoryRecord> {
    buckets
        .identity
        .iter()
        .chain(buckets.preference.iter())
        .chain(buckets.strategic.iter())
        .chain(buckets.domain.iter())
        .chain(buckets.procedural.iter())
        .chain(buckets.episodic.iter())
}

fn memory_total_chars(buckets: &RecallBuckets) -> usize {
    all_records(buckets)
        .map(|record| record.content.chars().count())
        .sum()
}

fn memory_total_tokens(buckets: &RecallBuckets) -> usize {
    all_records(buckets)
        .map(|record| crate::context::tokens::estimate_tokens(&record.content))
        .sum()
}

fn excluded_memory_count(buckets: &RecallBuckets, memory_tokens: usize) -> usize {
    let mut used = 0usize;
    let mut excluded = 0usize;
    for record in all_records(buckets) {
        let cost = crate::context::tokens::estimate_tokens(&record.content);
        if used + cost <= memory_tokens {
            used += cost;
        } else {
            excluded += 1;
        }
    }
    excluded
}

fn memory_bucket_summary(records: &[crate::memory::MemoryRecord]) -> serde_json::Value {
    serde_json::Value::Array(
        records
            .iter()
            .map(|record| {
                serde_json::json!({
                    "id": record.id,
                    "scope": record.scope,
                    "scope_id": record.scope_id,
                    "confidence": record.confidence,
                    "content": summarize(&record.content, 180),
                })
            })
            .collect(),
    )
}
