use crate::acp::AcpBackend;
use crate::config::*;

#[test]
fn mcp_servers_default_to_enabled_for_all_backends() {
    let config = NimiaConfig {
        context_engine: Some(ContextEngineConfig::default()),
        ..NimiaConfig::default()
    };
    for backend in [
        AcpBackend::ClaudeCode,
        AcpBackend::Codex,
        AcpBackend::Gemini,
        AcpBackend::Hermes,
        AcpBackend::OpenCode,
    ] {
        assert_eq!(
            context_mcp_servers(&config, backend).len(),
            2,
            "{backend} should enable MCP servers by default"
        );
    }
}

#[test]
fn all_backends_can_disable_mcp_servers() {
    let config = NimiaConfig {
        context_engine: Some(ContextEngineConfig::default()),
        context_engine_backend: Some(ContextEngineBackendConfig {
            claude_code: Some(BackendContextConfig {
                mcp_session_new: Some(false),
                ..BackendContextConfig::default()
            }),
            codex: Some(BackendContextConfig {
                mcp_session_new: Some(false),
                ..BackendContextConfig::default()
            }),
            gemini: Some(BackendContextConfig {
                mcp_session_new: Some(false),
                ..BackendContextConfig::default()
            }),
            hermes: Some(BackendContextConfig {
                mcp_session_new: Some(false),
                ..BackendContextConfig::default()
            }),
            opencode: Some(BackendContextConfig {
                mcp_session_new: Some(false),
                ..BackendContextConfig::default()
            }),
        }),
        ..NimiaConfig::default()
    };

    for backend in [
        AcpBackend::ClaudeCode,
        AcpBackend::Codex,
        AcpBackend::Gemini,
        AcpBackend::Hermes,
        AcpBackend::OpenCode,
    ] {
        assert_eq!(
            context_mcp_servers(&config, backend).len(),
            0,
            "{backend} should allow MCP servers to be disabled"
        );
    }
}

#[test]
fn context_mcp_server_enables_memory_route_logging() {
    let config = NimiaConfig::default();
    let servers = context_mcp_servers(&config, AcpBackend::Gemini);
    let context = servers
        .iter()
        .find(|server| server.name == "iota-context")
        .expect("iota-context server should be present");

    assert_eq!(
        context.env.get("RUST_LOG").map(String::as_str),
        Some("iota::context::server=info")
    );
}

#[test]
fn iota_mcp_command_alias_uses_iota_cli_path() {
    assert_eq!(
        context::resolve_iota_command_with_env("iota", Some("/tmp/current-iota".to_string())),
        "/tmp/current-iota"
    );
    assert_eq!(context::resolve_iota_command_with_env("iota", None), "iota");
    assert_eq!(
        context::resolve_iota_command_with_env(
            "/usr/bin/custom-iota",
            Some("/tmp/iota".to_string())
        ),
        "/usr/bin/custom-iota"
    );
}

#[test]
fn mcp_session_new_rejects_non_boolean_values() {
    let err = serde_yaml::from_str::<NimiaConfig>(
        r#"
context_engine_backend:
  codex:
    mcp_session_new: try
"#,
    )
    .unwrap_err();

    assert!(err.to_string().contains("invalid type"));
}

#[test]
fn recall_thresholds_default_values() {
    let config = NimiaConfig::default();
    let thresholds = context_recall_thresholds(&config);
    assert_eq!(thresholds.identity, 0.85);
    assert_eq!(thresholds.preference, 0.80);
    assert_eq!(thresholds.strategic, 0.80);
    assert_eq!(thresholds.domain, 0.80);
    assert_eq!(thresholds.procedural, 0.85);
    assert_eq!(thresholds.episodic, 0.80);
}

#[test]
fn episodic_compaction_keep_default_is_40() {
    let config = NimiaConfig::default();
    assert_eq!(context_episodic_compaction_keep(&config), 40);
}

#[test]
fn normalize_command_adds_cmd_suffix_on_windows() {
    let result = normalize_command("npx");
    if cfg!(windows) {
        assert_eq!(result, "npx.cmd");
    } else {
        assert_eq!(result, "npx");
    }
}

#[test]
fn expand_home_path_leaves_non_tilde_unchanged() {
    assert_eq!(
        expand_home_path("/absolute/path").unwrap(),
        "/absolute/path"
    );
    assert_eq!(expand_home_path("relative").unwrap(), "relative");
}

