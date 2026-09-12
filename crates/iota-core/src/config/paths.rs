use anyhow::{Context, Result};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct StorePaths {
    root: PathBuf,
}

impl StorePaths {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn resolve() -> Result<Self> {
        let home = dirs::home_dir().context("Failed to get home directory")?;
        Ok(Self::new(home.join(".i6").join("context")))
    }

    pub fn events_db(&self) -> PathBuf {
        self.root.join("events.sqlite")
    }

    pub fn memory_db(&self) -> PathBuf {
        self.root.join("memory.sqlite")
    }

    pub fn store_db(&self) -> PathBuf {
        self.root.join("store.sqlite")
    }
}

/// Name of the environment variable that relocates the Kanban data directory.
///
/// Tests and throwaway environments set this to a temp directory so they never
/// open (and mutate) the developer's live `~/.i6/kanban` database. Mirrors the
/// `IOTA_DAEMON_TOKEN_PATH` override in `daemon::auth`.
pub const KANBAN_DIR_ENV: &str = "IOTA_KANBAN_DIR";

/// Resolves the Kanban data directory (`~/.i6/kanban` by default).
fn kanban_dir() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os(KANBAN_DIR_ENV) {
        return Some(PathBuf::from(path));
    }
    Some(dirs::home_dir()?.join(".i6").join("kanban"))
}

/// The default `SqliteKanbanStore` database path (`~/.i6/kanban/iota.db`).
///
/// Kept alongside `StorePaths` so every `~/.i6`-rooted path this crate
/// resolves goes through one module instead of each caller re-deriving
/// `dirs::home_dir().join(".i6")...` independently. `iota-kanban` is a
/// separate, dependency-free published crate and cannot use this helper
/// directly; it resolves the same path with its own local
/// `crate::paths::default_shadows_dir` for that reason.
///
/// Honors [`KANBAN_DIR_ENV`].
pub fn kanban_db_path() -> Option<PathBuf> {
    Some(kanban_dir()?.join("iota.db"))
}

/// The default Kanban shadow-workspace directory (`~/.i6/kanban/shadows`).
///
/// Honors [`KANBAN_DIR_ENV`].
pub fn kanban_shadows_dir() -> Option<PathBuf> {
    Some(kanban_dir()?.join("shadows"))
}
