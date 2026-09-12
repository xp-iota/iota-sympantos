use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::{self, Write};
use std::sync::OnceLock;
use tokio::io::AsyncWriteExt;
use tokio::process::ChildStdin;
use tokio::sync::{RwLock, mpsc, oneshot};

use crate::runtime_event::ApprovalDecisionEvent;
use crate::store::approvals::{self, ApprovalStore};

use super::AcpBackend;

/// A pending approval request forwarded to the TUI.
pub struct ApprovalRequest {
    /// Human-readable tool name shown in the overlay.
    pub tool_name: String,
    /// Full params for storage.
    #[allow(dead_code)]
    pub params: Value,
    /// Reply with `true` = approved, `false` = denied.
    pub reply: oneshot::Sender<bool>,
}

/// When the TUI is active it installs a sender here; permission handling uses it
/// instead of blocking stdin.  Uses tokio::sync::RwLock so the channel can be
/// replaced when the TUI restarts within the same process, and reads never block
/// the tokio worker thread.
static TUI_APPROVAL_TX: OnceLock<RwLock<Option<mpsc::Sender<ApprovalRequest>>>> = OnceLock::new();
static SCOPED_APPROVAL_TX: OnceLock<RwLock<BTreeMap<String, mpsc::Sender<ApprovalRequest>>>> =
    OnceLock::new();

fn approval_lock() -> &'static RwLock<Option<mpsc::Sender<ApprovalRequest>>> {
    TUI_APPROVAL_TX.get_or_init(|| RwLock::new(None))
}

fn scoped_approval_lock() -> &'static RwLock<BTreeMap<String, mpsc::Sender<ApprovalRequest>>> {
    SCOPED_APPROVAL_TX.get_or_init(|| RwLock::new(BTreeMap::new()))
}

/// Install (or replace) the approval channel.  Call before starting the TUI event loop.
pub async fn install_tui_approval_channel(tx: mpsc::Sender<ApprovalRequest>) {
    *approval_lock().write().await = Some(tx);
}

/// Install an approval channel scoped to a specific execution id.
///
/// Desktop turns use their `turn_id` as the execution id so concurrent turns cannot steal each
/// other's approval requests. TUI keeps using the process-wide default channel above.
pub async fn install_scoped_approval_channel(
    execution_id: String,
    tx: mpsc::Sender<ApprovalRequest>,
) {
    scoped_approval_lock()
        .write()
        .await
        .insert(execution_id, tx);
}

pub async fn remove_scoped_approval_channel(execution_id: &str) {
    scoped_approval_lock().write().await.remove(execution_id);
}

