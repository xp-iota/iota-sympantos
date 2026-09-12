use anyhow::{Context, Result, bail};

use iota_core::daemon::{
    DaemonClientMessage, DaemonServerMessage, ObservabilitySummaryResponse, daemon_addr,
};
use iota_core::store::observability::{ObservabilityStore, StoredTokenUsage, TokenUsageSummary};

#[derive(Debug, PartialEq, Eq)]
enum ObservabilityCommand {
    LoggingRecent { limit: usize },
    LoggingEvents { execution_id: String },
    TokensRecent { limit: usize, json: bool },
    TokensSummary { since_secs: i64, json: bool },
    TokensExport { format: String },
    Metrics { prometheus: bool },
    Logs { execution_id: String },
    Trace { trace_id: String },
}

async fn run_logs_command_inner(execution_id: &str) -> Result<()> {
    let loki_url =
        std::env::var("IOTA_LOKI_URL").unwrap_or_else(|_| "http://localhost:3100".to_string());
    let query = format!(r#"{{iota_execution_id=\"{}\"}}"#, execution_id);
    let url = format!(
        "{}/loki/api/v1/query_range?query={}&limit=1000",
        loki_url,
        urlencoding::encode(&query)
    );
    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("Failed to connect to Loki at {}", loki_url))?;
    if !resp.status().is_success() {
        bail!("Loki query failed with status {}", resp.status());
    }
    let body: serde_json::Value = resp.json().await?;
    if let Some(results) = body["data"]["result"].as_array() {
        for stream in results {
            if let Some(values) = stream["values"].as_array() {
                for entry in values {
                    if let Some(arr) = entry.as_array()
                        && arr.len() >= 2
                        && let Some(line) = arr[1].as_str()
                    {
                        println!("{}", line);
                    }
                }
            }
        }
    } else {
        println!("No logs found for execution {}", execution_id);
    }
    Ok(())
}

async fn run_trace_command_inner(trace_id: &str) -> Result<()> {
    let jaeger_url =
        std::env::var("IOTA_JAEGER_URL").unwrap_or_else(|_| "http://localhost:16686".to_string());
    let url = format!("{}/api/traces/{}", jaeger_url, trace_id);
    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("Failed to connect to Jaeger at {}", jaeger_url))?;
    if !resp.status().is_success() {
        bail!("Jaeger query failed with status {}", resp.status());
    }
    let body: serde_json::Value = resp.json().await?;
    if let Some(traces) = body["data"].as_array() {
        for trace in traces {
            if let Some(spans) = trace["spans"].as_array() {
                for span in spans {
                    let name = span["operationName"].as_str().unwrap_or("?");
                    let duration_us = span["duration"].as_u64().unwrap_or(0);
                    let duration_ms = duration_us / 1000;
                    let depth = if span["references"]
                        .as_array()
                        .map(|r| r.is_empty())
                        .unwrap_or(true)
                    {
                        0
                    } else {
                        1
                    };
                    let indent = "  ".repeat(depth);
                    println!("{}├── {} ({}ms)", indent, name, duration_ms);
                }
            }
        }
    } else {
        println!("No trace found for {}", trace_id);
    }
    Ok(())
}

pub(super) async fn run_observability_command(args: &[String]) -> Result<()> {
    let command = parse_observability_args(args)?;

    match &command {
        ObservabilityCommand::TokensRecent { json, .. }
        | ObservabilityCommand::TokensSummary { json, .. } => {
            if let Ok(summary) = try_daemon_observability_summary().await {
                return print_daemon_summary(&summary, &command, *json);
            }
        }
        _ => {}
    }

    // Offline fallback: direct store access when daemon is unavailable
    let store = ObservabilityStore::open(&ObservabilityStore::default_path()?)?;
    match command {
        ObservabilityCommand::LoggingRecent { limit } => print_token_recent(&store, limit, false),
        ObservabilityCommand::LoggingEvents { execution_id } => {
            let records = store.token_usage_for_execution(&execution_id)?;
            println!("{}", serde_json::to_string_pretty(&records)?);
            Ok(())
        }
        ObservabilityCommand::TokensRecent { limit, json } => {
            print_token_recent(&store, limit, json)
        }
        ObservabilityCommand::TokensSummary { since_secs, json } => {
            let since_ts = iota_core::utils::now_ts() - since_secs;
            let summaries = store.token_summary_since(since_ts)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&summaries)?);
            } else {
                print_summary_table(&summaries);
            }
            Ok(())
        }
        ObservabilityCommand::TokensExport { format } => {
            if format != "json" {
                bail!("only json export is currently supported");
            }
            let records = store.recent_token_usage(10_000)?;
            println!("{}", serde_json::to_string_pretty(&records)?);
            Ok(())
        }
        ObservabilityCommand::Metrics { prometheus } => {
            let summaries = store.token_summary_since(0)?;
            if prometheus {
                print_prometheus_metrics(&summaries);
            } else {
                println!("{}", serde_json::to_string_pretty(&summaries)?);
            }
            Ok(())
        }
        ObservabilityCommand::Logs { execution_id } => run_logs_command_inner(&execution_id).await,
        ObservabilityCommand::Trace { trace_id } => run_trace_command_inner(&trace_id).await,
    }
}

