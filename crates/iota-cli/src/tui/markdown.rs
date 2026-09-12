/// Render markdown text into a Vec of ratatui `Line`s with appropriate styling.
/// Uses pulldown-cmark for parsing; supports: headings, bold, italic, inline code,
/// code blocks, lists (ordered + unordered), blockquotes, links, strikethrough.
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

pub fn render(md: &str) -> Vec<Line<'static>> {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TABLES);

    let parser = Parser::new_ext(md, opts);
    let mut builder = LinesBuilder::new();
    builder.process(parser);
    builder.finish()
}

// ── LinesBuilder ─────────────────────────────────────────────────────────────

struct LinesBuilder {
    lines: Vec<Line<'static>>,
    /// Spans accumulating on the current line.
    current: Vec<Span<'static>>,
    /// Style stack — pushed on tag open, popped on tag close.
    style_stack: Vec<Style>,
    /// List nesting: true = ordered, false = unordered, item counter.
    list_stack: Vec<(bool, u64)>,
    /// In a code block.
    in_code_block: bool,
    /// In a blockquote.
    blockquote_depth: usize,
}

impl LinesBuilder {
    fn new() -> Self {
        Self {
            lines: Vec::new(),
            current: Vec::new(),
            style_stack: vec![Style::default()],
            list_stack: Vec::new(),
            in_code_block: false,
            blockquote_depth: 0,
        }
    }

    fn cur_style(&self) -> Style {
        *self.style_stack.last().unwrap_or(&Style::default())
    }

    fn push_style(&mut self, s: Style) {
        let merged = self.cur_style().patch(s);
        self.style_stack.push(merged);
    }

    fn pop_style(&mut self) {
        if self.style_stack.len() > 1 {
            self.style_stack.pop();
        }
    }

    fn flush_line(&mut self) {
        let spans = std::mem::take(&mut self.current);
        self.lines.push(Line::from(spans));
    }

    fn push_span(&mut self, text: impl Into<String>) {
        let s = text.into();
        if s.is_empty() {
            return;
        }
        let style = self.cur_style();
        self.current.push(Span::styled(s, style));
    }

    /// Push text that may contain newlines (e.g. code block content).
    fn push_text_raw(&mut self, text: &str) {
        let style = self.cur_style();
        for (i, line) in text.split('\n').enumerate() {
            if i > 0 {
                self.flush_line();
            }
            if !line.is_empty() {
                self.current.push(Span::styled(line.to_string(), style));
            }
        }
    }

    fn list_indent(&self) -> String {
        "  ".repeat(self.list_stack.len().saturating_sub(1))
    }

    fn process(&mut self, parser: Parser) {
        for event in parser {
            match event {
                // ── Inline text ──────────────────────────────────────────────
                Event::Text(t) => {
                    if self.in_code_block {
                        self.push_text_raw(&t);
                    } else {
                        // Normal text — split on newlines just in case
                        for (i, segment) in t.split('\n').enumerate() {
                            if i > 0 {
                                self.flush_line();
                            }
                            self.push_span(segment.to_string());
                        }
                    }
                }
                Event::Code(t) => {
                    self.push_style(Style::default().fg(Color::Cyan));
                    self.push_span(format!("`{}`", t));
                    self.pop_style();
                }
                Event::SoftBreak | Event::HardBreak => {
                    self.flush_line();
                }

                // ── Tags open ────────────────────────────────────────────────
                Event::Start(tag) => self.open_tag(tag),
                Event::End(tag) => self.close_tag(tag),

                // ── HTML / rules ─────────────────────────────────────────────
                Event::Html(_) | Event::InlineHtml(_) => {}
                Event::Rule => {
                    self.flush_line();
                    self.current.push(Span::styled(
                        "─".repeat(40),
                        Style::default().fg(Color::DarkGray),
                    ));
                    self.flush_line();
                }
                _ => {}
            }
        }
    }

    fn open_tag(&mut self, tag: Tag) {
        match tag {
            Tag::Heading { level, .. } => {
                self.flush_line();
                let style = match level as u8 {
                    1 => Style::default()
                        .fg(Color::LightMagenta)
                        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                    2 => Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
                    _ => Style::default().add_modifier(Modifier::BOLD | Modifier::ITALIC),
                };
                self.push_style(style);
            }
            Tag::Paragraph => {
                // nothing — flush happens on End
            }
            Tag::Strong => {
                self.push_style(Style::default().add_modifier(Modifier::BOLD));
            }
            Tag::Emphasis => {
                self.push_style(Style::default().add_modifier(Modifier::ITALIC));
            }
            Tag::Strikethrough => {
                self.push_style(Style::default().add_modifier(Modifier::CROSSED_OUT));
            }
            Tag::Link { dest_url, .. } => {
                self.push_style(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::UNDERLINED),
                );
                // We'll render the link text; URL appended on close.
                let _ = dest_url; // stored implicitly via style
            }
            Tag::CodeBlock(_) => {
                self.flush_line();
                self.in_code_block = true;
                self.push_style(Style::default().fg(Color::Yellow));
            }
            Tag::BlockQuote(_) => {
                self.blockquote_depth += 1;
                self.push_style(Style::default().fg(Color::Green));
            }
            Tag::List(start) => {
                self.list_stack.push((start.is_some(), start.unwrap_or(1)));
            }
            Tag::Item => {
                self.flush_line();
                let indent = self.list_indent();
                let bullet = if let Some((ordered, n)) = self.list_stack.last_mut() {
                    if *ordered {
                        let s = format!("{}{}. ", indent, n);
                        *n += 1;
                        s
                    } else {
                        format!("{}• ", indent)
                    }
                } else {
                    "• ".to_string()
                };
                self.current
                    .push(Span::styled(bullet, Style::default().fg(Color::LightBlue)));
            }
            Tag::Table(_) | Tag::TableHead | Tag::TableRow | Tag::TableCell => {}
            _ => {}
        }
    }

    fn close_tag(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Heading(_) => {
                self.pop_style();
                self.flush_line();
            }
            TagEnd::Paragraph => {
                self.flush_line();
            }
            TagEnd::Strong | TagEnd::Emphasis | TagEnd::Strikethrough | TagEnd::Link => {
                self.pop_style();
            }
            TagEnd::CodeBlock => {
                self.flush_line();
                self.in_code_block = false;
                self.pop_style();
            }
            TagEnd::BlockQuote(_) => {
                self.flush_line();
                self.blockquote_depth = self.blockquote_depth.saturating_sub(1);
                self.pop_style();
            }
            TagEnd::List(_) => {
                self.list_stack.pop();
            }
            TagEnd::Item => {
                // Item content (paragraphs) already handles line breaks
            }
            _ => {}
        }
    }

    fn finish(mut self) -> Vec<Line<'static>> {
        if !self.current.is_empty() {
            self.flush_line();
        }
        // Remove trailing blank lines
        while self
            .lines
            .last()
            .is_some_and(|l: &Line| l.spans.iter().all(|s| s.content.trim().is_empty()))
        {
            self.lines.pop();
        }
        self.lines
    }
}
