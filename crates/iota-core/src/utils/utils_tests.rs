use crate::utils::*;

#[test]
fn now_ts_is_positive() {
    assert!(now_ts() > 0);
}

#[test]
fn summarize_short_string_unchanged() {
    assert_eq!(summarize("hello world", 20), "hello world");
}

#[test]
fn summarize_truncates_and_appends_ellipsis() {
    let result = summarize("hello world foo bar", 10);
    assert!(result.ends_with("..."));
    assert!(result.len() <= 10);
}

#[test]
fn summarize_collapses_whitespace() {
    assert_eq!(summarize("hello   world", 20), "hello world");
}
