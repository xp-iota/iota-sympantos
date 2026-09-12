use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use super::kanban_view::{KanbanSnapshot, render_lines as render_kanban_lines};
use super::state::ObservabilityMeta;
use super::{SPINNER_FRAMES, TuiApp, status_bar, theme};

impl TuiApp {
    /// Render the inline viewport: optional Kanban panel, spinner row, composer row, status row.
    /// Chat transcript is emitted to native terminal scrollback.
    pub(super) fn render(&self, frame: &mut Frame) {
        let area = frame.area();
        if self.kanban_view.active {
            let chunks = Layout::vertical([
                Constraint::Min(8),
                Constraint::Length(1),
                Constraint::Length(3),
                Constraint::Length(1),
            ])
            .split(area);
            self.render_kanban_panel(frame, chunks[0]);
            self.render_status_indicator(frame, chunks[1]);
            if self.pending_approval.is_some() {
                self.render_approval_overlay(frame, chunks[2]);
            } else {
                self.render_composer(frame, chunks[2]);
            }
            status_bar::render(
                frame,
                chunks[3],
                &self.cwd,
                self.active_backend,
                self.active_model.as_deref(),
                self.composer.is_searching(),
                self.composer.search_query(),
                self.running_turn,
                self.queued_prompt.is_some(),
                matches!(self.overlay, super::Overlay::QuitConfirm),
                self.latest_observability.as_ref(),
            );
        } else {
            let chunks = Layout::vertical([
                Constraint::Length(1),
                Constraint::Length(3),
                Constraint::Length(1),
            ])
            .split(area);

            self.render_status_indicator(frame, chunks[0]);
            if self.pending_approval.is_some() {
                self.render_approval_overlay(frame, chunks[1]);
            } else {
                self.render_composer(frame, chunks[1]);
            }
            status_bar::render(
                frame,
                chunks[2],
                &self.cwd,
                self.active_backend,
                self.active_model.as_deref(),
                self.composer.is_searching(),
                self.composer.search_query(),
                self.running_turn,
                self.queued_prompt.is_some(),
                matches!(self.overlay, super::Overlay::QuitConfirm),
                self.latest_observability.as_ref(),
            );
        }
    }

    fn render_kanban_panel(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(theme::composer_border_style(true))
            .title(Span::styled(" Kanban ", theme::system_notice_style()));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let mut state = self.kanban_view.clone();
        let lines =
            match KanbanSnapshot::load(self.kanban_store.as_ref(), state.board_slug.as_deref()) {
                Ok(snapshot) => {
                    render_kanban_lines(&mut state, &snapshot, inner.width, inner.height)
                }
                Err(err) => vec![format!("Kanban load failed: {}", err)],
            };

        let lines = lines.into_iter().map(Line::raw).collect::<Vec<_>>();
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
    }

    fn render_composer(&self, frame: &mut Frame, area: Rect) {
        let (title_hint, title_style) = if self.composer.is_searching() {
            let q = self.composer.search_query().unwrap_or("");
            (format!(" Ctrl+R: {}_ ", q), theme::system_notice_style())
        } else if let Some(hint) = self.slash_completion_hint() {
            (format!(" {} ", hint), theme::completion_hint_style())
        } else {
            (String::new(), theme::composer_border_style(true))
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(theme::composer_border_style(
                !self.running_turn && !self.composer.is_searching(),
            ))
            .title(Span::styled(title_hint, title_style));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        if self.composer.text.is_empty() && !self.running_turn {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    "Type a prompt or / command and press Enter… (? for help)",
                    Style::default().fg(ratatui::style::Color::DarkGray),
                ))),
                inner,
            );
            frame.set_cursor_position((inner.x, inner.y));
            return;
        }

        let (disp_lines, cur_row, cur_col) = self.composer.display_lines();
        let ghost = self.slash_ghost();
        let para_lines: Vec<Line> = disp_lines
            .iter()
            .enumerate()
            .map(|(row_idx, line_text)| {
                if row_idx == cur_row
                    && let Some(ref g) = ghost
                {
                    return Line::from(vec![
                        Span::raw(line_text.clone()),
                        Span::styled(g.clone(), theme::ghost_text_style()),
                    ]);
                }
                Line::raw(line_text.clone())
            })
            .collect();

        let vp_height = inner.height as usize;
        let scroll_row = if cur_row >= vp_height {
            (cur_row - vp_height + 1) as u16
        } else {
            0
        };

        frame.render_widget(
            Paragraph::new(para_lines)
                .scroll((scroll_row, 0))
                .wrap(Wrap { trim: false }),
            inner,
        );

        use unicode_width::UnicodeWidthStr;
        let cursor_display_width = if cur_row < disp_lines.len() {
            let line = &disp_lines[cur_row];
            let chars_before_cursor: String = line.chars().take(cur_col).collect();
            UnicodeWidthStr::width(chars_before_cursor.as_str())
        } else {
            0
        };
        let cursor_x = inner.x + cursor_display_width as u16;
        let cursor_y = inner.y + (cur_row as u16).saturating_sub(scroll_row);
        frame.set_cursor_position((cursor_x, cursor_y));
    }

    fn render_approval_overlay(&self, frame: &mut Frame, area: Rect) {
        let tool_name = self
            .pending_approval
            .as_ref()
            .map(|r| r.tool_name.as_str())
            .unwrap_or("tool");

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(theme::tool_call_style())
            .title(Span::styled(
                " Approval Required ",
                theme::tool_call_style(),
            ));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let line = Line::from(vec![
            Span::styled("Allow ", theme::assistant_text_style()),
            Span::styled(tool_name, theme::tool_call_style()),
            Span::styled("?  ", theme::assistant_text_style()),
            Span::styled("[y] ", theme::tool_result_ok_style()),
            Span::styled("approve  ", theme::assistant_text_style()),
            Span::styled("[n] ", theme::tool_result_err_style()),
            Span::styled("deny", theme::assistant_text_style()),
        ]);
        frame.render_widget(Paragraph::new(line), inner);
    }

    fn render_status_indicator(&self, frame: &mut Frame, area: Rect) {
        if !self.running_turn {
            return;
        }
        let elapsed = self
            .turn_started_at
            .map(|t| {
                let s = t.elapsed().as_secs();
                if s < 60 {
                    format!("{}s", s)
                } else {
                    format!("{}m {:02}s", s / 60, s % 60)
                }
            })
            .unwrap_or_default();
        let frame_char = SPINNER_FRAMES[(self.tick_count / 4) as usize % SPINNER_FRAMES.len()];
        let line = Line::from(vec![
            Span::styled(format!(" {} ", frame_char), theme::spinner_style()),
            Span::styled("Working… ", theme::spinner_style()),
            Span::styled(elapsed, theme::status_bar_hint_style()),
            Span::styled("  Esc to interrupt", theme::status_bar_hint_style()),
        ]);
        frame.render_widget(Paragraph::new(line), area);
    }
}

pub(super) fn observability_line(
    meta: &ObservabilityMeta,
    sparkline: Option<&str>,
) -> Option<String> {
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
    if let Some(execution_id) = meta.execution_id.as_deref() {
        parts.push(format!("exec {}", execution_id));
    }
    if let Some(sparkline) = sparkline.filter(|value| !value.is_empty()) {
        parts.push(format!("latency {}", sparkline));
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}
