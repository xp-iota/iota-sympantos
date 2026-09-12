use crate::tui::status_bar::*;

#[test]
fn observability_status_shows_input_cache_output_tokens() {
    let status = observability_status(&ObservabilityMeta {
        input_tokens: Some(12),
        cache_tokens: Some(7),
        output_tokens: Some(8),
        ..ObservabilityMeta::default()
    })
    .unwrap();

    assert_eq!(status, "12|7|8");
}

#[test]
fn observability_status_shows_full_token_breakdown() {
    let status = observability_status(&ObservabilityMeta {
        execution_id: Some("abcdef123456".to_string()),
        total_ms: Some(1234),
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
        status,
        "1234ms · in 277 · cache r24154/w3215 · out 85 · think 32 · total 27763 · exec abcdef12"
    );
}

#[test]
fn compact_path_limits_long_paths_and_keeps_tail() {
    let compact = compact_path("/Users/han/coding/creative/iota-sympantos", 24);

    assert!(compact.chars().count() <= 24);
    assert!(compact.starts_with('…'));
    assert!(compact.ends_with("iota-sympantos"));
}

#[test]
fn observability_spans_highlight_token_part() {
    let spans = observability_spans(&ObservabilityMeta {
        total_ms: Some(123),
        input_tokens: Some(12),
        cache_tokens: Some(7),
        output_tokens: Some(8),
        ..ObservabilityMeta::default()
    });

    let token = spans
        .iter()
        .find(|span| span.content.as_ref() == "12|7|8")
        .unwrap();
    assert_eq!(token.style, theme::status_bar_token_style());
}
