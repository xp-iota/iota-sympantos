use crate::acp::AcpBackend;
use crate::config::backend::BackendConfig;
use crate::config::context::BackendContextConfig;
use crate::config::model::ModelConfig;
use std::collections::BTreeMap;

pub trait BackendAdapter: Send + Sync {
    fn home_env_key(&self) -> Option<&'static str>;
    fn acp_command(&self) -> (&'static str, Vec<&'static str>);
    fn process_env(
        &self,
        section: &BackendConfig,
        context: Option<&BackendContextConfig>,
    ) -> BTreeMap<String, String>;
    fn additional_args(&self, _section: &BackendConfig) -> Vec<String> {
        Vec::new()
    }
}

pub fn get_adapter(backend: AcpBackend) -> &'static dyn BackendAdapter {
    match backend {
        AcpBackend::ClaudeCode => &ClaudeAdapter,
        AcpBackend::Codex => &CodexAdapter,
        AcpBackend::Gemini => &GeminiAdapter,
        AcpBackend::Hermes => &HermesAdapter,
        AcpBackend::OpenCode => &OpenCodeAdapter,
    }
}

// ---------------------------------------------------------------------------
// ClaudeCode Adapter
// ---------------------------------------------------------------------------

pub struct ClaudeAdapter;
impl BackendAdapter for ClaudeAdapter {
    fn home_env_key(&self) -> Option<&'static str> {
        Some("CLAUDE_CONFIG_DIR")
    }
    fn acp_command(&self) -> (&'static str, Vec<&'static str>) {
        let npx = if cfg!(windows) { "npx.cmd" } else { "npx" };
        (
            npx,
            vec!["-y", "@agentclientprotocol/claude-agent-acp@0.32.0"],
        )
    }
    fn process_env(
        &self,
        section: &BackendConfig,
        _context: Option<&BackendContextConfig>,
    ) -> BTreeMap<String, String> {
        let mut process_env = BTreeMap::new();
        let model = section.model.as_ref();
        if let Some(api_key) = model_value(model, |model| model.api_key.as_deref()) {
            crate::telemetry::redact::register_secret(&api_key);
            process_env.insert("ANTHROPIC_AUTH_TOKEN".to_string(), api_key.clone());
            process_env.insert("ANTHROPIC_API_KEY".to_string(), api_key);
        }
        if let Some(base_url) = model_value(model, |model| model.base_url.as_deref()) {
            process_env.insert("ANTHROPIC_BASE_URL".to_string(), base_url);
        }
        if let Some(name) = model_value(model, |model| model.name.as_deref()) {
            process_env.insert("ANTHROPIC_MODEL".to_string(), name.clone());
            process_env.insert("ANTHROPIC_SMALL_FAST_MODEL".to_string(), name.clone());
            process_env.insert("ANTHROPIC_DEFAULT_SONNET_MODEL".to_string(), name.clone());
            process_env.insert("ANTHROPIC_DEFAULT_OPUS_MODEL".to_string(), name.clone());
            process_env.insert("ANTHROPIC_DEFAULT_HAIKU_MODEL".to_string(), name);
        }
        process_env.insert("API_TIMEOUT_MS".to_string(), "3000000".to_string());
        process_env.insert(
            "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC".to_string(),
            "1".to_string(),
        );
        process_env
    }
}

// ---------------------------------------------------------------------------
// Codex Adapter
// ---------------------------------------------------------------------------

pub struct CodexAdapter;
impl BackendAdapter for CodexAdapter {
    fn home_env_key(&self) -> Option<&'static str> {
        None
    }
    fn acp_command(&self) -> (&'static str, Vec<&'static str>) {
        let npx = if cfg!(windows) { "npx.cmd" } else { "npx" };
        (npx, vec!["-y", "@zed-industries/codex-acp@0.12.0"])
    }
    fn process_env(
        &self,
        section: &BackendConfig,
        _context: Option<&BackendContextConfig>,
    ) -> BTreeMap<String, String> {
        let mut process_env = BTreeMap::new();
        let model = section.model.as_ref();
        if let Some(api_key) = model_value(model, |model| model.api_key.as_deref()) {
            crate::telemetry::redact::register_secret(&api_key);
            process_env.insert("ROUTER_API_KEY".to_string(), api_key.clone());
            process_env.insert("OPENAI_API_KEY".to_string(), api_key);
        }
        if let Some(base_url) = model_value(model, |model| model.base_url.as_deref()) {
            process_env.insert("OPENAI_BASE_URL".to_string(), base_url);
        }
        if let Some(name) = model_value(model, |model| model.name.as_deref()) {
            process_env.insert("OPENAI_MODEL".to_string(), name);
        }
        process_env
    }
    fn additional_args(&self, section: &BackendConfig) -> Vec<String> {
        codex_config_args(section)
    }
}

