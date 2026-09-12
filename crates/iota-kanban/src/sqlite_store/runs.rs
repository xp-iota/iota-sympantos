//! Run lifecycle (start/complete/heartbeat) for [`super::SqliteKanbanStore`].

use anyhow::{Result, bail};
use rusqlite::params;
use uuid::Uuid;

use crate::types::{Run, RunId, RunStatus, TaskId};
use crate::utils::now_ts;

use super::SqliteKanbanStore;

impl SqliteKanbanStore {
    pub(super) fn create_run_on_conn(
        conn: &rusqlite::Connection,
        task_id: TaskId,
        profile: &str,
    ) -> Result<RunId> {
        let id = Uuid::new_v4().to_string();
        let now = now_ts();
        conn.execute(
            "INSERT INTO runs (id, task_id, profile, status, started_at, last_heartbeat)
             VALUES (?1, ?2, ?3, 'running', ?4, ?4)",
            params![id, task_id as i64, profile, now],
        )?;
        Ok(id)
    }

    pub(super) fn complete_run_on_conn(
        conn: &rusqlite::Connection,
        run_id: &str,
        status: RunStatus,
        exit_code: Option<i32>,
    ) -> Result<()> {
        let now = now_ts();
        let rows = conn.execute(
            "UPDATE runs SET status = ?1, finished_at = ?2, exit_code = ?3 WHERE id = ?4",
            params![status.as_str(), now, exit_code, run_id],
        )?;
        if rows == 0 {
            bail!("run '{}' not found", run_id);
        }
        Ok(())
    }

    pub(super) fn heartbeat_impl(&self, run_id: &str) -> Result<()> {
        let now = now_ts();
        let conn = self.lock_conn();
        let rows = conn.execute(
            "UPDATE runs SET last_heartbeat = ?1 WHERE id = ?2 AND status = 'running'",
            params![now, run_id],
        )?;
        if rows == 0 {
            bail!("run '{}' not found or already finished", run_id);
        }
        Ok(())
    }

    pub(super) fn get_runs_impl(&self, task_id: TaskId) -> Result<Vec<Run>> {
        let conn = self.lock_conn();
        let mut stmt = conn.prepare(
            "SELECT id, task_id, profile, status, started_at, finished_at,
                    last_heartbeat, exit_code, output_summary
             FROM runs WHERE task_id = ?1 ORDER BY started_at DESC",
        )?;
        let rows = stmt.query_map(params![task_id as i64], row_to_run)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }
}

pub(super) fn row_to_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<Run> {
    let status_str: String = row.get(3)?;
    let status = status_str
        .parse::<RunStatus>()
        .map_err(|e| super::parse_err(3, e.to_string()))?;
    Ok(Run {
        id: row.get(0)?,
        task_id: row.get::<_, i64>(1)? as u64,
        profile: row.get(2)?,
        status,
        started_at: row.get(4)?,
        finished_at: row.get(5)?,
        last_heartbeat: row.get(6)?,
        exit_code: row.get(7)?,
        output_summary: row.get(8)?,
    })
}
