use std::collections::VecDeque;

use iota_core::acp::AcpBackend;

#[derive(Debug, Clone, Default)]
pub struct ObservabilityMeta {
    pub execution_id: Option<String>,
    pub total_ms: Option<u64>,
    pub prompt_ms: Option<u64>,
    pub input_tokens: Option<u64>,
    pub cache_tokens: Option<u64>,
    pub cache_read_input_tokens: Option<u64>,
    pub cache_creation_input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub thinking_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub provider_reported_total_tokens: Option<u64>,
    pub normalized_total_tokens: Option<u64>,
}

/// A single entry in the conversation history.
#[derive(Debug, Clone)]
pub enum ConversationEntry {
    UserMessage {
        text: String,
        backend: Option<AcpBackend>,
    },
    AssistantMessage {
        backend: AcpBackend,
        text: String,
        observability: Option<ObservabilityMeta>,
    },
    SystemNotice {
        text: String,
    },
    ToolResult {
        name: String,
        ok: bool,
        text: String,
    },
}

/// Scrollable conversation history, capped at `max_entries`.
pub struct HistoryState {
    pub entries: VecDeque<ConversationEntry>,
    max_entries: usize,
}

impl HistoryState {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            max_entries,
        }
    }

    pub fn push(&mut self, entry: ConversationEntry) {
        self.entries.push_back(entry);
        while self.entries.len() > self.max_entries {
            self.entries.pop_front();
        }
    }
}
