//! `iota_handoff_publish` and `iota_handoff_read` tool implementations.

use serde_json::{Value, json};

use super::{McpTool, ToolContext};

pub(super) struct HandoffPublishTool;
impl McpTool for HandoffPublishTool {
    fn name(&self) -> &'static str {
        "iota_handoff_publish"
    }

    fn description(&self) -> &'static str {
        "Publish a handoff summary when switching backends."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "additionalProperties": true
        })
    }

    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<Value, String> {
        dispatch_handoff_publish(ctx, args)
    }
}

pub(super) struct HandoffReadTool;
impl McpTool for HandoffReadTool {
    fn name(&self) -> &'static str {
        "iota_handoff_read"
    }

    fn description(&self) -> &'static str {
        "Read the latest handoff for this session."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "additionalProperties": true
        })
    }

    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<Value, String> {
        dispatch_handoff_read(ctx, args)
    }
}

fn dispatch_handoff_publish(ctx: &ToolContext, args: &Value) -> Result<Value, String> {
    let ledger = ctx
        .ledger
        .ok_or_else(|| "session ledger is unavailable".to_string())?;
    let session_id = args
        .get("session_id")
        .and_then(Value::as_str)
        .unwrap_or("local");
    let summary = args
        .get("summary")
        .and_then(Value::as_str)
        .ok_or_else(|| "summary is required".to_string())?;
    let from_backend = args.get("from_backend").and_then(Value::as_str);
    let to_backend = args.get("to_backend").and_then(Value::as_str);
    ledger
        .publish_handoff(session_id, from_backend, to_backend, ctx.workspace, summary)
        .map(|_| json!({"ok": true}))
        .map_err(|err| err.to_string())
}

fn dispatch_handoff_read(ctx: &ToolContext, args: &Value) -> Result<Value, String> {
    let ledger = ctx
        .ledger
        .ok_or_else(|| "session ledger is unavailable".to_string())?;
    let session_id = args
        .get("session_id")
        .and_then(Value::as_str)
        .unwrap_or("local");
    let to_backend = args.get("to_backend").and_then(Value::as_str);
    ledger
        .read_handoff(session_id, to_backend, ctx.workspace)
        .map(|handoff| json!({"handoff": handoff}))
        .map_err(|err| err.to_string())
}
