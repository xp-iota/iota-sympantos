use crate::mcp::router::*;
use serde_json::json;

#[test]
fn denies_external_mcp_tools() {
    let result = route_tool_call("external_shell", &json!({})).unwrap();
    assert_eq!(result.get("isError").and_then(Value::as_bool), Some(true));
}

#[test]
fn denies_unknown_iota_tools() {
    let result = route_tool_call("iota_nonexistent", &json!({})).unwrap();
    assert_eq!(result.get("isError").and_then(Value::as_bool), Some(true));
}
