use super::observability::ObservabilityStore;
use crate::runtime_event::token_usage_from_value;
use serde_json::json;
use std::path::Path;

fn assert_float_close(actual: Option<f64>, expected: f64) {
    let actual = actual.unwrap();
    let rel_err = (actual - expected).abs() / expected.abs();
    assert!(
        rel_err < 1e-9,
        "expected {expected}, got {actual} (relative error {rel_err:.2e})"
    );
}

#[test]
fn records_and_queries_token_usage_by_execution_id() {
    let store = ObservabilityStore::open(Path::new(":memory:")).unwrap();
    let usage = token_usage_from_value(&json!({
        "model": "claude-test",
        "usage": {
            "input_tokens": 277,
            "cache_read_input_tokens": 24154,
            "cache_creation_input_tokens": 3215,
            "output_tokens": 85
        }
    }))
    .unwrap();

    store
        .record_token_usage(Some("exec-1"), Some("session-1"), "claude-code", &usage)
        .unwrap();

    let records = store.token_usage_for_execution("exec-1").unwrap();
    assert_eq!(records.len(), 1);
    let record = &records[0];
    assert_eq!(record.execution_id.as_deref(), Some("exec-1"));
    assert_eq!(record.session_id.as_deref(), Some("session-1"));
    assert_eq!(record.backend, "claude-code");
    assert_eq!(record.provider.as_deref(), Some("anthropic"));
    assert_eq!(record.input_tokens, Some(277));
    assert_eq!(record.cache_read_input_tokens, Some(24154));
    assert_eq!(record.cache_creation_input_tokens, Some(3215));
    assert_eq!(record.output_tokens, Some(85));
    assert_eq!(
        record.normalized_total_tokens,
        Some(277 + 24154 + 3215 + 85)
    );
    assert_eq!(
        record.raw_payload["usage"]["cache_creation_input_tokens"],
        3215
    );
}

#[test]
fn summarizes_recent_token_usage_by_backend() {
    let store = ObservabilityStore::open(Path::new(":memory:")).unwrap();
    for total in [100_u64, 140, 160] {
        let usage = token_usage_from_value(&json!({
            "usage": {
                "inputTokens": total - 10,
                "outputTokens": 10,
                "totalTokens": total
            }
        }))
        .unwrap();
        store
            .record_token_usage(
                Some(&format!("exec-{total}")),
                Some("session"),
                "opencode",
                &usage,
            )
            .unwrap();
    }

    let summaries = store.token_summary_since(0).unwrap();
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].backend, "opencode");
    assert_eq!(summaries[0].count, 3);
    assert_eq!(summaries[0].provider_reported_total_mean, Some(400.0 / 3.0));
    assert_eq!(summaries[0].normalized_total_mean, Some(400.0 / 3.0));
    assert_float_close(
        summaries[0].provider_reported_total_stddev,
        30.550504633038933,
    );
    assert_float_close(summaries[0].normalized_total_stddev, 30.550504633038933);
    assert_float_close(summaries[0].provider_reported_total_cv, 0.229128784747792);
    assert_float_close(summaries[0].normalized_total_cv, 0.229128784747792);
}

#[test]
fn summarizes_cache_and_thinking_token_means() {
    let store = ObservabilityStore::open(Path::new(":memory:")).unwrap();
    let usage = token_usage_from_value(&json!({
        "usage": {
            "inputTokens": 100,
            "cachedReadTokens": 40,
            "cachedWriteTokens": 5,
            "outputTokens": 20,
            "thoughtTokens": 7,
            "totalTokens": 167
        }
    }))
    .unwrap();
    store
        .record_token_usage(Some("exec"), Some("session"), "claude-code", &usage)
        .unwrap();

    let summaries = store.token_summary_since(0).unwrap();
    assert_eq!(summaries[0].cache_read_input_tokens_mean, Some(40.0));
    assert_eq!(summaries[0].cache_creation_input_tokens_mean, Some(5.0));
    assert_eq!(summaries[0].thinking_tokens_mean, Some(7.0));
}

#[test]
fn summary_deduplicates_multiple_token_events_for_one_execution() {
    let store = ObservabilityStore::open(Path::new(":memory:")).unwrap();
    let usage_update = token_usage_from_value(&json!({
        "sessionUpdate": "usage_update",
        "used": 100,
        "size": 200000
    }))
    .unwrap();
    let final_usage = token_usage_from_value(&json!({
        "usage": {
            "inputTokens": 90,
            "outputTokens": 10,
            "totalTokens": 100
        }
    }))
    .unwrap();

    store
        .record_token_usage(Some("exec-1"), Some("session"), "opencode", &usage_update)
        .unwrap();
    store
        .record_token_usage(Some("exec-1"), Some("session"), "opencode", &final_usage)
        .unwrap();

    let recent = store.recent_token_executions(10).unwrap();
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].source, "usage");

    let summaries = store.token_summary_since(0).unwrap();
    assert_eq!(summaries[0].count, 1);
    assert_eq!(summaries[0].normalized_total_mean, Some(100.0));
}

#[test]
fn observability_metrics_records_latency_and_throughput() {
    use super::observability::ObservabilityMetrics;

    let metrics = ObservabilityMetrics::new();

    metrics.record_write_latency(1.5);
    metrics.record_write_latency(2.0);
    metrics.record_write_latency(3.0);
    metrics.record_write_latency(0.5);

    let percentiles = metrics.write_latency_percentiles();
    assert_eq!(percentiles.count, 4);
    assert!(percentiles.p50_ms.is_some());
    assert!(percentiles.p99_ms.is_some());

    metrics.record_stream_throughput(100.0);
    metrics.record_stream_throughput(200.0);

    let throughput = metrics.stream_throughput_summary();
    assert_eq!(throughput.count, 2);
    assert_eq!(throughput.mean_tokens_per_sec, Some(150.0));
}

#[test]
fn record_token_usage_with_metrics_records_latency() {
    use super::observability::ObservabilityMetrics;

    let store = ObservabilityStore::open(Path::new(":memory:")).unwrap();
    let metrics = ObservabilityMetrics::new();
    let usage = token_usage_from_value(&json!({
        "model": "test-model",
        "usage": { "input_tokens": 10, "output_tokens": 5 }
    }))
    .unwrap();

    store
        .record_token_usage_with_metrics(&metrics, Some("exec-m"), Some("sess"), "codex", &usage)
        .unwrap();

    let percentiles = metrics.write_latency_percentiles();
    assert_eq!(percentiles.count, 1);
    assert!(percentiles.p50_ms.unwrap() >= 0.0);
}
