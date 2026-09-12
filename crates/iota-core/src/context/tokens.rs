//! Token estimation and XML-safe rendering for the context capsule.
//!
//! Section budgets are configured in characters (`memory_chars` and friends),
//! but characters map to tokens at very different rates: a tokenizer splits
//! English prose at roughly four characters per token, while CJK text costs
//! about one token per character. Enforcing a character budget therefore let a
//! Chinese memory section spend ~4x the tokens of an English one of the same
//! configured size.
//!
//! Budgets are still *configured* in characters — that key is part of the
//! config file, the desktop runtime snapshot, and the memory-inject trace — but
//! they are *enforced* in token space: the configured character count is read
//! as an ASCII-equivalent budget and converted with
//! [`chars_budget_to_tokens`]. ASCII content therefore behaves exactly as
//! before, and CJK content is held to the same token cost.

/// Characters per token for non-CJK text.
///
/// Matches the rule of thumb for byte-pair encoders on English prose, code, and
/// markup. It is an estimate: the point is to stop counting a Chinese character
/// and an English character as the same cost, not to reproduce any single
/// tokenizer exactly.
const CHARS_PER_TOKEN: usize = 4;

/// Converts a budget configured in characters into a token budget.
pub fn chars_budget_to_tokens(chars: usize) -> usize {
    chars.div_ceil(CHARS_PER_TOKEN)
}

/// Estimates the token cost of `text`.
///
/// CJK (and other wide) scripts are counted one token per character; everything
/// else is counted at [`CHARS_PER_TOKEN`] characters per token. Newlines count
/// as their own token because tokenizers rarely merge them into neighbours.
pub fn estimate_tokens(text: &str) -> usize {
    let mut wide = 0usize;
    let mut narrow = 0usize;
    let mut newlines = 0usize;
    for ch in text.chars() {
        if ch == '\n' {
            newlines += 1;
        } else if is_wide_script(ch) {
            wide += 1;
        } else {
            narrow += 1;
        }
    }
    wide + newlines + narrow.div_ceil(CHARS_PER_TOKEN)
}

/// Whether `ch` belongs to a script a tokenizer typically spends a whole token
/// (or more) on per character.
fn is_wide_script(ch: char) -> bool {
    matches!(ch as u32,
        0x1100..=0x11FF     // Hangul Jamo
        | 0x2E80..=0x2FFF   // CJK radicals, Kangxi
        | 0x3000..=0x303F   // CJK punctuation
        | 0x3040..=0x30FF   // Hiragana, Katakana
        | 0x3130..=0x318F   // Hangul compatibility Jamo
        | 0x3400..=0x4DBF   // CJK extension A
        | 0x4E00..=0x9FFF   // CJK unified ideographs
        | 0xA000..=0xA4CF   // Yi
        | 0xAC00..=0xD7AF   // Hangul syllables
        | 0xF900..=0xFAFF   // CJK compatibility ideographs
        | 0xFF00..=0xFFEF   // Halfwidth and fullwidth forms
        | 0x20000..=0x3FFFF // CJK extensions B and beyond
    )
}

/// Escapes text that is embedded in the `<iota-context>` capsule.
///
/// Memory records, handoff summaries, skill descriptions, and workspace output
/// are all attacker-influenced to some degree: a memory entry containing
/// `</iota-context>` would otherwise close the capsule early and let the rest of
/// that entry read as a user request. Escaping `&`, `<`, and `>` keeps every
/// tag in the capsule one we emitted.
pub fn escape_capsule_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            other => out.push(other),
        }
    }
    out
}

/// Truncates `text` to `budget` tokens, cutting only at a line boundary.
///
/// Returns the kept text and whether anything was dropped. Cutting mid-line
/// (and, before this, mid-character-count) could split a memory record or a
/// git status entry in half, which reads as corrupted context rather than as
/// omitted context.
pub fn trim_to_tokens(text: &str, budget: usize) -> (String, bool) {
    if estimate_tokens(text) <= budget {
        return (text.to_string(), false);
    }
    let mut kept = String::new();
    let mut used = 0usize;
    for line in text.split_inclusive('\n') {
        let cost = estimate_tokens(line);
        if used + cost > budget {
            return (kept, true);
        }
        used += cost;
        kept.push_str(line);
    }
    // A single line larger than the budget: keep nothing rather than emit a
    // fragment.
    (kept, true)
}
