#![allow(dead_code)]

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::time::{Duration, Instant, timeout_at};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolCall {
    pub name: String,
    #[serde(default)]
    pub arguments: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolResult {
    pub ok: bool,
    #[serde(default)]
    pub content: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ── Persistent session (reuse across multiple calls) ─────────────────────────

/// A long-lived MCP server connection.  Create once via [`McpSession::start`]
/// and reuse across multiple [`McpSession::call`] invocations to avoid the
/// per-call process spawn overhead.
pub struct McpSession {
    _child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    timeout_ms: u64,
}

impl McpSession {
    /// Spawn the MCP server and complete the initialization handshake.
    pub async fn start(
        command: &str,
        args: &[String],
        env: &BTreeMap<String, String>,
        timeout_ms: u64,
    ) -> Result<Self> {
        let mut process = Command::new(command);
        configure_mcp_environment(&mut process, env);
        let mut child = process
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("Failed to start MCP server {}", command))?;
        let mut stdin = child.stdin.take().context("MCP stdin not piped")?;
        let stdout = child.stdout.take().context("MCP stdout not piped")?;
        if let Some(stderr) = child.stderr.take() {
            forward_mcp_stderr(command.to_string(), stderr);
        }
        let mut stdout = BufReader::new(stdout);

        write_json(&mut stdin, json!({"jsonrpc":"2.0","id":"init","method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"iota","version":env!("CARGO_PKG_VERSION")}}})).await?;
        wait_id(&mut stdout, "init", timeout_ms).await?;
        write_json(
            &mut stdin,
            json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
        )
        .await?;
        Ok(Self {
            _child: child,
            stdin,
            stdout,
            timeout_ms,
        })
    }

    /// Send a tool call on the existing connection.
    pub async fn call(&mut self, call: McpToolCall) -> Result<McpToolResult> {
        write_json(
            &mut self.stdin,
            json!({"jsonrpc":"2.0","id":"call","method":"tools/call","params":{"name":call.name,"arguments":call.arguments}}),
        )
        .await?;
        let result = wait_id(&mut self.stdout, "call", self.timeout_ms).await?;
        Ok(McpToolResult {
            ok: !result
                .get("isError")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            content: result,
            error: None,
        })
    }
}

// ── One-shot helper (kept for backwards compat / simple callers) ──────────────

/// Spawn a fresh MCP server process, make a single tool call, then exit.
///
/// For callers that make multiple sequential calls to the same server, prefer
/// [`McpSession::start`] + [`McpSession::call`] to amortize the spawn cost.
pub async fn call_stdio(
    command: &str,
    args: &[String],
    env: &BTreeMap<String, String>,
    call: McpToolCall,
    timeout_ms: u64,
) -> Result<McpToolResult> {
    let mut results = call_stdio_batch(command, args, env, vec![call], timeout_ms).await?;
    results
        .pop()
        .context("MCP batch returned no result for single call")
}

pub async fn call_stdio_batch(
    command: &str,
    args: &[String],
    env: &BTreeMap<String, String>,
    calls: Vec<McpToolCall>,
    timeout_ms: u64,
) -> Result<Vec<McpToolResult>> {
    let mut process = Command::new(command);
    configure_mcp_environment(&mut process, env);
    let mut child = process
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("Failed to start MCP server {}", command))?;
    let mut stdin = child.stdin.take().context("MCP stdin not piped")?;
    let stdout = child.stdout.take().context("MCP stdout not piped")?;
    if let Some(stderr) = child.stderr.take() {
        forward_mcp_stderr(command.to_string(), stderr);
    }
    let mut stdout = BufReader::new(stdout);

    write_json(&mut stdin, json!({"jsonrpc":"2.0","id":"init","method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"iota","version":env!("CARGO_PKG_VERSION")}}})).await?;
    wait_id(&mut stdout, "init", timeout_ms).await?;
    write_json(
        &mut stdin,
        json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
    )
    .await?;
    let mut results = Vec::with_capacity(calls.len());
    for (index, call) in calls.into_iter().enumerate() {
        let id = format!("call:{}", index);
        write_json(&mut stdin, json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":call.name,"arguments":call.arguments}})).await?;
        let result = wait_id(&mut stdout, &format!("call:{}", index), timeout_ms).await?;
        results.push(McpToolResult {
            ok: !result
                .get("isError")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            content: result,
            error: None,
        });
    }
    let _ = stdin.shutdown().await;
    let _ = child.kill().await;
    Ok(results)
}

pub(crate) const MAX_MCP_LINE_BYTES: usize = 1024 * 1024;

fn configure_mcp_environment(command: &mut Command, env: &BTreeMap<String, String>) {
    command.env_clear();
    for key in [
        "PATH",
        "HOME",
        "USERPROFILE",
        "TMPDIR",
        "TEMP",
        "TMP",
        "SystemRoot",
        "WINDIR",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    command.envs(env);
}

fn forward_mcp_stderr(label: String, stderr: ChildStderr) {
    tokio::spawn(async move {
        let mut reader = BufReader::new(stderr);
        loop {
            match read_limited_line(&mut reader).await {
                Ok(Some(line)) if !line.trim().is_empty() => {
                    eprintln!("[mcp stderr:{label}] {line}");
                }
                Ok(Some(_)) => {}
                Ok(None) => break,
                Err(error) => {
                    eprintln!("[mcp stderr:{label}] stream rejected: {error}");
                    break;
                }
            }
        }
    });
}

async fn write_json(stdin: &mut ChildStdin, value: Value) -> Result<()> {
    let mut line = serde_json::to_vec(&value)?;
    anyhow::ensure!(
        line.len() < MAX_MCP_LINE_BYTES,
        "MCP request exceeded {MAX_MCP_LINE_BYTES} byte limit"
    );
    line.push(b'\n');
    stdin.write_all(&line).await?;
    stdin.flush().await?;
    Ok(())
}

pub(crate) async fn read_limited_line<R: AsyncRead + Unpin>(
    reader: &mut BufReader<R>,
) -> Result<Option<String>> {
    let mut bytes = Vec::new();
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            if bytes.is_empty() {
                return Ok(None);
            }
            break;
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let consumed = newline.map_or(available.len(), |position| position + 1);
        let content_len = newline.unwrap_or(consumed);
        if bytes.len().saturating_add(content_len) > MAX_MCP_LINE_BYTES {
            anyhow::bail!("MCP line exceeded {MAX_MCP_LINE_BYTES} byte limit");
        }
        bytes.extend_from_slice(&available[..content_len]);
        reader.consume(consumed);
        if newline.is_some() {
            break;
        }
    }
    String::from_utf8(bytes)
        .map(Some)
        .context("MCP server output was not valid UTF-8")
}

pub(crate) async fn wait_id<R: AsyncRead + Unpin>(
    stdout: &mut BufReader<R>,
    id: &str,
    timeout_ms: u64,
) -> Result<Value> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        let line = timeout_at(deadline, read_limited_line(stdout))
            .await
            .map_err(|_| anyhow!("MCP request timed out after {timeout_ms}ms"))??
            .context("MCP server exited before response")?;
        let value: Value = serde_json::from_str(&line).with_context(|| {
            let preview = line.chars().take(256).collect::<String>();
            format!("MCP server emitted non-JSON line: {preview}")
        })?;
        if value.get("id").and_then(Value::as_str) != Some(id) {
            continue;
        }
        if let Some(error) = value.get("error") {
            return Err(anyhow!("MCP error: {error}"));
        }
        return Ok(value.get("result").cloned().unwrap_or(Value::Null));
    }
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
