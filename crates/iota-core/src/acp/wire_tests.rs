use crate::acp::wire::*;
use tokio::io::AsyncBufReadExt;

#[test]
fn parses_response_id_and_error() {
    let message =
        parse_message_line(r#"{"jsonrpc":"2.0","id":"x","result":{"ok":true}}"#, false).unwrap();
    assert!(is_response_id(&message, "x"));
    assert!(!is_response_id(&message, "y"));

    let error = parse_message_line(
        r#"{"jsonrpc":"2.0","id":"x","error":{"code":1,"message":"bad"}}"#,
        false,
    )
    .unwrap();
    let error = error.error.unwrap();
    assert_eq!(format_acp_error(&error), "ACP error 1: bad");
}

#[test]
fn numeric_id_is_matched() {
    let message = parse_message_line(r#"{"jsonrpc":"2.0","id":42,"result":{}}"#, false).unwrap();
    assert!(is_response_id(&message, "42"));
    assert!(!is_response_id(&message, "43"));
}

#[test]
fn error_without_code_omits_prefix() {
    let error = AcpWireError {
        code: None,
        message: "connection reset".to_string(),
        data: None,
    };
    assert_eq!(format_acp_error(&error), "connection reset");
}

#[test]
fn error_with_data_appends_data() {
    let error = AcpWireError {
        code: Some(500),
        message: "internal".to_string(),
        data: Some(serde_json::json!({"detail": "oops"})),
    };
    let text = format_acp_error(&error);
    assert!(text.contains("ACP error 500: internal"));
    assert!(text.contains("oops"));
}

#[test]
fn parse_rejects_non_json() {
    assert!(parse_message_line("not json", false).is_err());
}

#[test]
fn method_message_has_no_id() {
    let message =
        parse_message_line(r#"{"jsonrpc":"2.0","method":"ping","params":{}}"#, false).unwrap();
    assert!(!is_response_id(&message, "0"));
    assert_eq!(message.method.as_deref(), Some("ping"));
}

#[test]
fn null_id_does_not_match() {
    let message = parse_message_line(r#"{"jsonrpc":"2.0","id":null,"result":{}}"#, false).unwrap();
    assert!(!is_response_id(&message, "null"));
    assert!(!is_response_id(&message, ""));
}

#[test]
fn parses_message_with_all_fields() {
    let message = parse_message_line(
        r#"{"jsonrpc":"2.0","id":"a","method":"test","params":{"k":"v"},"result":{"ok":true}}"#,
        false,
    )
    .unwrap();
    assert_eq!(message.id, Some(serde_json::json!("a")));
    assert_eq!(message.method.as_deref(), Some("test"));
    assert!(message.params.is_some());
    assert!(message.result.is_some());
    assert!(message.error.is_none());
}

#[test]
fn parses_minimal_message() {
    let message = parse_message_line(r#"{"jsonrpc":"2.0"}"#, false).unwrap();
    assert!(message.id.is_none());
    assert!(message.method.is_none());
    assert!(message.params.is_none());
    assert!(message.result.is_none());
    assert!(message.error.is_none());
}

#[test]
fn error_code_zero_is_included() {
    let error = AcpWireError {
        code: Some(0),
        message: "zero".to_string(),
        data: None,
    };
    assert_eq!(format_acp_error(&error), "ACP error 0: zero");
}

#[tokio::test]
async fn read_next_line_returns_none_on_empty_stream() {
    let cursor = tokio::io::BufReader::new(std::io::Cursor::new(b""));
    let mut lines = cursor.lines();
    let result = read_next_line(&mut lines, 1000, "timeout").await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn read_next_line_returns_line() {
    let cursor = tokio::io::BufReader::new(std::io::Cursor::new(b"hello\n"));
    let mut lines = cursor.lines();
    let result = read_next_line(&mut lines, 1000, "timeout").await.unwrap();
    assert_eq!(result, Some("hello".to_string()));
}
