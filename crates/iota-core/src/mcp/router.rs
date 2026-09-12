//! ACP stream tool-call interceptor.
//!
//! When an ACP backend emits `tools/call` for an `iota_*` tool, this module
//! intercepts the call and executes it locally instead of forwarding it.
//!
//! Core tool logic lives in [`super::tool_dispatch`]; this module is
//! responsible only for:
//! 1. Detecting interceptable method names in the ACP stream.
//! 2. Opening short-lived store handles (the router has no long-lived state).
//! 3. Wrapping results in the MCP `content` envelope expected by ACP.

use anyhow::{Result, anyhow};
use serde_json::{Value, json};

#[cfg(feature = "kanban")]
use iota_kanban::SqliteKanbanStore;

use crate::memory::MemoryStore;
use crate::skill::SkillRegistry;
use crate::store::ledger::SessionLedger;

use super::tool_dispatch::{self, ToolContext};

pub fn try_intercept_tool_call(method: &str, params: Option<&Value>) -> Option<Result<Value>> {
    if !matches!(method, "tools/call" | "mcp/tools/call" | "mcp/tool_call") {
        return None;
    }
    let params = params.cloned().unwrap_or(Value::Null);
    let name = params
        .get("name")
        .or_else(|| params.get("toolName"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let arguments = params
        .get("arguments")
        .or_else(|| params.get("input"))
        .cloned()
        .unwrap_or(Value::Null);
    Some(route_tool_call(name, &arguments))
}

pub fn route_tool_call(name: &str, arguments: &Value) -> Result<Value> {
    if tool_dispatch::REGISTRY.is_known_tool(name) {
        return route_via_dispatch(name, arguments);
    }
    if is_fun_tool(name) {
        return route_fun_tool(name, arguments);
    }
    if name.starts_with("iota_") {
        return Ok(json!({
            "content":[{"type":"text","text":format!("iota tool '{}' is not routable in this context", name)}],
            "isError":true
        }));
    }
    Ok(json!({
        "content":[{"type":"text","text":format!("external MCP tool '{}' denied by iota policy", name)}],
        "isError":true
    }))
}

/// Open short-lived stores, dispatch via tool_dispatch, wrap result in MCP content envelope.
fn route_via_dispatch(name: &str, arguments: &Value) -> Result<Value> {
    let workspace = std::env::current_dir()?;
    let memory = MemoryStore::default_path()
        .ok()
        .and_then(|path| MemoryStore::open(&path).ok());
    let ledger = SessionLedger::default_path()
        .ok()
        .and_then(|path| SessionLedger::open(&path).ok());
    #[cfg(feature = "kanban")]
    let kanban = default_kanban_store();
    let skills = SkillRegistry::load(&workspace, &[]);

    let ctx = ToolContext {
        memory: memory.as_ref(),
        ledger: ledger.as_ref(),
        #[cfg(feature = "kanban")]
        kanban: kanban
            .as_ref()
            .map(|store| store as &dyn iota_kanban::KanbanStore),
        skills: &skills,
        workspace: &workspace,
    };

    match tool_dispatch::REGISTRY.dispatch(name, &ctx, arguments) {
        Ok(value) => {
            let text = serde_json::to_string(&value).unwrap_or_default();
            Ok(
                json!({"content":[{"type":"text","text":text}],"structuredContent":value,"isError":false}),
            )
        }
        Err(message) => Err(anyhow!(message)),
    }
}

fn route_fun_tool(name: &str, arguments: &Value) -> Result<Value> {
    // `run_tool_structured` so a resource-limit breach (timeout, output cap)
    // reaches the caller as a typed error rather than a bare string.
    match crate::skill::fun::run_tool_structured(name, arguments) {
        Ok(text) => Ok(json!({"content":[{"type":"text","text":text}],"isError":false})),
        Err(error) => Ok(json!({
            "content":[{"type":"text","text":error.to_string()}],
            "isError":true,
            "structuredContent": {"error_kind": error.kind.as_str()},
        })),
    }
}

fn is_fun_tool(name: &str) -> bool {
    crate::skill::fun::TOOLS
        .iter()
        .any(|(tool, _)| *tool == name)
}

#[cfg(feature = "kanban")]
fn default_kanban_store() -> Option<SqliteKanbanStore> {
    let path = crate::config::paths::kanban_db_path()?;
    SqliteKanbanStore::open(&path).ok()
}

#[cfg(test)]
#[path = "router_tests.rs"]
mod router_tests;
