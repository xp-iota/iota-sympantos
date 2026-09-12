use crate::acp::session::*;
use std::path::PathBuf;

fn server() -> AcpMcpServer {
    let mut env = BTreeMap::new();
    env.insert("TOKEN".to_string(), "redacted".to_string());
    AcpMcpServer {
        name: "iota-context".to_string(),
        command: "iota".to_string(),
        args: vec!["mcp".to_string(), "context".to_string()],
        env,
    }
}

#[test]
fn renders_gemini_mcp_servers_with_env_var_array() {
    let params = session_new_params(AcpBackend::Gemini, &PathBuf::from("."), &[server()]);
    let first = &params["mcpServers"][0];
    assert_eq!(first["name"], "iota-context");
    assert_eq!(first["type"], "stdio");
    assert_eq!(first["env"][0]["name"], "TOKEN");
    assert_eq!(first["env"][0]["value"], "redacted");
}

#[test]
fn renders_hermes_mcp_servers_with_env_var_array() {
    let params = session_new_params(AcpBackend::Hermes, &PathBuf::from("."), &[server()]);
    let first = &params["mcpServers"][0];
    assert_eq!(first["name"], "iota-context");
    assert_eq!(first["type"], "stdio");
    assert_eq!(first["env"][0]["name"], "TOKEN");
    assert_eq!(first["env"][0]["value"], "redacted");
}

#[test]
fn all_backends_render_nonempty_mcp_servers_in_session_new() {
    for backend in [
        AcpBackend::ClaudeCode,
        AcpBackend::Codex,
        AcpBackend::Gemini,
        AcpBackend::Hermes,
        AcpBackend::OpenCode,
    ] {
        let params = session_new_params(backend, &PathBuf::from("."), &[server()]);
        let servers = params["mcpServers"]
            .as_array()
            .unwrap_or_else(|| panic!("{backend} should include mcpServers"));
        assert_eq!(servers.len(), 1, "{backend} should include one MCP server");
        assert_eq!(servers[0]["name"], "iota-context");
        assert_eq!(servers[0]["type"], "stdio");
        assert_eq!(servers[0]["command"], "iota");
        assert_eq!(servers[0]["args"][0], "mcp");
        assert_eq!(servers[0]["args"][1], "context");
    }
}

#[test]
fn opencode_session_new_includes_empty_mcp_servers() {
    let params = session_new_params(AcpBackend::OpenCode, &PathBuf::from("."), &[]);
    assert_eq!(
        params["mcpServers"].as_array().map(Vec::len),
        Some(0),
        "OpenCode requires the mcpServers field even when no MCP servers are injected"
    );
}

#[test]
fn codex_session_new_includes_empty_mcp_servers() {
    let params = session_new_params(AcpBackend::Codex, &PathBuf::from("."), &[]);
    assert_eq!(
        params["mcpServers"].as_array().map(Vec::len),
        Some(0),
        "Codex requires the mcpServers field even when no MCP servers are injected"
    );
}

#[test]
fn gemini_omits_mcp_servers_when_empty() {
    let params = session_new_params(AcpBackend::Gemini, &PathBuf::from("."), &[]);
    assert!(params.get("mcpServers").is_none());
}

#[test]
fn hermes_omits_mcp_servers_when_empty() {
    let params = session_new_params(AcpBackend::Hermes, &PathBuf::from("."), &[]);
    assert!(params.get("mcpServers").is_none());
}

#[test]
fn claude_code_omits_mcp_servers_when_empty() {
    let params = session_new_params(AcpBackend::ClaudeCode, &PathBuf::from("."), &[]);
    assert!(params.get("mcpServers").is_none());
}

#[test]
fn always_send_empty_mcp_servers_option() {
    let options = AcpSessionOptions {
        always_send_empty_mcp_servers: true,
        mcp_env_shape: AcpMcpEnvShape::default(),
    };
    // Even ClaudeCode (which normally omits) should include mcpServers
    let params =
        session_new_params_with_options(AcpBackend::ClaudeCode, &PathBuf::from("."), &[], options);
    assert_eq!(params["mcpServers"].as_array().map(Vec::len), Some(0));
}

#[test]
fn cwd_is_included_in_params() {
    let params = session_new_params(AcpBackend::Gemini, &PathBuf::from("/tmp/test"), &[]);
    assert!(params["cwd"].as_str().unwrap().contains("test"));
}

#[test]
fn multiple_mcp_servers_rendered() {
    let mut env = BTreeMap::new();
    env.insert("KEY".to_string(), "val".to_string());
    let servers = vec![
        AcpMcpServer {
            name: "srv1".to_string(),
            command: "cmd1".to_string(),
            args: vec!["a".to_string()],
            env: env.clone(),
        },
        AcpMcpServer {
            name: "srv2".to_string(),
            command: "cmd2".to_string(),
            args: vec![],
            env: BTreeMap::new(),
        },
    ];
    let params = session_new_params(AcpBackend::Gemini, &PathBuf::from("."), &servers);
    let arr = params["mcpServers"].as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["name"], "srv1");
    assert_eq!(arr[1]["name"], "srv2");
    assert_eq!(arr[1]["env"].as_array().unwrap().len(), 0);
}

#[test]
fn env_shape_parse_recognizes_variants() {
    let valid = [
        "env_var_array",
        "env-var-array",
        "env_array",
        "env-array",
        "array_object",
        "array-object",
        "spec",
        "string_array",
        "string-array",
        "array",
        "object",
        "map",
    ];
    for v in valid {
        assert!(AcpMcpEnvShape::parse(v).is_some(), "should parse: {}", v);
    }
    assert!(AcpMcpEnvShape::parse("unknown").is_none());
}

#[test]
fn restore_params_include_exact_session_cwd_and_full_mcp_set() {
    let options = AcpSessionOptions {
        always_send_empty_mcp_servers: true,
        mcp_env_shape: AcpMcpEnvShape::default(),
    };
    let params = session_restore_params_with_options(
        AcpBackend::Hermes,
        "backend-session-opaque-7",
        &PathBuf::from("/workspace/project"),
        &[server()],
        options,
    );
    assert_eq!(params["sessionId"], "backend-session-opaque-7");
    assert_eq!(params["cwd"], "/workspace/project");
    assert_eq!(params["mcpServers"][0]["name"], "iota-context");
}
