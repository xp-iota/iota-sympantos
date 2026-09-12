use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::acp::extract_text;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data")]
pub enum RuntimeEvent {
    Output(OutputEvent),
    State(StateEvent),
    Log(LogEvent),
    ToolCall(ToolCallEvent),
    ToolResult(ToolResultEvent),
    Error(ErrorEvent),
    Extension(ExtensionEvent),
    TokenUsage(TokenUsageEvent),
    Memory(MemoryEvent),
    ApprovalRequest(ApprovalRequestEvent),
    ApprovalDecision(ApprovalDecisionEvent),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputEvent {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateEvent {
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEvent {
    pub ts: i64,
    pub level: String,
    pub target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route: Option<String>,
    pub event: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ok: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    #[serde(default)]
    pub fields: Value,
}

impl LogEvent {
    pub fn new(
        level: impl Into<String>,
        target: impl Into<String>,
        event: impl Into<String>,
    ) -> Self {
        Self {
            ts: crate::utils::now_ts(),
            level: level.into(),
            target: target.into(),
            execution_id: None,
            session_id: None,
            backend: None,
            route: None,
            event: event.into(),
            tool_name: None,
            tool_call_id: None,
            ok: None,
            latency_ms: None,
            fields: Value::Object(Default::default()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallEvent {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub arguments: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultEvent {
    pub id: String,
    pub name: String,
    pub ok: bool,
    #[serde(default)]
    pub result: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorEvent {
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionEvent {
    pub name: String,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsageEvent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation_input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_prompt_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_reported_total_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalized_total_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default)]
    pub payload: Value,
    #[serde(default)]
    pub raw_payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEvent {
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_id: Option<String>,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequestEvent {
    pub id: String,
    pub tool_name: String,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalDecisionEvent {
    pub request_id: String,
    pub approved: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl RuntimeEvent {
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::Output(_) => "output",
            Self::State(_) => "state",
            Self::Log(_) => "log",
            Self::ToolCall(_) => "tool_call",
            Self::ToolResult(_) => "tool_result",
            Self::Error(_) => "error",
            Self::Extension(_) => "extension",
            Self::TokenUsage(_) => "token_usage",
            Self::Memory(_) => "memory",
            Self::ApprovalRequest(_) => "approval_request",
            Self::ApprovalDecision(_) => "approval_decision",
        }
    }
}

pub fn map_acp_events(method: &str, params: Option<&Value>) -> Vec<RuntimeEvent> {
    let Some(params) = params else {
        return Vec::new();
    };
    match method {
        "session/update" | "session_update" => map_session_update_events(params),
        "session/complete" | "session_complete" => {
            let mut events = vec![RuntimeEvent::State(StateEvent {
                state: "complete".to_string(),
                detail: Some(params.clone()),
            })];
            if let Some(usage) = token_usage_from_value(params) {
                events.push(RuntimeEvent::TokenUsage(usage));
            }
            events
        }
        "session/request_permission" | "request_permission" | "permission/request" => {
            let id = params
                .get("requestId")
                .or_else(|| params.get("id"))
                .and_then(Value::as_str)
                .unwrap_or("permission")
                .to_string();
            let tool_name = params
                .get("toolName")
                .or_else(|| params.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("tool")
                .to_string();
            vec![RuntimeEvent::ApprovalRequest(ApprovalRequestEvent {
                id,
                tool_name,
                payload: params.clone(),
            })]
        }
        other => vec![RuntimeEvent::Extension(ExtensionEvent {
            name: other.to_string(),
            payload: params.clone(),
        })],
    }
}

pub fn token_usage_from_value(value: &Value) -> Option<TokenUsageEvent> {
    let found = find_usage_object(value)?;
    let usage = found.usage;
    let provider = found
        .provider
        .or_else(|| infer_provider(usage, found.source))
        .map(str::to_string);
    let input_main = first_u64(usage, &["input_tokens", "inputTokens", "promptTokenCount"]);
    let prompt_tokens = first_u64(usage, &["prompt_tokens", "promptTokens"]);
    let cache_read_input_tokens = first_u64(
        usage,
        &[
            "cache_read_input_tokens",
            "cacheReadInputTokens",
            "cachedReadTokens",
            "cache_tokens",
            "cacheTokens",
            "cached_tokens",
            "cachedTokens",
            "cachedContentTokenCount",
        ],
    )
    .or_else(|| first_nested_u64(usage, "input_tokens_details", &["cached_tokens"]))
    .or_else(|| first_nested_u64(usage, "prompt_tokens_details", &["cached_tokens"]))
    .or_else(|| first_nested_u64(usage, "promptTokensDetails", &["cachedTokens"]));
    let cache_creation_input_tokens = first_u64(
        usage,
        &[
            "cache_creation_input_tokens",
            "cacheCreationInputTokens",
            "cachedWriteTokens",
        ],
    );
    let input_tokens = first_u64(
        usage,
        &[
            "uncached_prompt_tokens",
            "uncachedPromptTokens",
            "uncached_input_tokens",
            "uncachedInputTokens",
        ],
    )
    .or_else(|| match (prompt_tokens, cache_read_input_tokens) {
        (Some(prompt), Some(cache)) => Some(prompt.saturating_sub(cache)),
        (Some(prompt), None) => Some(prompt),
        (None, _) => input_main,
    });
    let output_tokens = first_u64(
        usage,
        &[
            "output_tokens",
            "outputTokens",
            "completion_tokens",
            "completionTokens",
            "generated_tokens",
            "candidatesTokenCount",
        ],
    );
    let thinking_tokens = first_u64(
        usage,
        &[
            "thinking_tokens",
            "thinkingTokens",
            "thought_tokens",
            "thoughtTokens",
            "thoughtsTokenCount",
            "reasoning_tokens",
            "reasoningTokens",
        ],
    )
    .or_else(|| first_nested_u64(usage, "output_tokens_details", &["reasoning_tokens"]))
    .or_else(|| first_nested_u64(usage, "completion_tokens_details", &["reasoning_tokens"]));
    let tool_use_prompt_tokens = first_u64(
        usage,
        &["tool_use_prompt_tokens", "toolUsePromptTokenCount"],
    );
    let provider_reported_total_tokens = first_u64(
        usage,
        &["total_tokens", "totalTokens", "totalTokenCount", "used"],
    )
    .or_else(|| gemini_acp_model_usage_total(value, found.source));
    let normalized_total_tokens = normalized_total_tokens(
        provider.as_deref(),
        found.source,
        input_tokens,
        cache_read_input_tokens,
        cache_creation_input_tokens,
        output_tokens,
        thinking_tokens,
        tool_use_prompt_tokens,
        provider_reported_total_tokens,
    );
    let total_tokens = provider_reported_total_tokens.or_else(|| {
        input_tokens
            .zip(output_tokens)
            .map(|(input, output)| input + output + thinking_tokens.unwrap_or(0))
    });
    if input_tokens.is_none()
        && cache_read_input_tokens.is_none()
        && cache_creation_input_tokens.is_none()
        && output_tokens.is_none()
        && thinking_tokens.is_none()
        && total_tokens.is_none()
        && provider_reported_total_tokens.is_none()
        && normalized_total_tokens.is_none()
    {
        return None;
    }
    Some(TokenUsageEvent {
        provider,
        backend: None,
        execution_id: None,
        session_id: first_string(value, &["sessionId", "session_id"]),
        source: Some(found.source.to_string()),
        input_tokens,
        cache_tokens: cache_read_input_tokens,
        cache_read_input_tokens,
        cache_creation_input_tokens,
        output_tokens,
        thinking_tokens,
        tool_use_prompt_tokens,
        total_tokens,
        provider_reported_total_tokens,
        normalized_total_tokens,
        model: first_string(value, &["model", "modelName"])
            .or_else(|| first_string(usage, &["model", "modelName"]))
            .or_else(|| gemini_acp_model_usage_model(value, found.source)),
        payload: usage.clone(),
        raw_payload: value.clone(),
    })
}

pub fn map_acp_error(message: String, code: Option<i64>, data: Option<Value>) -> RuntimeEvent {
    RuntimeEvent::Error(ErrorEvent {
        message,
        code,
        data,
    })
}

struct FoundUsage<'a> {
    usage: &'a Value,
    source: &'static str,
    provider: Option<&'static str>,
}

fn find_usage_object(value: &Value) -> Option<FoundUsage<'_>> {
    for key in ["usage", "tokenUsage", "token_usage", "tokens"] {
        if let Some(candidate) = value.get(key).filter(|candidate| candidate.is_object()) {
            return Some(FoundUsage {
                usage: candidate,
                source: key,
                provider: None,
            });
        }
    }
    if let Some(candidate) = value
        .get("usageMetadata")
        .filter(|candidate| candidate.is_object())
    {
        return Some(FoundUsage {
            usage: candidate,
            source: "usageMetadata",
            provider: Some("gemini"),
        });
    }
    if let Some(candidate) = value
        .get("_meta")
        .and_then(|meta| meta.get("quota"))
        .and_then(|quota| quota.get("token_count"))
        .filter(|candidate| candidate.is_object())
    {
        return Some(FoundUsage {
            usage: candidate,
            source: "_meta.quota.token_count",
            provider: Some("gemini"),
        });
    }
    if value
        .get("sessionUpdate")
        .or_else(|| value.get("type"))
        .and_then(Value::as_str)
        == Some("usage_update")
        && value.get("used").is_some()
    {
        return Some(FoundUsage {
            usage: value,
            source: "session_update.usage_update",
            provider: Some("adapter"),
        });
    }
    if has_any_token_key(value) {
        return Some(FoundUsage {
            usage: value,
            source: "value",
            provider: None,
        });
    }
    None
}

fn has_any_token_key(value: &Value) -> bool {
    [
        "input_tokens",
        "inputTokens",
        "prompt_tokens",
        "promptTokens",
        "cache_tokens",
        "cacheTokens",
        "cached_tokens",
        "cachedTokens",
        "cache_read_input_tokens",
        "cacheReadInputTokens",
        "output_tokens",
        "outputTokens",
        "completion_tokens",
        "completionTokens",
        "total_tokens",
        "totalTokens",
        "totalTokenCount",
        "used",
        "usageMetadata",
    ]
    .iter()
    .any(|key| value.get(*key).is_some())
}

fn first_u64(value: &Value, keys: &[&str]) -> Option<u64> {
    keys.iter().find_map(|key| {
        value.get(*key).and_then(Value::as_u64).or_else(|| {
            value
                .get(*key)
                .and_then(Value::as_i64)
                .and_then(|v| v.try_into().ok())
        })
    })
}

fn first_nested_u64(value: &Value, object_key: &str, keys: &[&str]) -> Option<u64> {
    value
        .get(object_key)
        .filter(|nested| nested.is_object())
        .and_then(|nested| first_u64(nested, keys))
}

fn first_string(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .map(str::to_string)
}

fn gemini_acp_model_usage_total(value: &Value, source: &str) -> Option<u64> {
    if source != "_meta.quota.token_count" {
        return None;
    }
    let model_usage = value
        .get("_meta")
        .and_then(|meta| meta.get("quota"))
        .and_then(|quota| quota.get("model_usage"))
        .and_then(Value::as_array)?;

    let mut total = 0_u64;
    let mut found = false;
    for entry in model_usage {
        let Some(token_count) = entry
            .get("token_count")
            .filter(|token_count| token_count.is_object())
        else {
            continue;
        };
        let entry_total = first_u64(
            token_count,
            &["total_tokens", "totalTokens", "totalTokenCount"],
        )
        .or_else(|| {
            let input = first_u64(token_count, &["input_tokens", "inputTokens"])?;
            Some(
                input
                    + first_u64(token_count, &["output_tokens", "outputTokens"]).unwrap_or(0)
                    + first_u64(
                        token_count,
                        &[
                            "thinking_tokens",
                            "thinkingTokens",
                            "thought_tokens",
                            "thoughtTokens",
                            "thoughtsTokenCount",
                        ],
                    )
                    .unwrap_or(0),
            )
        });
        if let Some(entry_total) = entry_total {
            total += entry_total;
            found = true;
        }
    }
    found.then_some(total)
}

fn gemini_acp_model_usage_model(value: &Value, source: &str) -> Option<String> {
    if source != "_meta.quota.token_count" {
        return None;
    }
    value
        .get("_meta")
        .and_then(|meta| meta.get("quota"))
        .and_then(|quota| quota.get("model_usage"))
        .and_then(Value::as_array)?
        .iter()
        .find_map(|entry| entry.get("model").and_then(Value::as_str))
        .map(str::to_string)
}

fn infer_provider(usage: &Value, source: &str) -> Option<&'static str> {
    if source == "usageMetadata" || source == "_meta.quota.token_count" {
        return Some("gemini");
    }
    if usage.get("cache_read_input_tokens").is_some()
        || usage.get("cache_creation_input_tokens").is_some()
    {
        return Some("anthropic");
    }
    if usage.get("input_tokens_details").is_some()
        || usage.get("prompt_tokens_details").is_some()
        || usage.get("output_tokens_details").is_some()
        || usage.get("completion_tokens_details").is_some()
        || usage.get("prompt_tokens").is_some()
        || usage.get("completion_tokens").is_some()
    {
        return Some("openai");
    }
    if usage.get("inputTokens").is_some()
        || usage.get("outputTokens").is_some()
        || usage.get("totalTokens").is_some()
        || usage.get("thoughtTokens").is_some()
        || usage.get("used").is_some()
    {
        return Some("adapter");
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn normalized_total_tokens(
    provider: Option<&str>,
    source: &str,
    input_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    thinking_tokens: Option<u64>,
    tool_use_prompt_tokens: Option<u64>,
    provider_reported_total_tokens: Option<u64>,
) -> Option<u64> {
    if source == "session_update.usage_update" {
        return None;
    }
    match provider {
        Some("anthropic") => Some(
            input_tokens?
                + cache_read_input_tokens.unwrap_or(0)
                + cache_creation_input_tokens.unwrap_or(0)
                + output_tokens.unwrap_or(0)
                + thinking_tokens.unwrap_or(0),
        ),
        Some("openai" | "gemini" | "adapter") => provider_reported_total_tokens.or_else(|| {
            input_tokens.map(|input| {
                input
                    + output_tokens.unwrap_or(0)
                    + thinking_tokens.unwrap_or(0)
                    + tool_use_prompt_tokens.unwrap_or(0)
            })
        }),
        _ => provider_reported_total_tokens.or_else(|| {
            input_tokens
                .map(|input| input + output_tokens.unwrap_or(0) + thinking_tokens.unwrap_or(0))
        }),
    }
}

fn map_session_update_events(params: &Value) -> Vec<RuntimeEvent> {
    let update = params.get("update").unwrap_or(params);
    let mut events = token_usage_from_value(update)
        .or_else(|| token_usage_from_value(params))
        .map(|usage| vec![RuntimeEvent::TokenUsage(usage)])
        .unwrap_or_default();
    let update_type = update
        .get("sessionUpdate")
        .or_else(|| update.get("type"))
        .and_then(Value::as_str)
        .unwrap_or("unknown");

    let mapped = match update_type {
        "agent_message" | "agent_message_chunk" => {
            extract_text(update).map_or_else(Vec::new, |text| {
                vec![RuntimeEvent::Output(OutputEvent {
                    text,
                    role: Some("assistant".to_string()),
                })]
            })
        }
        "tool_call" | "tool_use" => vec![RuntimeEvent::ToolCall(ToolCallEvent {
            id: update
                .get("id")
                .or_else(|| update.get("toolCallId"))
                .and_then(Value::as_str)
                .unwrap_or("tool-call")
                .to_string(),
            name: update
                .get("name")
                .or_else(|| update.get("toolName"))
                .and_then(Value::as_str)
                .unwrap_or("tool")
                .to_string(),
            arguments: update
                .get("arguments")
                .or_else(|| update.get("input"))
                .cloned()
                .unwrap_or(Value::Null),
        })],
        "tool_result" | "tool_output" => vec![RuntimeEvent::ToolResult(ToolResultEvent {
            id: update
                .get("id")
                .or_else(|| update.get("toolCallId"))
                .and_then(Value::as_str)
                .unwrap_or("tool-call")
                .to_string(),
            name: update
                .get("name")
                .or_else(|| update.get("toolName"))
                .and_then(Value::as_str)
                .unwrap_or("tool")
                .to_string(),
            ok: !update
                .get("isError")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            result: update
                .get("result")
                .or_else(|| update.get("content"))
                .cloned()
                .unwrap_or(Value::Null),
        })],
        "tool_call_update" => map_tool_call_update(update),
        "error" => vec![RuntimeEvent::Error(ErrorEvent {
            message: update
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("ACP session update error")
                .to_string(),
            code: update.get("code").and_then(Value::as_i64),
            data: Some(update.clone()),
        })],
        other => vec![RuntimeEvent::State(StateEvent {
            state: other.to_string(),
            detail: Some(update.clone()),
        })],
    };
    events.extend(mapped);
    events
}

fn map_tool_call_update(update: &Value) -> Vec<RuntimeEvent> {
    let mut events = vec![RuntimeEvent::State(StateEvent {
        state: "tool_call_update".to_string(),
        detail: Some(update.clone()),
    })];
    let Some(name) = tool_update_name(update) else {
        return events;
    };
    let id = update
        .get("toolCallId")
        .or_else(|| update.get("id"))
        .and_then(Value::as_str)
        .unwrap_or("tool-call")
        .to_string();
    if let Some(arguments) = tool_update_arguments(update) {
        events.push(RuntimeEvent::ToolCall(ToolCallEvent {
            id: id.clone(),
            name: name.clone(),
            arguments,
        }));
    }
    if let Some(result) = tool_update_result(update) {
        events.push(RuntimeEvent::ToolResult(ToolResultEvent {
            id,
            name,
            ok: tool_update_ok(update, &result),
            result,
        }));
    }
    events
}

fn tool_update_name(update: &Value) -> Option<String> {
    let raw = update
        .get("name")
        .or_else(|| update.get("toolName"))
        .or_else(|| update.get("title"))
        .and_then(Value::as_str)
        .or_else(|| {
            update
                .get("_meta")
                .and_then(|meta| meta.get("claudeCode"))
                .and_then(|claude| claude.get("toolName"))
                .and_then(Value::as_str)
        })?;
    Some(normalize_tool_name(raw))
}

fn normalize_tool_name(name: &str) -> String {
    name.rsplit("__").next().unwrap_or(name).to_string()
}

fn tool_update_arguments(update: &Value) -> Option<Value> {
    update
        .get("rawInput")
        .or_else(|| update.get("arguments"))
        .or_else(|| update.get("input"))
        .cloned()
}

fn tool_update_result(update: &Value) -> Option<Value> {
    update
        .get("rawOutput")
        .and_then(parse_jsonish_string)
        .or_else(|| update.get("result").cloned())
        .or_else(|| {
            tool_update_has_terminal_status(update).then(|| {
                update
                    .get("_meta")
                    .and_then(|meta| meta.get("claudeCode"))
                    .and_then(|claude| claude.get("toolResponse"))
                    .and_then(parse_jsonish_string)
            })?
        })
        .or_else(|| {
            tool_update_has_terminal_status(update).then(|| {
                update.get("content").and_then(|content| {
                    if content.as_array().map(Vec::is_empty).unwrap_or(false) {
                        None
                    } else {
                        Some(content.clone())
                    }
                })
            })?
        })
}

fn tool_update_has_terminal_status(update: &Value) -> bool {
    update
        .get("status")
        .and_then(Value::as_str)
        .map(|status| {
            status.eq_ignore_ascii_case("completed") || status.eq_ignore_ascii_case("failed")
        })
        .unwrap_or(false)
}

fn parse_jsonish_string(value: &Value) -> Option<Value> {
    let text = value.as_str()?;
    serde_json::from_str(text)
        .ok()
        .or_else(|| Some(Value::String(text.to_string())))
}

fn tool_update_ok(update: &Value, result: &Value) -> bool {
    if update
        .get("status")
        .and_then(Value::as_str)
        .map(|status| status.eq_ignore_ascii_case("failed"))
        .unwrap_or(false)
    {
        return false;
    }
    !result
        .get("isError")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

#[cfg(test)]
#[path = "runtime_event_tests.rs"]
mod tests;
