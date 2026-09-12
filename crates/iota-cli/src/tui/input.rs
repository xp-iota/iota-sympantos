use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use unicode_segmentation::UnicodeSegmentation;

// ── Grapheme-aware helpers ────────────────────────────────────────────────────

/// Collect text into a Vec of grapheme cluster strings.
fn graphemes(s: &str) -> Vec<&str> {
    UnicodeSegmentation::graphemes(s, true).collect()
}

/// Byte offset of grapheme index `gi` inside `s`.
fn grapheme_to_byte(s: &str, gi: usize) -> usize {
    UnicodeSegmentation::grapheme_indices(s, true)
        .nth(gi)
        .map(|(b, _)| b)
        .unwrap_or(s.len())
}

/// Number of grapheme clusters in `s`.
fn grapheme_count(s: &str) -> usize {
    UnicodeSegmentation::graphemes(s, true).count()
}

// ── HistorySearch ─────────────────────────────────────────────────────────────

/// State for Ctrl+R incremental history search.
pub struct HistorySearch {
    pub query: String,
    pub match_idx: Option<usize>, // index into history vec (latest first)
}

impl HistorySearch {
    pub fn new() -> Self {
        Self {
            query: String::new(),
            match_idx: None,
        }
    }

    /// Find the most recent history entry at-or-before `start_before` that contains `query`.
    pub fn find(&self, history: &[String], start_before: Option<usize>) -> Option<usize> {
        if self.query.is_empty() {
            return None;
        }
        let q = self.query.to_ascii_lowercase();
        let max = start_before.unwrap_or(history.len());
        // Search backwards from max-1
        (0..max)
            .rev()
            .find(|&i| history[i].to_ascii_lowercase().contains(&q))
    }
}

// ── Composer ──────────────────────────────────────────────────────────────────

/// Multi-line composer with:
/// - Grapheme-aware cursor
/// - Shift+Enter newline insertion
/// - Kill buffer (Ctrl+K / Ctrl+Y)
/// - Word motion (Alt+B / Alt+F, Ctrl+W)
/// - Submission history recall (↑/↓)
/// - Ctrl+R incremental history search
pub struct Composer {
    /// Raw text buffer (may contain newlines).
    pub text: String,
    /// Cursor position in grapheme clusters.
    pub cursor: usize,

    // Kill buffer
    kill_buf: String,

    // History
    history: Vec<String>,
    history_cursor: Option<usize>,
    saved_draft: String,

    // Ctrl+R search mode
    pub search: Option<HistorySearch>,
}

impl Composer {
    pub fn new() -> Self {
        Self {
            text: String::new(),
            cursor: 0,
            kill_buf: String::new(),
            history: Vec::new(),
            history_cursor: None,
            saved_draft: String::new(),
            search: None,
        }
    }

    /// Returns true when in Ctrl+R search mode.
    pub fn is_searching(&self) -> bool {
        self.search.is_some()
    }

    /// Current search query string (for footer display).
    pub fn search_query(&self) -> Option<&str> {
        self.search.as_ref().map(|s| s.query.as_str())
    }