pub async fn answer_permission_request(
    stdin: &mut ChildStdin,
    id: Value,
    params: Value,
    execution_id: Option<&str>,
    backend: AcpBackend,
    tool_whitelist: &[String],
    cwd: Option<&std::path::Path>,
) -> Result<ApprovalDecisionEvent> {
    // SECURITY: the identity used for auto-approval decisions (`is_iota_tool`,
    // `tool_is_whitelisted`) must come only from fields the ACP backend uses
    // to identify *which tool it is calling* (`toolName`/`name`/`tool`),
    // never from `toolCall.title`, which is a free-text, human-readable
    // label the backend can set to anything (including something that looks
    // like a trusted/internal tool name) independent of which tool is
    // actually being invoked. Falling back to `title` for the auto-approval
    // identity check was the root cause of result.md S-02: a malicious or
    // buggy backend could name an arbitrary/dangerous tool call with a
    // spoofed title such as "iota_memory_write" and have it silently
    // auto-approved. `display_name` keeps `title` only for the
    // human-facing prompt/log text, where spoofing has no security effect.
    let identity_name = params
        .get("toolName")
        .or_else(|| params.get("name"))
        .or_else(|| params.get("tool"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let display_name = identity_name.clone().unwrap_or_else(|| {
        params
            .get("toolCall")
            .and_then(|tc| tc.get("title"))
            .and_then(Value::as_str)
            .unwrap_or("tool")
            .to_string()
    });

    // Read the channel once to avoid holding the lock across .await points and
    // to prevent double-locking (tokio::sync::RwLock is not reentrant).
    let scoped_tx = if let Some(execution_id) = execution_id {
        scoped_approval_lock()
            .read()
            .await
            .get(execution_id)
            .cloned()
    } else {
        None
    };
    let tui_tx: Option<mpsc::Sender<ApprovalRequest>> = if scoped_tx.is_some() {
        scoped_tx.clone()
    } else {
        approval_lock().read().await.clone()
    };

    // iota's own MCP tools are internal infrastructure — auto-approve without prompting.
    // Tool names may arrive as "iota_memory_write" or "mcp__iota-context__iota_memory_write".
    // Fail closed if no verifiable identity field was present: `identity_name.is_none()`
    // means we only have an untrusted `title` to go on, which must never grant
    // auto-approval (see comment above).
    let is_iota_tool = identity_name
        .as_deref()
        .map(is_internal_iota_tool_name)
        .unwrap_or(false);
    let whitelist_hit = identity_name
        .as_deref()
        .map(|name| tool_is_whitelisted(name, tool_whitelist))
        .unwrap_or(false);
    if is_iota_tool || whitelist_hit {
        send_approved_response(stdin, id.clone(), &params).await?;
        return Ok(ApprovalDecisionEvent {
            request_id: id
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| id.to_string()),
            approved: true,
            reason: Some(if is_iota_tool {
                "auto-approved iota tool".to_string()
            } else {
                format!("auto-approved by backend whitelist ({})", backend)
            }),
        });
    }

    let tool_name = display_name;
    let approved = if let Some(tx) = tui_tx.clone() {
        let (reply_tx, reply_rx) = oneshot::channel();
        let req = ApprovalRequest {
            tool_name: tool_name.clone(),
            params: params.clone(),
            reply: reply_tx,
        };
        if tx.send(req).await.is_ok() {
            reply_rx.await.unwrap_or(false)
        } else {
            false
        }
    } else {
        let store = ApprovalStore::open_default().ok();
        let persisted_id = if let Some(store) = &store {
            store
                .record_request(execution_id, "acp", &tool_name, &params)
                .ok()
        } else {
            None
        };
        let dimensions = approvals::classify_operation(&tool_name, &params, cwd);
        let policy = approvals::default_decision(&dimensions);
        let result = prompt_yes_no(&format!(
            "Approve ACP tool request '{}' [{}]? ",
            tool_name, policy.reason
        ))
        .await?;
        if let (Some(store), Some(request_id)) = (&store, persisted_id.as_deref())
            && let Err(error) =
                store.record_decision(request_id, result, "interactive user decision")
        {
            warn_lost_approval_decision(request_id, result, &error);
        }
        result
    };

    let via_tui = tui_tx.is_some();
    if via_tui
        && let Ok(store) = ApprovalStore::open_default()
        && let Ok(request_id) = store.record_request(execution_id, "acp", &tool_name, &params)
        && let Err(error) = store.record_decision(&request_id, approved, "tui user decision")
    {
        warn_lost_approval_decision(&request_id, approved, &error);
    }

    send_approved_or_denied_response(stdin, id.clone(), approved, &params).await?;
    Ok(ApprovalDecisionEvent {
        request_id: id
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| id.to_string()),
        approved,
        reason: Some(if via_tui {
            "tui user decision".to_string()
        } else {
            "interactive user decision".to_string()
        }),
    })
}

/// Returns whether `tool_name` identifies one of iota's own internal MCP
/// tools, using exact canonical-name comparison rather than
/// substring/prefix checks, cross-checked against the actual dispatch
/// registry (`mcp::tool_dispatch::REGISTRY`) rather than a hand-maintained
/// duplicate list — so a name can only be treated as internal if the
/// dispatcher would actually recognize and route it (AC2.1/AC2.2: no
/// auto-approval without an exact registry hit).
fn is_internal_iota_tool_name(tool_name: &str) -> bool {
    if !is_ascii_canonical(tool_name) || tool_name.trim() != tool_name {
        return false;
    }
    // Internal identity is intentionally *not* normalized: case, dashes,
    // whitespace, Unicode lookalikes, and arbitrary MCP server prefixes are
    // all different identities and must fail closed. Accept only an exact
    // registry name or the one canonical server-qualified representation
    // emitted for this process' iota-context MCP server.
    if crate::mcp::tool_dispatch::REGISTRY.is_known_tool(tool_name) {
        return true;
    }
    tool_name
        .strip_prefix("mcp__iota-context__")
        .filter(|tail| !tail.contains("__"))
        .is_some_and(|tail| crate::mcp::tool_dispatch::REGISTRY.is_known_tool(tail))
}

/// Rejects names containing non-ASCII characters, so Unicode confusables
/// (e.g. Cyrillic "а" standing in for Latin "a") cannot be used to craft a
/// visually similar but distinct tool name that would otherwise canonicalize
/// differently than a trusted name and slip past exact-match checks in an
/// unexpected way, or be visually confused with a trusted name in logs/UI.
fn is_ascii_canonical(value: &str) -> bool {
    value.is_ascii()
}

async fn send_response(stdin: &mut ChildStdin, id: Value, result: Value) -> Result<()> {
    let response = json!({ "jsonrpc": "2.0", "id": id, "result": result });
    let mut line = serde_json::to_vec(&response).context("Failed to serialize ACP response")?;
    line.push(b'\n');
    stdin
        .write_all(line.as_slice())
        .await
        .context("Failed to write ACP response")?;
    stdin.flush().await.context("Failed to flush ACP stdin")?;
    Ok(())
}

