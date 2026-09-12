use crate::acp::AcpBackend;
use crate::config::{
    BackendConfig, CommandConfig, ModelConfig, NimiaConfig, backend_process_env_with_context,
    backend_readiness,
};

#[test]
fn hermes_home_is_forwarded_to_the_acp_process() {
    let config = BackendConfig {
        home: Some("~/.hermes/profiles/iota-cockpit".to_string()),
        ..Default::default()
    };

    let env = backend_process_env_with_context(AcpBackend::Hermes, &config, None);

    assert_eq!(
        env.get("HERMES_HOME"),
        Some(&crate::config::expand_home_path("~/.hermes/profiles/iota-cockpit").unwrap())
    );
}

#[test]
fn backend_readiness_fails_for_missing_section() {
    let config = NimiaConfig::default();

    let result = backend_readiness(&config, AcpBackend::Gemini);

    assert!(!result.ok);
    assert_eq!(result.details, "missing section");
}

#[test]
fn backend_readiness_fails_for_disabled_backend() {
    let config = NimiaConfig {
        gemini: Some(gemini_config(false, "npx", Some("secret"))),
        ..Default::default()
    };

    let result = backend_readiness(&config, AcpBackend::Gemini);

    assert!(!result.ok);
    assert_eq!(result.details, "disabled");
}

#[test]
fn backend_readiness_fails_for_missing_command() {
    let config = NimiaConfig {
        gemini: Some(gemini_config(true, " ", Some("secret"))),
        ..Default::default()
    };

    let result = backend_readiness(&config, AcpBackend::Gemini);

    assert!(!result.ok);
    assert_eq!(result.details, "missing acp.command");
}

#[test]
fn backend_readiness_does_not_require_api_key() {
    let config = NimiaConfig {
        gemini: Some(gemini_config(true, "npx", Some("<api-key>"))),
        codex: Some(gemini_config(true, "npx", None)),
        ..Default::default()
    };

    let gemini = backend_readiness(&config, AcpBackend::Gemini);
    let codex = backend_readiness(&config, AcpBackend::Codex);

    assert!(gemini.ok);
    assert_eq!(gemini.details, "configured");
    assert!(codex.ok);
    assert_eq!(codex.details, "configured");
}

#[test]
fn backend_readiness_passes_for_configured_backend() {
    let config = NimiaConfig {
        gemini: Some(gemini_config(true, "npx", Some("secret"))),
        ..Default::default()
    };

    let result = backend_readiness(&config, AcpBackend::Gemini);

    assert!(result.ok);
    assert_eq!(result.details, "configured");
}

fn gemini_config(enabled: bool, command: &str, api_key: Option<&str>) -> BackendConfig {
    BackendConfig {
        enabled,
        acp: Some(CommandConfig {
            command: command.to_string(),
            args: vec![],
        }),
        model: Some(ModelConfig {
            api_key: api_key.map(str::to_string),
            ..Default::default()
        }),
        ..Default::default()
    }
}
