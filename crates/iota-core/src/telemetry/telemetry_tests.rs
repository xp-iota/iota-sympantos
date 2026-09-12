use crate::telemetry::TelemetryConfig;

#[test]
fn otlp_is_disabled_without_explicit_env() {
    let config = TelemetryConfig::from_env_values(None, None);

    assert!(!config.enabled);
    assert_eq!(config.endpoint, "http://localhost:4317");
}

#[test]
fn endpoint_env_enables_otlp() {
    let config = TelemetryConfig::from_env_values(Some("http://otel:4317"), None);

    assert!(config.enabled);
    assert_eq!(config.endpoint, "http://otel:4317");
}

#[test]
fn enabled_env_can_enable_default_endpoint() {
    let config = TelemetryConfig::from_env_values(None, Some("true"));

    assert!(config.enabled);
    assert_eq!(config.endpoint, "http://localhost:4317");
}

#[test]
fn enabled_env_can_disable_explicit_endpoint() {
    let config = TelemetryConfig::from_env_values(Some("http://otel:4317"), Some("false"));

    assert!(!config.enabled);
    assert_eq!(config.endpoint, "http://otel:4317");
}

#[test]
fn empty_endpoint_does_not_enable_otlp() {
    let config = TelemetryConfig::from_env_values(Some("  "), None);

    assert!(!config.enabled);
    assert_eq!(config.endpoint, "http://localhost:4317");
}
