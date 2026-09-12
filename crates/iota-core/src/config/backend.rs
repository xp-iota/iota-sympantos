use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::acp::AcpBackend;

use super::NimiaConfig;
use super::context::BackendContextConfig;
use crate::config::model::ModelConfig;
use crate::config::{expand_home_path, normalize_command};

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct CommandConfig {
    pub command: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
pub struct BackendVersionMapping {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acp: Option<String>,
    #[serde(default, alias = "backend", skip_serializing_if = "Option::is_none")]
    pub bin: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct BackendConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acp: Option<CommandConfig>,
    #[serde(default, alias = "versions", skip_serializing_if = "Option::is_none")]
    pub version_mapping: Option<BackendVersionMapping>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelConfig>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_whitelist: Vec<String>,
}

#[cfg(test)]
#[path = "backend_tests.rs"]
mod tests;

fn default_enabled() -> bool {
    true
}

pub fn backend_config(config: &NimiaConfig, backend: AcpBackend) -> Option<&BackendConfig> {
    match backend {
        AcpBackend::ClaudeCode => config.claude_code.as_ref(),
        AcpBackend::Codex => config.codex.as_ref(),
        AcpBackend::Gemini => config.gemini.as_ref(),
        AcpBackend::Hermes => config.hermes.as_ref(),
        AcpBackend::OpenCode => config.opencode.as_ref(),
    }
}

pub fn command_label(command: &CommandConfig) -> String {
    if command.command.trim().is_empty() {
        return "missing command".to_string();
    }
    let mut parts = Vec::with_capacity(command.args.len() + 1);
    parts.push(command.command.clone());
    parts.extend(command.args.iter().cloned());
    parts.join(" ")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendReadiness {
    pub ok: bool,
    pub details: String,
}

pub fn backend_readiness(config: &NimiaConfig, backend: AcpBackend) -> BackendReadiness {
    let Some(section) = backend_config(config, backend) else {
        return BackendReadiness {
            ok: false,
            details: "missing section".to_string(),
        };
    };
    if !section.enabled {
        return BackendReadiness {
            ok: false,
            details: "disabled".to_string(),
        };
    }
    if section
        .acp
        .as_ref()
        .is_some_and(|acp| !acp.command.trim().is_empty())
    {
        return BackendReadiness {
            ok: true,
            details: "configured".to_string(),
        };
    }
    BackendReadiness {
        ok: false,
        details: "missing acp.command".to_string(),
    }
}

pub fn configured_model(section: &BackendConfig) -> Option<String> {
    section
        .model
        .as_ref()
        .and_then(|model| model.name.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub fn normalized_acp_command(
    backend: AcpBackend,
    section: &BackendConfig,
    acp: &CommandConfig,
) -> (String, Vec<String>) {
    let mut args = acp.args.clone();
    let adapter = super::adapters::get_adapter(backend);
    args.extend(adapter.additional_args(section));
    (normalize_command(&acp.command), args)
}

pub fn backend_process_env_with_context(
    backend: AcpBackend,
    section: &BackendConfig,
    backend_context: Option<&BackendContextConfig>,
) -> BTreeMap<String, String> {
    let adapter = super::adapters::get_adapter(backend);
    let mut process_env = adapter.process_env(section, backend_context);

    if backend_context.map(|cfg| cfg.override_home).unwrap_or(true)
        && let Some(home) = section.home.as_deref().filter(|value| !value.is_empty())
        && let Some(env_key) = adapter.home_env_key()
    {
        process_env
            .entry(env_key.to_string())
            .or_insert(expand_home_path(home).unwrap_or_else(|_| home.to_string()));
    }

    process_env
}

pub fn backend_home_env_key(backend: AcpBackend) -> Option<&'static str> {
    super::adapters::get_adapter(backend).home_env_key()
}