    /// Handle a key event. Returns the resulting action.
    pub fn handle_key(&mut self, key: KeyEvent) -> ComposerAction {
        // ── Ctrl+R search mode ───────────────────────────────────────────────
        if self.search.is_some() {
            return self.handle_search_key(key);
        }

        match (key.modifiers, key.code) {
            // Submit on plain Enter
            (KeyModifiers::NONE, KeyCode::Enter) => {
                return ComposerAction::Submit;
            }

            // Shift+Enter → newline
            (KeyModifiers::SHIFT, KeyCode::Enter) => {
                self.insert_char('\n');
                return ComposerAction::Changed;
            }

            // ── History recall ───────────────────────────────────────────────
            (KeyModifiers::NONE, KeyCode::Up) => {
                if self.history.is_empty() {
                    return ComposerAction::ScrollHistory;
                }
                match self.history_cursor {
                    None => {
                        self.saved_draft = self.text.clone();
                        let idx = self.history.len() - 1;
                        self.history_cursor = Some(idx);
                        self.set_text(self.history[idx].clone());
                    }
                    Some(0) => {}
                    Some(ref mut idx) => {
                        *idx -= 1;
                        let t = self.history[*idx].clone();
                        self.set_text(t);
                    }
                }
                return ComposerAction::Changed;
            }
            (KeyModifiers::NONE, KeyCode::Down) => {
                match self.history_cursor {
                    None => return ComposerAction::ScrollHistory,
                    Some(idx) if idx + 1 >= self.history.len() => {
                        self.history_cursor = None;
                        let d = self.saved_draft.clone();
                        self.set_text(d);
                    }
                    Some(ref mut idx) => {
                        *idx += 1;
                        let t = self.history[*idx].clone();
                        self.set_text(t);
                    }
                }
                return ComposerAction::Changed;
            }

            // ── Cursor movement ──────────────────────────────────────────────
            (KeyModifiers::NONE, KeyCode::Left) => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                }
                return ComposerAction::Changed;
            }
            (KeyModifiers::NONE, KeyCode::Right) => {
                let max = grapheme_count(&self.text);
                if self.cursor < max {
                    self.cursor += 1;
                }
                return ComposerAction::Changed;
            }
            (KeyModifiers::NONE, KeyCode::Home) => {
                // Move to start of current line
                let gs = graphemes(&self.text);
                let mut line_start = 0;
                for i in (0..self.cursor).rev() {
                    if gs.get(i) == Some(&"\n") {
                        line_start = i + 1;
                        break;
                    }
                }
                self.cursor = line_start;
                return ComposerAction::Changed;
            }
            (KeyModifiers::NONE, KeyCode::End) => {
                // Move to end of current line
                let gs = graphemes(&self.text);
                let len = gs.len();
                let mut pos = self.cursor;
                while pos < len && gs[pos] != "\n" {
                    pos += 1;
                }
                self.cursor = pos;
                return ComposerAction::Changed;
            }
            (KeyModifiers::CONTROL, KeyCode::Char('a')) => {
                let gs = graphemes(&self.text);
                let mut line_start = 0;
                for i in (0..self.cursor).rev() {
                    if gs.get(i) == Some(&"\n") {
                        line_start = i + 1;
                        break;
                    }
                }
                self.cursor = line_start;
                return ComposerAction::Changed;
            }
            (KeyModifiers::CONTROL, KeyCode::Char('e')) => {
                let gs = graphemes(&self.text);
                let len = gs.len();
                let mut pos = self.cursor;
                while pos < len && gs[pos] != "\n" {
                    pos += 1;
                }
                self.cursor = pos;
                return ComposerAction::Changed;
            }

            // Alt+B — word backward
            (KeyModifiers::ALT, KeyCode::Char('b')) => {
                self.cursor = self.word_backward(self.cursor);
                return ComposerAction::Changed;
            }
            // Alt+F — word forward
            (KeyModifiers::ALT, KeyCode::Char('f')) => {
                self.cursor = self.word_forward(self.cursor);
                return ComposerAction::Changed;
            }

            // ── Deletion ─────────────────────────────────────────────────────
            (KeyModifiers::NONE, KeyCode::Backspace) => {
                if self.cursor > 0 {
                    let byte_pos = grapheme_to_byte(&self.text, self.cursor - 1);
                    let end_byte = grapheme_to_byte(&self.text, self.cursor);
                    self.text.drain(byte_pos..end_byte);
                    self.cursor -= 1;
                }
                return ComposerAction::Changed;
            }
            (KeyModifiers::NONE, KeyCode::Delete) => {
                let len = grapheme_count(&self.text);
                if self.cursor < len {
                    let byte_pos = grapheme_to_byte(&self.text, self.cursor);
                    let end_byte = grapheme_to_byte(&self.text, self.cursor + 1);
                    self.text.drain(byte_pos..end_byte);
                }
                return ComposerAction::Changed;
            }

            // Ctrl+W — kill word backward
            (KeyModifiers::CONTROL, KeyCode::Char('w')) => {
                let target = self.word_backward(self.cursor);
                if target < self.cursor {
                    let b1 = grapheme_to_byte(&self.text, target);
                    let b2 = grapheme_to_byte(&self.text, self.cursor);
                    self.kill_buf = self.text[b1..b2].to_string();
                    self.text.drain(b1..b2);
                    self.cursor = target;
                }
                return ComposerAction::Changed;
            }

