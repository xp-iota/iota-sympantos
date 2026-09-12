//! Generic local-daemon IPC client primitives, shared by every desktop app
//! that talks to a sidecar/daemon process over a loopback TCP JSON-line
//! protocol (see `iota-desktop`'s daemon client and `cockpit-desktop`'s
//! runner client for the two current consumers).
//!
//! This module intentionally knows nothing about any concrete wire protocol
//! (no `DaemonClientMessage`, no `RunnerCommand`, ...). It only captures the
//! parts that are identical regardless of payload shape:
//!
//! - [`negotiate_version`] — pure min/max protocol version negotiation.
//! - [`ConnectionState`] / [`ReconnectConfig`] / [`backoff_delay_ms`] —
//!   the exponential-backoff-with-jitter reconnect state machine.
//! - [`HeartbeatConfig`] — shared tunables for a ping/pong liveness loop.
//! - [`autostart`] — locate and spawn a sidecar binary, then wait for either
//!   raw TCP availability or a caller-provided protocol readiness probe.
//!
//! Concrete consumers own their own wire message enums, `Hello`/`Ping`
//! framing, and TCP read/write loops; they compose this module's building
//! blocks around that domain-specific protocol.

mod autostart;
mod backoff;
mod version;

pub use autostart::{
    AutostartError, locate_sidecar_binary, spawn_sidecar, spawn_sidecar_with_probe, wait_for_port,
};
pub use backoff::{
    ConnectionState, HeartbeatConfig, ReconnectConfig, backoff_delay_ms, next_backoff_delay_ms,
    time_jitter_factor,
};
pub use version::{VersionNegotiationError, negotiate_version};