// ---------------------------------------------------------------------------
// Gemini Adapter
// ---------------------------------------------------------------------------

pub struct GeminiAdapter;
impl BackendAdapter for GeminiAdapter {
    fn home_env_key(&self) -> Option<&'static str> {
        Some("GEMINI_CONFIG_DIR")
    }
    fn acp_command(&self) -> (&'static str, Vec<&'static str>) {
        let npx = if cfg!(windows) { "npx.cmd" } else { "npx" };
        (npx, vec!["-y", "@google/gemini-cli@0.41.2", "--acp"])
    }
    fn process_env(
        &self,
        section: &BackendConfig,
        _context: Option<&BackendContextConfig>,
    ) -> BTreeMap<String, String> {
        let mut process_env = BTreeMap::new();
        let model = section.model.as_ref();
        if let Some(api_key) = model_value(model, |model| model.api_key.as_deref()) {
            crate::telemetry::redact::register_secret(&api_key);
            process_env.insert("GEMINI_API_KEY".to_string(), api_key);
        }
        if let Some(name) = model_value(model, |model| model.name.as_deref()) {
            process_env.insert("GEMINI_MODEL".to_string(), name);
        }
        process_env
    }
}

// ---------------------------------------------------------------------------
// Hermes Adapter
// ---------------------------------------------------------------------------

pub struct HermesAdapter;
impl BackendAdapter for HermesAdapter {
    fn home_env_key(&self) -> Option<&'static str> {
        Some("HERMES_HOME")
    }
    fn acp_command(&self) -> (&'static str, Vec<&'static str>) {
        ("hermes", vec!["acp"])
    }
    fn process_env(
        &self,
        section: &BackendConfig,
        _context: Option<&BackendContextConfig>,
    ) -> BTreeMap<String, String> {
        let mut process_env = BTreeMap::new();
        let Some(model) = section.model.as_ref() else {
            // No iota model override: let Hermes load its provider, model, and
            // credentials from its own configuration and process environment.
            return process_env;
        };
        let model = Some(model);
        let configured = model_value(model, |model| model.provider.as_deref()).is_some()
            || model_value(model, |model| model.name.as_deref()).is_some()
            || model_value(model, |model| model.base_url.as_deref()).is_some()
            || model_value(model, |model| model.api_key.as_deref()).is_some();
        if !configured {
            return process_env;
        }
        let provider = model_value(model, |model| model.provider.as_deref())
            .or_else(|| {
                model_value(model, |model| model.base_url.as_deref())
                    .map(|url| infer_hermes_provider(&url).to_string())
            })
            .unwrap_or_else(|| "minimax-cn".to_string());
        let api_key = model_value(model, |model| model.api_key.as_deref()).unwrap_or_default();
        let base_url = model_value(model, |model| model.base_url.as_deref())
            .unwrap_or_else(|| default_hermes_base_url(&provider).to_string());
        render_hermes_provider_env(&mut process_env, &provider, &api_key, &base_url);
        process_env.insert("HERMES_INFERENCE_PROVIDER".to_string(), provider);
        if let Some(name) = model_value(model, |model| model.name.as_deref()) {
            process_env.insert("HERMES_MODEL".to_string(), name);
        }
        process_env
    }
}

// ---------------------------------------------------------------------------
// OpenCode Adapter
// ---------------------------------------------------------------------------

pub struct OpenCodeAdapter;
impl BackendAdapter for OpenCodeAdapter {
    fn home_env_key(&self) -> Option<&'static str> {
        Some("OPENCODE_CONFIG_DIR")
    }
    fn acp_command(&self) -> (&'static str, Vec<&'static str>) {
        let npx = if cfg!(windows) { "npx.cmd" } else { "npx" };
        (npx, vec!["-y", "opencode-ai@1.14.40", "acp"])
    }
    fn process_env(
        &self,
        section: &BackendConfig,
        _context: Option<&BackendContextConfig>,
    ) -> BTreeMap<String, String> {
        let mut process_env = BTreeMap::new();
        let model = section.model.as_ref();
        if let Some(name) = model_value(model, |model| model.name.as_deref()) {
            process_env.insert("OPENCODE_MODEL".to_string(), name);
        }
        process_env
    }
}

