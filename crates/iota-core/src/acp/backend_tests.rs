use crate::acp::backend::*;
use crate::acp::parser;

#[test]
fn parse_all_aliases() {
    let cases = [
        ("claude", AcpBackend::ClaudeCode),
        ("claude-code", AcpBackend::ClaudeCode),
        ("claudecode", AcpBackend::ClaudeCode),
        ("codex", AcpBackend::Codex),
        ("gemini", AcpBackend::Gemini),
        ("gemini-cli", AcpBackend::Gemini),
        ("hermes", AcpBackend::Hermes),
        ("hermes-agent", AcpBackend::Hermes),
        ("opencode", AcpBackend::OpenCode),
        ("open-code", AcpBackend::OpenCode),
    ];
    for (input, expected) in cases {
        assert_eq!(
            AcpBackend::parse(input).unwrap(),
            expected,
            "input={}",
            input
        );
    }
}

#[test]
fn parse_unknown_backend_errors() {
    assert!(AcpBackend::parse("unknown").is_err());
}

#[test]
fn display_round_trips() {
    for backend in ALL_BACKENDS {
        let text = backend.to_string();
        assert_eq!(AcpBackend::parse(&text).unwrap(), backend);
    }
}

#[test]
fn command_returns_valid_executable() {
    for backend in ALL_BACKENDS {
        let (exe, args) = backend.command();
        assert!(!exe.is_empty());
        assert!(!args.is_empty());
    }
}

#[test]
fn is_backend_alias_matches_known() {
    assert!(parser::is_backend_alias("codex"));
    assert!(parser::is_backend_alias("gemini-cli"));
    assert!(!parser::is_backend_alias("unknown-tool"));
}
