//! Tests for log redaction.
//!
//! These run in one process with a shared registry, so they use distinctive
//! values rather than asserting the registry is empty.

use super::{REDACTED, redact, register_secret};

#[test]
fn registered_secret_is_redacted_in_any_surrounding_text() {
    register_secret("sk-registered-secret-value-1234");

    let line = "backend failed: request used sk-registered-secret-value-1234 as credential";
    let redacted = redact(line);

    assert!(!redacted.contains("sk-registered-secret-value-1234"));
    assert!(redacted.contains(REDACTED));
    assert!(redacted.starts_with("backend failed: request used "));
}

#[test]
fn short_values_and_placeholders_are_not_registered() {
    // Registering these would replace ordinary words with `[redacted]` and make
    // logs unreadable.
    register_secret("abc");
    register_secret("<api-key>");

    assert_eq!(redact("abc is a fine word"), "abc is a fine word");
    assert!(redact("configured api key is the <api-key> placeholder").contains("placeholder"));
}

#[test]
fn environment_style_api_keys_are_redacted_without_registration() {
    let line =
        "spawning backend with ANTHROPIC_API_KEY=sk-ant-unregistered-9999 ANTHROPIC_MODEL=opus";
    let redacted = redact(line);

    assert!(!redacted.contains("sk-ant-unregistered-9999"));
    assert!(
        redacted.contains("ANTHROPIC_MODEL=opus"),
        "non-secret env values must survive: {redacted}"
    );
}

#[test]
fn json_style_credentials_are_redacted() {
    let line = r#"{"provider":"openai","api_key":"sk-json-unregistered-4242","name":"gpt"}"#;
    let redacted = redact(line);

    assert!(!redacted.contains("sk-json-unregistered-4242"));
    assert!(redacted.contains("\"name\":\"gpt\""));
}

#[test]
fn authorization_and_cookie_headers_are_redacted() {
    let line =
        "GET /v1/messages authorization: Bearer abcdefghijklmnop cookie: session=zzzzzzzzzzzz";
    let redacted = redact(line);

    assert!(!redacted.contains("abcdefghijklmnop"));
    assert!(!redacted.contains("zzzzzzzzzzzz"));
}

#[test]
fn prose_mentioning_a_key_name_is_left_alone() {
    // "token budget" and friends appear in ordinary log messages; rewriting them
    // would hide real information.
    let line = "context token budget exceeded, trimming episodic recall";
    assert_eq!(redact(line), line);
}

#[test]
fn multibyte_log_content_survives_redaction() {
    let line = "上下文预算超限：api_key=sk-multibyte-secret-7777 已脱敏，其余内容保持不变";
    let redacted = redact(line);

    assert!(!redacted.contains("sk-multibyte-secret-7777"));
    assert!(redacted.contains("上下文预算超限"));
    assert!(redacted.contains("其余内容保持不变"));
}
