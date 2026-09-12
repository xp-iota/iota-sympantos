//! Reusable runtime primitives for orchestrating ACP coding assistants.
//!
//! The module tree remains public for advanced integrations. The root exports
//! below cover the normal engine construction and execution path.

pub mod acp;
pub mod config;
pub mod context;
pub mod daemon;
pub mod engine;
pub mod fs_secure;
pub mod ipc_client;
pub mod mcp;
pub mod memory;
pub mod resources;
pub mod runtime_event;
pub mod runtime_snapshot;
pub mod skill;
pub mod store;
pub mod telemetry;
pub mod utils;

pub use acp::{AcpBackend, AcpPromptOutput, DEFAULT_TIMEOUT_MS, TurnCancelled};
pub use config::NimiaConfig;
pub use engine::IotaEngine;
pub use resources::LocalResources;
pub use runtime_event::RuntimeEvent;
