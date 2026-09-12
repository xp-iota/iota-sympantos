use serde::{Deserialize, Serialize};

use super::{BackendConfig, ContextEngineBackendConfig, ContextEngineConfig};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StoreConfig {
    #[serde(default = "default_cache_retention_days")]
    pub cache_retention_days: i64,

    #[serde(default = "default_cache_running_ttl_secs")]
    pub cache_running_ttl_secs: i64,

    #[serde(default = "default_observability_retention_days")]
    pub observability_retention_days: i64,

    #[serde(default = "default_approvals_max_pending_age_secs")]
    pub approvals_max_pending_age_secs: i64,

    /// Maximum number of workspaces the daemon keeps engines (and therefore ACP
    /// subprocesses) alive for.
    #[serde(default = "default_engine_pool_max")]
    pub engine_pool_max: usize,

    /// Idle time in seconds after which a workspace's engine is released.
    #[serde(default = "default_engine_pool_idle_ttl_secs")]
    pub engine_pool_idle_ttl_secs: u64,
}

fn default_cache_retention_days() -> i64 {
    30
}

fn default_cache_running_ttl_secs() -> i64 {
    3600
}

fn default_observability_retention_days() -> i64 {
    90
}

fn default_approvals_max_pending_age_secs() -> i64 {
    604800 // 7 days
}

fn default_engine_pool_max() -> usize {
    16
}

fn default_engine_pool_idle_ttl_secs() -> u64 {
    1800 // 30 minutes
}

impl Default for StoreConfig {
    fn default() -> Self {
        Self {
            cache_retention_days: default_cache_retention_days(),
            cache_running_ttl_secs: default_cache_running_ttl_secs(),
            observability_retention_days: default_observability_retention_days(),
            approvals_max_pending_age_secs: default_approvals_max_pending_age_secs(),
            engine_pool_max: default_engine_pool_max(),
            engine_pool_idle_ttl_secs: default_engine_pool_idle_ttl_secs(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct NimiaConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_code: Option<BackendConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex: Option<BackendConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gemini: Option<BackendConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opencode: Option<BackendConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hermes: Option<BackendConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_engine: Option<ContextEngineConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_engine_backend: Option<ContextEngineBackendConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store: Option<StoreConfig>,
}
