//! `iota_session_summary` tool implementation.

use serde_json::{Value, json};

use super::{McpTool, ToolContext};

pub(super) struct SessionSummaryTool;
impl McpTool for SessionSummaryTool {
    fn name(&self) -> &'static str {
        "iota_session_summary"
    }

    fn description(&self) -> &'static str {
        "Read summary of the current iota session."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "session_id": {"type": "string"}
            }
        })
    }

    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<Value, String> {
        dispatch_session_summary(ctx, args)
    }
}

fn dispatch_session_summary(ctx: &ToolContext, args: &Value) -> Result<Value, String> {
    let ledger = ctx
        .ledger
        .ok_or_else(|| "session ledger is unavailable".to_string())?;
    let session_id = args
        .get("session_id")
        .and_then(Value::as_str)
        .unwrap_or("local");
    ledger
        .summary(session_id)
        .map(|summary| {
            json!({"summary": summary.map(|s| json!({
                "iota_session_id": s.iota_session_id,
                "cwd": s.cwd,
                "active_backend": s.active_backend,
                "turn_count": s.turn_count,
                "last_output_summary": s.last_output_summary,
            }))})
        })
        .map_err(|err| err.to_string())
}
