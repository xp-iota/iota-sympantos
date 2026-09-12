use crate::acp::*;

#[test]
fn synthetic_output_has_zero_timing() {
    let output = AcpPromptOutput::synthetic("test".to_string());
    assert_eq!(output.text, "test");
    assert_eq!(output.timing.total_ms, 0);
    assert!(!output.timing.client_started);
    assert!(output.backend_session_id.is_none());
}

#[test]
fn should_forward_backend_stderr_patterns() {
    assert!(util::should_forward_backend_stderr(
        "context MCP memory loaded"
    ));
    assert!(util::should_forward_backend_stderr(
        "iota::context::server init"
    ));
    assert!(util::should_forward_backend_stderr("[iota log] something"));
    assert!(util::should_forward_backend_stderr("[mcp stderr: x]"));
    assert!(!util::should_forward_backend_stderr("some random output"));
}

#[test]
fn elapsed_ms_is_non_negative() {
    let now = std::time::Instant::now();
    assert!(util::elapsed_ms(now) < 100);
}
