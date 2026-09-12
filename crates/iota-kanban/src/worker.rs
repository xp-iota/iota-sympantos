use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::io::Write;

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Instant;

use super::types::*;

// ---------------------------------------------------------------------------
// WorkerConfig
// ---------------------------------------------------------------------------

pub struct WorkerConfig {
    pub hermes_bin: PathBuf,
    /// Extra environment variables forwarded to the hermes worker process.
    /// Use this to inject inference-provider config (HERMES_INFERENCE_PROVIDER, etc.).
    pub extra_env: BTreeMap<String, String>,
}

// ---------------------------------------------------------------------------
// WorkerEnv
// ---------------------------------------------------------------------------

pub struct WorkerEnv {
    pub task_id: TaskId,
    /// The iota-side run ID (UUID string) for the main store.
    pub run_id: String,
    /// The hermes-compatible integer run ID from the shadow's `task_runs` table.
    pub shadow_run_id: i64,
    pub shadow_path: PathBuf,
    pub board_slug: String,
    pub profile: String,
}

// ---------------------------------------------------------------------------
// WorkerHandle
// ---------------------------------------------------------------------------

pub struct WorkerHandle {
    pub run_id: RunId,
    pub child: Child,
    pub started_at: Instant,
}

impl WorkerHandle {
    /// Spawn `hermes -p <profile> --yolo -z "work kanban task <id>"` with
    /// the full kanban toolset. Uses oneshot mode (`-z`) which bypasses
    /// prompt_toolkit / terminal output — safe for piped stdout.
    /// Hermes reads the task via `kanban_show` (which connects to the shadow
    /// DB via `HERMES_KANBAN_DB`), executes the work, and calls
    /// `kanban_complete(summary=..., metadata=...)` to transition the task
    /// to done with structured handoff data.
    pub fn spawn(config: &WorkerConfig, env: WorkerEnv) -> Result<Self> {
        // Route stdout/stderr to log files alongside the shadow directory
        // (not inside it, so cleanup won't delete them before we read).
        let shadow_dir = env
            .shadow_path
            .parent()
            .unwrap_or(std::path::Path::new("."));
        let logs_dir = shadow_dir.parent().unwrap_or(shadow_dir);
        let stderr_path = logs_dir.join(format!("{}.stderr.log", env.task_id));
        let stdout_path = logs_dir.join(format!("{}.stdout.log", env.task_id));
        let stderr_file = std::fs::File::create(&stderr_path)
            .with_context(|| format!("creating stderr log {}", stderr_path.display()))?;
        let stdout_file = std::fs::File::create(&stdout_path)
            .with_context(|| format!("creating stdout log {}", stdout_path.display()))?;
        let mut stderr_file = stderr_file;
        writeln!(
            stderr_file,
            "[iota] starting hermes worker task={} run={} profile={} shadow_db={}",
            env.task_id,
            env.run_id,
            env.profile,
            env.shadow_path.display()
        )
        .with_context(|| format!("writing stderr log header {}", stderr_path.display()))?;

        let prompt = format!("work kanban task {}", env.task_id);

        let mut command = Command::new(&config.hermes_bin);
        command
            .args(["-p", &env.profile, "--yolo", "-z", &prompt])
            .env("HERMES_KANBAN_TASK", env.task_id.to_string())
            .env("HERMES_KANBAN_RUN_ID", env.shadow_run_id.to_string())
            .env(
                "HERMES_KANBAN_DB",
                env.shadow_path.to_string_lossy().as_ref(),
            )
            .env("HERMES_KANBAN_BOARD", &env.board_slug)
            .envs(&config.extra_env)
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout_file))
            .stderr(Stdio::from(stderr_file));
        configure_process_tree_root(&mut command);
        let child = command.spawn().context("spawning hermes process")?;

        Ok(Self {
            run_id: env.run_id,
            child,
            started_at: Instant::now(),
        })
    }

    /// Returns the exit code if the child has finished, or `None` if still running.
    pub fn is_finished(&mut self) -> Option<i32> {
        match self.child.try_wait() {
            Ok(Some(status)) => Some(status.code().unwrap_or(-1)),
            _ => None,
        }
    }

    /// Kill the child process.
    pub fn kill(&mut self) -> Result<()> {
        kill_process_tree(&mut self.child).context("killing hermes process")
    }

    /// Seconds since the worker was spawned.
    pub fn elapsed_secs(&self) -> u64 {
        self.started_at.elapsed().as_secs()
    }
}

impl Drop for WorkerHandle {
    fn drop(&mut self) {
        // Best-effort: kill the process tree and wait for it to exit.
        // This prevents orphaned PowerShell/consulate windows from lingering
        // if the WorkerHandle is removed from the Dispatcher without an
        // explicit kill() call (e.g. health_check removing a finished worker).
        let _ = kill_process_tree(&mut self.child);
        let _ = self.child.wait();
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
        // On Windows, we rely on `taskkill /T /F` in kill_process_tree to recursively
        // terminate the entire process tree. There is no cross-platform way to set a
        // new process group without the windows-sys crate (CREATE_NEW_PROCESS_GROUP),
        // so we accept the current behaviour: explicit kill() uses taskkill, and Drop
        // also calls kill_process_tree to clean up.
        let _ = command;
    }
}

fn kill_process_tree(child: &mut Child) -> Result<()> {
    #[cfg(windows)]
    {
        let status = Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .context("running taskkill")?;
        if !status.success() {
            child.kill()?;
        }
        Ok(())
    }
    #[cfg(unix)]
    {
        let pid = child.id().to_string();
        let _ = Command::new("kill")
            .args(["-TERM", "--", &format!("-{}", pid)])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = Command::new("kill")
            .args(["-KILL", "--", &format!("-{}", pid)])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        child.kill()?;
        Ok(())
    }
}

#[cfg(test)]
fn write_context_or_kill(
    child: &mut Child,
    context: &str,
    write_context: impl FnOnce(&mut Child, &str) -> std::io::Result<()>,
) -> Result<()> {
    if let Err(err) = write_context(child, context) {
        let _ = kill_process_tree(child);
        let _ = child.wait();
        return Err(err).context("writing context to hermes stdin");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// build_worker_context (test-only, retained for unit tests)
// ---------------------------------------------------------------------------

/// Build a markdown context string for hermes, including the task details,
/// any prior runs, and any comments.
#[cfg(test)]
pub fn build_worker_context(task: &Task, comments: &[Comment], prior_runs: &[Run]) -> String {
    let body = task.body.as_deref().unwrap_or("");
    let mut out = format!("# Task: {}\n\n{}", task.title, body);

    if !prior_runs.is_empty() {
        out.push_str("\n\n## Prior attempts\n");
        for run in prior_runs {
            let summary = run.output_summary.as_deref().unwrap_or("none");
            out.push_str(&format!(
                "- run `{}` (profile: {}, status: {}, summary: {})\n",
                run.id, run.profile, run.status, summary
            ));
        }
    }

    if !comments.is_empty() {
        out.push_str("\n\n## Comments\n");
        for comment in comments {
            out.push_str(&format!("- {}: {}\n", comment.author, comment.body));
        }
    }

    out
}

#[cfg(test)]
#[path = "worker_tests.rs"]
mod worker_tests;
