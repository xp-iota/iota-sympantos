//! Redaction for anything iota writes to a log sink.
//!
//! Secrets reach log output by several routes that are easy to miss: backend
//! adapters put model API keys into child-process environments, an error from a
//! backend can quote the request it failed to send, and a panic or debug format
//! of a config struct prints whatever field it holds. Auditing every call site
//! does not scale, so redaction happens at the boundary instead — every log line
//! passes through [`redact`] on its way out.
//!
//! Two mechanisms, because neither is sufficient alone:
//!
//! - **Known values.** [`register_secret`] records the actual secret as it is
//!   loaded from config, so any later appearance in any format is caught, even
//!   in text iota never parsed.
//! - **Known shapes.** [`redact`] also rewrites `NAME=value` / `"name": "value"`
//!   for the credential-carrying key names iota and its backends use, which
//!   covers secrets that never passed through iota's own config.

use std::collections::BTreeSet;
use std::sync::{Mutex, OnceLock};

/// What replaces a redacted value.
pub const REDACTED: &str = "[redacted]";

/// Shortest secret worth registering.
///
/// Registering a very short value would turn every incidental occurrence of
/// that substring into `[redacted]` and make logs unreadable.
const MIN_SECRET_LEN: usize = 8;

/// Key names whose values are credentials.
///
/// Matched case-insensitively against both environment-variable style
/// (`ANTHROPIC_API_KEY=...`) and structured style (`"api_key": "..."`).
const SECRET_KEY_NAMES: &[&str] = &[
    "api_key",
    "apikey",
    "auth_token",
    "authorization",
    "cookie",
    "password",
    "secret",
    "session_token",
    "token",
];

fn registry() -> &'static Mutex<BTreeSet<String>> {
    static SECRETS: OnceLock<Mutex<BTreeSet<String>>> = OnceLock::new();
    SECRETS.get_or_init(|| Mutex::new(BTreeSet::new()))
}

/// Registers a literal secret value so it is redacted wherever it appears.
///
/// Call this wherever a credential enters the process — config load, token
/// creation, backend environment assembly. Values shorter than
/// [`MIN_SECRET_LEN`] and obvious placeholders are ignored.
pub fn register_secret(value: &str) {
    let value = value.trim();
    if value.len() < MIN_SECRET_LEN || is_placeholder(value) {
        return;
    }
    let mut guard = crate::utils::lock_or_recover(registry());
    guard.insert(value.to_string());
}

/// Whether `value` is one of the template placeholders shipped in
/// `nimia.yaml.template`, which are not secrets and would otherwise redact
/// legitimate log text.
fn is_placeholder(value: &str) -> bool {
    matches!(value, "<api-key>" | "YOUR_API_KEY" | "changeme")
}

/// Redacts every registered secret value and every recognized credential field
/// in `text`.
pub fn redact(text: &str) -> String {
    let mut out = redact_registered(text);
    out = redact_key_values(&out);
    out
}

fn redact_registered(text: &str) -> String {
    let guard = crate::utils::lock_or_recover(registry());
    if guard.is_empty() {
        return text.to_string();
    }
    let mut out = text.to_string();
    for secret in guard.iter() {
        if out.contains(secret.as_str()) {
            out = out.replace(secret.as_str(), REDACTED);
        }
    }
    out
}

