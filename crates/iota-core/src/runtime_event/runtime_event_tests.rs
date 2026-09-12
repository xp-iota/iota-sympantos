use crate::runtime_event::*;
use serde_json::json;

fn first_acp_event(method: &str, params: Option<&serde_json::Value>) -> Option<RuntimeEvent> {
    map_acp_events(method, params).into_iter().next()
}

#[test]
fn maps_agent_message_to_output() {
    let event = first_acp_event(
        "session/update",
        Some(&json!({"update":{"sessionUpdate":"agent_message_chunk","content":[{"text":"hi"}]}})),
    )
    .unwrap();
    assert!(matches!(event, RuntimeEvent::Output(OutputEvent { text, .. }) if text == "hi"));
}

#[test]
fn extracts_token_usage_from_session_complete_payload() {
    let usage = token_usage_from_value(&json!({
        "model": "test-model",
        "usage": {
            "prompt_tokens": 12,
            "completion_tokens": 8
        }
    }))
    .unwrap();
    assert_eq!(usage.input_tokens, Some(12));
    assert_eq!(usage.output_tokens, Some(8));
    assert_eq!(usage.total_tokens, Some(20));
    assert_eq!(usage.model.as_deref(), Some("test-model"));
}

#[test]
fn extracts_cache_token_usage_separately() {
    let usage = token_usage_from_value(&json!({
        "usage": {
            "prompt_tokens": 19,
            "prompt_tokens_details": {
                "cached_tokens": 7
            },
            "completion_tokens": 8
        }
    }))
    .unwrap();

    assert_eq!(usage.input_tokens, Some(12));
    assert_eq!(usage.cache_tokens, Some(7));
    assert_eq!(usage.output_tokens, Some(8));
}

#[test]
fn uncached_prompt_tokens_take_precedence_for_input_tokens() {
    let usage = token_usage_from_value(&json!({
        "usage": {
            "prompt_tokens": 19,
            "uncached_prompt_tokens": 11,
            "prompt_tokens_details": {
                "cached_tokens": 7
            },
            "completion_tokens": 8
        }
    }))
    .unwrap();

    assert_eq!(usage.input_tokens, Some(11));
    assert_eq!(usage.cache_tokens, Some(7));
    assert_eq!(usage.output_tokens, Some(8));
}

#[test]
fn maps_session_complete_to_state() {
    let event = first_acp_event(
        "session/complete",
        Some(&json!({
            "model": "test-model",
            "usage": {
                "prompt_tokens": 12,
                "completion_tokens": 8
            }
        })),
    )
    .unwrap();
    assert!(matches!(event, RuntimeEvent::State(StateEvent { state, .. }) if state == "complete"));
}

#[test]
fn session_update_usage_emits_token_usage() {
    let events = map_acp_events(
        "session/update",
        Some(&json!({
            "update": {
                "sessionUpdate": "usage",
                "usage": {
                    "prompt_tokens": 19,
                    "prompt_tokens_details": {
                        "cached_tokens": 7
                    },
                    "completion_tokens": 8
                }
            }
        })),
    );

    let usage = events
        .iter()
        .find_map(|event| match event {
            RuntimeEvent::TokenUsage(usage) => Some(usage),
            _ => None,
        })
        .unwrap();

    assert_eq!(usage.input_tokens, Some(12));
    assert_eq!(usage.cache_tokens, Some(7));
    assert_eq!(usage.output_tokens, Some(8));
}

#[test]
fn normalizes_anthropic_cache_read_and_creation_tokens() {
    let usage = token_usage_from_value(&json!({
        "model": "claude-test",
        "usage": {
            "input_tokens": 277,
            "cache_read_input_tokens": 24154,
            "cache_creation_input_tokens": 3215,
            "output_tokens": 85
        }
    }))
    .unwrap();

    assert_eq!(usage.provider.as_deref(), Some("anthropic"));
    assert_eq!(usage.model.as_deref(), Some("claude-test"));
    assert_eq!(usage.source.as_deref(), Some("usage"));
    assert_eq!(usage.input_tokens, Some(277));
    assert_eq!(usage.cache_read_input_tokens, Some(24154));
    assert_eq!(usage.cache_creation_input_tokens, Some(3215));
    assert_eq!(usage.output_tokens, Some(85));
    assert_eq!(usage.normalized_total_tokens, Some(277 + 24154 + 3215 + 85));
    assert_eq!(usage.provider_reported_total_tokens, None);
    assert_eq!(
        usage.raw_payload["usage"]["cache_creation_input_tokens"],
        3215
    );
}

