use super::input::Composer;
use super::slash_command::slash_completions;
use iota_core::acp::AcpBackend;

pub struct Autocompleter;

impl Autocompleter {
    /// Returns the ghost-text suffix to display after the cursor when typing a
    /// slash command. For example, `/he` on Hermes returns `Some("lp")` so the
    /// render layer can append it in dim gray.
    pub fn ghost_text(composer: &Composer, active_backend: AcpBackend) -> Option<String> {
        let text = &composer.text;
        if !text.starts_with('/') || text.contains(char::is_whitespace) {
            return None;
        }
        if !composer.cursor_at_end() {
            return None;
        }
        let prefix = &text[1..];
        let completions = slash_completions(prefix, active_backend);
        let first = completions.first()?;
        if first.len() > prefix.len() {
            Some(first[prefix.len()..].to_string())
        } else {
            None
        }
    }

    /// Returns a space-separated list of matching slash command names for the
    /// composer border title. Returns `None` when not in slash-typing mode.
    pub fn completion_hint(composer: &Composer, active_backend: AcpBackend) -> Option<String> {
        let text = &composer.text;
        if !text.starts_with('/') || text.contains(char::is_whitespace) {
            return None;
        }
        let prefix = &text[1..];
        let completions = slash_completions(prefix, active_backend);
        if completions.is_empty() {
            return None;
        }
        const MAX_SHOW: usize = 8;
        let shown: Vec<&str> = completions.iter().copied().take(MAX_SHOW).collect();
        let mut hint = shown.join("  ");
        if completions.len() > MAX_SHOW {
            hint.push_str(&format!("  +{}", completions.len() - MAX_SHOW));
        }
        Some(hint)
    }

    /// Accept the first slash-command completion by replacing the composer text.
    /// Returns `true` if a completion was accepted.
    pub fn tab_complete(composer: &mut Composer, active_backend: AcpBackend) -> bool {
        let text = composer.text.clone();
        if !text.starts_with('/') || text.contains(char::is_whitespace) {
            return false;
        }
        if !composer.cursor_at_end() {
            return false;
        }
        let prefix = &text[1..];
        let completions = slash_completions(prefix, active_backend);
        if let Some(&first) = completions.first()
            && first != prefix
        {
            let completed = format!("/{}", first);
            composer.cursor = completed.chars().count();
            composer.text = completed;
            return true;
        }
        false
    }
}
