use crate::daemon::proto::*;
use crate::runtime_event::{OutputEvent, RuntimeEvent};

#[test]
fn legacy_prompt_request_still_roundtrips() {
    let request = DaemonPromptRequest {
        backend: "gemini".to_string(),
        cwd: "/tmp/project".to_string(),
        prompt: "hello".to_string(),
        execution_id: Some("exec-1".to_string()),
        timeout_ms: Some(1000),
        timing: true,
        auth_token: None,
    };

    let json = serde_json::to_string(&request).unwrap();
    assert!(!json.contains("StartTurn"));

    let decoded: DaemonPromptRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.backend, "gemini");
    assert_eq!(decoded.execution_id.as_deref(), Some("exec-1"));
}

#[test]
fn desktop_start_turn_roundtrips() {
    let message = DaemonClientMessage::StartTurn {
        turn_id: "turn-1".to_string(),
        cwd: "/tmp/project".into(),
        backend: "codex".to_string(),
        prompt: "implement feature".to_string(),
        timeout_ms: Some(600_000),
        request_id: None,
    };

    let json = serde_json::to_string(&message).unwrap();
    assert!(json.contains("\"type\":\"start_turn\""));

    let decoded: DaemonClientMessage = serde_json::from_str(&json).unwrap();
    assert!(matches!(
        decoded,
        DaemonClientMessage::StartTurn { turn_id, backend, .. }
            if turn_id == "turn-1" && backend == "codex"
    ));
}

#[test]
fn desktop_server_event_roundtrips_runtime_event() {
    let message = DaemonServerMessage::TurnEvent {
        turn_id: "turn-1".to_string(),
        event: Box::new(RuntimeEvent::Output(OutputEvent {
            text: "chunk".to_string(),
            role: Some("assistant".to_string()),
        })),
    };

    let json = serde_json::to_string(&message).unwrap();
    assert!(json.contains("\"type\":\"turn_event\""));

    let decoded: DaemonServerMessage = serde_json::from_str(&json).unwrap();
    if let DaemonServerMessage::TurnEvent { event, .. } = decoded {
        let RuntimeEvent::Output(OutputEvent { text, .. }) = *event else {
            panic!("expected Output event");
        };
        assert_eq!(text, "chunk");
        return;
    }
    panic!("decoded message did not match expected structure");
}

#[test]
fn desktop_config_snapshot_masks_api_keys() {
    let mut config = crate::config::NimiaConfig::default();
    let model = crate::config::ModelConfig {
        api_key: Some("secret-value".to_string()),
        ..Default::default()
    };
    let backend = crate::config::BackendConfig {
        enabled: true,
        model: Some(model),
        ..Default::default()
    };
    config.gemini = Some(backend);

    let snapshot = DesktopConfigSnapshot::from_config(&config);
    let json = serde_json::to_string(&snapshot).unwrap();

    assert!(!json.contains("secret-value"));
    assert!(json.contains("\"api_key_configured\":true"));
}

#[test]
fn desktop_model_update_preserves_untouched_fields() {
    let mut config = config_with_gemini_model();

    apply_desktop_model_update(
        &mut config,
        AcpBackend::Gemini,
        DesktopModelConfig {
            name: Some("gemini-2.5-flash".to_string()),
            ..Default::default()
        },
    );

    let model = config.gemini.unwrap().model.unwrap();
    assert_eq!(model.provider.as_deref(), Some("google"));
    assert_eq!(model.name.as_deref(), Some("gemini-2.5-flash"));
    assert_eq!(model.base_url.as_deref(), Some("https://example.test"));
    assert_eq!(model.api_key.as_deref(), Some("secret-value"));
}

#[test]
fn desktop_model_update_clears_blank_text_fields() {
    let mut config = config_with_gemini_model();

    apply_desktop_model_update(
        &mut config,
        AcpBackend::Gemini,
        DesktopModelConfig {
            provider: Some(" ".to_string()),
            base_url: Some(String::new()),
            ..Default::default()
        },
    );

    let model = config.gemini.unwrap().model.unwrap();
    assert_eq!(model.provider, None);
    assert_eq!(model.name.as_deref(), Some("gemini-1.5-pro"));
    assert_eq!(model.base_url, None);
    assert_eq!(model.api_key.as_deref(), Some("secret-value"));
}

#[test]
fn hello_carries_version_range_schema_and_no_secrets() {
    let hello = DaemonClientMessage::hello("new-client".to_string(), None);
    let json = serde_json::to_string(&hello).unwrap();

    // The handshake advertises the supported range and payload schema so the
    // server can negotiate instead of guessing.
    assert!(json.contains(&format!(
        "\"min_version\":{}",
        crate::daemon::PROTOCOL_VERSION_MIN
    )));
    assert!(json.contains(&format!(
        "\"max_version\":{}",
        crate::daemon::PROTOCOL_VERSION_MAX
    )));
    assert!(json.contains(&format!(
        "\"schema_version\":{}",
        crate::daemon::DESKTOP_SCHEMA_VERSION
    )));

    let decoded: DaemonClientMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, hello);
}