async fn try_daemon_observability_summary() -> Result<ObservabilitySummaryResponse> {
    let addr = daemon_addr();
    let mut stream = tokio::net::TcpStream::connect(&addr).await?;

    let hello = DaemonClientMessage::hello(
        "iota-cli".to_string(),
        iota_core::daemon::auth::load_or_create_token().ok(),
    );
    let mut line = serde_json::to_vec(&hello)?;
    line.push(b'\n');
    tokio::io::AsyncWriteExt::write_all(&mut stream, &line).await?;
    tokio::io::AsyncWriteExt::flush(&mut stream).await?;

    let mut reader = tokio::io::BufReader::new(&mut stream);
    let mut resp_line = String::new();
    tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut resp_line).await?;
    let hello_resp: DaemonServerMessage = serde_json::from_str(resp_line.trim())?;
    if !matches!(hello_resp, DaemonServerMessage::HelloAccepted { .. }) {
        bail!("daemon handshake failed");
    }

    let get_msg = DaemonClientMessage::GetObservabilitySummary {
        cwd: None,
        request_id: None,
    };
    let mut line = serde_json::to_vec(&get_msg)?;
    line.push(b'\n');
    tokio::io::AsyncWriteExt::write_all(&mut stream, &line).await?;
    tokio::io::AsyncWriteExt::flush(&mut stream).await?;

    let mut reader = tokio::io::BufReader::new(&mut stream);
    let mut resp_line = String::new();
    tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut resp_line).await?;
    let msg: DaemonServerMessage = serde_json::from_str(resp_line.trim())?;
    match msg {
        DaemonServerMessage::ObservabilitySummary { summary } => Ok(summary),
        _ => bail!("unexpected daemon response"),
    }
}

