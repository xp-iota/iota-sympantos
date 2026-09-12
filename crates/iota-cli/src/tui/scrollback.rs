//! Helpers for pushing conversation entries into the terminal's native
//! scrollback buffer via `Terminal::insert_before`.
//!
//! Inspired by codex's `insert_history.rs` — the chat transcript lives in the
//! terminal's own scrollback, leaving the inline viewport for the composer +
//! status bar. This gives users native scroll, copy and selection.

use ratatui::Terminal;
use ratatui::backend::Backend;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget, Wrap};

use iota_core::acp::AcpBackend;

use super::state::{ConversationEntry, ObservabilityMeta};
use super::{markdown, theme};

/// Push a single conversation entry into the terminal scrollback above the
/// inline viewport. A trailing blank line is added for breathing room.
pub(super) fn insert_entry<B: Backend>(
    terminal: &mut Terminal<B>,
    entry: &ConversationEntry,
) -> std::io::Result<()> {
    let mut lines = entry_to_lines(entry);
    if lines.is_empty() {
        return Ok(());
    }
    lines.push(Line::raw(""));
    insert_lines(terminal, lines)
}

/// Push arbitrary owned lines into terminal scrollback above the inline viewport.
pub(super) fn insert_lines<B: Backend>(
    terminal: &mut Terminal<B>,
    lines: Vec<Line<'static>>,
) -> std::io::Result<()> {
    let width = terminal.size()?.width.max(1);
    let para = Paragraph::new(lines.clone()).wrap(Wrap { trim: false });
    let height = para.line_count(width) as u16;
    if height == 0 {
        return Ok(());
    }
    terminal.insert_before(height, |buf| {
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .render(buf.area, buf);
    })
}

/// Render the iota logo + version banner. Inserted once at TUI startup so the
/// banner participates in normal terminal scrollback.
pub(super) fn banner_lines() -> Vec<Line<'static>> {
    let version = env!("CARGO_PKG_VERSION");
    let build_time = env!("BUILD_TIMESTAMP");
    vec![
        Line::from(Span::styled(
            format!("│ ιώτα  v{}-{} │", version, build_time),
            theme::banner_style(),
        )),
        Line::raw(""),
    ]
}

fn entry_to_lines(entry: &ConversationEntry) -> Vec<Line<'static>> {
    match entry {
        ConversationEntry::UserMessage { text, backend } => user_lines(text, *backend),
        ConversationEntry::AssistantMessage {
            backend,
            text,
            observability,
        } => assistant_lines(*backend, text, observability.as_ref()),
        ConversationEntry::SystemNotice { text } => vec![Line::from(Span::styled(
            format!("── {} ──", text),
            theme::system_notice_style(),
        ))],
        ConversationEntry::ToolResult { name, ok, text } => {
            let (icon, style) = if *ok {
                ("✓", theme::tool_result_ok_style())
            } else {
                ("✗", theme::tool_result_err_style())
            };
            vec![Line::from(vec![
                Span::styled(format!("{} ", icon), style),
                Span::styled(name.clone(), theme::tool_call_style()),
                Span::raw(" → "),
                Span::raw(text.clone()),
            ])]
        }
    }
}

fn user_lines(text: &str, backend: Option<AcpBackend>) -> Vec<Line<'static>> {
    let label = if let Some(backend) = backend {
        Span::styled(
            format!("{}  ", assistant_label(backend)),
            theme::user_label_style(),
        )
    } else {
        Span::styled("●  ".to_string(), theme::user_label_style())
    };
    let mut out: Vec<Line<'static>> = Vec::new();
    let mut first = true;
    for raw in text.split('\n') {
        if first {
            out.push(Line::from(vec![label.clone(), Span::raw(raw.to_string())]));
            first = false;
        } else {
            out.push(Line::from(vec![
                Span::raw("     "),
                Span::raw(raw.to_string()),
            ]));
        }
    }
    if out.is_empty() {
        out.push(Line::from(label));
    }
    out
}

fn assistant_lines(
    backend: AcpBackend,
    text: &str,
    observability: Option<&ObservabilityMeta>,
) -> Vec<Line<'static>> {
    let label = Span::styled(assistant_label(backend), theme::assistant_label_style());
    let mut out = Vec::new();
    let mut first = true;
    for md in markdown::render(text) {
        if first {
            let mut spans = vec![label.clone(), Span::raw("  ")];
            spans.extend(md.spans);
            out.push(Line::from(spans));
            first = false;
        } else {
            let mut spans: Vec<Span<'static>> = vec![Span::raw("     ")];
            spans.extend(md.spans);
            out.push(Line::from(spans));
        }
    }
    if out.is_empty() {
        out.push(Line::from(label));
    }
    if let Some(meta) = observability
        && let Some(line) = observability_line(meta)
    {
        out.push(Line::from(Span::styled(
            format!("     {}", line),
            theme::status_bar_hint_style(),
        )));
    }
    out
}

