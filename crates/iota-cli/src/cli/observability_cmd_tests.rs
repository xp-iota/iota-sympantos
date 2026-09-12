use crate::cli::observability_cmd::*;

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

#[test]
fn parses_tokens_recent_json_limit() {
    let command =
        parse_observability_args(&args(&["tokens", "recent", "--limit", "7", "--json"])).unwrap();

    assert!(matches!(
        command,
        ObservabilityCommand::TokensRecent {
            limit: 7,
            json: true
        }
    ));
}

#[test]
fn parses_tokens_summary_since_hours() {
    let command = parse_observability_args(&args(&["tokens", "summary", "--since", "2h"])).unwrap();

    assert!(matches!(
        command,
        ObservabilityCommand::TokensSummary {
            since_secs: 7200,
            json: false
        }
    ));
}

#[test]
fn parses_prometheus_metrics() {
    let command = parse_observability_args(&args(&["metrics", "--prometheus"])).unwrap();

    assert!(matches!(
        command,
        ObservabilityCommand::Metrics { prometheus: true }
    ));
}

#[test]
fn parses_logs_alias() {
    let command = parse_observability_args(&args(&["logs", "exec-1"])).unwrap();

    assert!(matches!(
        command,
        ObservabilityCommand::Logs { execution_id } if execution_id == "exec-1"
    ));
}

#[test]
fn parses_trace_alias() {
    let command = parse_observability_args(&args(&["trace", "trace-1"])).unwrap();

    assert!(matches!(
        command,
        ObservabilityCommand::Trace { trace_id } if trace_id == "trace-1"
    ));
}