// ---------------------------------------------------------------------------
// Private Helpers
// ---------------------------------------------------------------------------

fn model_value<F>(model: Option<&ModelConfig>, getter: F) -> Option<String>
where
    F: FnOnce(&ModelConfig) -> Option<&str>,
{
    getter(model?)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn codex_config_args(section: &BackendConfig) -> Vec<String> {
    let Some(model) = section.model.as_ref() else {
        return Vec::new();
    };
    let provider = model_value(Some(model), |model| model.provider.as_deref());
    let mut args = Vec::new();
    if let Some(name) = model_value(Some(model), |model| model.name.as_deref()) {
        push_codex_config_arg(&mut args, "model", &name);
    }
    if let Some(provider) = provider.as_deref() {
        push_codex_config_arg(&mut args, "model_provider", provider);
    }
    if let (Some(provider), Some(base_url)) = (
        provider.as_deref(),
        model_value(Some(model), |model| model.base_url.as_deref()),
    ) {
        push_codex_config_arg(
            &mut args,
            &format!("model_providers.{}.name", provider),
            provider,
        );
        push_codex_config_arg(
            &mut args,
            &format!("model_providers.{}.base_url", provider),
            &base_url,
        );
        push_codex_config_arg(
            &mut args,
            &format!("model_providers.{}.env_key", provider),
            "ROUTER_API_KEY",
        );
        push_codex_config_arg(
            &mut args,
            &format!("model_providers.{}.wire_api", provider),
            "responses",
        );
    }
    args
}

fn push_codex_config_arg(args: &mut Vec<String>, key: &str, value: &str) {
    args.push("-c".to_string());
    args.push(format!("{}={}", key, toml_string(value)));
}

fn toml_string(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\t', "\\t")
    )
}

fn infer_hermes_provider(base_url: &str) -> &'static str {
    let normalized = base_url.to_lowercase();
    if normalized.contains("minimaxi.com") {
        "minimax-cn"
    } else if normalized.contains("minimax.io") {
        "minimax"
    } else if normalized.contains("anthropic.com") {
        "anthropic"
    } else {
        "custom"
    }
}

fn default_hermes_base_url(provider: &str) -> &'static str {
    match provider {
        "minimax-cn" => "https://api.minimaxi.com/anthropic",
        "minimax" => "https://api.minimax.io/anthropic",
        "anthropic" => "https://api.anthropic.com",
        _ => "",
    }
}

fn render_hermes_provider_env(
    env: &mut BTreeMap<String, String>,
    provider: &str,
    api_key: &str,
    base_url: &str,
) {
    match provider {
        "minimax-cn" => {
            insert_non_empty(env, "MINIMAX_CN_API_KEY", api_key);
            insert_non_empty(env, "MINIMAX_CN_BASE_URL", base_url);
        }
        "minimax" => {
            insert_non_empty(env, "MINIMAX_API_KEY", api_key);
            insert_non_empty(env, "MINIMAX_BASE_URL", base_url);
        }
        "anthropic" => {
            insert_non_empty(env, "ANTHROPIC_API_KEY", api_key);
            insert_non_empty(env, "ANTHROPIC_TOKEN", api_key);
            insert_non_empty(env, "ANTHROPIC_BASE_URL", base_url);
        }
        _ => {
            insert_non_empty(env, "OPENAI_API_KEY", api_key);
            insert_non_empty(env, "OPENAI_BASE_URL", base_url);
        }
    }
}

fn insert_non_empty(env: &mut BTreeMap<String, String>, key: &str, value: &str) {
    if !value.is_empty() {
        if is_secret_env_key(key) {
            crate::telemetry::redact::register_secret(value);
        }
        env.insert(key.to_string(), value.to_string());
    }
}

/// Whether `key` names an environment variable that carries a credential.
///
/// Values under these keys are registered for log redaction: they are handed to
/// a child process, and a backend that fails can quote its own environment or
/// request back to us.
fn is_secret_env_key(key: &str) -> bool {
    let key = key.to_ascii_uppercase();
    key.ends_with("_API_KEY")
        || key.ends_with("_AUTH_TOKEN")
        || key.ends_with("_TOKEN")
        || key == "API_KEY"
}