            // Ctrl+K — kill to end of line (saves to kill buffer)
            (KeyModifiers::CONTROL, KeyCode::Char('k')) => {
                let gs = graphemes(&self.text);
                let len = gs.len();
                let mut eol = self.cursor;
                while eol < len && gs[eol] != "\n" {
                    eol += 1;
                }
                if eol > self.cursor {
                    let b1 = grapheme_to_byte(&self.text, self.cursor);
                    let b2 = grapheme_to_byte(&self.text, eol);
                    self.kill_buf = self.text[b1..b2].to_string();
                    self.text.drain(b1..b2);
                }
                return ComposerAction::Changed;
            }

            // Ctrl+U — kill to beginning of line
            (KeyModifiers::CONTROL, KeyCode::Char('u')) => {
                let gs = graphemes(&self.text);
                let mut bol = 0;
                for i in (0..self.cursor).rev() {
                    if gs.get(i) == Some(&"\n") {
                        bol = i + 1;
                        break;
                    }
                }
                if self.cursor > bol {
                    let b1 = grapheme_to_byte(&self.text, bol);
                    let b2 = grapheme_to_byte(&self.text, self.cursor);
                    self.kill_buf = self.text[b1..b2].to_string();
                    self.text.drain(b1..b2);
                    self.cursor = bol;
                }
                return ComposerAction::Changed;
            }

            // Ctrl+Y — yank kill buffer
            (KeyModifiers::CONTROL, KeyCode::Char('y')) => {
                if !self.kill_buf.is_empty() {
                    let kb = self.kill_buf.clone();
                    let klen = grapheme_count(&kb);
                    let byte_pos = grapheme_to_byte(&self.text, self.cursor);
                    self.text.insert_str(byte_pos, &kb);
                    self.cursor += klen;
                }
                return ComposerAction::Changed;
            }

            // Ctrl+R — enter history search mode
            (KeyModifiers::CONTROL, KeyCode::Char('r')) => {
                self.saved_draft = self.text.clone();
                self.search = Some(HistorySearch::new());
                return ComposerAction::SearchMode;
            }

            // Printable character (including unicode)
            (mods, KeyCode::Char(ch))
                if mods == KeyModifiers::NONE || mods == KeyModifiers::SHIFT =>
            {
                self.insert_char(ch);
                return ComposerAction::Changed;
            }

