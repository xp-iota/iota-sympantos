//! Default filesystem paths for this crate's `~/.i6/kanban` state.
//!
//! `iota-kanban` is published independently and has no dependency on
//! `iota-core`, so it cannot reuse `iota_core::config::paths` (which
//! resolves the sibling `~/.i6/kanban/iota.db` store path). This module is
//! the single place within this crate where `~/.i6`-rooted defaults are
//! derived, so `DispatcherConfig::default` and any future caller do not
//! each re-derive `dirs::home_dir().join(".i6")...` independently.

use std::path::PathBuf;

/// The default Kanban shadow-workspace directory (`~/.i6/kanban/shadows`),
/// used by `DispatcherConfig::default` when the caller does not override
/// `shadows_dir`. Falls back to the current directory if the home
/// directory cannot be resolved, matching the previous inline default.
pub fn default_shadows_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".i6")
        .join("kanban")
        .join("shadows")
}