#[test]
fn hello_without_a_version_range_still_deserializes() {
    // An older client sends only `protocol_version`; the range fields default
    // to that single version so negotiation can still proceed.
    let json = r#"{"type":"hello","client_name":"legacy","protocol_version":3}"#;
    let decoded: DaemonClientMessage = serde_json::from_str(json).unwrap();
    match decoded {
        DaemonClientMessage::Hello {
            min_version,
            max_version,
            schema_version,
            capabilities,
            ..
        } => {
            assert_eq!(min_version, None);
            assert_eq!(max_version, None);
            assert_eq!(schema_version, None);
            assert!(capabilities.is_empty());
        }
        other => panic!("expected hello, got {other:?}"),
    }
}

#[test]
fn request_id_roundtrips_and_is_omitted_when_absent() {
    let message = DaemonClientMessage::Ping {
        seq: 7,
        request_id: Some("req-42".to_string()),
    };
    let json = serde_json::to_string(&message).unwrap();
    assert!(json.contains("\"request_id\":\"req-42\""));
    assert_eq!(message.request_id(), Some("req-42"));

    let anonymous = DaemonClientMessage::Ping {
        seq: 7,
        request_id: None,
    };
    let json = serde_json::to_string(&anonymous).unwrap();
    assert!(
        !json.contains("request_id"),
        "an absent correlation id must not be serialized"
    );
    assert_eq!(anonymous.request_id(), None);
}

#[test]
fn hello_has_no_request_id_and_reports_its_kind() {
    let hello = DaemonClientMessage::hello("c".to_string(), None);
    assert_eq!(hello.request_id(), None);
    assert_eq!(hello.kind(), "hello");
    assert_eq!(
        DaemonClientMessage::GetConfig { request_id: None }.kind(),
        "get_config"
    );
}

#[test]
fn protocol_error_carries_code_and_retryability() {
    // A version mismatch is deterministic: retrying it cannot help, so the
    // client must be told not to.
    let message = DaemonServerMessage::protocol_error(
        "unsupported version",
        DaemonErrorCode::UnsupportedVersion,
        Some("req-1"),
    );
    let json = serde_json::to_string(&message).unwrap();
    assert!(json.contains("\"code\":\"unsupported_version\""));
    assert!(json.contains("\"retryable\":false"));
    assert!(json.contains("\"request_id\":\"req-1\""));

    // A transient backend failure is worth retrying.
    let transient =
        DaemonServerMessage::protocol_error("backend down", DaemonErrorCode::BackendError, None);
    let json = serde_json::to_string(&transient).unwrap();
    assert!(json.contains("\"retryable\":true"));
}

#[test]
fn error_codes_partition_retryable_and_terminal_failures() {
    assert!(!DaemonErrorCode::UnsupportedVersion.is_retryable());
    assert!(!DaemonErrorCode::InvalidRequest.is_retryable());
    assert!(!DaemonErrorCode::Unauthenticated.is_retryable());
    assert!(!DaemonErrorCode::NotFound.is_retryable());
    assert!(!DaemonErrorCode::Cancelled.is_retryable());
    assert!(DaemonErrorCode::Unavailable.is_retryable());
    assert!(DaemonErrorCode::BackendError.is_retryable());
    assert!(DaemonErrorCode::Internal.is_retryable());
}

#[test]
fn protocol_error_without_a_code_still_deserializes() {
    // Messages produced before the code field existed must remain readable.
    let json = r#"{"type":"protocol_error","message":"old daemon"}"#;
    let decoded: DaemonServerMessage = serde_json::from_str(json).unwrap();
    match decoded {
        DaemonServerMessage::ProtocolError {
            message,
            code,
            retryable,
            request_id,
        } => {
            assert_eq!(message, "old daemon");
            assert_eq!(code, None);
            assert!(!retryable);
            assert_eq!(request_id, None);
        }
        other => panic!("expected protocol_error, got {other:?}"),
    }
}

#[test]
fn hello_accepted_with_negotiated_version() {
    let msg = DaemonServerMessage::HelloAccepted {
        protocol_version: 3,
        negotiated_version: Some(3),
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("\"negotiated_version\":3"));
}

#[test]
fn hello_accepted_without_negotiated_version_backward_compat() {
    let json = r#"{"type":"hello_accepted","protocol_version":2}"#;
    let decoded: DaemonServerMessage = serde_json::from_str(json).unwrap();
    match decoded {
        DaemonServerMessage::HelloAccepted {
            protocol_version,
            negotiated_version,
        } => {
            assert_eq!(protocol_version, 2);
            assert_eq!(negotiated_version, None);
        }
        _ => panic!("expected HelloAccepted"),
    }
}

