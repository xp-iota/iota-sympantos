use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use tokio::io::BufReader;
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::mpsc;

use crate::runtime_event::RuntimeEvent;

use super::AcpBackend;
use super::session::{AcpMcpServer, AcpSessionOptions};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpStartupTiming {
    pub process_spawn_ms: u64,
    pub init_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpPromptTiming {
    pub client_started: bool,
    pub process_spawned: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_spawn_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub init_ms: Option<u64>,
    pub session_reused: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_new_ms: Option<u64>,
    pub prompt_ms: u64,
    pub total_ms: u64,
}

#[derive(Debug)]
pub struct AcpPromptOutput {
    pub text: String,
    pub timing: AcpPromptTiming,
    pub backend_session_id: Option<String>,
    pub execution_id: Option<String>,
    pub events: Vec<RuntimeEvent>,
}

impl AcpPromptOutput {
    pub fn synthetic(text: String) -> Self {
        Self {
            text,
            backend_session_id: None,
            execution_id: None,
            events: Vec::new(),
            timing: AcpPromptTiming {
                client_started: false,
                process_spawned: false,
                process_spawn_ms: None,
                init_ms: None,
                session_reused: false,
                session_new_ms: None,
                prompt_ms: 0,
                total_ms: 0,
            },
        }
    }
}

pub(super) struct AcpSessionResolution {
    pub(super) session_id: String,
    pub(super) reused: bool,
    pub(super) session_new_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct AcpAgentCapabilities {
    pub(super) load_session: bool,
    pub(super) resume_session: bool,
}

pub struct AcpClientStartOptions {
    pub backend: AcpBackend,
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
    pub command_override: Option<(String, Vec<String>)>,
    pub mcp_servers: Vec<AcpMcpServer>,
    pub session_options: AcpSessionOptions,
    pub tool_whitelist: Vec<String>,
    pub show_native: bool,
    pub timeout_ms: u64,
}

pub struct AcpClient {
    pub(super) backend: AcpBackend,
    pub(super) cwd: PathBuf,
    pub(super) session_id: Option<String>,
    pub(super) agent_capabilities: AcpAgentCapabilities,
    pub(super) stdin: ChildStdin,
    pub(super) lines: tokio::io::Lines<BufReader<ChildStdout>>,
    pub(super) child: Child,
    pub(super) show_native: bool,
    pub(super) timeout_ms: u64,
    pub(super) prompt_counter: u64,
    pub(super) startup_timing: AcpStartupTiming,
    pub(super) mcp_servers: Vec<AcpMcpServer>,
    pub(super) session_options: AcpSessionOptions,
    pub(super) tool_whitelist: Vec<String>,
    /// When set, each streamed output chunk is forwarded to the TUI/desktop UI.
    pub(super) stream_tx: Option<mpsc::Sender<String>>,
    /// When set, runtime events are forwarded while the prompt is still running.
    pub(super) event_tx: Option<mpsc::Sender<RuntimeEvent>>,
}
