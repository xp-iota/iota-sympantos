pub mod metrics;
pub mod redact;
pub mod stderr;

use anyhow::Result;
use opentelemetry::KeyValue;
use opentelemetry::global;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::{LogExporter, MetricExporter, SpanExporter, WithExportConfig};
use opentelemetry_sdk::{
    Resource, logs::SdkLoggerProvider, metrics::SdkMeterProvider, trace::SdkTracerProvider,
};
use tracing_subscriber::{EnvFilter, Layer, layer::SubscriberExt, util::SubscriberInitExt};

pub struct TelemetryConfig {
    pub endpoint: String,
    pub enabled: bool,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        let endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok();
        let enabled = std::env::var("OTEL_ENABLED").ok();
        Self::from_env_values(endpoint.as_deref(), enabled.as_deref())
    }
}

impl TelemetryConfig {
    fn from_env_values(endpoint: Option<&str>, enabled: Option<&str>) -> Self {
        let endpoint = endpoint.filter(|value| !value.trim().is_empty());
        Self {
            endpoint: endpoint.unwrap_or("http://localhost:4317").to_string(),
            enabled: enabled
                .map(otel_enabled_value)
                .unwrap_or_else(|| endpoint.is_some()),
        }
    }
}

fn otel_enabled_value(value: &str) -> bool {
    !matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "false" | "0" | "no" | "off"
    )
}

pub struct OtelGuard {
    tracer_provider: Option<SdkTracerProvider>,
    meter_provider: Option<SdkMeterProvider>,
    logger_provider: Option<SdkLoggerProvider>,
}

impl Drop for OtelGuard {
    fn drop(&mut self) {
        if let Some(tp) = self.tracer_provider.take() {
            let _ = tp.shutdown();
        }
        if let Some(mp) = self.meter_provider.take() {
            let _ = mp.shutdown();
        }
        if let Some(lp) = self.logger_provider.take() {
            let _ = lp.shutdown();
        }
    }
}

fn build_resource() -> Resource {
    let version = env!("CARGO_PKG_VERSION");
    let host = hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown".to_string());
    Resource::builder()
        .with_attributes([
            KeyValue::new("service.name", "iota"),
            KeyValue::new("service.version", version),
            KeyValue::new("host.name", host),
        ])
        .build()
}

pub fn init(config: &TelemetryConfig) -> Result<OtelGuard> {
    let resource = build_resource();

    if !config.enabled {
        let filter = logging_filter();
        tracing_subscriber::registry()
            .with(stderr::stderr_layer().with_filter(filter))
            .try_init()
            .ok();
        return Ok(OtelGuard {
            tracer_provider: None,
            meter_provider: None,
            logger_provider: None,
        });
    }

    // Traces
    let span_exporter = SpanExporter::builder()
        .with_tonic()
        .with_endpoint(&config.endpoint)
        .build()?;
    let tracer_provider = SdkTracerProvider::builder()
        .with_resource(resource.clone())
        .with_batch_exporter(span_exporter)
        .build();
    global::set_tracer_provider(tracer_provider.clone());

    // Metrics
    let metric_exporter = MetricExporter::builder()
        .with_tonic()
        .with_endpoint(&config.endpoint)
        .build()?;
    let metric_reader =
        opentelemetry_sdk::metrics::PeriodicReader::builder(metric_exporter).build();
    let meter_provider = SdkMeterProvider::builder()
        .with_resource(resource.clone())
        .with_reader(metric_reader)
        .build();
    global::set_meter_provider(meter_provider.clone());

    // Logs
    let log_exporter = LogExporter::builder()
        .with_tonic()
        .with_endpoint(&config.endpoint)
        .build()?;
    let logger_provider = SdkLoggerProvider::builder()
        .with_resource(resource)
        .with_batch_exporter(log_exporter)
        .build();

    // tracing-opentelemetry bridge
    let otel_trace_layer =
        tracing_opentelemetry::layer().with_tracer(tracer_provider.tracer("iota"));
    let otel_log_layer =
        opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge::new(&logger_provider);
    let filter = logging_filter();
    let stderr_layer = stderr::stderr_layer();

    tracing_subscriber::registry()
        .with(filter)
        .with(otel_trace_layer)
        .with(otel_log_layer)
        .with(stderr_layer)
        .try_init()
        .ok();

    Ok(OtelGuard {
        tracer_provider: Some(tracer_provider),
        meter_provider: Some(meter_provider),
        logger_provider: Some(logger_provider),
    })
}

fn logging_filter() -> EnvFilter {
    let env_val = std::env::var("IOTA_LOG")
        .or_else(|_| std::env::var("RUST_LOG"))
        .unwrap_or_else(|_| "warn,iota_core=info".to_string());
    EnvFilter::try_new(&env_val).unwrap_or_else(|_| EnvFilter::new("warn,iota_core=info"))
}

#[cfg(test)]
#[path = "telemetry_tests.rs"]
mod tests;
