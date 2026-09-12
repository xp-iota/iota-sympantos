use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::acp::AcpBackend;
use crate::acp::session::{AcpMcpEnvShape, AcpMcpServer, AcpSessionOptions};

use super::{CommandConfig, NimiaConfig, backend_config, expand_home_path, normalize_command};

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct EmbeddingConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ContextEngineConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_context_injection")]
    pub injection: ContextInjection,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_db: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skill_roots: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budgets: Option<ContextBudgetsConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recall_thresholds: Option<RecallThresholdsConfig>,
    #[serde(default = "default_episodic_compaction_keep")]
    pub episodic_compaction_keep: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp: Option<CommandConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fun: Option<CommandConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding: Option<EmbeddingConfig>,
}

impl Default for ContextEngineConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            injection: default_context_injection(),
            memory_db: None,
            skill_roots: Vec::new(),
            budgets: None,
            recall_thresholds: None,
            episodic_compaction_keep: default_episodic_compaction_keep(),
            mcp: None,
            fun: None,
            embedding: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ContextInjection {
    #[default]
    Auto,
    Off,
    Prompt,
    Mcp,
}

impl ContextInjection {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Auto => "auto",
            Self::Off => "off",
            Self::Prompt => "prompt",
            Self::Mcp => "mcp",
        }
    }

    pub fn is_off(&self) -> bool {
        matches!(self, Self::Off)
    }

    /// Whether the context fabric should be embedded in the ACP prompt.
    ///
    /// `Mcp` deliberately keeps the context service available as tools while
    /// avoiding a second copy of the same material in every model request.
    pub fn injects_prompt(&self) -> bool {
        matches!(self, Self::Auto | Self::Prompt)
    }
}

impl Serialize for ContextInjection {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ContextInjection {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Self::Auto,
            "off" => Self::Off,
            "prompt" => Self::Prompt,
            "mcp" => Self::Mcp,
            _ => {
                return Err(serde::de::Error::custom(format!(
                    "invalid context_engine.injection '{}'; expected auto, off, prompt, or mcp",
                    value
                )));
            }
        })
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub struct ContextBudgetsConfig {
    #[serde(default = "default_memory_chars")]
    pub memory_chars: usize,
    #[serde(default = "default_skills_chars")]
    pub skills_chars: usize,
    #[serde(default = "default_working_memory_chars", alias = "dialogue_chars")]
    pub working_memory_chars: usize,
    #[serde(default = "default_workspace_chars")]
    pub workspace_chars: usize,
    #[serde(default = "default_handoff_chars")]
    pub handoff_chars: usize,
}

