use iota_core::acp::AcpBackend;
use iota_core::config::{BackendConfig, NimiaConfig};

use super::slash_command::{SlashAction, parse_slash_command};
use super::{ConversationEntry, TuiApp};

fn config_with_all_backends_enabled() -> NimiaConfig {
    NimiaConfig {
        claude_code: Some(BackendConfig {
            enabled: true,
            ..BackendConfig::default()
        }),
        codex: Some(BackendConfig {
            enabled: true,
            ..BackendConfig::default()
        }),
        gemini: Some(BackendConfig {
            enabled: true,
            ..BackendConfig::default()
        }),
        hermes: Some(BackendConfig {
            enabled: true,
            ..BackendConfig::default()
        }),
        opencode: Some(BackendConfig {
            enabled: true,
            ..BackendConfig::default()
        }),
        ..NimiaConfig::default()
    }
}

fn latest_notice(app: &TuiApp) -> Option<&str> {
    app.history
        .entries
        .iter()
        .rev()
        .find_map(|entry| match entry {
            ConversationEntry::SystemNotice { text } => Some(text.as_str()),
            _ => None,
        })
}

#[test]
fn parses_backend_command_with_aliases_for_all_five_backends() {
    let cases = [
        ("/claude", AcpBackend::ClaudeCode),
        ("/codex", AcpBackend::Codex),
        ("/gemini", AcpBackend::Gemini),
        ("/hermes", AcpBackend::Hermes),
        ("/opencode", AcpBackend::OpenCode),
        ("/backend claude-code", AcpBackend::ClaudeCode),
        ("/backend gemini-cli", AcpBackend::Gemini),
        ("/backend open-code", AcpBackend::OpenCode),
    ];

    for (input, backend) in cases {
        assert_eq!(
            parse_slash_command(input, AcpBackend::Codex)
                .unwrap()
                .action,
            SlashAction::SwitchBackend(backend),
            "input={input}"
        );
    }
}

#[test]
fn parses_local_tui_commands() {
    assert_eq!(
        parse_slash_command("/backend", AcpBackend::Codex)
            .unwrap()
            .action,
        SlashAction::ListBackends
    );
    assert_eq!(
        parse_slash_command("/?", AcpBackend::Codex).unwrap().action,
        SlashAction::Help
    );
    assert_eq!(
        parse_slash_command("/help", AcpBackend::Codex)
            .unwrap()
            .action,
        SlashAction::Help
    );
    assert_eq!(
        parse_slash_command("/clear", AcpBackend::Codex)
            .unwrap()
            .action,
        SlashAction::Clear
    );
    assert_eq!(
        parse_slash_command("/model", AcpBackend::Codex)
            .unwrap()
            .action,
        SlashAction::Model
    );
    assert_eq!(
        parse_slash_command("/quit", AcpBackend::Codex)
            .unwrap()
            .action,
        SlashAction::Quit
    );
    assert_eq!(
        parse_slash_command("/exit", AcpBackend::Codex)
            .unwrap()
            .action,
        SlashAction::Quit
    );
}

#[test]
fn non_slash_or_multiline_input_is_not_a_slash_command() {
    assert!(parse_slash_command("hello /codex", AcpBackend::Codex).is_none());
    assert!(parse_slash_command(" /codex", AcpBackend::Codex).is_none());
    assert!(parse_slash_command("/codex\nrun this", AcpBackend::Codex).is_none());
}

#[test]
fn dynamic_or_unknown_slash_command_is_submitted_to_active_backend() {
    let mut app = TuiApp::new_for_test(config_with_all_backends_enabled()).unwrap();
    app.composer.text = "/my-command arg".to_string();
    app.composer.cursor = "/my-command arg".len();

    app.submit();

    assert!(app.running_turn);
    assert!(matches!(
        app.history.entries.back(),
        Some(ConversationEntry::UserMessage { text, .. }) if text == "/my-command arg"
    ));
}

#[test]
fn slash_command_switches_backend_without_submitting_prompt() {
    let mut app = TuiApp::new_for_test(config_with_all_backends_enabled()).unwrap();
    app.active_backend = AcpBackend::Codex;
    app.composer.text = "/gemini".to_string();
    app.composer.cursor = "/gemini".len();

    app.submit();

    assert!(!app.running_turn);
    assert_eq!(app.active_backend, AcpBackend::Gemini);
    assert!(latest_notice(&app).unwrap().contains("Switched to gemini"));
}

#[test]
fn help_is_submit_to_backend_for_hermes_and_gemini() {
    // Hermes ACP: _cmd_help; Gemini ACP: HelpCommand — both handle /help natively
    for backend in [AcpBackend::Hermes, AcpBackend::Gemini] {
        let parsed = parse_slash_command("/help", backend).unwrap();
        assert_eq!(
            parsed.action,
            SlashAction::SubmitToBackend,
            "backend={backend}: /help should passthrough to ACP"
        );
    }
}

