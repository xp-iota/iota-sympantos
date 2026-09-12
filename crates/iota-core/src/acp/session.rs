use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::Path;

use crate::acp::AcpBackend;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AcpMcpEnvShape {
    #[default]
    EnvVarArray,
}

impl AcpMcpEnvShape {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "env_var_array" | "env-var-array" | "env_array" | "env-array" | "array_object"
            | "array-object" | "spec" | "string_array" | "string-array" | "array" | "object"
            | "map" => Some(Self::EnvVarArray),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AcpSessionOptions {
    pub always_send_empty_mcp_servers: bool,
    pub mcp_env_shape: AcpMcpEnvShape,
}

#[derive(Debug, Clone)]
pub struct AcpMcpServer {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
}

#[cfg(test)]
pub fn session_new_params(backend: AcpBackend, cwd: &Path, servers: &[AcpMcpServer]) -> Value {
    session_new_params_with_options(backend, cwd, servers, AcpSessionOptions::default())
}

pub fn session_new_params_with_options(
    backend: AcpBackend,
    cwd: &Path,
    servers: &[AcpMcpServer],
    options: AcpSessionOptions,
) -> Value {
    let cwd = cwd.display().to_string();
    let mcp_servers = servers
        .iter()
        .map(|server| render_mcp_server(server, options.mcp_env_shape))
        .collect::<Vec<_>>();
    let requires_mcp_servers_field = options.always_send_empty_mcp_servers
        || matches!(backend, AcpBackend::Codex | AcpBackend::OpenCode);
    if mcp_servers.is_empty() && !requires_mcp_servers_field {
        json!({ "cwd": cwd })
    } else {
        json!({ "cwd": cwd, "mcpServers": mcp_servers })
    }
}

pub fn session_restore_params_with_options(
    backend: AcpBackend,
    session_id: &str,
    cwd: &Path,
    servers: &[AcpMcpServer],
    options: AcpSessionOptions,
) -> Value {
    let mut params = session_new_params_with_options(backend, cwd, servers, options);
    if let Some(object) = params.as_object_mut() {
        object.insert(
            "sessionId".to_string(),
            Value::String(session_id.to_string()),
        );
    }
    params
}

fn render_mcp_server(server: &AcpMcpServer, env_shape: AcpMcpEnvShape) -> Value {
    let env: Value = match env_shape {
        AcpMcpEnvShape::EnvVarArray => server
            .env
            .iter()
            .map(|(key, value)| json!({ "name": key, "value": value }))
            .collect::<Vec<_>>()
            .into(),
    };
    json!({
        "name": server.name,
        "type": "stdio",
        "command": server.command,
        "args": server.args,
        "env": env,
    })
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod session_tests;
