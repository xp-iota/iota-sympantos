//! Shared tool dispatch logic for iota MCP tools.
//!
//! Both the stdio MCP server (`mcp::server`) and the ACP stream interceptor
//! (`mcp::router`) delegate tool execution to this module so that parsing,
//! validation, and business logic live in exactly one place.
//!
//! Tool implementations are grouped into domain submodules to keep any
//! single file navigable:
//! - [`memory`][]: `iota_memory_search` / `iota_memory_write`.
//! - [`skill`][]: `iota_skill_search` / `iota_skill_load`.
//! - [`session`][]: `iota_session_summary`.
//! - [`handoff`][]: `iota_handoff_publish` / `iota_handoff_read`.
//! - [`kanban`][]: `iota_kanban_create_task` / `iota_kanban_list_tasks` /
//!   `iota_kanban_ready_task` (behind the `kanban` feature).
//!
//! This module itself keeps the [`ToolContext`], the [`McpTool`] trait, the
//! [`McpToolRegistry`], and the parsers/validators shared across domains.

use std::path::Path;

use serde_json::{Value, json};

#[cfg(feature = "kanban")]
use iota_kanban::KanbanStore;

use crate::memory::{
    MemoryFacet, MemoryMergeMode, MemoryScope, MemorySearchMode, MemoryStore, MemoryType,
};
use crate::skill::SkillRegistry;
use crate::store::ledger::SessionLedger;

mod handoff;
#[cfg(feature = "kanban")]
mod kanban;
mod memory;
mod session;
mod skill;

// ---------------------------------------------------------------------------
// ToolContext — injected dependencies for tool handlers
// ---------------------------------------------------------------------------

/// All external dependencies a tool handler may need, passed by the caller so
/// this module never opens databases or reads the filesystem on its own.
pub struct ToolContext<'a> {
    pub memory: Option<&'a MemoryStore>,
    pub ledger: Option<&'a SessionLedger>,
    #[cfg(feature = "kanban")]
    pub kanban: Option<&'a dyn KanbanStore>,
    pub skills: &'a SkillRegistry,
    pub workspace: &'a Path,
}

// ---------------------------------------------------------------------------
// McpTool Trait & Registry
// ---------------------------------------------------------------------------

use std::collections::HashMap;
use std::sync::LazyLock;

/// Trait defining a dynamic MCP tool.
pub trait McpTool: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn input_schema(&self) -> Value;
    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<Value, String>;
}

pub struct McpToolRegistry {
    tools: HashMap<String, Box<dyn McpTool>>,
}

impl McpToolRegistry {
    pub fn new() -> Self {
        let mut tools: HashMap<String, Box<dyn McpTool>> = HashMap::new();

        let t = memory::MemorySearchTool;
        tools.insert(t.name().to_string(), Box::new(t));
        let t = memory::MemoryWriteTool;
        tools.insert(t.name().to_string(), Box::new(t));
        let t = skill::SkillSearchTool;
        tools.insert(t.name().to_string(), Box::new(t));
        let t = skill::SkillLoadTool;
        tools.insert(t.name().to_string(), Box::new(t));
        let t = session::SessionSummaryTool;
        tools.insert(t.name().to_string(), Box::new(t));
        let t = handoff::HandoffPublishTool;
        tools.insert(t.name().to_string(), Box::new(t));
        let t = handoff::HandoffReadTool;
        tools.insert(t.name().to_string(), Box::new(t));
        #[cfg(feature = "kanban")]
        {
            let t = kanban::KanbanCreateTaskTool;
            tools.insert(t.name().to_string(), Box::new(t));
            let t = kanban::KanbanListTasksTool;
            tools.insert(t.name().to_string(), Box::new(t));
            let t = kanban::KanbanReadyTaskTool;
            tools.insert(t.name().to_string(), Box::new(t));
        }

        Self { tools }
    }