#[test]
fn help_is_local_for_claude_codex_opencode() {
    // Claude Code, Codex, OpenCode: /help is TUI-level only, not triggerable via ACP
    // → iota shows its own local help overlay instead
    for backend in [
        AcpBackend::ClaudeCode,
        AcpBackend::Codex,
        AcpBackend::OpenCode,
    ] {
        let parsed = parse_slash_command("/help", backend).unwrap();
        assert_eq!(
            parsed.action,
            SlashAction::Help,
            "backend={backend}: /help should show iota local help"
        );
    }
}

#[test]
fn compact_is_submit_to_backend_for_hermes_and_opencode() {
    // Only Hermes and OpenCode have confirmed ACP slash command handling for /compact.
    // Claude Code ACP: empirically fails (LLM explains the command instead of executing it)
    // Codex ACP: no slash handling found in app-server source
    // Gemini ACP: no CompressCommand in acpCommandHandler
    for backend in [AcpBackend::Hermes, AcpBackend::OpenCode] {
        let parsed = parse_slash_command("/compact", backend).unwrap();
        assert_eq!(
            parsed.action,
            SlashAction::SubmitToBackend,
            "backend={backend}"
        );
    }
}

#[test]
fn compact_is_not_intercepted_for_gemini_claude_codex() {
    // Gemini ACP: no CompressCommand in acpCommandHandler
    // Claude Code ACP: empirically fails — command not intercepted by ACP layer
    // Codex ACP: no slash handling found in app-server source
    for backend in [
        AcpBackend::Gemini,
        AcpBackend::ClaudeCode,
        AcpBackend::Codex,
    ] {
        assert!(
            parse_slash_command("/compact", backend).is_none(),
            "backend={backend}: /compact should not be in COMMAND_SPECS (unconfirmed ACP handling)"
        );
    }
}

#[test]
fn compact_submits_to_backend_without_local_interception() {
    // Hermes and OpenCode: confirmed ACP slash handling
    for backend in [AcpBackend::Hermes, AcpBackend::OpenCode] {
        let mut app = TuiApp::new_for_test(config_with_all_backends_enabled()).unwrap();
        app.active_backend = backend;
        app.composer.text = "/compact".to_string();
        app.composer.cursor = app.composer.text.len();

        app.submit();

        assert!(
            app.running_turn,
            "backend={backend}: /compact should start a backend turn"
        );
        assert_eq!(
            app.history
                .entries
                .back()
                .map(|e| matches!(e, ConversationEntry::UserMessage { .. })),
            Some(true),
            "backend={backend}"
        );
    }
}

#[test]
fn compress_alias_is_normalised_to_compact_before_forwarding() {
    // The alias /compress must be sent to the backend as /compact (canonical name)
    // so the backend's ACP slash handler recognises it.
    for backend in [AcpBackend::Hermes, AcpBackend::OpenCode] {
        let mut app = TuiApp::new_for_test(config_with_all_backends_enabled()).unwrap();
        app.active_backend = backend;
        app.composer.text = "/compress".to_string();
        app.composer.cursor = app.composer.text.len();

        app.submit();

        assert!(
            app.running_turn,
            "backend={backend}: /compress should start a backend turn"
        );
        // The forwarded message must use the canonical name, not the alias.
        assert!(
            matches!(
                app.history.entries.back(),
                Some(ConversationEntry::UserMessage { text, .. }) if text == "/compact"
            ),
            "backend={backend}: alias /compress must be normalised to /compact"
        );
    }
}

#[test]
fn memory_command_parses_properly() {
    let parsed = parse_slash_command("/memory", AcpBackend::Gemini).unwrap();
    assert_eq!(parsed.action, SlashAction::Memory);
    assert_eq!(parsed.name, "memory");

    let parsed_alias = parse_slash_command("/mem", AcpBackend::Gemini).unwrap();
    assert_eq!(parsed_alias.action, SlashAction::Memory);
    assert_eq!(parsed_alias.name, "memory");
}

#[test]
fn memory_command_execution_records_system_notice() {
    let mut app = TuiApp::new_for_test(config_with_all_backends_enabled()).unwrap();
    app.composer.text = "/memory".to_string();
    app.composer.cursor = "/memory".len();

    app.submit();

    assert!(!app.running_turn);
    let notice = latest_notice(&app).expect("Notice should be recorded");
    assert!(
        notice.contains("No recalled memories found")
            || notice.contains("Recalled memories")
            || notice.contains("Memory store is not initialized"),
        "notice should contain memory-related info: {}",
        notice
    );
}
