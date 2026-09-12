use crate::acp::permission::*;
use tokio::sync::mpsc;

#[test]
fn wildcard_star_matches_everything() {
    assert!(tool_is_whitelisted("any_tool", &["*".to_string()]));
}

#[test]
fn exact_match() {
    assert!(tool_is_whitelisted(
        "iota_memory_write",
        &["iota_memory_write".to_string()]
    ));
}

#[test]
fn prefix_wildcard() {
    assert!(tool_is_whitelisted(
        "iota_skill_run",
        &["iota_skill_*".to_string()]
    ));
}

#[test]
fn suffix_wildcard() {
    assert!(tool_is_whitelisted(
        "mcp__iota_read",
        &["*_read".to_string()]
    ));
}

#[test]
fn no_match_returns_false() {
    assert!(!tool_is_whitelisted(
        "dangerous_tool",
        &["safe_tool".to_string()]
    ));
}

#[test]
fn empty_rule_never_matches() {
    assert!(!tool_is_whitelisted("any", &["".to_string()]));
}

#[test]
fn empty_whitelist_never_matches() {
    assert!(!tool_is_whitelisted("any", &[]));
}

#[test]
fn mcp_prefixed_tool_matches_tail() {
    assert!(tool_is_whitelisted(
        "mcp__iota-context__iota_memory_write",
        &["iota_memory_write".to_string()]
    ));
}

#[test]
fn dash_underscore_canonicalization() {
    assert!(tool_is_whitelisted(
        "iota-memory-write",
        &["iota_memory_write".to_string()]
    ));
}

#[test]
fn case_insensitive_matching() {
    assert!(tool_is_whitelisted(
        "Iota_Memory_Write",
        &["iota_memory_write".to_string()]
    ));
}

#[test]
fn canonical_tool_name_normalizes() {
    assert_eq!(canonical_tool_name("Foo-Bar Baz"), "foo_barbaz");
}

#[test]
fn wildcard_match_exact() {
    assert!(wildcard_match("abc", "abc"));
    assert!(!wildcard_match("abc", "xyz"));
}

#[test]
fn wildcard_match_star_alone() {
    assert!(wildcard_match("anything", "*"));
}

#[test]
fn wildcard_match_prefix() {
    assert!(wildcard_match("iota_skill_run", "iota_skill_*"));
    assert!(!wildcard_match("other_run", "iota_skill_*"));
}

#[test]
fn wildcard_match_suffix() {
    assert!(wildcard_match("mcp__tool_read", "*_read"));
    assert!(!wildcard_match("mcp__tool_write", "*_read"));
}

#[test]
fn tool_rule_match_with_double_underscore_prefix() {
    assert!(tool_rule_match(
        "mcp__context__iota_memory_write",
        "iota_memory_write"
    ));
}

#[test]
fn multiple_rules_any_match_wins() {
    let rules = vec!["safe_tool".to_string(), "iota_*".to_string()];
    assert!(tool_is_whitelisted("iota_read", &rules));
    assert!(tool_is_whitelisted("safe_tool", &rules));
    assert!(!tool_is_whitelisted("dangerous", &rules));
}

#[tokio::test]
async fn scoped_approval_channel_is_registered_and_removed() {
    let (tx, _rx) = mpsc::channel(1);
    install_scoped_approval_channel("turn-test".to_string(), tx).await;
    assert!(
        scoped_approval_lock()
            .read()
            .await
            .contains_key("turn-test")
    );

    remove_scoped_approval_channel("turn-test").await;
    assert!(
        !scoped_approval_lock()
            .read()
            .await
            .contains_key("turn-test")
    );
}

// ---------------------------------------------------------------------------
// S-02 regression tests: spoofed / malicious tool names must not be
// auto-approved by exploiting substring, prefix, Unicode, or nested-field
// matching bugs.
// ---------------------------------------------------------------------------

#[test]
fn spoofed_name_with_iota_prefix_is_not_internal_tool() {
    // Starts with "iota_" but is not one of the actually-registered
    // internal tool names — must not be treated as internal.
    assert!(!is_internal_iota_tool_name("iota_evil_tool"));
    assert!(!is_internal_iota_tool_name("iota_memory_write_but_evil"));
}

#[test]
fn exact_internal_tool_name_is_recognized() {
    assert!(is_internal_iota_tool_name("iota_memory_write"));
    assert!(is_internal_iota_tool_name(
        "mcp__iota-context__iota_memory_write"
    ));
}

#[test]
fn internal_tool_identity_rejects_case_format_and_server_variants() {
    assert!(!is_internal_iota_tool_name("IOTA_MEMORY_WRITE"));
    assert!(!is_internal_iota_tool_name("iota-memory-write"));
    assert!(!is_internal_iota_tool_name(
        " mcp__iota-context__iota_memory_write"
    ));
    assert!(!is_internal_iota_tool_name(
        "mcp__unknown-server__iota_memory_write"
    ));
    assert!(!is_internal_iota_tool_name(
        "mcp__iota-context__nested__iota_memory_write"
    ));
}

#[test]
fn substring_containing_iota_marker_does_not_bypass_whitelist() {
    // A rule targeting a legitimate tool must not match a different tool
    // whose name merely contains the rule as a substring.
    assert!(!tool_is_whitelisted(
        "iota_memory_write_and_also_delete_everything",
        &["iota_memory_write".to_string()]
    ));
}

#[test]
fn nested_server_name_cannot_smuggle_unrelated_tail_tool() {
    // Even though the rule matches a tail exactly, a server segment that
    // itself contains "__" must not let an unrelated dangerous tool ride
    // along disguised as the tail.
    assert!(!tool_is_whitelisted(
        "mcp__iota_memory_write__delete_all_files",
        &["iota_memory_write".to_string()]
    ));
}

#[test]
fn non_ascii_confusable_rule_never_matches() {
    // Cyrillic "а" (U+0430) instead of Latin "a" in "iota_memory_write".
    let confusable_rule = "iot\u{0430}_memory_write";
    assert!(!tool_is_whitelisted(
        "iota_memory_write",
        &[confusable_rule.to_string()]
    ));
}

#[test]
fn non_ascii_confusable_tool_name_never_matches_legit_rule() {
    let confusable_tool = "iot\u{0430}_memory_write";
    assert!(!tool_is_whitelisted(
        confusable_tool,
        &["iota_memory_write".to_string()]
    ));
    assert!(!is_internal_iota_tool_name(confusable_tool));
}

#[test]
fn is_internal_iota_tool_name_rejects_non_ascii() {
    assert!(!is_internal_iota_tool_name("iot\u{0430}_memory_write"));
}
