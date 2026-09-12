use crate::mcp::client::{
    MAX_MCP_LINE_BYTES, McpToolCall, McpToolResult, read_limited_line, wait_id,
};
use serde_json::json;

#[test]
fn mcp_tool_call_serializes_correctly() {
    let call = McpToolCall {
        name: "read_file".to_string(),
        arguments: json!({"path": "/tmp/test.txt"}),
    };
    let json = serde_json::to_string(&call).unwrap();
    assert!(json.contains("\"name\":\"read_file\""));
    assert!(json.contains("\"path\":\"/tmp/test.txt\""));
}

#[test]
fn mcp_tool_call_default_arguments_is_null() {
    let call: McpToolCall = serde_json::from_str(r#"{"name":"list_tools"}"#).unwrap();
    assert_eq!(call.name, "list_tools");
    assert!(call.arguments.is_null());
}

#[test]
fn mcp_tool_result_ok_roundtrips() {
    let result = McpToolResult {
        ok: true,
        content: json!({"text": "file contents here"}),
        error: None,
    };
    let json = serde_json::to_string(&result).unwrap();
    assert!(!json.contains("error"));

    let decoded: McpToolResult = serde_json::from_str(&json).unwrap();
    assert!(decoded.ok);
    assert_eq!(decoded.content["text"], "file contents here");
    assert!(decoded.error.is_none());
}

#[test]
fn mcp_tool_result_error_roundtrips() {
    let result = McpToolResult {
        ok: false,
        content: json!(null),
        error: Some("tool not found".to_string()),
    };
    let json = serde_json::to_string(&result).unwrap();
    assert!(json.contains("\"error\":\"tool not found\""));

    let decoded: McpToolResult = serde_json::from_str(&json).unwrap();
    assert!(!decoded.ok);
    assert_eq!(decoded.error.as_deref(), Some("tool not found"));
}

#[tokio::test]
async fn mcp_session_start_fails_with_nonexistent_command() {
    use crate::mcp::client::McpSession;
    use std::collections::BTreeMap;

    let result = McpSession::start("/nonexistent/mcp-server", &[], &BTreeMap::new(), 1000).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn call_stdio_fails_with_nonexistent_command() {
    use crate::mcp::client::call_stdio;
    use std::collections::BTreeMap;

    let result = call_stdio(
        "/nonexistent/mcp-server",
        &[],
        &BTreeMap::new(),
        McpToolCall {
            name: "test".to_string(),
            arguments: json!({}),
        },
        1000,
    )
    .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn wait_id_uses_an_absolute_deadline() {
    use tokio::io::{AsyncWriteExt, BufReader, duplex};
    use tokio::time::{Duration, Instant, sleep};

    let (client, mut server) = duplex(4_096);
    let writer = tokio::spawn(async move {
        for index in 0..100_u32 {
            let line =
                format!("{{\"jsonrpc\":\"2.0\",\"id\":\"other-{index}\",\"result\":{{}}}}\n");
            if server.write_all(line.as_bytes()).await.is_err() {
                break;
            }
            sleep(Duration::from_millis(2)).await;
        }
    });
    let mut reader = BufReader::new(client);
    let started = Instant::now();
    let error = wait_id(&mut reader, "target", 30).await.unwrap_err();
    assert!(error.to_string().contains("timed out"));
    assert!(started.elapsed() < Duration::from_millis(250));
    writer.abort();
}

#[tokio::test]
async fn read_limited_line_rejects_oversized_output() {
    use tokio::io::{AsyncWriteExt, BufReader, duplex};

    let (client, mut server) = duplex(8_192);
    let writer = tokio::spawn(async move {
        let mut line = vec![b'x'; MAX_MCP_LINE_BYTES + 1];
        line.push(b'\n');
        let _ = server.write_all(&line).await;
    });
    let mut reader = BufReader::new(client);
    let error = read_limited_line(&mut reader).await.unwrap_err();
    assert!(error.to_string().contains("byte limit"));
    writer.abort();
}