            _ => {}
        }
        ComposerAction::Ignored
    }

    /// Handle key input while in Ctrl+R search mode.
    fn handle_search_key(&mut self, key: KeyEvent) -> ComposerAction {
        let search = self.search.as_mut().unwrap();
        match (key.modifiers, key.code) {
            // Accept current match as editable draft
            (KeyModifiers::NONE, KeyCode::Enter) => {
                let preview = search
                    .match_idx
                    .and_then(|i| self.history.get(i))
                    .cloned()
                    .unwrap_or_else(|| self.saved_draft.clone());
                self.search = None;
                self.set_text(preview);
                return ComposerAction::Changed;
            }
            // Cancel — restore saved draft
            (KeyModifiers::NONE, KeyCode::Esc) => {
                self.search = None;
                let d = self.saved_draft.clone();
                self.set_text(d);
                return ComposerAction::Changed;
            }
            // Ctrl+R again — find next (older) match
            (KeyModifiers::CONTROL, KeyCode::Char('r')) => {
                let start = search.match_idx; // search older than current match
                let next = search.find(
                    &self.history,
                    start, // find before this index
                );
                search.match_idx = next;
                // Update composer preview
                let preview = next
                    .and_then(|i| self.history.get(i))
                    .cloned()
                    .unwrap_or_default();
                if !preview.is_empty() {
                    let gc = grapheme_count(&preview);
                    self.text = preview;
                    self.cursor = gc;
                }
                return ComposerAction::Changed;
            }
            // Backspace in query
            (KeyModifiers::NONE, KeyCode::Backspace) => {
                let search = self.search.as_mut().unwrap();
                if !search.query.is_empty() {
                    let gs: Vec<&str> = graphemes(&search.query).into_iter().collect();
                    let new_q: String = gs[..gs.len() - 1].concat();
                    search.query = new_q;
                    self.refresh_search();
                }
                return ComposerAction::Changed;
            }
            // Type into query
            (mods, KeyCode::Char(ch))
                if mods == KeyModifiers::NONE || mods == KeyModifiers::SHIFT =>
            {
                let search = self.search.as_mut().unwrap();
                search.query.push(ch);
                self.refresh_search();
                return ComposerAction::Changed;
            }
            _ => {}
        }
        ComposerAction::Ignored
    }

    /// Re-run the search with the current query and update the preview text.
    fn refresh_search(&mut self) {
        let search = self.search.as_mut().unwrap();
        let new_match = search.find(&self.history, None);
        search.match_idx = new_match;
        let preview = new_match
            .and_then(|i| self.history.get(i))
            .cloned()
            .unwrap_or_default();
        if !preview.is_empty() {
            let gc = grapheme_count(&preview);
            self.text = preview;
            self.cursor = gc;
        }
    }

    /// Take the current text for submission, clear the buffer, and push to history.
    pub fn take_submit(&mut self) -> String {
        let text = self.text.trim().to_string();
        self.text.clear();
        self.cursor = 0;
        self.history_cursor = None;
        self.saved_draft.clear();
        self.search = None;
        if !text.is_empty() {
            // Avoid duplicates at the tail
            if self.history.last().map(String::as_str) != Some(&text) {
                self.history.push(text.clone());
            }
        }
        text
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

    fn insert_char(&mut self, ch: char) {
        let byte_pos = grapheme_to_byte(&self.text, self.cursor);
        self.text.insert(byte_pos, ch);
        self.cursor += 1;
    }

    fn set_text(&mut self, text: String) {
        self.cursor = grapheme_count(&text);
        self.text = text;
    }

    fn word_backward(&self, from: usize) -> usize {
        let gs = graphemes(&self.text);
        let mut i = from;
        // Skip trailing whitespace
        while i > 0 && gs[i - 1].chars().all(|c| !c.is_alphanumeric()) {
            i -= 1;
        }
        // Skip word chars
        while i > 0 && gs[i - 1].chars().any(|c| c.is_alphanumeric()) {
            i -= 1;
        }
        i
    }

    fn word_forward(&self, from: usize) -> usize {
        let gs = graphemes(&self.text);
        let len = gs.len();
        let mut i = from;
        // Skip leading whitespace
        while i < len && gs[i].chars().all(|c| !c.is_alphanumeric()) {
            i += 1;
        }
        // Skip word chars
        while i < len && gs[i].chars().any(|c| c.is_alphanumeric()) {
            i += 1;
        }
        i
    }

    /// Returns true when the cursor is positioned at the end of the text buffer.
    pub fn cursor_at_end(&self) -> bool {
        self.cursor >= grapheme_count(&self.text)
    }

    /// Build display lines split by newline, for multi-line rendering.
    /// Returns (lines, cursor_row, cursor_col_in_row).
    pub fn display_lines(&self) -> (Vec<String>, usize, usize) {
        let gs: Vec<&str> = graphemes(&self.text).into_iter().collect();
        let mut lines: Vec<String> = Vec::new();
        let mut cur_line = String::new();
        let mut cursor_row = 0;
        let mut cursor_col = 0;

        for (idx, &g) in gs.iter().enumerate() {
            if idx == self.cursor {
                cursor_row = lines.len();
                cursor_col = grapheme_count(&cur_line);
            }
            if g == "\n" {
                lines.push(cur_line.clone());
                cur_line.clear();
            } else {
                cur_line.push_str(g);
            }
        }
        // Cursor at end (past all graphemes)
        if self.cursor >= gs.len() {
            cursor_row = lines.len();
            cursor_col = grapheme_count(&cur_line);
        }
        lines.push(cur_line);

        (lines, cursor_row, cursor_col)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ComposerAction {
    Submit,
    Changed,
    ScrollHistory,
    SearchMode,
    Ignored,
}

#[cfg(test)]
#[path = "input_tests.rs"]
mod input_tests;