#[test]
fn normalizes_openai_responses_cached_and_reasoning_tokens() {
    let usage = token_usage_from_value(&json!({
        "model": "gpt-test",
        "usage": {
            "input_tokens": 100,
            "input_tokens_details": {
                "cached_tokens": 40
            },
            "output_tokens": 30,
            "output_tokens_details": {
                "reasoning_tokens": 9
            },
            "total_tokens": 130
        }
    }))
    .unwrap();

    assert_eq!(usage.provider.as_deref(), Some("openai"));
    assert_eq!(usage.input_tokens, Some(100));
    assert_eq!(usage.cache_read_input_tokens, Some(40));
    assert_eq!(usage.output_tokens, Some(30));
    assert_eq!(usage.thinking_tokens, Some(9));
    assert_eq!(usage.provider_reported_total_tokens, Some(130));
    assert_eq!(usage.normalized_total_tokens, Some(130));
}

#[test]
fn normalizes_standard_gemini_usage_metadata_without_double_counting_cache() {
    let usage = token_usage_from_value(&json!({
        "model": "gemini-test",
        "usageMetadata": {
            "promptTokenCount": 1000,
            "cachedContentTokenCount": 400,
            "candidatesTokenCount": 50,
            "thoughtsTokenCount": 20,
            "toolUsePromptTokenCount": 7,
            "totalTokenCount": 1077
        }
    }))
    .unwrap();

    assert_eq!(usage.provider.as_deref(), Some("gemini"));
    assert_eq!(usage.input_tokens, Some(1000));
    assert_eq!(usage.cache_read_input_tokens, Some(400));
    assert_eq!(usage.output_tokens, Some(50));
    assert_eq!(usage.thinking_tokens, Some(20));
    assert_eq!(usage.tool_use_prompt_tokens, Some(7));
    assert_eq!(usage.provider_reported_total_tokens, Some(1077));
    assert_eq!(usage.normalized_total_tokens, Some(1077));
}

#[test]
fn normalizes_gemini_acp_quota_token_count() {
    let usage = token_usage_from_value(&json!({
        "_meta": {
            "quota": {
                "token_count": {
                    "input_tokens": 14993,
                    "output_tokens": 36
                }
            }
        }
    }))
    .unwrap();

    assert_eq!(usage.provider.as_deref(), Some("gemini"));
    assert_eq!(usage.source.as_deref(), Some("_meta.quota.token_count"));
    assert_eq!(usage.input_tokens, Some(14993));
    assert_eq!(usage.output_tokens, Some(36));
    assert_eq!(usage.normalized_total_tokens, Some(15029));
}

#[test]
fn normalizes_gemini_acp_model_usage_as_provider_total() {
    let usage = token_usage_from_value(&json!({
        "stopReason": "end_turn",
        "_meta": {
            "quota": {
                "token_count": {
                    "input_tokens": 14983,
                    "output_tokens": 30
                },
                "model_usage": [
                    {
                        "model": "gemini-2.5-flash",
                        "token_count": {
                            "input_tokens": 14983,
                            "output_tokens": 30
                        }
                    }
                ]
            }
        }
    }))
    .unwrap();

    assert_eq!(usage.provider.as_deref(), Some("gemini"));
    assert_eq!(usage.source.as_deref(), Some("_meta.quota.token_count"));
    assert_eq!(usage.model.as_deref(), Some("gemini-2.5-flash"));
    assert_eq!(usage.input_tokens, Some(14983));
    assert_eq!(usage.output_tokens, Some(30));
    assert_eq!(usage.provider_reported_total_tokens, Some(15013));
    assert_eq!(usage.normalized_total_tokens, Some(15013));
    assert_eq!(
        usage.raw_payload["_meta"]["quota"]["model_usage"][0]["model"],
        "gemini-2.5-flash"
    );
}

#[test]
fn normalizes_adapter_thought_tokens_from_camel_case_usage() {
    let usage = token_usage_from_value(&json!({
        "usage": {
            "inputTokens": 19081,
            "outputTokens": 32,
            "thoughtTokens": 32,
            "totalTokens": 19145
        }
    }))
    .unwrap();

    assert_eq!(usage.provider.as_deref(), Some("adapter"));
    assert_eq!(usage.input_tokens, Some(19081));
    assert_eq!(usage.output_tokens, Some(32));
    assert_eq!(usage.thinking_tokens, Some(32));
    assert_eq!(usage.provider_reported_total_tokens, Some(19145));
    assert_eq!(usage.normalized_total_tokens, Some(19145));
}

#[test]
fn normalizes_codex_usage_update_used_as_adapter_total_only() {
    let events = map_acp_events(
        "session/update",
        Some(&json!({
            "sessionId": "s1",
            "update": {
                "sessionUpdate": "usage_update",
                "used": 23045,
                "size": 258400
            }
        })),
    );

    let usage = events
        .iter()
        .find_map(|event| match event {
            RuntimeEvent::TokenUsage(usage) => Some(usage),
            _ => None,
        })
        .unwrap();

    assert_eq!(usage.provider.as_deref(), Some("adapter"));
    assert_eq!(usage.source.as_deref(), Some("session_update.usage_update"));
    assert_eq!(usage.provider_reported_total_tokens, Some(23045));
    assert_eq!(usage.normalized_total_tokens, None);
    assert_eq!(usage.input_tokens, None);
    assert_eq!(usage.output_tokens, None);
}

