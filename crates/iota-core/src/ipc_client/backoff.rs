//! Exponential-backoff reconnect state machine and heartbeat tunables shared
//! by desktop IPC clients. This module is transport-agnostic: it computes
//! delays and tracks connection state, but does not itself open sockets or
//! know about any wire protocol (see `super::autostart` for spawning a
//! sidecar, and each consumer's own client module for the actual read/write
//! loop).

/// Lifecycle of a persistent local-daemon connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    Connected,
    Reconnecting,
    Disconnected,
}

/// Tunables for exponential-backoff reconnection with jitter.
#[derive(Debug, Clone, Copy)]
pub struct ReconnectConfig {
    pub initial_delay_ms: u64,
    pub max_delay_ms: u64,
    /// Jitter applied as a +/- percentage of the current delay, to avoid
    /// thundering-herd reconnects when multiple clients drop at once.
    pub jitter_percent: u8,
}

impl Default for ReconnectConfig {
    fn default() -> Self {
        Self {
            initial_delay_ms: 1_000,
            max_delay_ms: 30_000,
            jitter_percent: 20,
        }
    }
}

/// Tunables for a periodic ping/pong liveness check over an established
/// connection.
#[derive(Debug, Clone, Copy)]
pub struct HeartbeatConfig {
    pub interval_secs: u64,
    /// Consecutive missed pongs before the connection is declared dead and
    /// reconnection is triggered.
    pub max_misses: u8,
}

impl Default for HeartbeatConfig {
    fn default() -> Self {
        Self {
            interval_secs: 30,
            max_misses: 3,
        }
    }
}

/// Apply +/- jitter to the current reconnect delay without advancing the
/// exponential-backoff state. Keeping these two operations separate avoids
/// forcing callers to seed the state with `initial_delay_ms / 2`.
///
/// `random_factor` is clamped to `[0.0, 1.0]`, so malformed or external
/// randomness sources cannot produce an unbounded delay.
pub fn backoff_delay_ms(delay_ms: u64, config: &ReconnectConfig, random_factor: f64) -> u64 {
    let random_factor = if random_factor.is_finite() {
        random_factor.clamp(0.0, 1.0)
    } else {
        0.5
    };
    let jitter_range = delay_ms as f64 * (config.jitter_percent as f64 / 100.0);
    let jitter = (random_factor * 2.0 - 1.0) * jitter_range;
    ((delay_ms as f64) + jitter).max(100.0) as u64
}

/// Advance the unjittered reconnect delay, doubling it with saturation and
/// capping it at the configured maximum.
pub fn next_backoff_delay_ms(delay_ms: u64, config: &ReconnectConfig) -> u64 {
    delay_ms.saturating_mul(2).min(config.max_delay_ms)
}

/// Return a time-derived factor spanning `[0.0, 1.0)`. This is intentionally
/// lightweight: reconnect jitter does not require cryptographic randomness.
pub fn time_jitter_factor() -> f64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    (nanos as f64) / 1_000_000_000.0
}

#[cfg(test)]
#[path = "backoff_tests.rs"]
mod tests;
