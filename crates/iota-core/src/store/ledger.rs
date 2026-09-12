use anyhow::{Context, Result};
use rusqlite::{OptionalExtension, params};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;

use crate::utils::now_ts;

#[derive(Clone)]
pub struct SessionLedger {
    pool: Arc<super::db::DbPool>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionSummary {
    pub iota_session_id: String,
    pub cwd: String,
    pub active_backend: Option<String>,
    pub turn_count: i64,
    pub last_output_summary: Option<String>,
}

impl SessionLedger {
    pub fn open(path: &Path) -> Result<Self> {
        let pool = super::db::DbPool::shared(path)?;
        let ledger = Self { pool };
        ledger.init()?;
        Ok(ledger)
    }

    fn init(&self) -> Result<()> {
        let conn = self.pool.write("ledger.init");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS sessions (iota_session_id TEXT PRIMARY KEY, cwd TEXT NOT NULL, active_backend TEXT, model TEXT, turn_count INTEGER DEFAULT 0, created_at INTEGER NOT NULL, last_used_at INTEGER NOT NULL);\nCREATE TABLE IF NOT EXISTS backend_sessions (iota_session_id TEXT NOT NULL, backend TEXT NOT NULL, backend_session_id TEXT, cwd TEXT NOT NULL, created_at INTEGER NOT NULL, last_used_at INTEGER NOT NULL, PRIMARY KEY (iota_session_id, backend, cwd));\nCREATE TABLE IF NOT EXISTS turns (turn_id TEXT PRIMARY KEY, iota_session_id TEXT NOT NULL, backend TEXT NOT NULL, execution_id TEXT, prompt_hash TEXT, output_summary TEXT, status TEXT, started_at INTEGER, finished_at INTEGER);\nCREATE TABLE IF NOT EXISTS handoffs (iota_session_id TEXT NOT NULL, from_backend TEXT, to_backend TEXT, cwd TEXT NOT NULL, summary TEXT NOT NULL, created_at INTEGER NOT NULL);",
        )?;
        Ok(())
    }

    pub fn default_path() -> Result<PathBuf> {
        Ok(crate::config::paths::StorePaths::resolve()?.store_db())
    }

    pub fn latest_session_for_cwd(&self, cwd: &Path) -> Result<Option<String>> {
        let conn = self.pool.read("ledger.latest_session_for_cwd");
        conn.query_row(
            "SELECT iota_session_id FROM sessions WHERE cwd = ?1 ORDER BY last_used_at DESC LIMIT 1",
            params![cwd.display().to_string()],
            |row| row.get(0),
        )
        .optional()
        .context("Failed to read latest session")
    }

    pub fn ensure_session(
        &self,
        session_id: &str,
        cwd: &Path,
        active_backend: Option<&str>,
        model: Option<&str>,
    ) -> Result<()> {
        let now = now_ts();
        let conn = self.pool.write("ledger.ensure_session");
        conn.execute(
            "INSERT INTO sessions (iota_session_id, cwd, active_backend, model, turn_count, created_at, last_used_at)\n             VALUES (?1, ?2, ?3, ?4, 0, ?5, ?5)\n             ON CONFLICT(iota_session_id) DO UPDATE SET cwd=excluded.cwd, active_backend=COALESCE(excluded.active_backend, sessions.active_backend), model=COALESCE(excluded.model, sessions.model), last_used_at=excluded.last_used_at",
            params![session_id, cwd.display().to_string(), active_backend, model, now],
        )?;
        Ok(())
    }

    pub fn record_backend_session(
        &self,
        session_id: &str,
        backend: &str,
        backend_session_id: Option<&str>,
        cwd: &Path,
    ) -> Result<()> {
        let now = now_ts();
        let conn = self.pool.write("ledger.record_backend_session");
        conn.execute(
            "INSERT INTO backend_sessions (iota_session_id, backend, backend_session_id, cwd, created_at, last_used_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)\n             ON CONFLICT(iota_session_id, backend, cwd) DO UPDATE SET backend_session_id=COALESCE(excluded.backend_session_id, backend_sessions.backend_session_id), last_used_at=excluded.last_used_at",
            params![session_id, backend, backend_session_id, cwd.display().to_string(), now],
        )?;
        Ok(())
    }

