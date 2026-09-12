use crate::cli::info_cmd::*;

#[test]
fn backend_info_includes_version_mapping() {
    let config = NimiaConfig {
        codex: Some(config::BackendConfig {
            enabled: true,
            acp: Some(config::CommandConfig {
                command: "npx".to_string(),
                args: vec![
                    "-y".to_string(),
                    "@zed-industries/codex-acp@0.12.0".to_string(),
                ],
            }),
            version_mapping: Some(config::BackendVersionMapping {
                acp: Some("0.12.0".to_string()),
                bin: Some("0.128.0".to_string()),
            }),
            ..config::BackendConfig::default()
        }),
        ..NimiaConfig::default()
    };

    let value = serde_json::to_value(backend_info(&config, acp::AcpBackend::Codex)).unwrap();

    assert_eq!(value["version_mapping"]["acp"], "0.12.0");
    assert_eq!(value["version_mapping"]["bin"], "0.128.0");
}

#[test]
fn backend_info_does_not_infer_codex_bin_from_acp_adapter() {
    let config = NimiaConfig {
        codex: Some(config::BackendConfig {
            enabled: true,
            acp: Some(config::CommandConfig {
                command: "npx".to_string(),
                args: vec![
                    "-y".to_string(),
                    "@zed-industries/codex-acp@0.12.0".to_string(),
                ],
            }),
            ..config::BackendConfig::default()
        }),
        ..NimiaConfig::default()
    };

    let value = serde_json::to_value(backend_info(&config, acp::AcpBackend::Codex)).unwrap();

    assert_eq!(value["version_mapping"]["acp"], "0.12.0");
    assert!(value["version_mapping"]["bin"].is_null());
}