fn assistant_label(backend: AcpBackend) -> &'static str {
    match backend {
        AcpBackend::ClaudeCode => "■ cc",
        AcpBackend::Codex => "■ cx",
        AcpBackend::Gemini => "■ gm",
        AcpBackend::Hermes => "■ hm",
        AcpBackend::OpenCode => "■ oc",
    }
}

fn observability_line(meta: &ObservabilityMeta) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(total_ms) = meta.total_ms {
        parts.push(format!("total {}ms", total_ms));
    }
    if let Some(prompt_ms) = meta.prompt_ms {
        parts.push(format!("prompt {}ms", prompt_ms));
    }
    if meta.cache_read_input_tokens.is_some()
        || meta.cache_creation_input_tokens.is_some()
        || meta.thinking_tokens.is_some()
        || meta.normalized_total_tokens.is_some()
        || meta.provider_reported_total_tokens.is_some()
    {
        if let Some(input) = meta.input_tokens {
            parts.push(format!("in {}", input));
        }
        if meta.cache_read_input_tokens.is_some() || meta.cache_creation_input_tokens.is_some() {
            parts.push(format!(
                "cache r{}/w{}",
                meta.cache_read_input_tokens.unwrap_or(0),
                meta.cache_creation_input_tokens.unwrap_or(0)
            ));
        }
        if let Some(output) = meta.output_tokens {
            parts.push(format!("out {}", output));
        }
        if let Some(thinking) = meta.thinking_tokens {
            parts.push(format!("think {}", thinking));
        }
        if let Some(total) = meta
            .normalized_total_tokens
            .or(meta.provider_reported_total_tokens)
            .or(meta.total_tokens)
        {
            parts.push(format!("{} tokens", total));
        }
    } else if let Some(tokens) = meta.total_tokens {
        parts.push(format!("{} tokens", tokens));
    } else if meta.input_tokens.is_some() || meta.output_tokens.is_some() {
        parts.push(format!(
            "{} in / {} out",
            meta.input_tokens.unwrap_or(0),
            meta.output_tokens.unwrap_or(0)
        ));
    }
    if parts.is_empty() {
        return None;
    }
    let line = parts.join(" · ");
    meta.execution_id
        .as_deref()
        .map(|execution_id| execution_id.chars().take(8).collect::<String>())
        .filter(|short| !short.is_empty())
        .map_or(Some(line.clone()), |short| {
            Some(format!("{}: {}", short, line))
        })
}

/// Insert a help block describing the keyboard shortcuts.
pub(super) fn insert_help<B: Backend>(terminal: &mut Terminal<B>) -> std::io::Result<()> {
    let items: &[(&str, &str)] = &[
        ("Enter", "Send prompt"),
        ("Shift+Enter", "Insert newline"),
        ("Tab", "Queue prompt while running"),
        ("↑ / ↓", "History recall"),
        ("Ctrl+R", "Search history"),
        ("Ctrl+K / Ctrl+Y", "Kill / yank"),
        ("Ctrl+W", "Delete word backward"),
        ("Alt+B / Alt+F", "Word backward / forward"),
        ("Ctrl+E", "Export transcript to file"),
        ("Ctrl+B", "Cycle backend"),
        ("/backend", "List enabled backends"),
        ("/backend <name>", "Switch backend"),
        (
            "/codex",
            "Switch directly; also /claude, /gemini, /hermes, /opencode",
        ),
        ("/model", "Show active backend model"),
        ("/goal", "Show current goal"),
        ("/goal <text>", "Set current goal"),
        ("/goal clear", "Clear current goal"),
        ("/status", "Show iota session status"),
        ("/export", "Export transcript to file"),
        ("/clear", "Clear transcript view"),
        ("/quit", "Open quit confirmation"),
        (
            "Backend commands",
            "Known provider slash commands pass through as prompts",
        ),
        (
            "Custom commands",
            "Unknown slash commands pass through for provider-specific handlers",
        ),
        ("Ctrl+L", "Clear transcript view"),
        ("Esc", "Interrupt running turn"),
        ("?", "Show this help"),
        ("Ctrl+C", "Quit (press twice)"),
    ];
    let mut lines: Vec<Line<'static>> = vec![Line::from(Span::styled(
        "── Keyboard Shortcuts ──",
        theme::system_notice_style(),
    ))];
    for (k, v) in items {
        lines.push(Line::from(vec![
            Span::styled(format!("  {:18}", k), theme::tool_call_style()),
            Span::styled(v.to_string(), theme::assistant_text_style()),
        ]));
    }
    lines.push(Line::raw(""));
    insert_lines(terminal, lines)
}

#[cfg(test)]
#[path = "scrollback_tests.rs"]
mod tests;