#[test]
fn context_injection_deserializes_normalized_values() {
    let injection: ContextInjection = serde_yaml::from_str("\" McP \"").unwrap();
    assert_eq!(injection, ContextInjection::Mcp);
    assert_eq!(serde_yaml::to_string(&injection).unwrap(), "mcp\n");
}

#[test]
fn context_injection_rejects_unknown_values() {
    let err = serde_yaml::from_str::<ContextInjection>("\"maybe\"").unwrap_err();
    assert!(err.to_string().contains("invalid context_engine.injection"));
}

#[test]
fn backend_version_mapping_deserializes_from_nimia() {
    let section: BackendConfig = serde_yaml::from_str(
        r#"
enabled: true
version_mapping:
    acp: "0.12.0"
    bin: "0.128.0"
"#,
    )
    .unwrap();

    assert_eq!(
        section.version_mapping,
        Some(BackendVersionMapping {
            acp: Some("0.12.0".to_string()),
            bin: Some("0.128.0".to_string()),
        })
    );
}

#[test]
fn store_paths_join_expected_database_names() {
    let root = std::path::PathBuf::from("root").join("context");
    let paths = paths::StorePaths::new(root.clone());
    assert_eq!(paths.events_db(), root.join("events.sqlite"));
    assert_eq!(paths.memory_db(), root.join("memory.sqlite"));
    assert_eq!(paths.store_db(), root.join("store.sqlite"));
}

#[test]
fn hermes_without_model_config_preserves_its_own_environment() {
    let missing_model = BackendConfig::default();
    let empty_model = BackendConfig {
        model: Some(ModelConfig::default()),
        ..BackendConfig::default()
    };

    assert!(
        get_adapter(AcpBackend::Hermes)
            .process_env(&missing_model, None)
            .is_empty()
    );
    assert!(
        get_adapter(AcpBackend::Hermes)
            .process_env(&empty_model, None)
            .is_empty()
    );
}

#[test]
fn hermes_explicit_model_config_maps_provider_environment() {
    let section = BackendConfig {
        model: Some(ModelConfig {
            provider: Some("anthropic".to_string()),
            name: Some("claude-sonnet".to_string()),
            base_url: Some("https://api.anthropic.com".to_string()),
            api_key: Some("test-key".to_string()),
        }),
        ..BackendConfig::default()
    };

    let env = get_adapter(AcpBackend::Hermes).process_env(&section, None);
    assert_eq!(
        env.get("HERMES_INFERENCE_PROVIDER").map(String::as_str),
        Some("anthropic")
    );
    assert_eq!(
        env.get("HERMES_MODEL").map(String::as_str),
        Some("claude-sonnet")
    );
    assert_eq!(
        env.get("ANTHROPIC_BASE_URL").map(String::as_str),
        Some("https://api.anthropic.com")
    );
    assert_eq!(
        env.get("ANTHROPIC_API_KEY").map(String::as_str),
        Some("test-key")
    );
}

#[test]
fn no_backend_can_leak_its_api_key_into_a_log_line() {
    // Adapters differ in whether they pass the key through the environment at
    // all (OpenCode reads its own config, for instance). The invariant is
    // therefore not "every backend sets a key env var" but "any backend that
    // does has registered it for redaction" — which is what fails when a new
    // adapter is added with a hand-written `process_env`.
    let mut backends_passing_the_key = 0;
    for (index, backend) in crate::acp::ALL_BACKENDS.iter().enumerate() {
        let secret = format!("sk-redaction-probe-{index}-abcdefgh");
        let section = BackendConfig {
            enabled: true,
            model: Some(ModelConfig {
                api_key: Some(secret.clone()),
                provider: Some("minimax".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };

        let env = get_adapter(*backend).process_env(&section, None);
        let rendered = format!("{env:?}");
        if rendered.contains(&secret) {
            backends_passing_the_key += 1;
        }

        let redacted = crate::telemetry::redact::redact(&rendered);
        assert!(
            !redacted.contains(&secret),
            "{backend} did not register its api key for redaction: {redacted}"
        );
    }
    assert!(
        backends_passing_the_key >= 3,
        "expected most backends to receive the key through the environment, got {backends_passing_the_key}"
    );
}
