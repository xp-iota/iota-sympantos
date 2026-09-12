//! Guards the docs against drifting away from the code they describe.
//!
//! `docs/` is large and hand-written, so a rename or a version bump can leave it
//! quietly wrong — the reader then trusts a document that no longer matches the
//! binary. These tests assert the facts that are mechanically checkable:
//! backend list and pinned ACP command versions, the daemon protocol version,
//! and the metric names. Prose stays hand-written.

use iota_core::acp::ALL_BACKENDS;
use iota_core::config::get_adapter;

fn repo_root() -> std::path::PathBuf {
    // CARGO_MANIFEST_DIR is crates/iota-core.
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn read_doc(name: &str) -> String {
    let path = repo_root().join("docs").join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()))
}

fn read_source(relative: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()))
}

#[test]
fn command_doc_lists_every_backend_with_its_pinned_command() {
    let doc = read_doc("command.md");
    for backend in ALL_BACKENDS {
        let (program, args) = get_adapter(backend).acp_command();
        assert!(
            doc.contains(&backend.to_string()),
            "docs/command.md does not mention backend {backend}"
        );
        // The pinned package version is the fact most likely to drift: it is
        // bumped in the adapter and forgotten in the table.
        let pinned = args
            .iter()
            .find(|arg| arg.contains('@') && arg.contains('/') || arg.contains("@1"))
            .or_else(|| args.first());
        if let Some(pinned) = pinned {
            assert!(
                doc.contains(pinned) || doc.contains(program),
                "docs/command.md is missing {backend}'s command ({program} {pinned})"
            );
        }
    }
}

#[test]
fn command_doc_states_the_current_daemon_protocol_version() {
    let doc = read_doc("command.md");
    let version = iota_core::daemon::DESKTOP_PROTOCOL_VERSION;
    assert!(
        doc.contains(&format!("`{version}`")),
        "docs/command.md does not state protocol version {version}"
    );
}

#[test]
fn command_doc_lists_every_daemon_protocol_message() {
    // A new message type that no client knows about is a support problem, so the
    // wire vocabulary has to stay documented.
    let doc = read_doc("command.md");
    let source = read_source("src/daemon/proto.rs");

    for (enum_name, label) in [
        ("pub enum DaemonClientMessage", "client"),
        ("pub enum DaemonServerMessage", "server"),
    ] {
        let body = source
            .split_once(enum_name)
            .unwrap_or_else(|| panic!("{enum_name} not found in proto.rs"))
            .1;
        let body = body.split_once("\n}").expect("enum body").0;
        for line in body.lines() {
            let trimmed = line.trim();
            // Variant declarations are the only `Name {` lines at this level.
            let Some(name) = trimmed.strip_suffix(" {") else {
                continue;
            };
            if !name
                .chars()
                .next()
                .is_some_and(|first| first.is_ascii_uppercase())
            {
                continue;
            }
            let wire = to_snake_case(name);
            assert!(
                mentions_token(&doc, &wire),
                "docs/command.md does not document the {label} message `{wire}`"
            );
        }
    }
}

#[test]
fn observability_doc_lists_every_registered_metric() {
    let doc = read_doc("observability.md");
    let source = read_source("src/telemetry/metrics.rs");

    let mut found = 0;
    for fragment in source.split("\"iota.").skip(1) {
        let Some(name) = fragment.split('"').next() else {
            continue;
        };
        let metric = format!("iota.{name}");
        assert!(
            mentions_token(&doc, &metric),
            "docs/observability.md does not document metric {metric}"
        );
        found += 1;
    }
    assert!(
        found >= 15,
        "expected to scan the full metric list, only found {found}"
    );
}

/// Whether `doc` mentions `token` as a whole name.
///
/// A plain `contains` is not enough: `memory_context_snapshot` is a substring of
/// `get_memory_context_snapshot`, and `iota.db.reader_fallback` is a substring of
/// a renamed `iota.db.reader_fallback_v2`, so substring matching would let both
/// renames pass unnoticed — which is exactly the drift these tests exist to
/// catch.
fn mentions_token(doc: &str, token: &str) -> bool {
    let is_name_char = |ch: char| ch.is_ascii_alphanumeric() || ch == '_' || ch == '.';
    let mut from = 0usize;
    while let Some(offset) = doc[from..].find(token) {
        let start = from + offset;
        let end = start + token.len();
        let before_ok = doc[..start]
            .chars()
            .next_back()
            .is_none_or(|ch| !is_name_char(ch));
        let after_ok = doc[end..].chars().next().is_none_or(|ch| !is_name_char(ch));
        if before_ok && after_ok {
            return true;
        }
        from = start + token.len();
    }
    false
}

fn to_snake_case(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 4);
    for (index, ch) in name.char_indices() {
        if ch.is_ascii_uppercase() {
            if index != 0 {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}
