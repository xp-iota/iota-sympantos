use anyhow::{Context, Result, bail};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use super::shadow::ShadowMaterializer;
use super::store::KanbanStore;
use super::types::*;

pub struct AdvancedBridge {
    hermes_bin: PathBuf,
    timeout: Duration,
    materializer: ShadowMaterializer,
}

#[derive(Debug)]
pub struct SpecifyResult {
    pub task_id: TaskId,
    pub spec_body: String,
}

#[derive(Debug)]
pub struct DecomposeResult {
    pub parent_id: TaskId,
    pub child_ids: Vec<TaskId>,
}

impl AdvancedBridge {
    pub fn new(hermes_bin: PathBuf, shadows_dir: PathBuf) -> Self {
        Self {
            hermes_bin,
            timeout: Duration::from_secs(120),
            materializer: ShadowMaterializer::new(shadows_dir),
        }
    }

    #[cfg(test)]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Check if hermes binary is available
    pub fn is_available(&self) -> bool {
        Command::new(&self.hermes_bin)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// Expand a vague task description into a structured spec via LLM
    pub fn specify(&self, task_id: TaskId, store: &dyn KanbanStore) -> Result<SpecifyResult> {
        if !self.hermes_bin.exists() {
            anyhow::bail!("hermes binary not found: {}", self.hermes_bin.display());
        }
        let task = store.get_task(task_id)?;
        let board = self.get_board_for_task(&task, store)?;
        let shadow = self.materializer.materialize(&task, &board, store)?;

        let result = {
            let mut command = Command::new(&self.hermes_bin);
            command
                .args(["kanban", "specify", &task_id.to_string(), "--json"])
                .env("HERMES_KANBAN_DB", &shadow.path)
                .env("HERMES_KANBAN_BOARD", &board.slug);
            run_with_timeout(&mut command, self.timeout)
                .with_context(|| "failed to run hermes kanban specify")
        };
        let cleanup_result = self.materializer.cleanup(task_id);
        let output = result?;
        cleanup_result?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("hermes kanban specify failed: {}", stderr.trim());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);

        // Try to parse as JSON, fall back to raw text as the spec body
        let spec_body = if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&stdout) {
            parsed
                .get("spec")
                .and_then(|v| v.as_str())
                .or_else(|| parsed.get("body").and_then(|v| v.as_str()))
                .unwrap_or(&stdout)
                .to_string()
        } else {
            stdout.trim().to_string()
        };

        // Write spec back to task body
        store.update_task(
            task_id,
            super::types::TaskPatch {
                body: Some(Some(spec_body.clone())),
                ..Default::default()
            },
        )?;

        Ok(SpecifyResult { task_id, spec_body })
    }

    /// Decompose a large task into subtasks via LLM
    pub fn decompose(&self, task_id: TaskId, store: &dyn KanbanStore) -> Result<DecomposeResult> {
        if !self.hermes_bin.exists() {
            anyhow::bail!("hermes binary not found: {}", self.hermes_bin.display());
        }
        let task = store.get_task(task_id)?;
        let board = self.get_board_for_task(&task, store)?;
        let shadow = self.materializer.materialize(&task, &board, store)?;

        let existing_shadow_task_ids = read_shadow_task_ids(&shadow.path)?;

        let result = (|| -> Result<Vec<ShadowTaskRow>> {
            let mut command = Command::new(&self.hermes_bin);
            command
                .args(["kanban", "decompose", &task_id.to_string(), "--json"])
                .env("HERMES_KANBAN_DB", &shadow.path)
                .env("HERMES_KANBAN_BOARD", &board.slug);
            let output = run_with_timeout(&mut command, self.timeout)
                .with_context(|| "failed to run hermes kanban decompose")?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                bail!("hermes kanban decompose failed: {}", stderr.trim());
            }

            read_new_shadow_tasks(&shadow.path, &existing_shadow_task_ids)
        })();
        let cleanup_result = self.materializer.cleanup(task_id);
        let new_tasks = result?;
        cleanup_result?;

        // Import new tasks into iota store
        let mut child_ids = Vec::new();
        for shadow_task in new_tasks {
            let new_id = store.create_task(CreateTaskRequest {
                board_id: task.board_id,
                title: shadow_task.title,
                body: shadow_task.body,
                status: None, // triage
                assignee: shadow_task.assignee,
                priority: Some(task.priority),
                tags: task.tags.clone(),
                workspace_kind: task.workspace_kind.clone(),
                workspace_path: task.workspace_path.clone(),
            })?;
            store.create_link(task_id, new_id, LinkKind::Parent)?;
            child_ids.push(new_id);
        }

        Ok(DecomposeResult {
            parent_id: task_id,
            child_ids,
        })
    }

    fn get_board_for_task(&self, task: &Task, store: &dyn KanbanStore) -> Result<Board> {
        store
            .list_boards()?
            .into_iter()
            .find(|b| b.id == task.board_id)
            .ok_or_else(|| anyhow::anyhow!("board not found for task {}", task.id))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ShadowTaskRow {
    title: String,
    body: Option<String>,
    assignee: Option<String>,
}

fn read_shadow_task_ids(db_path: &Path) -> Result<Vec<String>> {
    let conn = rusqlite::Connection::open(db_path)?;
    let mut stmt = conn.prepare("SELECT id FROM tasks")?;
    let ids = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ids)
}

fn read_new_shadow_tasks(db_path: &Path, existing_ids: &[String]) -> Result<Vec<ShadowTaskRow>> {
    let existing_ids: HashSet<&str> = existing_ids.iter().map(|s| s.as_str()).collect();
    let conn = rusqlite::Connection::open(db_path)?;
    let mut stmt = conn.prepare("SELECT id, title, body, assignee FROM tasks ORDER BY id")?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            ShadowTaskRow {
                title: row.get(1)?,
                body: row.get(2)?,
                assignee: row.get(3)?,
            },
        ))
    })?;

    let mut tasks = Vec::new();
    for row in rows {
        let (id, task) = row?;
        if !existing_ids.contains(id.as_str()) {
            tasks.push(task);
        }
    }
    Ok(tasks)
}

fn run_with_timeout(command: &mut Command, timeout: Duration) -> Result<Output> {
    configure_process_tree_root(command);
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let started = std::time::Instant::now();

    loop {
        if child.try_wait()?.is_some() {
            return child.wait_with_output().map_err(Into::into);
        }
        if started.elapsed() >= timeout {
            kill_child_tree(&mut child);
            bail!("hermes command timed out after {}ms", timeout.as_millis());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn configure_process_tree_root(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(not(unix))]
    {
        let _ = command;
    }
}

fn kill_child_tree(child: &mut std::process::Child) {
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(not(windows))]
    {
        let pid = child.id().to_string();
        let _ = Command::new("kill")
            .args(["-TERM", &format!("-{}", pid)])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = child.kill();
    }
}

// ---------------------------------------------------------------------------
// Public helpers
// ---------------------------------------------------------------------------

/// Returns `Ok(())` if the bridge is available, or an `Err` with a human-readable
/// message when the hermes binary is not found or not executable.
///
/// Use this before calling `specify` or `decompose` to surface a clear error
/// to the user instead of a cryptic process spawn failure.
pub fn ensure_bridge_available(bridge: &AdvancedBridge) -> Result<()> {
    if bridge.is_available() {
        Ok(())
    } else {
        anyhow::bail!(
            "hermes binary not found or not executable: {}",
            bridge.hermes_bin.display()
        )
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "bridge_tests.rs"]
mod tests;
