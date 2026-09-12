//! Local resource inputs supplied by the application embedding iota-core.
//!
//! `iota-sympantos-core` is publishable as a Cargo dependency. It never
//! downloads project content: applications provide their local skill roots and
//! configuration, keeping project policy and credentials on the host machine.

use std::path::PathBuf;

#[derive(Debug, Clone, Default)]
pub struct LocalResources {
    skill_roots: Vec<PathBuf>,
}

impl LocalResources {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_skill_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.skill_roots.push(root.into());
        self
    }

    /// Standard project-local resource layout. The host owns this directory;
    /// iota-core merely receives the paths and reads them locally.
    pub fn from_workspace(workspace: impl Into<PathBuf>) -> Self {
        let workspace = workspace.into();
        Self::new()
            .with_skill_root(workspace.join("skills"))
            .with_skill_root(workspace.join(".iota").join("skills"))
    }

    pub fn skill_roots(&self) -> &[PathBuf] {
        &self.skill_roots
    }
}

#[cfg(test)]
#[path = "resources_tests.rs"]
mod tests;