    pub fn record_turn(
        &self,
        session_id: &str,
        backend: &str,
        execution_id: Option<&str>,
        prompt_hash: &str,
        output_summary: &str,
        status: &str,
    ) -> Result<String> {
        let turn_id = Uuid::new_v4().to_string();
        let now = now_ts();
        let conn = self.pool.write("ledger.record_turn");
        conn.execute(
            "INSERT INTO turns (turn_id, iota_session_id, backend, execution_id, prompt_hash, output_summary, status, started_at, finished_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
            params![turn_id, session_id, backend, execution_id, prompt_hash, output_summary, status, now],
        )?;
        conn.execute(
            "UPDATE sessions SET active_backend = ?2, turn_count = turn_count + 1, last_used_at = ?3 WHERE iota_session_id = ?1",
            params![session_id, backend, now],
        )?;
        Ok(turn_id)
    }

    pub fn publish_handoff(
        &self,
        session_id: &str,
        from_backend: Option<&str>,
        to_backend: Option<&str>,
        cwd: &Path,
        summary: &str,
    ) -> Result<()> {
        let conn = self.pool.write("ledger.publish_handoff");
        conn.execute(
            "INSERT INTO handoffs (iota_session_id, from_backend, to_backend, cwd, summary, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![session_id, from_backend, to_backend, cwd.display().to_string(), summary, now_ts()],
        )?;
        Ok(())
    }

    pub fn read_handoff(
        &self,
        session_id: &str,
        to_backend: Option<&str>,
        cwd: &Path,
    ) -> Result<Option<String>> {
        let conn = self.pool.read("ledger.read_handoff");
        conn.query_row(
            "SELECT summary FROM handoffs WHERE iota_session_id = ?1 AND cwd = ?2 AND (?3 IS NULL OR to_backend = ?3 OR to_backend IS NULL) ORDER BY created_at DESC LIMIT 1",
            params![session_id, cwd.display().to_string(), to_backend],
            |row| row.get(0),
        ).optional().context("Failed to read handoff")
    }

    pub fn summary(&self, session_id: &str) -> Result<Option<SessionSummary>> {
        let conn = self.pool.read("ledger.summary");
        conn.query_row(
            "SELECT s.iota_session_id, s.cwd, s.active_backend, s.turn_count, (SELECT output_summary FROM turns t WHERE t.iota_session_id = s.iota_session_id ORDER BY finished_at DESC LIMIT 1)\n             FROM sessions s WHERE s.iota_session_id = ?1",
            params![session_id],
            |row| Ok(SessionSummary {
                iota_session_id: row.get(0)?,
                cwd: row.get(1)?,
                active_backend: row.get(2)?,
                turn_count: row.get(3)?,
                last_output_summary: row.get(4)?,
            }),
        )
        .optional()
        .context("Failed to read session summary")
    }

    #[allow(dead_code)]
    pub fn session_stats(&self, session_id: &str) -> Result<Option<(i64, Option<i64>, i64)>> {
        let conn = self.pool.read("ledger.session_stats");
        conn.query_row(
            "SELECT s.turn_count,
                (SELECT COUNT(*) FROM turns t WHERE t.iota_session_id = s.iota_session_id),
                (SELECT COUNT(DISTINCT backend) FROM turns t WHERE t.iota_session_id = s.iota_session_id)
         FROM sessions s WHERE s.iota_session_id = ?1",
            params![session_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .context("Failed to read session stats")
    }

    #[allow(dead_code)]
    pub fn get_handoff_history(&self, session_id: &str) -> Result<Vec<(String, String, String)>> {
        let conn = self.pool.read("ledger.get_handoff_history");
        let mut stmt = conn.prepare(
            "SELECT from_backend, to_backend, summary FROM handoffs
         WHERE iota_session_id = ?1
         ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            Ok((
                row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                row.get::<_, String>(2)?,
            ))
        })?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }
}

#[cfg(test)]
#[path = "ledger_tests.rs"]
mod ledger_tests;
