//! TCP JSON-line protocol types for the iota daemon.
//!
//! These structs define the wire format exchanged between the CLI client and
//! the background daemon process over `127.0.0.1:47661` (default).

use serde::{Deserialize, Serialize};

use crate::acp::AcpPromptTiming;
use crate::runtime_event::RuntimeEvent;

/// A prompt request sent by the CLI to the daemon.
#[derive(Debug, Serialize, Deserialize)]
pub struct DaemonPromptRequest {
    pub backend: String,
    pub cwd: String,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_id: Option<String>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub timing: bool,
    /// Local-machine auth token proving the caller can read the daemon's
    /// token file (`~/.i6/daemon.token`), i.e. is the same OS user as the
    /// daemon. Required for this request type; see `daemon::auth`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_token: Option<String>,
}

/// Response to both prompt and warm requests.
#[derive(Debug, Serialize, Deserialize)]
pub struct DaemonPromptResponse {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timing: Option<AcpPromptTiming>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warmed: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<RuntimeEvent>,
}

/// A warm-up request sent by the CLI to pre-start ACP backends.
#[derive(Debug, Serialize, Deserialize)]
pub struct DaemonWarmRequest {
    #[serde(rename = "type")]
    pub request_type: String,
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub backends: Vec<String>,
    /// See [`DaemonPromptRequest::auth_token`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_token: Option<String>,
}

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::acp::AcpBackend;
use crate::config::{BackendConfig, ModelConfig, NimiaConfig};

pub const DESKTOP_PROTOCOL_VERSION: u32 = 3;
pub const PROTOCOL_VERSION_MIN: u32 = 3;
pub const PROTOCOL_VERSION_MAX: u32 = 3;

/// Schema version of the message *payloads* (as opposed to the transport
/// protocol version above).
///
/// Lets a field be added or repurposed within one protocol version without
/// silently changing its meaning: a client that sees a higher schema version
/// than it understands can refuse rather than misread the payload.
pub const DESKTOP_SCHEMA_VERSION: u32 = 1;

/// Machine-readable classification of a daemon-side failure.
///
/// Previously every failure arrived as `ProtocolError { message: String }`, so
/// a client could only match on prose to decide whether retrying made sense.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonErrorCode {
    /// The client's protocol version is outside the supported range.
    UnsupportedVersion,
    /// The request was malformed or missing required fields.
    InvalidRequest,
    /// Authentication failed or the connection was never authenticated.
    Unauthenticated,
    /// No such turn, approval, or other referenced entity.
    NotFound,
    /// The daemon is shutting down or shedding load; retry later.
    Unavailable,
    /// The turn was cancelled before it produced a result.
    Cancelled,
    /// The backend produced an error while running the turn.
    BackendError,
    /// The request exceeded a size or time limit.
    LimitExceeded,
    /// An unexpected internal failure.
    Internal,
}

