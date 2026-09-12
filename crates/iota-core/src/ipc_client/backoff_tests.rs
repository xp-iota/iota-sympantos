use crate::ipc_client::backoff::{
    ConnectionState, ReconnectConfig, backoff_delay_ms, next_backoff_delay_ms, time_jitter_factor,
};

#[test]
fn jitter_does_not_advance_the_backoff_state() {
    let config = ReconnectConfig {
        initial_delay_ms: 1_000,
        max_delay_ms: 30_000,
        jitter_percent: 0,
    };
    assert_eq!(backoff_delay_ms(1_000, &config, 0.5), 1_000);
    assert_eq!(next_backoff_delay_ms(1_000, &config), 2_000);
}

#[test]
fn next_backoff_delay_is_capped_at_the_configured_maximum() {
    let config = ReconnectConfig {
        initial_delay_ms: 1_000,
        max_delay_ms: 5_000,
        jitter_percent: 0,
    };
    assert_eq!(next_backoff_delay_ms(4_000, &config), 5_000);
    assert_eq!(next_backoff_delay_ms(u64::MAX, &config), 5_000);
}

#[test]
fn jitter_stays_within_the_configured_percentage_band() {
    let config = ReconnectConfig {
        initial_delay_ms: 1_000,
        max_delay_ms: 30_000,
        jitter_percent: 20,
    };
    let low = backoff_delay_ms(2_000, &config, 0.0);
    let high = backoff_delay_ms(2_000, &config, 1.0);
    assert_eq!(low, 1_600);
    assert_eq!(high, 2_400);
}

#[test]
fn jitter_factor_is_clamped_and_non_finite_values_use_the_midpoint() {
    let config = ReconnectConfig {
        initial_delay_ms: 1_000,
        max_delay_ms: 30_000,
        jitter_percent: 20,
    };
    assert_eq!(backoff_delay_ms(1_000, &config, -10.0), 800);
    assert_eq!(backoff_delay_ms(1_000, &config, 10.0), 1_200);
    assert_eq!(backoff_delay_ms(1_000, &config, f64::NAN), 1_000);
}

#[test]
fn delay_never_drops_below_the_100ms_floor() {
    let config = ReconnectConfig {
        initial_delay_ms: 0,
        max_delay_ms: 0,
        jitter_percent: 100,
    };
    assert_eq!(backoff_delay_ms(0, &config, 0.0), 100);
}

#[test]
fn time_jitter_factor_is_in_the_documented_range() {
    let factor = time_jitter_factor();
    assert!((0.0..1.0).contains(&factor));
}

#[test]
fn connection_state_round_trips_through_snake_case_json() {
    let json = serde_json::to_string(&ConnectionState::Reconnecting).unwrap();
    assert_eq!(json, "\"reconnecting\"");
    let parsed: ConnectionState = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, ConnectionState::Reconnecting);
}
