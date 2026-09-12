use crate::acp::client::{agent_capabilities_from_initialize, session_restore_route};
use crate::acp::types::AcpAgentCapabilities;
use serde_json::json;

#[test]
fn parses_load_and_resume_capabilities_independently() {
    let load = agent_capabilities_from_initialize(&json!({
        "agentCapabilities": { "loadSession": true }
    }));
    assert!(load.load_session);
    assert!(!load.resume_session);

    let resume = agent_capabilities_from_initialize(&json!({
        "agentCapabilities": { "sessionCapabilities": { "resume": {} } }
    }));
    assert!(!resume.load_session);
    assert!(resume.resume_session);
}

#[test]
fn missing_restore_capabilities_fail_closed() {
    assert_eq!(
        agent_capabilities_from_initialize(&json!({"agentCapabilities": {}})),
        AcpAgentCapabilities::default()
    );
}

#[test]
fn different_active_session_routes_through_resume_without_new_process() {
    let capabilities = AcpAgentCapabilities {
        load_session: true,
        resume_session: true,
    };
    assert_eq!(
        session_restore_route(capabilities, Some("session-a"), "session-b").unwrap(),
        Some(("session/resume", "resume"))
    );
    assert_eq!(
        session_restore_route(capabilities, Some("session-a"), "session-a").unwrap(),
        None
    );
}
