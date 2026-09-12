pub mod adapters;
pub mod backend;
pub mod context;
pub mod effective;
pub mod helpers;
pub mod loader;
pub mod model;
pub mod paths;
pub mod schema;

pub use adapters::{BackendAdapter, get_adapter};
#[allow(unused_imports)]
pub use backend::BackendVersionMapping;
pub use backend::{
    BackendConfig, BackendReadiness, CommandConfig, backend_config,
    backend_process_env_with_context, backend_readiness, command_label, configured_model,
    normalized_acp_command,
};
#[allow(unused_imports)]
pub use context::context_mcp_session_enabled;
pub use context::{
    BackendContextConfig, ContextEngineBackendConfig, ContextEngineConfig, EmbeddingConfig,
    RecallThresholdsConfig,
};
#[allow(unused_imports)]
pub use context::{ContextBudgetsConfig, ContextInjection};
pub use context::{
    backend_context_config, context_embedding_config, context_episodic_compaction_keep,
    context_mcp_servers, context_memory_db_path, context_recall_thresholds,
    context_session_options, context_skill_roots, context_tool_whitelist,
};
pub use effective::EffectiveConfig;
pub use helpers::{expand_home_path, normalize_command};
pub use loader::{config_path, read_config, save_config};
#[allow(unused_imports)]
pub use model::ModelConfig;
pub use schema::{NimiaConfig, StoreConfig};

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;

pub fn store_config() -> StoreConfig {
    if let Ok(cfg) = read_config() {
        cfg.store.unwrap_or_default()
    } else {
        StoreConfig::default()
    }
}