fn print_daemon_summary(
    summary: &ObservabilitySummaryResponse,
    command: &ObservabilityCommand,
    json: bool,
) -> Result<()> {
    match command {
        ObservabilityCommand::TokensSummary { .. } => {
            if json {
                println!("{}", serde_json::to_string_pretty(&summary.token_summary)?);
            } else {
                println!(
                    "backend\tcount\tinput_tokens_mean\toutput_tokens_mean\tnormalized_total_mean"
                );
                for entry in &summary.token_summary {
                    println!(
                        "{}\t{}\t{}\t{}\t{}",
                        entry.backend,
                        entry.count,
                        entry
                            .input_tokens_mean
                            .map(|v| format!("{:.1}", v))
                            .unwrap_or_else(|| "-".to_string()),
                        entry
                            .output_tokens_mean
                            .map(|v| format!("{:.1}", v))
                            .unwrap_or_else(|| "-".to_string()),
                        entry
                            .normalized_total_mean
                            .map(|v| format!("{:.1}", v))
                            .unwrap_or_else(|| "-".to_string()),
                    );
                }
            }
        }
        ObservabilityCommand::TokensRecent { .. } => {
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&summary.recent_token_executions)?
                );
            } else {
                println!(
                    "id\tbackend\tmodel\tinput_tokens\toutput_tokens\tnormalized_total_tokens"
                );
                for entry in &summary.recent_token_executions {
                    println!(
                        "{}\t{}\t{}\t{}\t{}\t{}",
                        entry.execution_id.as_deref().unwrap_or(&entry.id),
                        entry.backend,
                        entry.model.as_deref().unwrap_or("-"),
                        entry
                            .input_tokens
                            .map(|v| v.to_string())
                            .unwrap_or_else(|| "-".to_string()),
                        entry
                            .output_tokens
                            .map(|v| v.to_string())
                            .unwrap_or_else(|| "-".to_string()),
                        entry
                            .normalized_total_tokens
                            .map(|v| v.to_string())
                            .unwrap_or_else(|| "-".to_string()),
                    );
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn parse_observability_args(args: &[String]) -> Result<ObservabilityCommand> {
    match args.first().map(String::as_str) {
        Some("logging") => parse_logging_args(&args[1..]),
        Some("tokens") => parse_tokens_args(&args[1..]),
        Some("metrics") => Ok(ObservabilityCommand::Metrics {
            prometheus: args.iter().any(|arg| arg == "--prometheus"),
        }),
        Some("logs") => {
            let execution_id = args
                .get(1)
                .context("Usage: iota observability logs <execution_id>")?;
            Ok(ObservabilityCommand::Logs {
                execution_id: execution_id.clone(),
            })
        }
        Some("trace") => {
            let trace_id = args
                .get(1)
                .context("Usage: iota observability trace <trace_id>")?;
            Ok(ObservabilityCommand::Trace {
                trace_id: trace_id.clone(),
            })
        }
        _ => bail!("Usage: iota observability <logging|tokens|metrics|logs|trace> ..."),
    }
}

fn parse_logging_args(args: &[String]) -> Result<ObservabilityCommand> {
    match args.first().map(String::as_str) {
        Some("recent") => Ok(ObservabilityCommand::LoggingRecent {
            limit: parse_limit(args, 20)?,
        }),
        Some("events") => {
            let execution_id = args
                .get(1)
                .context("Usage: iota observability logging events <execution_id>")?;
            Ok(ObservabilityCommand::LoggingEvents {
                execution_id: execution_id.clone(),
            })
        }
        _ => bail!("Usage: iota observability logging <recent|events> ..."),
    }
}

fn parse_tokens_args(args: &[String]) -> Result<ObservabilityCommand> {
    match args.first().map(String::as_str) {
        Some("recent") => Ok(ObservabilityCommand::TokensRecent {
            limit: parse_limit(args, 20)?,
            json: has_json_flag(args),
        }),
        Some("summary") => Ok(ObservabilityCommand::TokensSummary {
            since_secs: parse_since(args, 3600)?,
            json: has_json_flag(args),
        }),
        Some("export") => Ok(ObservabilityCommand::TokensExport {
            format: parse_format(args),
        }),
        _ => bail!("Usage: iota observability tokens <recent|summary|export> ..."),
    }
}

fn parse_limit(args: &[String], default: usize) -> Result<usize> {
    match args.iter().position(|arg| arg == "--limit") {
        Some(index) => Ok(args
            .get(index + 1)
            .context("--limit requires a value")?
            .parse()
            .context("--limit must be an integer")?),
        None => Ok(default),
    }
}

fn parse_since(args: &[String], default_secs: i64) -> Result<i64> {
    let Some(index) = args.iter().position(|arg| arg == "--since") else {
        return Ok(default_secs);
    };
    let value = args.get(index + 1).context("--since requires a value")?;
    let (number, multiplier) = match value.chars().last() {
        Some('s') => (&value[..value.len() - 1], 1),
        Some('m') => (&value[..value.len() - 1], 60),
        Some('h') => (&value[..value.len() - 1], 3600),
        Some('d') => (&value[..value.len() - 1], 86_400),
        _ => (value.as_str(), 1),
    };
    let amount: i64 = number
        .parse()
        .context("--since must be like 60s, 15m, 2h, or 1d")?;
    Ok(amount * multiplier)
}

fn parse_format(args: &[String]) -> String {
    args.iter()
        .position(|arg| arg == "--format")
        .and_then(|index| args.get(index + 1))
        .cloned()
        .unwrap_or_else(|| "json".to_string())
}

fn has_json_flag(args: &[String]) -> bool {
    args.iter()
        .any(|arg| arg == "--json" || arg == "--format=json")
}

fn print_token_recent(store: &ObservabilityStore, limit: usize, json: bool) -> Result<()> {
    let records = store.recent_token_executions(limit)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&records)?);
    } else {
        print_recent_table(&records);
    }
    Ok(())
}