fn config_with_gemini_model() -> crate::config::NimiaConfig {
    let model = crate::config::ModelConfig {
        provider: Some("google".to_string()),
        name: Some("gemini-1.5-pro".to_string()),
        base_url: Some("https://example.test".to_string()),
        api_key: Some("secret-value".to_string()),
    };
    let backend = crate::config::BackendConfig {
        enabled: true,
        model: Some(model),
        ..Default::default()
    };

    crate::config::NimiaConfig {
        gemini: Some(backend),
        ..Default::default()
    }
}

#[test]
fn memory_context_snapshot_request_roundtrips() {
    let message = DaemonClientMessage::GetMemoryContextSnapshot {
        cwd: PathBuf::from("/tmp/iota-workspace"),
        scope_mode: DesktopMemoryScopeMode::Workspace,
        request_id: None,
    };

    let json = serde_json::to_string(&message).unwrap();
    assert!(json.contains("\"type\":\"get_memory_context_snapshot\""));
    assert!(json.contains("\"scope_mode\":\"workspace\""));

    let decoded: DaemonClientMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, message);
}

#[test]
fn memory_context_snapshot_response_roundtrips() {
    let snapshot = DesktopMemoryContextSnapshot {
        cwd: PathBuf::from("/tmp/iota-workspace"),
        scope_mode: DesktopMemoryScopeMode::All,
        memory: DesktopMemoryBuckets::default(),
        memory_summary: DesktopMemorySummary::default(),
        runtime_context: Some(DesktopRuntimeContextSnapshot {
            turn_id: "turn-1".to_string(),
            backend: "codex".to_string(),
            cwd: PathBuf::from("/tmp/iota-workspace"),
            session_id: "session-1".to_string(),
            model: Some("model-a".to_string()),
            created_at: 123,
            capsule_text: "<iota-context>\n</iota-context>\n\nUser request:\nhello".to_string(),
            sections: vec![DesktopContextSection {
                name: "session".to_string(),
                chars: 24,
                preview: "iota_session_id: session-1".to_string(),
            }],
            budgets: DesktopContextBudgetsSnapshot::default(),
        }),
        context_engine: DesktopContextEngineSnapshot {
            enabled: true,
            memory_db: Some(PathBuf::from("/Users/example/.i6/context/memory.sqlite")),
            budgets: DesktopContextBudgetsSnapshot::default(),
        },
        errors: vec![],
    };

    let message = DaemonServerMessage::MemoryContextSnapshot { snapshot };
    let json = serde_json::to_string(&message).unwrap();
    assert!(json.contains("\"type\":\"memory_context_snapshot\""));

    let decoded: DaemonServerMessage = serde_json::from_str(&json).unwrap();
    assert!(matches!(
        decoded,
        DaemonServerMessage::MemoryContextSnapshot { .. }
    ));
}

#[test]
fn desktop_config_snapshot_never_carries_api_key_update_for_any_backend() {
    // `api_key_update` is an inbound-only field: the desktop sends it to set a
    // key and must never receive one back. Asserting it per backend keeps a new
    // backend from being added with a hand-written snapshot that leaks.
    let mut config = crate::config::NimiaConfig::default();
    let model = crate::config::ModelConfig {
        api_key: Some("outbound-secret-should-never-appear".to_string()),
        ..Default::default()
    };
    let backend = crate::config::BackendConfig {
        enabled: true,
        model: Some(model),
        ..Default::default()
    };
    config.claude_code = Some(backend.clone());
    config.codex = Some(backend.clone());
    config.gemini = Some(backend.clone());
    config.hermes = Some(backend.clone());
    config.opencode = Some(backend);

    let snapshot = DesktopConfigSnapshot::from_config(&config);
    let json = serde_json::to_string(&snapshot).unwrap();

    assert!(
        !json.contains("outbound-secret-should-never-appear"),
        "config snapshot leaked key material: {json}"
    );
    assert!(
        !json.contains("api_key_update"),
        "api_key_update must never be serialized outbound: {json}"
    );
    for backend in crate::acp::ALL_BACKENDS {
        let entry = snapshot
            .backends
            .get(&backend.to_string())
            .unwrap_or_else(|| panic!("missing snapshot for {backend}"));
        let model = entry.model.as_ref().expect("model snapshot");
        assert!(
            model.api_key_configured,
            "{backend} should report a key is set"
        );
        assert_eq!(model.api_key_update, None, "{backend} echoed a key back");
    }
}

#[test]
fn server_messages_carrying_a_config_snapshot_do_not_include_key_material() {
    let mut config = crate::config::NimiaConfig::default();
    config.gemini = Some(crate::config::BackendConfig {
        enabled: true,
        model: Some(crate::config::ModelConfig {
            api_key: Some("message-secret-should-never-appear".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    });

    let message = DaemonServerMessage::ConfigSnapshot {
        config: DesktopConfigSnapshot::from_config(&config),
    };
    let json = serde_json::to_string(&message).unwrap();

    assert!(
        !json.contains("message-secret-should-never-appear"),
        "a server message leaked key material: {json}"
    );
}