    pub fn is_known_tool(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    pub fn dispatch(&self, name: &str, ctx: &ToolContext, args: &Value) -> Result<Value, String> {
        if let Some(tool) = self.tools.get(name) {
            tool.execute(ctx, args)
        } else {
            Err(format!("unknown tool {}", name))
        }
    }

    pub fn list_tools(&self) -> Vec<Value> {
        // Sort tools by name to ensure stable list order
        let mut sorted_tools: Vec<&Box<dyn McpTool>> = self.tools.values().collect();
        sorted_tools.sort_by_key(|t| t.name());
        sorted_tools
            .into_iter()
            .map(|tool| {
                json!({
                    "name": tool.name(),
                    "description": tool.description(),
                    "inputSchema": tool.input_schema(),
                })
            })
            .collect()
    }
}

pub static REGISTRY: LazyLock<McpToolRegistry> = LazyLock::new(McpToolRegistry::new);

/// Execute a named iota tool and return the raw business-logic result.
///
/// Compatibility wrapper forwarding to the global `REGISTRY`.
#[allow(dead_code)]
pub fn dispatch_tool(ctx: &ToolContext, name: &str, args: &Value) -> Result<Value, String> {
    REGISTRY.dispatch(name, ctx, args)
}

/// Return whether `name` is a tool this module can dispatch.
///
/// Compatibility wrapper forwarding to the global `REGISTRY`.
#[allow(dead_code)]
pub fn is_known_tool(name: &str) -> bool {
    REGISTRY.is_known_tool(name)
}

// ---------------------------------------------------------------------------
// Parsers & validators (single canonical copy, shared across domains)
// ---------------------------------------------------------------------------

pub(super) fn parse_memory_type(value: &str) -> Result<MemoryType, String> {
    match value {
        "semantic" => Ok(MemoryType::Semantic),
        "episodic" => Ok(MemoryType::Episodic),
        "procedural" => Ok(MemoryType::Procedural),
        other => Err(format!("invalid memory type {}", other)),
    }
}

pub(super) fn parse_memory_facet(value: &str) -> Result<MemoryFacet, String> {
    match value {
        "identity" => Ok(MemoryFacet::Identity),
        "preference" => Ok(MemoryFacet::Preference),
        "strategic" => Ok(MemoryFacet::Strategic),
        "domain" => Ok(MemoryFacet::Domain),
        other => Err(format!("invalid memory facet {}", other)),
    }
}

pub(super) fn parse_memory_scope(value: &str) -> Result<MemoryScope, String> {
    match value {
        "session" => Ok(MemoryScope::Session),
        "project" => Ok(MemoryScope::Project),
        "user" => Ok(MemoryScope::User),
        "global" => Ok(MemoryScope::Global),
        other => Err(format!("invalid memory scope {}", other)),
    }
}

pub(super) fn parse_memory_merge_mode(value: &str) -> Result<MemoryMergeMode, String> {
    match value {
        "auto" => Ok(MemoryMergeMode::Auto),
        "add" => Ok(MemoryMergeMode::Add),
        "update" => Ok(MemoryMergeMode::Update),
        "none" => Ok(MemoryMergeMode::None),
        other => Err(format!("invalid memory merge_mode {}", other)),
    }
}

pub(super) fn parse_memory_search_mode(value: &str) -> Result<MemorySearchMode, String> {
    match value {
        "keyword" => Ok(MemorySearchMode::Keyword),
        "vector" => Ok(MemorySearchMode::Vector),
        "hybrid" => Ok(MemorySearchMode::Hybrid),
        other => Err(format!("invalid memory search mode {}", other)),
    }
}

pub(super) fn validate_memory_shape(
    memory_type: MemoryType,
    facet: Option<MemoryFacet>,
) -> Result<(), String> {
    if memory_type == MemoryType::Semantic && facet.is_none() {
        return Err("semantic memory requires a facet".to_string());
    }
    if memory_type != MemoryType::Semantic && facet.is_some() {
        return Err("only semantic memory may set facet".to_string());
    }
    Ok(())
}

pub(super) fn required_string<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{} is required", key))
}

pub(super) fn required_confidence(args: &Value) -> Result<f64, String> {
    let confidence = args
        .get("confidence")
        .and_then(value_as_f64)
        .ok_or_else(|| "confidence is required".to_string())?;
    if !(0.0..=1.0).contains(&confidence) {
        return Err("confidence must be between 0 and 1".to_string());
    }
    Ok(confidence)
}

pub(super) fn value_as_f64(value: &Value) -> Option<f64> {
    value.as_f64().or_else(|| {
        value
            .as_str()
            .and_then(|raw| raw.trim().parse::<f64>().ok())
    })
}

pub(super) fn default_memory_scope_id(
    scope: &MemoryScope,
    args: &Value,
    workspace: &Path,
) -> String {
    match scope {
        MemoryScope::User => "local-user".to_string(),
        MemoryScope::Project => workspace.display().to_string(),
        MemoryScope::Session => args
            .get("source_session_id")
            .or_else(|| args.get("session_id"))
            .and_then(Value::as_str)
            .unwrap_or("local")
            .to_string(),
        MemoryScope::Global => "global".to_string(),
    }
}

#[cfg(test)]
#[path = "../tool_dispatch_tests.rs"]
mod tool_dispatch_tests;
