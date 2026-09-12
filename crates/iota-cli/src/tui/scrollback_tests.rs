use iota_core::acp::AcpBackend;

use crate::tui::scrollback::*;

#[test]
fn user_message_uses_solid_circle_prompt() {
    let lines = user_lines("hello", None);

    assert_eq!(lines[0].spans[0].content.as_ref(), "●  ");
}

#[test]
fn banner_wraps_logo_and_version_with_separators() {
    let lines = banner_lines();
    let text = lines[0].spans[0].content.as_ref();

    assert!(text.starts_with("│ "));
    assert!(text.ends_with(" │"));
    assert_eq!(lines[0].spans[0].style, theme::banner_style());
}

#[test]
fn claude_code_message_uses_solid_square_abbreviation() {
    let entry = ConversationEntry::AssistantMessage {
        backend: AcpBackend::ClaudeCode,
        text: "hello".into(),
        observability: None,
    };

    let lines = entry_to_lines(&entry);

    // First span is the label, second is spacing, third is content
    assert_eq!(lines[0].spans[0].content.as_ref(), "■ cc");
    assert!(lines[0].spans.len() >= 2);
}

#[test]
fn assistant_labels_use_solid_square_and_two_letter_abbreviations() {
    let cases = [
        (AcpBackend::ClaudeCode, "■ cc"),
        (AcpBackend::Codex, "■ cx"),
        (AcpBackend::Gemini, "■ gm"),
        (AcpBackend::Hermes, "■ hm"),
        (AcpBackend::OpenCode, "■ oc"),
    ];

    for (backend, expected) in cases {
        assert_eq!(assistant_label(backend), expected);
    }
}

#[test]
fn observability_line_shortens_execution_id() {
    let line = observability_line(&ObservabilityMeta {
        execution_id: Some("2255c021-eb0c-494e-b538-25b4499a2b85".into()),
        total_ms: Some(4634),
        prompt_ms: Some(4199),
        total_tokens: Some(28624),
        ..ObservabilityMeta::default()
    })
    .unwrap();

    assert_eq!(
        line,
        "2255c021: total 4634ms · prompt 4199ms · 28624 tokens"
    );
}

#[test]
fn observability_line_shows_full_token_breakdown() {
    let line = observability_line(&ObservabilityMeta {
        execution_id: Some("2255c021-eb0c-494e-b538-25b4499a2b85".into()),
        total_ms: Some(4634),
        input_tokens: Some(277),
        cache_read_input_tokens: Some(24154),
        cache_creation_input_tokens: Some(3215),
        output_tokens: Some(85),
        thinking_tokens: Some(32),
        normalized_total_tokens: Some(27763),
        ..ObservabilityMeta::default()
    })
    .unwrap();

    assert_eq!(
        line,
        "2255c021: total 4634ms · in 277 · cache r24154/w3215 · out 85 · think 32 · 27763 tokens"
    );
}
