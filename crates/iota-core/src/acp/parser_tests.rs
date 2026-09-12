use crate::acp::parser::*;

#[test]
fn parses_run_flags_and_prompt_parts() {
    let cwd = std::env::current_dir().unwrap();
    let args = vec![
        "--backend".to_string(),
        "gemini".to_string(),
        "--cwd".to_string(),
        cwd.display().to_string(),
        "--show-native".to_string(),
        "--daemon".to_string(),
        "--log-events".to_string(),
        "--timing".to_string(),
        "--timeout-ms".to_string(),
        "1234".to_string(),
        "hello".to_string(),
        "world".to_string(),
    ];

    let options = parse_acp_args(&args).unwrap();

    assert_eq!(options.backend, AcpBackend::Gemini);
    assert_eq!(options.cwd, cwd);
    assert_eq!(options.prompt, "hello world");
    assert!(options.show_native);
    assert!(options.use_daemon);
    assert!(options.log_events);
    assert!(options.timing);
    assert_eq!(options.timeout_ms, 1234);
}

#[test]
fn parses_positional_backend_alias_before_prompt() {
    let options = parse_acp_args(&[
        "open-code".to_string(),
        "inspect".to_string(),
        "repo".to_string(),
    ])
    .unwrap();

    assert_eq!(options.backend, AcpBackend::OpenCode);
    assert_eq!(options.prompt, "inspect repo");
}

#[test]
fn double_dash_treats_backend_like_prompt_text() {
    let options = parse_acp_args(&[
        "codex".to_string(),
        "--".to_string(),
        "gemini".to_string(),
        "literal".to_string(),
    ])
    .unwrap();

    assert_eq!(options.backend, AcpBackend::Codex);
    assert_eq!(options.prompt, "gemini literal");
}

#[test]
fn rejects_zero_timeout() {
    let result = parse_acp_args(&[
        "--timeout-ms".to_string(),
        "0".to_string(),
        "prompt".to_string(),
    ]);

    let err = match result {
        Ok(_) => panic!("zero timeout should be rejected"),
        Err(err) => err,
    };
    assert!(err.to_string().contains("greater than 0"));
}

#[test]
fn parses_5backend_flag() {
    let options = parse_acp_args(&["5backend".to_string(), "test prompt".to_string()]).unwrap();

    assert!(options.multi_backend);
    assert_eq!(options.prompt, "test prompt");
}
