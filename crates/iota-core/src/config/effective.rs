use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::acp::AcpBackend;
use crate::acp::session::{AcpMcpServer, AcpSessionOptions};

use super::{
    BackendConfig, BackendContextConfig, ContextEngineConfig, EmbeddingConfig, NimiaConfig,
    RecallThresholdsConfig, backend_config, backend_context_config, context_embedding_config,
    context_episodic_compaction_keep, context_mcp_servers, context_memory_db_path,
    context_recall_thresholds, context_session_options, context_skill_roots,
    context_tool_whitelist,
};

#[derive(Debug, Clone)]
pub struct EffectiveConfig {
    backends: BTreeMap<AcpBackend, BackendConfig>,
    backend_context: BTreeMap<AcpBackend, BackendContextConfig>,
    context_engine: ContextEngineConfig,
    memory_db_path: Option<PathBuf>,
    skill_roots: Vec<PathBuf>,
    mcp_servers: BTreeMap<AcpBackend, Vec<AcpMcpServer>>,
    session_options: BTreeMap<AcpBackend, AcpSessionOptions>,
    tool_whitelist: BTreeMap<AcpBackend, Vec<String>>,
    recall_thresholds: RecallThresholdsConfig,
    episodic_compaction_keep: usize,
    embedding: Option<EmbeddingConfig>,
}

impl EffectiveConfig {
    pub fn from_config(config: &NimiaConfig) -> Self {
        let backends = crate::acp::ALL_BACKENDS
            .iter()
            .filter_map(|backend| {
                backend_config(config, *backend)
                    .cloned()
                    .map(|cfg| (*backend, cfg))
            })
            .collect();
        let backend_context = crate::acp::ALL_BACKENDS
            .iter()
            .filter_map(|backend| {
                backend_context_config(config, *backend)
                    .cloned()
                    .map(|cfg| (*backend, cfg))
            })
            .collect();
        let mcp_servers = crate::acp::ALL_BACKENDS
            .iter()
            .map(|backend| (*backend, context_mcp_servers(config, *backend)))
            .collect();
        let session_options = crate::acp::ALL_BACKENDS
            .iter()
            .map(|backend| (*backend, context_session_options(config, *backend)))
            .collect();
        let tool_whitelist = crate::acp::ALL_BACKENDS
            .iter()
            .map(|backend| (*backend, context_tool_whitelist(config, *backend)))
            .collect();
        Self {
            backends,
            backend_context,
            context_engine: config.context_engine.clone().unwrap_or_default(),
            memory_db_path: context_memory_db_path(config).ok(),
            skill_roots: context_skill_roots(config),
            mcp_servers,
            session_options,
            tool_whitelist,
            recall_thresholds: context_recall_thresholds(config),
            episodic_compaction_keep: context_episodic_compaction_keep(config),
            embedding: context_embedding_config(config),
        }
    }

    pub fn backend_config(&self, backend: AcpBackend) -> Option<&BackendConfig> {
        self.backends.get(&backend)
    }

    pub fn backend_context_config(&self, backend: AcpBackend) -> Option<&BackendContextConfig> {
        self.backend_context.get(&backend)
    }

    pub fn context_engine(&self) -> &ContextEngineConfig {
        &self.context_engine
    }

    pub fn memory_db_path(&self) -> Option<&PathBuf> {
        self.memory_db_path.as_ref()
    }

    pub fn skill_roots(&self) -> &[PathBuf] {
        &self.skill_roots
    }

    pub fn context_mcp_servers(&self, backend: AcpBackend) -> Vec<AcpMcpServer> {
        self.mcp_servers.get(&backend).cloned().unwrap_or_default()
    }

    pub fn context_session_options(&self, backend: AcpBackend) -> AcpSessionOptions {
        self.session_options
            .get(&backend)
            .copied()
            .unwrap_or_default()
    }

    pub fn context_tool_whitelist(&self, backend: AcpBackend) -> Vec<String> {
        self.tool_whitelist
            .get(&backend)
            .cloned()
            .unwrap_or_default()
    }

    pub fn recall_thresholds(&self) -> &RecallThresholdsConfig {
        &self.recall_thresholds
    }

    pub fn episodic_compaction_keep(&self) -> usize {
        self.episodic_compaction_keep
    }

    pub fn embedding_config(&self) -> Option<EmbeddingConfig> {
        self.embedding.clone()
    }
}
