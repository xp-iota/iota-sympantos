use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::collections::BTreeMap;

use crate::mcp::client::McpToolCall;
use crate::runtime_event::{RuntimeEvent, ToolCallEvent, ToolResultEvent};
use crate::skill::{Skill, render_template};

pub struct SkillRunOutput {
    pub text: String,
    pub events: Vec<RuntimeEvent>,
}

pub async fn run_engine_skill(skill: &Skill, prompt: &str) -> Result<Option<SkillRunOutput>> {
    if !skill.metadata.execution.mode.is_mcp() {
        return Ok(None);
    }

    let mut events = Vec::new();
    let mut tool_outputs = Vec::new();
    if !skill.metadata.execution.tools.is_empty() {
        let server = skill
            .metadata
            .execution
            .server
            .as_deref()
            .unwrap_or("iota-fun");
        let (command, args) = server_command(server)?;
        for tool in &skill.metadata.execution.tools {
            let arguments = tool_arguments();
            let call_id = format!("skill:{}:{}", skill.metadata.name, tool.label());
            events.push(RuntimeEvent::ToolCall(ToolCallEvent {
                id: call_id,
                name: tool.name.clone(),
                arguments,
            }));
        }
        let calls = skill
            .metadata
            .execution
            .tools
            .iter()
            .map(|tool| {
                (
                    format!("skill:{}:{}", skill.metadata.name, tool.label()),
                    tool.name.clone(),
                    tool.label().to_string(),
                    tool_arguments(),
                )
            })
            .collect::<Vec<_>>();
        let results = if skill.metadata.execution.parallel {
            run_batch(command, args, calls).await
        } else {
            run_sequential(command, args, calls).await
        };
        for (call_id, tool, alias, ok, result) in results {
            tool_outputs.push(json!({"name": tool, "as": alias, "result": result.clone()}));
            events.push(RuntimeEvent::ToolResult(ToolResultEvent {
                id: call_id,
                name: tool,
                ok,
                result,
            }));
        }
    }

    let mut text = render_template(skill, prompt);
    for item in &tool_outputs {
        if let (Some(alias), Some(result)) =
            (item.get("as").and_then(Value::as_str), item.get("result"))
        {
            text = text.replace(&format!("{{{{{}}}}}", alias), &render_tool_result(result));
        }
    }
    if text.contains("{{tool_results}}") {
        text = text.replace("{{tool_results}}", &render_tool_results(&tool_outputs));
    }

    events.push(RuntimeEvent::ToolResult(ToolResultEvent {
        id: format!("skill:{}", skill.metadata.name),
        name: skill.metadata.name.clone(),
        ok: true,
        result: json!({"mode":"mcp","text":text}),
    }));
    Ok(Some(SkillRunOutput { text, events }))
}

fn server_command(server: &str) -> Result<(String, Vec<String>)> {
    let subcommand = match server {
        "iota-fun" => "fun",
        "iota-context" => "context",
        _ => anyhow::bail!("unsupported skill MCP server '{server}'"),
    };
    let command = std::env::current_exe()
        .context("failed to resolve current executable for trusted MCP server")?
        .display()
        .to_string();
    Ok((command, vec!["mcp".to_string(), subcommand.to_string()]))
}

fn render_tool_results(results: &[Value]) -> String {
    serde_json::to_string_pretty(results).unwrap_or_else(|_| "[]".to_string())
}

fn render_tool_result(result: &Value) -> String {
    let text = extract_mcp_text(result).unwrap_or_else(|| {
        serde_json::to_string_pretty(result).unwrap_or_else(|_| result.to_string())
    });
    if result
        .get("isError")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        format!("ERROR: {}", concise_error(&text))
    } else {
        text
    }
}

fn concise_error(text: &str) -> String {
    text.lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("tool failed")
        .to_string()
}

fn extract_mcp_text(result: &Value) -> Option<String> {
    if let Some(text) = result.as_str() {
        let text = text.trim();
        return (!text.is_empty()).then(|| text.to_string());
    }
    if let Some(content) = result.get("content").and_then(Value::as_array) {
        let text = content
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        if !text.is_empty() {
            return Some(text);
        }
    }
    if let Some(error) = result.get("error").and_then(Value::as_str) {
        let error = error.trim();
        return (!error.is_empty()).then(|| error.to_string());
    }
    None
}

fn tool_arguments() -> Value {
    json!({})
}

async fn run_sequential(
    command: String,
    args: Vec<String>,
    calls: Vec<(String, String, String, Value)>,
) -> Vec<(String, String, String, bool, Value)> {
    let mut results = Vec::new();
    for (call_id, tool, alias, arguments) in calls {
        results.push(run_one_tool(&command, &args, call_id, tool, alias, arguments).await);
    }
    results
}

async fn run_batch(
    command: String,
    args: Vec<String>,
    calls: Vec<(String, String, String, Value)>,
) -> Vec<(String, String, String, bool, Value)> {
    let futures = calls.into_iter().map(|(call_id, tool, alias, arguments)| {
        let command = command.clone();
        let args = args.clone();
        async move { run_one_tool(&command, &args, call_id, tool, alias, arguments).await }
    });
    futures_util::future::join_all(futures).await
}

async fn run_one_tool(
    command: &str,
    args: &[String],
    call_id: String,
    tool: String,
    alias: String,
    arguments: Value,
) -> (String, String, String, bool, Value) {
    let result = crate::mcp::client::call_stdio(
        command,
        args,
        &BTreeMap::new(),
        McpToolCall {
            name: tool.clone(),
            arguments,
        },
        10_000,
    )
    .await;
    match result {
        Ok(result) => (call_id, tool, alias, result.ok, result.content),
        Err(err) => (
            call_id,
            tool,
            alias,
            false,
            json!({"error": err.to_string()}),
        ),
    }
}
