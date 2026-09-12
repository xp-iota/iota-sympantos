use crate::acp::message::*;
use serde_json::json;

#[test]
fn extract_text_from_string_value() {
    assert_eq!(extract_text(&json!("hello")), Some("hello".to_string()));
}

#[test]
fn extract_text_from_text_key() {
    assert_eq!(extract_text(&json!({"text": "hi"})), Some("hi".to_string()));
}

#[test]
fn extract_text_from_content_key() {
    assert_eq!(
        extract_text(&json!({"content": "data"})),
        Some("data".to_string())
    );
}

#[test]
fn extract_text_from_message_key() {
    assert_eq!(
        extract_text(&json!({"message": "msg"})),
        Some("msg".to_string())
    );
}

#[test]
fn extract_text_from_result_key() {
    assert_eq!(
        extract_text(&json!({"result": "res"})),
        Some("res".to_string())
    );
}

#[test]
fn extract_text_from_output_key() {
    assert_eq!(
        extract_text(&json!({"output": "out"})),
        Some("out".to_string())
    );
}

#[test]
fn extract_text_from_content_object_with_text() {
    assert_eq!(
        extract_text(&json!({"content": {"text": "nested"}})),
        Some("nested".to_string())
    );
}

#[test]
fn extract_text_from_content_array() {
    let value = json!({"content": [{"type": "text", "text": "a"}, {"type": "text", "text": "b"}]});
    assert_eq!(extract_text(&value), Some("ab".to_string()));
}

#[test]
fn extract_text_from_empty_content_array_returns_none() {
    assert_eq!(extract_text(&json!({"content": []})), None);
}

#[test]
fn extract_text_from_number_returns_none() {
    assert_eq!(extract_text(&json!(42)), None);
}

#[test]
fn extract_final_text_prefers_final_message() {
    let value = json!({"finalMessage": "final", "text": "other"});
    assert_eq!(extract_final_text(&value), Some("final".to_string()));
}

#[test]
fn extract_final_text_falls_back_to_extract_text() {
    let value = json!({"text": "fallback"});
    assert_eq!(extract_final_text(&value), Some("fallback".to_string()));
}

#[test]
fn terminal_result_text_is_used_without_stream_updates() {
    let result = json!({"stopReason": "end_turn", "content": "final text"});
    assert_eq!(
        text_from_terminal_result(&result, false),
        Some("final text".to_string())
    );
}

#[test]
fn terminal_result_text_is_not_appended_after_stream_updates() {
    let result = json!({"stopReason": "end_turn", "content": "complete echoed prompt"});
    assert_eq!(text_from_terminal_result(&result, true), None);
}

#[test]
fn is_terminal_result_with_stop_reason() {
    assert!(is_terminal_result(&json!({"stopReason": "end_turn"})));
}

#[test]
fn is_terminal_result_with_text() {
    assert!(is_terminal_result(&json!({"text": "done"})));
}

#[test]
fn is_terminal_result_empty_is_false() {
    assert!(!is_terminal_result(&json!({"foo": 1})));
}

#[test]
fn text_from_agent_message() {
    let params = json!({"update": {"sessionUpdate": "agent_message", "text": "chunk"}});
    assert_eq!(
        text_from_session_update(Some(&params)),
        Some("chunk".to_string())
    );
}

#[test]
fn text_from_agent_message_chunk() {
    let params = json!({"update": {"sessionUpdate": "agent_message_chunk", "text": "c"}});
    assert_eq!(
        text_from_session_update(Some(&params)),
        Some("c".to_string())
    );
}

#[test]
fn text_from_type_field() {
    let params = json!({"update": {"type": "agent_message", "text": "t"}});
    assert_eq!(
        text_from_session_update(Some(&params)),
        Some("t".to_string())
    );
}

#[test]
fn text_from_unknown_update_type_returns_none() {
    let params = json!({"update": {"sessionUpdate": "tool_call", "text": "ignored"}});
    assert_eq!(text_from_session_update(Some(&params)), None);
}

#[test]
fn text_from_none_params_returns_none() {
    assert_eq!(text_from_session_update(None), None);
}

#[test]
fn extracts_name_and_arguments() {
    let params = json!({"name": "read_file", "arguments": {"path": "/tmp"}});
    let (name, args) = acp_tool_call_parts(Some(&params));
    assert_eq!(name, "read_file");
    assert_eq!(args, json!({"path": "/tmp"}));
}

#[test]
fn uses_tool_name_key() {
    let params = json!({"toolName": "write", "input": {"data": "x"}});
    let (name, args) = acp_tool_call_parts(Some(&params));
    assert_eq!(name, "write");
    assert_eq!(args, json!({"data": "x"}));
}

#[test]
fn defaults_when_no_params() {
    let (name, args) = acp_tool_call_parts(None);
    assert_eq!(name, "tool");
    assert!(args.is_null());
}

#[test]
fn extracts_id_from_message() {
    let msg = AcpWireMessage {
        id: Some(json!("req-1")),
        method: None,
        params: None,
        result: None,
        error: None,
    };
    assert_eq!(permission_request_id(&msg).unwrap(), json!("req-1"));
}

#[test]
fn falls_back_to_request_id_in_params() {
    let msg = AcpWireMessage {
        id: None,
        method: None,
        params: Some(json!({"requestId": "fallback-id"})),
        result: None,
        error: None,
    };
    assert_eq!(permission_request_id(&msg).unwrap(), json!("fallback-id"));
}

#[test]
fn errors_when_no_id() {
    let msg = AcpWireMessage {
        id: None,
        method: None,
        params: Some(json!({})),
        result: None,
        error: None,
    };
    assert!(permission_request_id(&msg).is_err());
}

#[test]
fn turn_cancelled_displays_and_is_error() {
    let err: Box<dyn std::error::Error> = Box::new(TurnCancelled);
    assert_eq!(err.to_string(), "turn cancelled before completion");
}

#[test]
fn turn_cancelled_downcasts_from_anyhow() {
    let err: anyhow::Error = TurnCancelled.into();
    assert!(err.downcast_ref::<TurnCancelled>().is_some());
}

#[test]
fn cancel_notification_has_no_id() {
    let notification = JsonRpcNotification {
        jsonrpc: "2.0",
        method: "session/cancel",
        params: json!({ "sessionId": "sess-1" }),
    };
    let value = serde_json::to_value(&notification).unwrap();
    assert_eq!(value["jsonrpc"], "2.0");
    assert_eq!(value["method"], "session/cancel");
    assert_eq!(value["params"]["sessionId"], "sess-1");
    // A notification must not carry an id, or the backend would try to reply.
    assert!(value.get("id").is_none());
}
