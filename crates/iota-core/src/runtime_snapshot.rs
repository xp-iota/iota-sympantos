//! Neutral runtime-context snapshot types shared by `engine` and
//! `daemon::proto`.
//!
//! `IotaEngine` records a snapshot of the most recent turn's assembled
//! context capsule (for desktop/TUI inspectors) and the desktop daemon wire
//! protocol serializes that same snapshot to clients. Both sides need the
//! same types, but `engine` must not depend on `daemon` (daemon is a
//! transport/presentation concern layered above the engine, per
//! `docs/architecture.md`). These types therefore live in this dependency-free
//! module: `engine` owns and populates them directly, and `daemon::proto`
//! re-exports them for the desktop wire format instead of redefining them.
//!
//! This module intentionally has no dependency on `daemon` or any
//! presentation-layer crate.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ContextBudgetsSnapshot {
    pub memory_chars: usize,
    pub skills_chars: usize,
    pub working_memory_chars: usize,
    pub workspace_chars: usize,
    pub handoff_chars: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContextSection {
    pub name: String,
    pub chars: usize,
    pub preview: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeContextSnapshot {
    pub turn_id: String,
    pub backend: String,
    pub cwd: PathBuf,
    pub session_id: String,
    pub model: Option<String>,
    pub created_at: i64,
    pub capsule_text: String,
    pub sections: Vec<ContextSection>,
    pub budgets: ContextBudgetsSnapshot,
}
