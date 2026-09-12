use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::tui::render::observability_line;
use crate::tui::state::{ConversationEntry, HistoryState};
use iota_core::acp::AcpBackend;

pub(super) fn export_history(
    cwd: &Path,
    active_backend: AcpBackend,
    active_model: Option<&str>,
    history: &HistoryState,
) -> Result<PathBuf> {
    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let filename = format!("iota_transcript_{}.txt", timestamp);
    let path = cwd.join(&filename);

    let mut content = String::new();
    content.push_str("iota TUI Transcript\n");
    content.push_str(&format!(
        "Exported: {}\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
    ));
    content.push_str(&format!("Backend: {}\n", active_backend));
    if let Some(model) = active_model {
        content.push_str(&format!("Model: {}\n", model));
    }
    content.push_str(&format!("Working Directory: {}\n", cwd.display()));
    content.push('\n');
    content.push_str(&"=".repeat(80));
    content.push_str("\n\n");

    for entry in &history.entries {
        match entry {
            ConversationEntry::UserMessage { text, .. } => {
                content.push_str("YOU:\n");
                content.push_str(text);
                content.push_str("\n\n");
            }
            ConversationEntry::AssistantMessage {
                backend,
                text,
                observability,
            } => {
                content.push_str(&format!("{}:\n", backend.to_string().to_uppercase()));
                content.push_str(text);
                content.push('\n');
                if let Some(meta) = observability
                    && let Some(line) = observability_line(meta, None)
                {
                    content.push_str(&format!("[{}]\n", line));
                }
                content.push('\n');
            }
            ConversationEntry::SystemNotice { text } => {
                content.push_str(&format!("── {} ──\n\n", text));
            }
            ConversationEntry::ToolResult { name, ok, text } => {
                let icon = if *ok { "✓" } else { "✗" };
                content.push_str(&format!("{} {} → {}\n\n", icon, name, text));
            }
        }
    }

    fs::write(&path, content)?;
    Ok(path)
}