fn print_recent_table(records: &[StoredTokenUsage]) {
    println!(
        "execution_id\tbackend\tprovider\tinput\tcache_read\tcache_write\toutput\tthinking\tprovider_total\tnormalized_total"
    );
    for record in records {
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            record.execution_id.as_deref().unwrap_or("-"),
            record.backend,
            record.provider.as_deref().unwrap_or("-"),
            fmt_opt(record.input_tokens),
            fmt_opt(record.cache_read_input_tokens),
            fmt_opt(record.cache_creation_input_tokens),
            fmt_opt(record.output_tokens),
            fmt_opt(record.thinking_tokens),
            fmt_opt(record.provider_reported_total_tokens),
            fmt_opt(record.normalized_total_tokens)
        );
    }
}

fn print_summary_table(summaries: &[TokenUsageSummary]) {
    println!(
        "backend\tcount\tinput\tcache_read\tcache_creation\toutput\tthinking\tprovider_total\tnormalized_total"
    );
    for summary in summaries {
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            summary.backend,
            summary.count,
            fmt_mean_std_cv(
                summary.input_tokens_mean,
                summary.input_tokens_stddev,
                summary.input_tokens_cv
            ),
            fmt_mean_std_cv(
                summary.cache_read_input_tokens_mean,
                summary.cache_read_input_tokens_stddev,
                summary.cache_read_input_tokens_cv
            ),
            fmt_mean_std_cv(
                summary.cache_creation_input_tokens_mean,
                summary.cache_creation_input_tokens_stddev,
                summary.cache_creation_input_tokens_cv
            ),
            fmt_mean_std_cv(
                summary.output_tokens_mean,
                summary.output_tokens_stddev,
                summary.output_tokens_cv
            ),
            fmt_mean_std_cv(
                summary.thinking_tokens_mean,
                summary.thinking_tokens_stddev,
                summary.thinking_tokens_cv
            ),
            fmt_mean_std_cv(
                summary.provider_reported_total_mean,
                summary.provider_reported_total_stddev,
                summary.provider_reported_total_cv
            ),
            fmt_mean_std_cv(
                summary.normalized_total_mean,
                summary.normalized_total_stddev,
                summary.normalized_total_cv
            ),
        );
    }
}

fn print_prometheus_metrics(summaries: &[TokenUsageSummary]) {
    let usage_count: u64 = summaries.iter().map(|summary| summary.count).sum();
    println!("iota_token_usage_count {}", usage_count);
    for summary in summaries {
        let labels = format!("{{backend=\"{}\"}}", summary.backend);
        println!("iota_token_usage_count{} {}", labels, summary.count);
        if let Some(mean) = summary.input_tokens_mean {
            println!(
                "iota_token_input_total{} {}",
                labels,
                mean * summary.count as f64
            );
        }
        if let Some(mean) = summary.cache_read_input_tokens_mean {
            println!(
                "iota_token_cache_read_total{} {}",
                labels,
                mean * summary.count as f64
            );
        }
        if let Some(mean) = summary.cache_creation_input_tokens_mean {
            println!(
                "iota_token_cache_creation_total{} {}",
                labels,
                mean * summary.count as f64
            );
        }
        if let Some(mean) = summary.output_tokens_mean {
            println!(
                "iota_token_output_total{} {}",
                labels,
                mean * summary.count as f64
            );
        }
        if let Some(mean) = summary.thinking_tokens_mean {
            println!(
                "iota_token_thinking_total{} {}",
                labels,
                mean * summary.count as f64
            );
        }
        if let Some(mean) = summary.provider_reported_total_mean {
            println!(
                "iota_token_provider_reported_total{} {}",
                labels,
                mean * summary.count as f64
            );
        }
        if let Some(mean) = summary.normalized_total_mean {
            println!(
                "iota_token_normalized_total{} {}",
                labels,
                mean * summary.count as f64
            );
        }
    }
}

fn fmt_opt(value: Option<u64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".to_string())
}

/// Format as "mean±std(CV=x%)" when stddev is available, or just "mean" when not.
fn fmt_mean_std_cv(mean: Option<f64>, stddev: Option<f64>, cv: Option<f64>) -> String {
    let Some(mean) = mean else {
        return "-".to_string();
    };
    match (stddev, cv) {
        (Some(std), Some(cv)) => format!("{mean:.1}±{std:.1}(CV={:.0}%)", cv * 100.0),
        (Some(std), None) => format!("{mean:.1}±{std:.1}"),
        _ => format!("{mean:.1}"),
    }
}

#[cfg(test)]
#[path = "observability_cmd_tests.rs"]
mod tests;
