use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::state::ObservabilityMeta;
use iota_core::acp::AcpBackend;

use super::theme;

const MAX_CWD_CHARS: usize = 36;

const KEY_HINTS: &[(&str, &str)] = &[
    ("Wheel/Trackpad", "scroll"),
    ("Drag", "select"),
    ("/", "command"),
    ("Ctrl+B", "backend"),
    ("?", "help"),
    ("Ctrl+C", "quit"),
];

#[allow(clippy::too_many_arguments)]
pub fn render(
    frame: &mut Frame,
    area: Rect,
    cwd: &std::path::Path,
    backend: AcpBackend,
    model: Option<&str>,
    is_searching: bool,
    search_query: Option<&str>,
    running: bool,
    has_queued: bool,
    quit_confirm: bool,
    latest_observability: Option<&ObservabilityMeta>,
) {
    let model_str = model.unwrap_or("—");
    let cwd_str = compact_path(&cwd.display().to_string(), MAX_CWD_CHARS);
    let left_text = format!(" {}  {} · {} ", cwd_str, backend, model_str);
    let left = Span::styled(left_text.clone(), theme::status_bar_style());

    // Middle context hint
    let middle: Option<Line> = if quit_confirm {
        Some(Line::from(Span::styled(
            "  Press Ctrl+C again to quit",
            theme::tool_result_err_style(),
        )))
    } else if is_searching {
        let q = search_query.unwrap_or("");
        Some(Line::from(Span::styled(
            format!("  Ctrl+R search: {}_", q),
            theme::system_notice_style(),
        )))
    } else if has_queued {
        Some(Line::from(Span::styled(
            "  [queued]",
            theme::status_bar_hint_style(),
        )))
    } else if running {
        Some(Line::from(Span::styled(
            "  running…",
            theme::status_bar_hint_style(),
        )))
    } else {
        latest_observability.map(|observability| Line::from(observability_spans(observability)))
    };

    let hints_width: usize = KEY_HINTS.iter().map(|(k, h)| k.len() + h.len() + 5).sum();
    let middle_width = middle
        .as_ref()
        .map(|line| line.spans.iter().map(|s| s.content.len()).sum())
        .unwrap_or(0);
    let left_width = left_text.len();
    let pad = (area.width as usize).saturating_sub(left_width + middle_width + hints_width);

    let mut spans: Vec<Span> = vec![left];
    if let Some(m) = middle {
        spans.extend(m.spans);
    }
    if pad > 0 {
        spans.push(Span::raw(" ".repeat(pad)));
    }
    for (key, hint) in KEY_HINTS {
        spans.push(Span::styled(
            format!("[{}] ", key),
            theme::status_bar_hint_style(),
        ));
        spans.push(Span::styled(
            format!("{} ", hint),
            theme::status_bar_hint_style(),
        ));
    }

    let line = Line::from(spans);
    frame.render_widget(Paragraph::new(line), area);
}

fn compact_path(path: &str, max_chars: usize) -> String {
    let char_count = path.chars().count();
    if char_count <= max_chars {
        return path.to_string();
    }
    if max_chars <= 1 {
        return "…".to_string();
    }
    let tail = path
        .chars()
        .rev()
        .take(max_chars - 1)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    format!("…{}", tail)
}

fn observability_spans(observability: &ObservabilityMeta) -> Vec<Span<'static>> {
    let Some(status) = observability_status(observability) else {
        return Vec::new();
    };

    let mut spans = vec![Span::styled("  ", theme::status_bar_hint_style())];
    let mut first = true;
    for part in status.split(" · ") {
        if !first {
            spans.push(Span::styled(" · ", theme::status_bar_hint_style()));
        }
        let style = if is_token_status_part(part) {
            theme::status_bar_token_style()
        } else {
            theme::status_bar_hint_style()
        };
        spans.push(Span::styled(part.to_string(), style));
        first = false;
    }
    spans
}

fn observability_status(observability: &ObservabilityMeta) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(total_ms) = observability.total_ms {
        parts.push(format!("{}ms", total_ms));
    }
    if observability.cache_read_input_tokens.is_some()
        || observability.cache_creation_input_tokens.is_some()
        || observability.thinking_tokens.is_some()
        || observability.normalized_total_tokens.is_some()
        || observability.provider_reported_total_tokens.is_some()
    {
        if let Some(input) = observability.input_tokens {
            parts.push(format!("in {}", input));
        }
        if observability.cache_read_input_tokens.is_some()
            || observability.cache_creation_input_tokens.is_some()
        {
            parts.push(format!(
                "cache r{}/w{}",
                observability.cache_read_input_tokens.unwrap_or(0),
                observability.cache_creation_input_tokens.unwrap_or(0)
            ));
        }
        if let Some(output) = observability.output_tokens {
            parts.push(format!("out {}", output));
        }
        if let Some(thinking) = observability.thinking_tokens {
            parts.push(format!("think {}", thinking));
        }
        if let Some(total) = observability
            .normalized_total_tokens
            .or(observability.provider_reported_total_tokens)
            .or(observability.total_tokens)
        {
            parts.push(format!("total {}", total));
        }
    } else if observability.input_tokens.is_some()
        || observability.cache_tokens.is_some()
        || observability.output_tokens.is_some()
    {
        parts.push(format!(
            "{}|{}|{}",
            observability.input_tokens.unwrap_or(0),
            observability.cache_tokens.unwrap_or(0),
            observability.output_tokens.unwrap_or(0)
        ));
    } else if let Some(tokens) = observability.total_tokens {
        parts.push(format!("{} tok", tokens));
    }
    if let Some(execution_id) = observability.execution_id.as_deref() {
        let short = execution_id.chars().take(8).collect::<String>();
        if !short.is_empty() {
            parts.push(format!("exec {}", short));
        }
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

fn is_token_status_part(part: &str) -> bool {
    let mut segments = part.split('|');
    matches!(
        (segments.next(), segments.next(), segments.next(), segments.next()),
        (Some(a), Some(b), Some(c), None)
            if !a.is_empty()
                && !b.is_empty()
                && !c.is_empty()
                && a.chars().all(|ch| ch.is_ascii_digit())
                && b.chars().all(|ch| ch.is_ascii_digit())
                && c.chars().all(|ch| ch.is_ascii_digit())
    )
}

#[cfg(test)]
#[path = "status_bar_tests.rs"]
mod tests;
