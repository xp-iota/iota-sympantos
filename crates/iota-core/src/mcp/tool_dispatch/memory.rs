//! `iota_memory_search` and `iota_memory_write` tool implementations.

use serde_json::{Value, json};

use crate::memory::{MemoryInsert, MemoryMergeMode, MemorySearchMode};

use super::{
    McpTool, ToolContext, default_memory_scope_id, parse_memory_facet, parse_memory_merge_mode,
    parse_memory_scope, parse_memory_search_mode, parse_memory_type, required_confidence,
    required_string, validate_memory_shape,
};

pub(super) struct MemorySearchTool;
impl McpTool for MemorySearchTool {
    fn name(&self) -> &'static str {
        "iota_memory_search"
    }

    fn description(&self) -> &'static str {
        "Search unified iota memory by keyword. Returns matching records across all types and scopes."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Search keyword"},
                "limit": {"type": "integer", "description": "Max results (default 20)"},
                "mode": {"type": "string", "enum": ["hybrid", "vector", "keyword"], "description": "Search strategy (default hybrid)"}
            }
        })
    }

    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<Value, String> {
        dispatch_memory_search(ctx, args)
    }
}

pub(super) struct MemoryWriteTool;
impl McpTool for MemoryWriteTool {
    fn name(&self) -> &'static str {
        "iota_memory_write"
    }

    fn description(&self) -> &'static str {
        "Persist one memory record to iota's unified memory store. Classification, split, scope, and confidence policy is defined by the core skill `iota-memory-taxonomy`; load that skill before choosing memory fields. This tool enforces only the storage protocol shape."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["content", "type", "scope", "confidence"],
            "properties": {
                "content":    {"type": "string"},
                "type":       {"type": "string", "enum": ["semantic", "episodic", "procedural"]},
                "facet":      {"type": "string", "enum": ["identity", "preference", "strategic", "domain"]},
                "scope":      {"type": "string", "enum": ["user", "project", "session", "global"]},
                "scope_id":   {"type": "string"},
                "merge_mode": {"type": "string", "enum": ["auto", "add", "update", "none"]},
                "confidence": {"type": "number", "minimum": 0, "maximum": 1},
                "ttl_days":   {"type": "integer"},
                "metadata":   {"type": "object"},
                "source_backend": {"type": "string"},
                "source_session_id": {"type": "string"},
                "source_execution_id": {"type": "string"},
                "supersedes": {"type": "string"}
            },
            "allOf": [
                {
                    "if": {"properties": {"type": {"const": "semantic"}}, "required": ["type"]},
                    "then": {"required": ["facet"]}
                },
                {
                    "if": {"properties": {"type": {"enum": ["episodic", "procedural"]}}, "required": ["type"]},
                    "then": {"not": {"required": ["facet"]}}
                }
            ]
        })
    }

    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<Value, String> {
        dispatch_memory_write(ctx, args)
    }
}

fn dispatch_memory_search(ctx: &ToolContext, args: &Value) -> Result<Value, String> {
    let query = args.get("query").and_then(Value::as_str).unwrap_or("");
    let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(20) as usize;
    let mode = args
        .get("mode")
        .and_then(Value::as_str)
        .map(parse_memory_search_mode)
        .transpose()?
        .unwrap_or(MemorySearchMode::Hybrid);
    let memory = ctx
        .memory
        .ok_or_else(|| "memory store is unavailable".to_string())?;
    let records = memory
        .search_with_mode(query, limit, mode)
        .map_err(|err| err.to_string())?;
    Ok(json!({"records": records, "mode": format!("{:?}", mode).to_lowercase()}))
}

fn dispatch_memory_write(ctx: &ToolContext, args: &Value) -> Result<Value, String> {
    let memory = ctx
        .memory
        .ok_or_else(|| "memory store is unavailable".to_string())?;
    let content = args
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| "content is required".to_string())?;
    let memory_type = parse_memory_type(required_string(args, "type")?)?;
    let facet = args
        .get("facet")
        .and_then(Value::as_str)
        .map(parse_memory_facet)
        .transpose()?;
    validate_memory_shape(memory_type.clone(), facet.clone())?;
    let scope = parse_memory_scope(required_string(args, "scope")?)?;
    let scope_id = args
        .get("scope_id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| default_memory_scope_id(&scope, args, ctx.workspace));
    let confidence = required_confidence(args)?;
    let ttl_days = args.get("ttl_days").and_then(Value::as_i64).unwrap_or(7);
    let merge_mode = args
        .get("merge_mode")
        .and_then(Value::as_str)
        .map(parse_memory_merge_mode)
        .transpose()?
        .unwrap_or(MemoryMergeMode::Auto);
    let id = memory
        .insert_with_merge(
            MemoryInsert {
                memory_type,
                facet,
                scope,
                scope_id,
                content: content.to_string(),
                confidence,
                source_backend: args
                    .get("source_backend")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                source_session_id: args
                    .get("source_session_id")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                source_execution_id: args
                    .get("source_execution_id")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                metadata_json: args.get("metadata").map(Value::to_string),
                ttl_days,
                supersedes: args
                    .get("supersedes")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            },
            merge_mode,
        )
        .map_err(|err| err.to_string())?;
    Ok(json!({"id": id, "merge_mode": format!("{:?}", merge_mode).to_lowercase()}))
}
