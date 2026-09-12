use criterion::{Criterion, criterion_group, criterion_main};

fn bench_jsonline_throughput(c: &mut Criterion) {
    let msg = serde_json::json!({
        "type": "text_chunk",
        "turn_id": "bench-turn-001",
        "chunk": "x".repeat(100)
    });

    c.bench_function("daemon_jsonline_serialize", |b| {
        b.iter(|| {
            let _ = serde_json::to_vec(&msg).unwrap();
        });
    });

    let serialized = serde_json::to_vec(&msg).unwrap();
    c.bench_function("daemon_jsonline_deserialize", |b| {
        b.iter(|| {
            let _: serde_json::Value = serde_json::from_slice(&serialized).unwrap();
        });
    });
}

fn bench_first_token_latency(c: &mut Criterion) {
    let hello = serde_json::json!({
        "type": "hello",
        "client_name": "bench-client",
        "protocol_version": 2,
        "min_version": 2,
        "max_version": 3
    });
    let hello_accepted = serde_json::json!({
        "type": "hello_accepted",
        "protocol_version": 3,
        "negotiated_version": 3
    });
    let start_turn = serde_json::json!({
        "type": "start_turn",
        "turn_id": "bench-turn-001",
        "cwd": "/tmp/bench",
        "backend": "codex",
        "prompt": "hello world",
        "timeout_ms": 600000
    });
    let text_chunk = serde_json::json!({
        "type": "text_chunk",
        "turn_id": "bench-turn-001",
        "chunk": "First token output"
    });

    c.bench_function("daemon_first_token_serialize_chain", |b| {
        b.iter(|| {
            let _ = serde_json::to_vec(&hello).unwrap();
            let _ = serde_json::to_vec(&hello_accepted).unwrap();
            let _ = serde_json::to_vec(&start_turn).unwrap();
            let _ = serde_json::to_vec(&text_chunk).unwrap();
        });
    });
}

criterion_group!(
    benches,
    bench_jsonline_throughput,
    bench_first_token_latency
);
criterion_main!(benches);
