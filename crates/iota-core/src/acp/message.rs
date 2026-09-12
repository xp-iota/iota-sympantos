use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::Value;

use super::wire::AcpWireMessage;

#[derive(Debug, Serialize)]
pub(super) struct JsonRpcRequest<'a> {
    pub(super) jsonrpc: &'static str,
    pub(super) id: String,
    pub(super) method: &'a str,
    pub(super) params: Value,
}

#[derive(Debug, Serialize)]
pub(super) struct JsonRpcResponse {
    pub(super) jsonrpc: &'static str,
    pub(super) id: Value,
    pub(super) result: Value,
}

/// A JSON-RPC notification carries no `id`, so the backend does not reply to it.
/// The ACP `session/cancel` control message is sent this way.
#[derive(Debug, Serialize)]
pub(super) struct JsonRpcNotification<'a> {
    pub(super) jsonrpc: &'static str,
    pub(super) method: &'a str,
    pub(super) params: Value,
}

/// Returned when a turn is stopped early by a caller-supplied cancellation signal.
///
/// This is distinct from a timeout or a backend error: it means we deliberately
/// sent the backend a `session/cancel` notification and stopped awaiting the turn.
/// Callers (the engine) map it to [`ExecutionStatus::Cancelled`] rather than
/// `Failed` so cancellation leaves durable, honest evidence.
///
/// [`ExecutionStatus::Cancelled`]: crate::store::cache::ExecutionStatus::Cancelled
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurnCancelled;

impl std::fmt::Display for TurnCancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("turn cancelled before completion")
    }
}

impl std::error::Error for TurnCancelled {}

pub(super) fn text_from_session_update(params: Option<&Value>) -> Option<String> {
    let params = params?;
    let update = params.get("update").unwrap_or(params);
    let session_update = update
        .get("sessionUpdate")
        .or_else(|| update.get("type"))
        .and_then(Value::as_str);

    match session_update {
        Some("agent_message") | Some("agent_message_chunk") => extract_text(update),
        _ => None,
    }
}

pub(super) fn extract_final_text(value: &Value) -> Option<String> {
    value
        .get("finalMessage")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| extract_text(value))
}

pub fn extract_text(value: &Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        return Some(text.to_string());
    }

    for key in ["text", "content", "message", "result", "output"] {
        if let Some(text) = value.get(key).and_then(Value::as_str) {
            return Some(text.to_string());
        }
    }

    if let Some(content) = value.get("content").and_then(Value::as_object)
        && let Some(text) = content.get("text").and_then(Value::as_str)
    {
        return Some(text.to_string());
    }

    if let Some(content) = value.get("content").and_then(Value::as_array) {
        let text = content
            .iter()
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .collect::<String>();
        if !text.is_empty() {
            return Some(text);
        }
    }

    None
}

pub(super) fn text_from_terminal_result(result: &Value, streamed: bool) -> Option<String> {
    if streamed { None } else { extract_text(result) }
}

pub(super) fn is_terminal_result(result: &Value) -> bool {
    result.get("stopReason").and_then(Value::as_str).is_some() || extract_text(result).is_some()
}

pub(super) fn permission_request_id(message: &AcpWireMessage) -> Result<Value> {
    message
        .id
        .clone()
        .or_else(|| {
            message
                .params
                .as_ref()
                .and_then(|params| params.get("requestId").cloned())
        })
        .context("ACP permission request did not include an id or requestId")
}

pub(super) fn acp_tool_call_parts(params: Option<&Value>) -> (String, Value) {
    let params = params.unwrap_or(&Value::Null);
    let name = params
        .get("name")
        .or_else(|| params.get("toolName"))
        .and_then(Value::as_str)
        .unwrap_or("tool")
        .to_string();
    let arguments = params
        .get("arguments")
        .or_else(|| params.get("input"))
        .cloned()
        .unwrap_or(Value::Null);
    (name, arguments)
}

#[cfg(test)]
#[path = "message_tests.rs"]
mod message_tests;
