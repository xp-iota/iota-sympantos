---
name: iota-src-telemetry
description: Use when working on OpenTelemetry setup, OTLP export, stderr logging, metrics counters/histograms, telemetry shutdown, or files under crates/iota-core/src/telemetry.
triggers:
  - crates/iota-core/src/telemetry
  - TelemetryConfig
  - OtelGuard
  - IotaMetrics
  - OTLP
  - IOTA_LOG
---

# telemetry — Observability

OpenTelemetry integration providing OTLP trace/log/metric export, stderr logging, and runtime metrics.

## Responsibilities

- Initialize OpenTelemetry SDK with OTLP exporter
- Structured logging via `tracing` subscriber
- Metrics via OpenTelemetry counters, up-down counters, and histograms
- Console output formatting

## Sub-modules

| Module | Purpose |
| :--------| :---------|
| `stderr` | Console log formatting and filtering |
| `metrics` | Runtime metrics: execution count, queued prompts, token counts, prompt/init duration |
| `mod` | OTLP trace/log/metric provider setup and shutdown guard |

## Key Types

- `TelemetryConfig` — OTLP endpoint and enabled flag
- `OtelGuard` — RAII guard for graceful shutdown
- `IotaMetrics` — lazily initialized metric instruments