impl DaemonErrorCode {
    /// Whether retrying the same request could plausibly succeed.
    ///
    /// Client errors (bad version, malformed, unauthenticated, not found)
    /// are deterministic: retrying them changes nothing, so they report
    /// `false` and clients do not loop.
    pub fn is_retryable(self) -> bool {
        matches!(
            self,
            Self::Unavailable | Self::BackendError | Self::Internal
        )
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::UnsupportedVersion => "unsupported_version",
            Self::InvalidRequest => "invalid_request",
            Self::Unauthenticated => "unauthenticated",
            Self::NotFound => "not_found",
            Self::Unavailable => "unavailable",
            Self::Cancelled => "cancelled",
            Self::BackendError => "backend_error",
            Self::LimitExceeded => "limit_exceeded",
            Self::Internal => "internal",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ObservabilitySummaryResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_secs: Option<i64>,
    #[serde(default)]
    pub token_summary: Vec<TokenSummaryEntry>,
    #[serde(default)]
    pub recent_token_executions: Vec<RecentTokenExecution>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub write_latency: Option<LatencyPercentiles>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_throughput: Option<ThroughputSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LatencyPercentiles {
    pub p50_ms: Option<f64>,
    pub p99_ms: Option<f64>,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ThroughputSummary {
    pub mean_tokens_per_sec: Option<f64>,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TokenSummaryEntry {
    pub backend: String,
    pub count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens_mean: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens_mean: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalized_total_mean: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RecentTokenExecution {
    pub id: String,
    pub ts: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_id: Option<String>,
    pub backend: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalized_total_tokens: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DaemonClientMessage {
    Hello {
        client_name: String,
        protocol_version: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min_version: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_version: Option<u32>,
        /// Payload schema version the client speaks. See
        /// [`DESKTOP_SCHEMA_VERSION`].
        #[serde(default, skip_serializing_if = "Option::is_none")]
        schema_version: Option<u32>,
        /// Optional feature tags the client supports. Unknown tags are
        /// ignored, so a client may advertise new capabilities safely.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        capabilities: Vec<String>,
        /// See [`DaemonPromptRequest::auth_token`]. Authenticates the whole
        /// desktop connection: once accepted, subsequent sensitive messages
        /// on this connection do not need to repeat the token.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        auth_token: Option<String>,
    },
    StartTurn {
        turn_id: String,
        cwd: PathBuf,
        backend: String,
        prompt: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_ms: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    RespondApproval {
        approval_id: String,
        approved: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    CancelTurn {
        turn_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    GetConfig {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    SaveBackendModel {
        backend: String,
        model: DesktopModelConfig,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    CheckBackend {
        backend: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    GetObservabilitySummary {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<PathBuf>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    GetMemoryContextSnapshot {
        cwd: PathBuf,
        scope_mode: DesktopMemoryScopeMode,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    Ping {
        seq: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DesktopMemoryScopeMode {
    Workspace,
    All,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DesktopMemoryBuckets {
    pub identity: Vec<DesktopMemoryRecord>,
    pub preference: Vec<DesktopMemoryRecord>,
    pub strategic: Vec<DesktopMemoryRecord>,
    pub domain: Vec<DesktopMemoryRecord>,
    pub procedural: Vec<DesktopMemoryRecord>,
    pub episodic: Vec<DesktopMemoryRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DesktopMemoryRecord {
    pub id: String,
    #[serde(rename = "type")]
    pub memory_type: String,
    pub facet: Option<String>,
    pub scope: String,
    pub scope_id: String,
    pub content: String,
    pub confidence: f64,
    pub created_at: i64,
    pub updated_at: i64,
    pub expires_at: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DesktopMemorySummary {
    pub identity: usize,
    pub preference: usize,
    pub strategic: usize,
    pub domain: usize,
    pub procedural: usize,
    pub episodic: usize,
}

// `DesktopContextBudgetsSnapshot`, `DesktopContextSection`, and
// `DesktopRuntimeContextSnapshot` used to be defined here, but `engine`
// needed the same types without depending on `daemon` (see
// `crate::runtime_snapshot` for the rationale). They now live in the
// neutral `runtime_snapshot` module and are re-exported under their
// original desktop-protocol names so the wire format is unchanged.
pub use crate::runtime_snapshot::{
    ContextBudgetsSnapshot as DesktopContextBudgetsSnapshot,
    ContextSection as DesktopContextSection,
    RuntimeContextSnapshot as DesktopRuntimeContextSnapshot,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DesktopContextEngineSnapshot {
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_db: Option<PathBuf>,
    pub budgets: DesktopContextBudgetsSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DesktopSnapshotError {
    pub area: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DesktopMemoryContextSnapshot {
    pub cwd: PathBuf,
    pub scope_mode: DesktopMemoryScopeMode,
    pub memory: DesktopMemoryBuckets,
    pub memory_summary: DesktopMemorySummary,
    pub runtime_context: Option<DesktopRuntimeContextSnapshot>,
    pub context_engine: DesktopContextEngineSnapshot,
    pub errors: Vec<DesktopSnapshotError>,
}

impl DaemonClientMessage {
    /// The caller-supplied correlation id, when the variant carries one.
    ///
    /// `Hello` carries none: it is the handshake, not a request.
    pub fn request_id(&self) -> Option<&str> {
        match self {
            Self::Hello { .. } => None,
            Self::StartTurn { request_id, .. }
            | Self::RespondApproval { request_id, .. }
            | Self::CancelTurn { request_id, .. }
            | Self::GetConfig { request_id }
            | Self::SaveBackendModel { request_id, .. }
            | Self::CheckBackend { request_id, .. }
            | Self::GetObservabilitySummary { request_id, .. }
            | Self::GetMemoryContextSnapshot { request_id, .. }
            | Self::Ping { request_id, .. } => request_id.as_deref(),
        }
    }

    /// Short type name used in diagnostics and audit records.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Hello { .. } => "hello",
            Self::StartTurn { .. } => "start_turn",
            Self::RespondApproval { .. } => "respond_approval",
            Self::CancelTurn { .. } => "cancel_turn",
            Self::GetConfig { .. } => "get_config",
            Self::SaveBackendModel { .. } => "save_backend_model",
            Self::CheckBackend { .. } => "check_backend",
            Self::GetObservabilitySummary { .. } => "get_observability_summary",
            Self::GetMemoryContextSnapshot { .. } => "get_memory_context_snapshot",
            Self::Ping { .. } => "ping",
        }
    }
}

impl DaemonClientMessage {
    /// Builds the version-negotiation handshake at the current protocol and
    /// schema versions.
    ///
    /// Every client must send this exact shape; constructing it in one place
    /// keeps the version constants from drifting away from the call sites.
    pub fn hello(client_name: impl Into<String>, auth_token: Option<String>) -> Self {
        Self::Hello {
            client_name: client_name.into(),
            protocol_version: DESKTOP_PROTOCOL_VERSION,
            min_version: Some(PROTOCOL_VERSION_MIN),
            max_version: Some(PROTOCOL_VERSION_MAX),
            schema_version: Some(DESKTOP_SCHEMA_VERSION),
            capabilities: Vec::new(),
            auth_token,
        }
    }
}

impl DaemonServerMessage {
    /// Builds a `ProtocolError` carrying a code and the request it answers.
    pub fn protocol_error(
        message: impl Into<String>,
        code: DaemonErrorCode,
        request_id: Option<&str>,
    ) -> Self {
        Self::ProtocolError {
            message: message.into(),
            code: Some(code),
            retryable: code.is_retryable(),
            request_id: request_id.map(str::to_string),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
pub enum DaemonServerMessage {
    HelloAccepted {
        protocol_version: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        negotiated_version: Option<u32>,
    },
    ProtocolError {
        message: String,
        /// Machine-readable failure class. Absent only for messages produced
        /// before the field existed.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<DaemonErrorCode>,
        /// Whether retrying the same request could succeed. Mirrors
        /// `code.is_retryable()`; sent explicitly so a client need not know
        /// the code table.
        #[serde(default)]
        retryable: bool,
        /// Echo of the request's `request_id`, so a client can correlate a
        /// failure with the request that caused it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    TurnStarted {
        turn_id: String,
    },
    TextChunk {
        turn_id: String,
        chunk: String,
    },
    TurnEvent {
        turn_id: String,
        event: Box<RuntimeEvent>,
    },
    ApprovalRequested {
        turn_id: String,
        approval_id: String,
        tool_name: String,
        params: serde_json::Value,
    },
    ApprovalResponded {
        approval_id: String,
        accepted: bool,
    },
    TurnCompleted {
        turn_id: String,
        text: String,
        timing: crate::acp::AcpPromptTiming,
    },
    TurnFailed {
        turn_id: String,
        error: String,
    },
    TurnCancelled {
        turn_id: String,
        accepted: bool,
    },
    ConfigSnapshot {
        config: DesktopConfigSnapshot,
    },
    BackendCheckResult {
        backend: String,
        ok: bool,
        details: String,
    },
    ObservabilitySummary {
        summary: ObservabilitySummaryResponse,
    },
    MemoryContextSnapshot {
        snapshot: DesktopMemoryContextSnapshot,
    },
    Pong {
        seq: u64,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DesktopModelConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_key_configured: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_update: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DesktopBackendSnapshot {
    pub backend: String,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<DesktopModelConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DesktopConfigSnapshot {
    pub config_path: PathBuf,
    pub backends: BTreeMap<String, DesktopBackendSnapshot>,
}

impl DesktopConfigSnapshot {
    pub fn from_config(config: &NimiaConfig) -> Self {
        let mut backends = BTreeMap::new();
        for backend in crate::acp::ALL_BACKENDS {
            let key = backend.to_string();
            let snapshot = backend_snapshot(config, backend);
            backends.insert(key, snapshot);
        }

        Self {
            config_path: crate::config::config_path().unwrap_or_else(|_| {
                dirs::home_dir()
                    .map(|home| home.join(".i6").join("nimia.yaml"))
                    .unwrap_or_default()
            }),
            backends,
        }
    }
}

fn backend_snapshot(config: &NimiaConfig, backend: AcpBackend) -> DesktopBackendSnapshot {
    let section = match backend {
        AcpBackend::ClaudeCode => config.claude_code.as_ref(),
        AcpBackend::Codex => config.codex.as_ref(),
        AcpBackend::Gemini => config.gemini.as_ref(),
        AcpBackend::Hermes => config.hermes.as_ref(),
        AcpBackend::OpenCode => config.opencode.as_ref(),
    };

    DesktopBackendSnapshot {
        backend: backend.to_string(),
        enabled: section.map(|cfg| cfg.enabled).unwrap_or(true),
        model: section.and_then(|cfg| cfg.model.as_ref()).map(mask_model),
    }
}

fn mask_model(model: &ModelConfig) -> DesktopModelConfig {
    DesktopModelConfig {
        provider: model.provider.clone(),
        name: model.name.clone(),
        base_url: model.base_url.clone(),
        api_key_configured: model
            .api_key
            .as_deref()
            .map(|key| {
                let key = key.trim();
                !key.is_empty() && key != "<api-key>" && key != "YOUR_API_KEY"
            })
            .unwrap_or(false),
        api_key_update: None,
    }
}

pub fn apply_desktop_model_update(
    config: &mut NimiaConfig,
    backend: AcpBackend,
    update: DesktopModelConfig,
) {
    let section: &mut Option<BackendConfig> = match backend {
        AcpBackend::ClaudeCode => &mut config.claude_code,
        AcpBackend::Codex => &mut config.codex,
        AcpBackend::Gemini => &mut config.gemini,
        AcpBackend::Hermes => &mut config.hermes,
        AcpBackend::OpenCode => &mut config.opencode,
    };

    let mut backend_config = section.clone().unwrap_or_default();
    let mut model = backend_config.model.clone().unwrap_or_default();
    if update.provider.is_some() {
        model.provider = normalize_optional_text(update.provider);
    }
    if update.name.is_some() {
        model.name = normalize_optional_text(update.name);
    }
    if update.base_url.is_some() {
        model.base_url = normalize_optional_text(update.base_url);
    }
    if update.api_key_update.is_some() {
        model.api_key = normalize_optional_text(update.api_key_update);
    }
    backend_config.model = Some(model);
    *section = Some(backend_config);
}

fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
#[path = "proto_tests.rs"]
mod proto_tests;
