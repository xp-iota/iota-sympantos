//! `iota_skill_search` and `iota_skill_load` tool implementations.

use serde_json::{Value, json};

use super::{McpTool, ToolContext};

pub(super) struct SkillSearchTool;
impl McpTool for SkillSearchTool {
    fn name(&self) -> &'static str {
        "iota_skill_search"
    }

    fn description(&self) -> &'static str {
        "Search available iota skill index for the current backend."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "backend": {"type": "string"}
            }
        })
    }

    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<Value, String> {
        dispatch_skill_search(ctx, args)
    }
}

pub(super) struct SkillLoadTool;
impl McpTool for SkillLoadTool {
    fn name(&self) -> &'static str {
        "iota_skill_load"
    }

    fn description(&self) -> &'static str {
        "Load the full body of a named iota skill."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["name"],
            "properties": {
                "name": {"type": "string"}
            }
        })
    }

    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<Value, String> {
        dispatch_skill_load(ctx, args)
    }
}

fn dispatch_skill_search(ctx: &ToolContext, args: &Value) -> Result<Value, String> {
    let backend = args
        .get("backend")
        .and_then(Value::as_str)
        .unwrap_or("codex");
    let backend = crate::acp::AcpBackend::parse(backend).map_err(|err| err.to_string())?;
    Ok(
        json!({"index": ctx.skills.skill_index(backend, 4000), "diagnostics": ctx.skills.diagnostics()}),
    )
}

fn dispatch_skill_load(ctx: &ToolContext, args: &Value) -> Result<Value, String> {
    let name = args
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| "name is required".to_string())?;
    let skill = ctx
        .skills
        .get(name)
        .ok_or_else(|| format!("skill '{}' not found", name))?;
    Ok(json!({"metadata": skill.metadata, "body": skill.body}))
}