/// Rewrites `key=value`, `key: value`, and `"key": "value"` for credential key
/// names.
///
/// Scans on character boundaries so multi-byte log content is preserved.
fn redact_key_values(text: &str) -> String {
    let lower = text.to_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0usize;

    while cursor < text.len() {
        let Some((key_start, key_end)) = next_secret_key(&lower, cursor) else {
            out.push_str(&text[cursor..]);
            return out;
        };
        // Everything up to and including the key name is kept verbatim.
        out.push_str(&text[cursor..key_end]);
        let _ = key_start;

        let rest = &text[key_end..];
        match value_span(rest) {
            Some((prefix_len, value_len)) => {
                out.push_str(&rest[..prefix_len]);
                let value = &rest[prefix_len..prefix_len + value_len];
                // `authorization: Bearer <token>` carries the scheme in the
                // first word; redacting that instead of the token would leave
                // the credential in the log.
                if is_auth_scheme(value) {
                    out.push_str(value);
                    let after_scheme = &rest[prefix_len + value_len..];
                    match value_span_unseparated(after_scheme) {
                        Some((scheme_gap, token_len)) => {
                            out.push_str(&after_scheme[..scheme_gap]);
                            out.push_str(REDACTED);
                            cursor = key_end + prefix_len + value_len + scheme_gap + token_len;
                        }
                        None => {
                            cursor = key_end + prefix_len + value_len;
                        }
                    }
                } else {
                    out.push_str(REDACTED);
                    cursor = key_end + prefix_len + value_len;
                }
            }
            None => {
                cursor = key_end;
                // No value shape here; copy one character so the scan advances.
                if let Some(ch) = text[cursor..].chars().next() {
                    out.push(ch);
                    cursor += ch.len_utf8();
                }
            }
        }
    }
    out
}

/// Finds the next credential key name at or after `from`, returning its byte
/// range in the original text.
fn next_secret_key(lower: &str, from: usize) -> Option<(usize, usize)> {
    let mut best: Option<(usize, usize)> = None;
    for name in SECRET_KEY_NAMES {
        if let Some(offset) = lower[from..].find(name) {
            let start = from + offset;
            let end = start + name.len();
            if best.is_none_or(|(best_start, _)| start < best_start) {
                best = Some((start, end));
            }
        }
    }
    best
}

/// Authentication schemes that precede the credential in a header value.
fn is_auth_scheme(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "bearer" | "basic" | "digest" | "token"
    )
}

/// [`value_span`] for the token after an auth scheme, where the separator is
/// whitespace rather than `:` or `=`.
fn value_span_unseparated(rest: &str) -> Option<(usize, usize)> {
    let value_start = rest.find(|ch: char| !matches!(ch, ' ' | '\t'))?;
    if value_start == 0 {
        return None;
    }
    let value_end = rest[value_start..]
        .find(['"', '\'', ',', '}', '\n', ' '])
        .map(|offset| value_start + offset)
        .unwrap_or(rest.len());
    Some((value_start, value_end - value_start))
}

/// Measures the separator and value that follow a key name.
///
/// Returns `(separator_len, value_len)`, or `None` when what follows is not a
/// value (so the key name was part of ordinary prose like "token budget").
fn value_span(rest: &str) -> Option<(usize, usize)> {
    let mut chars = rest.char_indices();
    let mut separator_end = None;
    for (index, ch) in chars.by_ref() {
        match ch {
            '"' | '\'' | ' ' | ':' | '=' | '\t' => continue,
            _ => {
                separator_end = Some(index);
                break;
            }
        }
    }
    let value_start = separator_end?;
    // A separator must actually have been present, otherwise `token` in prose
    // would swallow the following word.
    if value_start == 0 {
        return None;
    }
    let separator = &rest[..value_start];
    if !separator.contains([':', '=']) {
        return None;
    }
    let value_end = rest[value_start..]
        .find(['"', '\'', ',', '}', '\n', ' '])
        .map(|offset| value_start + offset)
        .unwrap_or(rest.len());
    Some((value_start, value_end - value_start))
}

/// A `MakeWriter` that redacts formatted log lines before they reach stderr.
pub struct RedactingStderr;

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for RedactingStderr {
    type Writer = RedactingWriter;

    fn make_writer(&'a self) -> Self::Writer {
        RedactingWriter
    }
}

pub struct RedactingWriter;

impl std::io::Write for RedactingWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        // Log formatters hand over one complete event per write, so redacting
        // per write cannot split a secret across calls.
        let text = String::from_utf8_lossy(buf);
        let redacted = redact(&text);
        std::io::Write::write_all(&mut std::io::stderr(), redacted.as_bytes())?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        std::io::Write::flush(&mut std::io::stderr())
    }
}

#[cfg(test)]
#[path = "redact_tests.rs"]
mod redact_tests;