impl Default for ContextBudgetsConfig {
    fn default() -> Self {
        Self {
            memory_chars: default_memory_chars(),
            skills_chars: default_skills_chars(),
            working_memory_chars: default_working_memory_chars(),
            workspace_chars: default_workspace_chars(),
            handoff_chars: default_handoff_chars(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub struct RecallThresholdsConfig {
    #[serde(default = "default_identity_threshold")]
    pub identity: f64,
    #[serde(default = "default_preference_threshold")]
    pub preference: f64,
    #[serde(default = "default_strategic_threshold")]
    pub strategic: f64,
    #[serde(default = "default_domain_threshold")]
    pub domain: f64,
    #[serde(default = "default_procedural_threshold")]
    pub procedural: f64,
    #[serde(default = "default_episodic_threshold")]
    pub episodic: f64,
}

impl Default for RecallThresholdsConfig {
    fn default() -> Self {
        Self {
            identity: default_identity_threshold(),
            preference: default_preference_threshold(),
            strategic: default_strategic_threshold(),
            domain: default_domain_threshold(),
            procedural: default_procedural_threshold(),
            episodic: default_episodic_threshold(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ContextEngineBackendConfig {
    #[serde(default)]
    pub gemini: Option<BackendContextConfig>,
    #[serde(default)]
    pub opencode: Option<BackendContextConfig>,
    #[serde(default)]
    pub hermes: Option<BackendContextConfig>,
    #[serde(default, rename = "claude-code")]
    pub claude_code: Option<BackendContextConfig>,
    #[serde(default)]
    pub codex: Option<BackendContextConfig>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct BackendContextConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_session_new: Option<bool>,
    #[serde(default)]
    pub always_send_empty_mcp_servers: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_env_shape: Option<String>,
    #[serde(default)]
    pub override_home: bool,
}

pub fn context_memory_db_path(config: &NimiaConfig) -> Result<PathBuf> {
    if let Some(path) = config
        .context_engine
        .as_ref()
        .and_then(|cfg| cfg.memory_db.as_deref())
        .filter(|value| !value.trim().is_empty())
    {
        return Ok(PathBuf::from(expand_home_path(path)?));
    }
    Ok(super::paths::StorePaths::resolve()?.memory_db())
}

pub fn context_skill_roots(config: &NimiaConfig) -> Vec<PathBuf> {
    config
        .context_engine
        .as_ref()
        .map(|cfg| {
            cfg.skill_roots
                .iter()
                .filter_map(|root| expand_home_path(root).ok().map(PathBuf::from))
                .collect()
        })
        .unwrap_or_default()
}

pub fn context_mcp_servers(config: &NimiaConfig, backend: AcpBackend) -> Vec<AcpMcpServer> {
    if !context_mcp_session_enabled(config, backend) {
        return Vec::new();
    }
    let Some(engine) = config.context_engine.as_ref() else {
        return default_context_mcp_servers();
    };
    if !engine.enabled || engine.injection.is_off() {
        return Vec::new();
    }

    let mut servers = Vec::new();
    if let Some(server) =
        command_to_mcp_server("iota-context", engine.mcp.as_ref(), &["mcp", "context"])
    {
        servers.push(server);
    }
    if let Some(server) = command_to_mcp_server("iota-fun", engine.fun.as_ref(), &["mcp", "fun"]) {
        servers.push(server);
    }
    if servers.is_empty() {
        default_context_mcp_servers()
    } else {
        servers
    }
}

pub fn context_mcp_session_enabled(config: &NimiaConfig, backend: AcpBackend) -> bool {
    let backend_config = backend_context_config(config, backend);
    if let Some(value) = backend_config.and_then(|cfg| cfg.mcp_session_new) {
        return value;
    }
    true
}

pub fn backend_context_config(
    config: &NimiaConfig,
    backend: AcpBackend,
) -> Option<&BackendContextConfig> {
    config
        .context_engine_backend
        .as_ref()
        .and_then(|cfg| match backend {
            AcpBackend::ClaudeCode => cfg.claude_code.as_ref(),
            AcpBackend::Codex => cfg.codex.as_ref(),
            AcpBackend::Gemini => cfg.gemini.as_ref(),
            AcpBackend::Hermes => cfg.hermes.as_ref(),
            AcpBackend::OpenCode => cfg.opencode.as_ref(),
        })
}

pub fn context_session_options(config: &NimiaConfig, backend: AcpBackend) -> AcpSessionOptions {
    let Some(backend_context) = backend_context_config(config, backend) else {
        return AcpSessionOptions::default();
    };
    AcpSessionOptions {
        always_send_empty_mcp_servers: backend_context.always_send_empty_mcp_servers,
        mcp_env_shape: backend_context
            .mcp_env_shape
            .as_deref()
            .and_then(AcpMcpEnvShape::parse)
            .unwrap_or_default(),
    }
}

pub fn context_tool_whitelist(config: &NimiaConfig, backend: AcpBackend) -> Vec<String> {
    backend_config(config, backend)
        .map(|cfg| cfg.tool_whitelist.clone())
        .unwrap_or_default()
}

pub fn context_recall_thresholds(config: &NimiaConfig) -> RecallThresholdsConfig {
    config
        .context_engine
        .as_ref()
        .and_then(|cfg| cfg.recall_thresholds)
        .unwrap_or_default()
}

pub fn context_episodic_compaction_keep(config: &NimiaConfig) -> usize {
    config
        .context_engine
        .as_ref()
        .map(|cfg| cfg.episodic_compaction_keep.max(1))
        .unwrap_or_else(default_episodic_compaction_keep)
}

pub fn context_embedding_config(config: &NimiaConfig) -> Option<EmbeddingConfig> {
    config
        .context_engine
        .as_ref()
        .and_then(|cfg| cfg.embedding.clone())
}

fn command_to_mcp_server(
    default_name: &str,
    command: Option<&CommandConfig>,
    default_args: &[&str],
) -> Option<AcpMcpServer> {
    let (command, args) = match command {
        Some(command) if !command.command.trim().is_empty() => {
            let args = command
                .args
                .iter()
                .filter_map(|arg| expand_home_path(arg).ok())
                .collect::<Vec<_>>();
            (
                normalize_command(&resolve_iota_command(
                    &expand_home_path(&command.command).ok()?,
                )),
                args,
            )
        }
        Some(_) => return None,
        None => {
            let command = std::env::current_exe()
                .ok()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "iota".to_string());
            (
                command,
                default_args.iter().map(|arg| arg.to_string()).collect(),
            )
        }
    };
    Some(AcpMcpServer {
        name: default_name.to_string(),
        command,
        args,
        env: default_mcp_server_env(default_name),
    })
}

fn resolve_iota_command(command: &str) -> String {
    resolve_iota_command_with_env(command, std::env::var("IOTA_CLI_PATH").ok())
}

pub(super) fn resolve_iota_command_with_env(
    command: &str,
    iota_cli_path: Option<String>,
) -> String {
    let trimmed = command.trim();
    let is_iota = trimmed == "iota" || trimmed == "iota.exe";
    if !is_iota {
        return trimmed.to_string();
    }

    iota_cli_path
        .filter(|path| !path.trim().is_empty())
        .unwrap_or_else(|| trimmed.to_string())
}

fn default_context_mcp_servers() -> Vec<AcpMcpServer> {
    [
        command_to_mcp_server("iota-context", None, &["mcp", "context"]),
        command_to_mcp_server("iota-fun", None, &["mcp", "fun"]),
    ]
    .into_iter()
    .flatten()
    .collect()
}

fn default_mcp_server_env(default_name: &str) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    if default_name == "iota-context" {
        let rust_log = std::env::var("IOTA_CONTEXT_MCP_RUST_LOG")
            .unwrap_or_else(|_| "iota::context::server=info".to_string());
        env.insert("RUST_LOG".to_string(), rust_log);
    }
    env
}

fn default_enabled() -> bool {
    true
}

fn default_context_injection() -> ContextInjection {
    ContextInjection::Auto
}

fn default_identity_threshold() -> f64 {
    0.85
}

fn default_preference_threshold() -> f64 {
    0.80
}

fn default_strategic_threshold() -> f64 {
    0.80
}

fn default_domain_threshold() -> f64 {
    0.80
}

fn default_procedural_threshold() -> f64 {
    0.85
}

fn default_episodic_threshold() -> f64 {
    0.80
}

fn default_episodic_compaction_keep() -> usize {
    40
}

fn default_memory_chars() -> usize {
    1200
}

fn default_skills_chars() -> usize {
    600
}

fn default_working_memory_chars() -> usize {
    800
}

fn default_workspace_chars() -> usize {
    400
}

fn default_handoff_chars() -> usize {
    400
}