async fn send_approved_response(stdin: &mut ChildStdin, id: Value, params: &Value) -> Result<()> {
    // Claude-code ACP adapter expects: {outcome: {outcome: "selected", optionId: "..."}}
    // Prefer "allow_always" to persist the decision across the session.
    if let Some(option_id) = params
        .get("options")
        .and_then(Value::as_array)
        .and_then(|opts| {
            opts.iter()
                .find(|o| o.get("optionId").and_then(Value::as_str) == Some("allow_always"))
                .or_else(|| {
                    opts.iter()
                        .find(|o| o.get("optionId").and_then(Value::as_str) == Some("allow"))
                })
                .or_else(|| {
                    opts.iter().find(|o| {
                        o.get("optionId")
                            .and_then(Value::as_str)
                            .map(|s| s.starts_with("allow"))
                            == Some(true)
                    })
                })
                .and_then(|o| o.get("optionId").and_then(Value::as_str))
        })
    {
        return send_response(
            stdin,
            id,
            json!({
                "outcome": { "outcome": "selected", "optionId": option_id }
            }),
        )
        .await;
    }
    send_response(stdin, id, json!({ "approved": true })).await
}

async fn send_approved_or_denied_response(
    stdin: &mut ChildStdin,
    id: Value,
    approved: bool,
    params: &Value,
) -> Result<()> {
    if approved {
        send_approved_response(stdin, id, params).await
    } else {
        // Use outcome format for denial as well.
        let reject_id = params
            .get("options")
            .and_then(Value::as_array)
            .and_then(|opts| {
                opts.iter()
                    .find(|o| o.get("optionId").and_then(Value::as_str) == Some("reject"))
                    .and_then(|o| o.get("optionId").and_then(Value::as_str))
            });
        if let Some(option_id) = reject_id {
            send_response(
                stdin,
                id,
                json!({ "outcome": { "outcome": "selected", "optionId": option_id } }),
            )
            .await
        } else {
            send_response(stdin, id, json!({ "approved": false })).await
        }
    }
}

fn tool_is_whitelisted(tool_name: &str, rules: &[String]) -> bool {
    rules.iter().any(|rule| tool_rule_match(tool_name, rule))
}

fn tool_rule_match(tool_name: &str, rule: &str) -> bool {
    let rule = canonical_tool_name(rule);
    if rule.is_empty() {
        return false;
    }
    // SECURITY (result.md S-02): Unicode confusables (e.g. Cyrillic "а")
    // could otherwise be used to craft a tool name that is visually
    // identical to a trusted name but bypasses the checks below in
    // unexpected ways once passed through case-folding; reject anything
    // non-ASCII outright rather than trying to normalize it.
    if !is_ascii_canonical(&rule) || !is_ascii_canonical(tool_name) {
        return false;
    }
    if rule == "*" {
        return true;
    }

    let tool = canonical_tool_name(tool_name);
    let tool_tail = tool.split("__").last().unwrap_or(tool.as_str());

    // A bare, non-glob rule (e.g. "iota_memory_write") matches the tool's
    // exact full name or its exact tail after the last "__" server
    // separator — never a substring/prefix of either. This closes the
    // S-02 bypass where a crafted server name like
    // "mcp__iota_memory_write_evil__actually_dangerous_tool" or a nested
    // "__"-delimited segment could satisfy a naive substring check.
    // Callers that legitimately want prefix/suffix matching must say so
    // explicitly with a trailing/leading "*", which only administrators
    // configure via `tool_whitelist` — this input is not attacker
    // controlled, unlike the ACP backend's reported tool name.
    wildcard_match(&tool, &rule) || wildcard_match(tool_tail, &rule)
}

fn wildcard_match(text: &str, pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(stripped) = pattern.strip_suffix('*') {
        return text.starts_with(stripped);
    }
    if let Some(stripped) = pattern.strip_prefix('*') {
        return text.ends_with(stripped);
    }
    text == pattern
}

fn canonical_tool_name(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .replace('-', "_")
        .replace(' ', "")
}

async fn prompt_yes_no(message: &str) -> Result<bool> {
    let message = message.to_string();
    tokio::task::spawn_blocking(move || -> Result<bool> {
        print!("{}(y/n): ", message);
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        Ok(input.trim().eq_ignore_ascii_case("y"))
    })
    .await
    .context("Permission prompt task failed")?
}

/// Reports an approval decision that could not be persisted.
///
/// The approvals store is the audit trail for what the user allowed a backend
/// to do. The decision itself has already been applied by the time it is
/// written, so failing the turn would be worse than proceeding — but losing the
/// record silently would leave no evidence that a tool ran with consent.
fn warn_lost_approval_decision(request_id: &str, approved: bool, error: &anyhow::Error) {
    let category = crate::store::ErrorCategory::classify(error);
    tracing::warn!(
        request_id = %request_id,
        approved,
        store = "approvals",
        category = category.as_str(),
        error = %error,
        "failed to persist an approval decision; the audit trail is incomplete for this request"
    );
    crate::telemetry::metrics::get().record_storage_degraded("approvals", category.as_str());
}

#[cfg(test)]
#[path = "permission_tests.rs"]
mod tests;