#[test]
fn tool_call_event_is_mapped() {
    let event = first_acp_event(
        "session/update",
        Some(&json!({"update":{"sessionUpdate":"tool_call","id":"t1","name":"iota_memory_search","arguments":{"query":"rust"}}})),
    )
    .unwrap();
    assert!(
        matches!(event, RuntimeEvent::ToolCall(ToolCallEvent { name, .. }) if name == "iota_memory_search")
    );
}

#[test]
fn claude_tool_call_update_emits_real_tool_call() {
    let events = crate::runtime_event::map_acp_events(
        "session/update",
        Some(&json!({
            "update": {
                "sessionUpdate": "tool_call_update",
                "toolCallId": "call-1",
                "title": "mcp__iota-context__iota_memory_write",
                "rawInput": {
                    "type": "semantic",
                    "facet": "domain",
                    "scope": "project",
                    "scope_id": "iota-sympantos",
                    "content": "remember this",
                    "confidence": 0.91
                }
            }
        })),
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, RuntimeEvent::State(StateEvent { state, .. }) if state == "tool_call_update"))
    );
    assert!(events.iter().any(|event| {
        matches!(
            event,
            RuntimeEvent::ToolCall(ToolCallEvent { id, name, arguments })
                if id == "call-1"
                    && name == "iota_memory_write"
                    && arguments.get("scope_id").and_then(serde_json::Value::as_str) == Some("iota-sympantos")
        )
    }));
}

#[test]
fn claude_tool_call_update_emits_real_tool_result() {
    let events = crate::runtime_event::map_acp_events(
        "session/update",
        Some(&json!({
            "update": {
                "sessionUpdate": "tool_call_update",
                "toolCallId": "call-1",
                "_meta": {
                    "claudeCode": {
                        "toolName": "mcp__iota-context__iota_memory_search",
                        "toolResponse": "{\"records\":[{\"id\":\"m1\"}],\"mode\":\"hybrid\"}"
                    }
                },
                "status": "completed"
            }
        })),
    );
    assert!(events.iter().any(|event| {
        matches!(
            event,
            RuntimeEvent::ToolResult(ToolResultEvent { id, name, ok, result })
                if id == "call-1"
                    && name == "iota_memory_search"
                    && *ok
                    && result.get("records").and_then(serde_json::Value::as_array).map(Vec::len) == Some(1)
        )
    }));
}

#[test]
fn claude_failed_tool_call_update_emits_failed_tool_result() {
    let events = crate::runtime_event::map_acp_events(
        "session/update",
        Some(&json!({
            "update": {
                "sessionUpdate": "tool_call_update",
                "toolCallId": "call-1",
                "title": "mcp__iota-context__iota_memory_write",
                "rawOutput": "only semantic memory may set facet",
                "status": "failed"
            }
        })),
    );
    assert!(events.iter().any(|event| {
        matches!(
            event,
            RuntimeEvent::ToolResult(ToolResultEvent { id, name, ok, result })
                if id == "call-1"
                    && name == "iota_memory_write"
                    && !*ok
                    && result.as_str() == Some("only semantic memory may set facet")
        )
    }));
}

#[test]
fn unknown_session_update_maps_to_state() {
    let event = first_acp_event(
        "session/update",
        Some(&json!({"update":{"sessionUpdate":"thinking"}})),
    )
    .unwrap();
    assert!(matches!(event, RuntimeEvent::State(StateEvent { state, .. }) if state == "thinking"));
}

#[test]
fn error_update_maps_to_error_event() {
    let event = first_acp_event(
        "session/update",
        Some(&json!({"update":{"sessionUpdate":"error","message":"timeout","code":504}})),
    )
    .unwrap();
    assert!(
        matches!(event, RuntimeEvent::Error(ErrorEvent { message, code, .. }) if message == "timeout" && code == Some(504))
    );
}

#[test]
fn session_complete_emits_token_usage_too() {
    let events = crate::runtime_event::map_acp_events(
        "session/complete",
        Some(&json!({"model":"gpt-4o","usage":{"prompt_tokens":10,"completion_tokens":5}})),
    );
    assert_eq!(events.len(), 2);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, RuntimeEvent::TokenUsage(_)))
    );
    assert!(events.iter().any(|e| matches!(e, RuntimeEvent::State(_))));
}

#[test]
fn request_permission_maps_to_approval_request() {
    let event = first_acp_event(
        "session/request_permission",
        Some(&json!({"requestId":"req-1","toolName":"shell","command":"rm -rf /tmp/x"})),
    )
    .unwrap();
    assert!(
        matches!(event, RuntimeEvent::ApprovalRequest(ApprovalRequestEvent { id, tool_name, .. }) if id == "req-1" && tool_name == "shell")
    );
}
