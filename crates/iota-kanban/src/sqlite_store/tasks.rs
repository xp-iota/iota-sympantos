//! Task CRUD and status transitions for [`super::SqliteKanbanStore`].

use anyhow::{Result, bail};
use rusqlite::params;
use std::path::PathBuf;

use crate::state_machine::validate_transition;
use crate::types::{CreateTaskRequest, Status, Task, TaskFilter, TaskId, TaskPatch};
use crate::utils::now_ts;

use super::SqliteKanbanStore;

impl SqliteKanbanStore {
    pub(super) fn create_task_on_conn(
        conn: &rusqlite::Connection,
        req: CreateTaskRequest,
    ) -> Result<TaskId> {
        let now = now_ts();
        let status = req.status.unwrap_or(Status::Triage).as_str();
        let priority = req.priority.unwrap_or(0);
        let tags_json = serde_json::to_string(&req.tags)?;
        let workspace_path = req
            .workspace_path
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned());
        let id = Self::new_entity_id_on_conn(conn, "tasks")?;
        conn.execute(
            "INSERT INTO tasks
             (id, board_id, title, body, status, assignee, priority, tags,
              workspace_kind, workspace_path, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
            params![
                id,
                req.board_id as i64,
                req.title,
                req.body,
                status,
                req.assignee,
                priority as i64,
                tags_json,
                req.workspace_kind,
                workspace_path,
                now,
            ],
        )?;
        Ok(id as u64)
    }

    pub(super) fn get_task_impl(&self, id: TaskId) -> Result<Task> {
        let conn = self.lock_conn();
        conn.query_row(
            "SELECT id, board_id, title, body, status, assignee, priority, tags,
                    workspace_kind, workspace_path, created_at, updated_at,
                    claimed_at, claim_ttl_secs
             FROM tasks WHERE id = ?1",
            params![id as i64],
            row_to_task,
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => anyhow::anyhow!("task {} not found", id),
            other => other.into(),
        })
    }

    pub(super) fn update_task_on_conn(
        conn: &rusqlite::Connection,
        id: TaskId,
        patch: TaskPatch,
    ) -> Result<()> {
        use rusqlite::types::Value;

        let mut parts: Vec<String> = Vec::new();
        let mut values: Vec<Value> = Vec::new();
        let status_patch = patch.status;
        let now = now_ts();

        if let Some(v) = patch.title {
            parts.push(format!("title = ?{}", values.len() + 1));
            values.push(Value::Text(v));
        }
        if let Some(v) = patch.body {
            parts.push(format!("body = ?{}", values.len() + 1));
            values.push(v.map(Value::Text).unwrap_or(Value::Null));
        }
        if let Some(v) = status_patch {
            parts.push(format!("status = ?{}", values.len() + 1));
            values.push(Value::Text(v.as_str().to_owned()));
            if v == Status::Running {
                parts.push(format!("claimed_at = ?{}", values.len() + 1));
                values.push(Value::Integer(now));
            }
        }
        if let Some(v) = patch.assignee {
            parts.push(format!("assignee = ?{}", values.len() + 1));
            values.push(v.map(Value::Text).unwrap_or(Value::Null));
        }
        if let Some(v) = patch.priority {
            parts.push(format!("priority = ?{}", values.len() + 1));
            values.push(Value::Integer(v as i64));
        }
        if let Some(v) = patch.tags {
            parts.push(format!("tags = ?{}", values.len() + 1));
            values.push(Value::Text(serde_json::to_string(&v)?));
        }
        if let Some(v) = patch.workspace_kind {
            parts.push(format!("workspace_kind = ?{}", values.len() + 1));
            values.push(v.map(Value::Text).unwrap_or(Value::Null));
        }
        if let Some(v) = patch.workspace_path {
            parts.push(format!("workspace_path = ?{}", values.len() + 1));
            values.push(
                v.map(|p| Value::Text(p.to_string_lossy().into_owned()))
                    .unwrap_or(Value::Null),
            );
        }

        if parts.is_empty() {
            return Ok(());
        }

        parts.push(format!("updated_at = ?{}", values.len() + 1));
        values.push(Value::Integer(now));
        let id_param = values.len() + 1;
        let sql = format!(
            "UPDATE tasks SET {} WHERE id = ?{id_param}",
            parts.join(", ")
        );
        values.push(Value::Integer(id as i64));

        if let Some(to) = status_patch {
            let current_str: String = conn
                .query_row(
                    "SELECT status FROM tasks WHERE id = ?1",
                    params![id as i64],
                    |row| row.get(0),
                )
                .map_err(|e| match e {
                    rusqlite::Error::QueryReturnedNoRows => {
                        anyhow::anyhow!("task {} not found", id)
                    }
                    other => anyhow::Error::from(other),
                })?;
            let from: Status = current_str.parse()?;
            validate_transition(from, to)?;
        }
        let rows = conn.execute(&sql, rusqlite::params_from_iter(values))?;
        if rows == 0 {
            bail!("task {} not found", id);
        }
        Ok(())
    }

    pub(super) fn list_tasks_impl(&self, filter: TaskFilter) -> Result<Vec<Task>> {
        use rusqlite::types::Value;

        let mut conditions: Vec<String> = Vec::new();
        let mut values: Vec<Value> = Vec::new();

        if let Some(board_id) = filter.board_id {
            conditions.push(format!("board_id = ?{}", values.len() + 1));
            values.push(Value::Integer(board_id as i64));
        }
        if let Some(status) = filter.status {
            conditions.push(format!("status = ?{}", values.len() + 1));
            values.push(Value::Text(status.as_str().to_owned()));
        }
        if let Some(assignee) = filter.assignee {
            conditions.push(format!("assignee = ?{}", values.len() + 1));
            values.push(Value::Text(assignee));
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };
        let limit_clause = match filter.limit {
            Some(n) => format!("LIMIT {n}"),
            None => String::new(),
        };
        let sql = format!(
            "SELECT id, board_id, title, body, status, assignee, priority, tags,
                    workspace_kind, workspace_path, created_at, updated_at,
                    claimed_at, claim_ttl_secs
             FROM tasks {where_clause}
             ORDER BY priority DESC, created_at ASC
             {limit_clause}"
        );

        let conn = self.lock_conn();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(values), row_to_task)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub(super) fn delete_task_on_conn(conn: &rusqlite::Connection, id: TaskId) -> Result<()> {
        let rows = conn.execute("DELETE FROM tasks WHERE id = ?1", params![id as i64])?;
        if rows == 0 {
            bail!("task {} not found", id);
        }
        Ok(())
    }

    pub(super) fn transition_on_conn(
        conn: &rusqlite::Connection,
        id: TaskId,
        to: Status,
    ) -> Result<()> {
        let current_str: String = conn
            .query_row(
                "SELECT status FROM tasks WHERE id = ?1",
                params![id as i64],
                |row| row.get(0),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => anyhow::anyhow!("task {} not found", id),
                other => anyhow::Error::from(other),
            })?;
        let from: Status = current_str.parse()?;
        validate_transition(from, to)?;
        let now = now_ts();
        if to == Status::Running {
            conn.execute(
                "UPDATE tasks SET status = ?1, claimed_at = ?2, updated_at = ?2 WHERE id = ?3",
                params![to.as_str(), now, id as i64],
            )?;
        } else {
            conn.execute(
                "UPDATE tasks SET status = ?1, updated_at = ?2 WHERE id = ?3",
                params![to.as_str(), now, id as i64],
            )?;
        }
        Ok(())
    }
}

pub(super) fn row_to_task(row: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
    let status_str: String = row.get(4)?;
    let status = status_str
        .parse::<Status>()
        .map_err(|e| super::parse_err(4, e.to_string()))?;

    let tags_json: String = row.get(7)?;
    let tags: Vec<String> =
        serde_json::from_str(&tags_json).map_err(|e| super::parse_err(7, e.to_string()))?;

    let workspace_path: Option<String> = row.get(9)?;

    Ok(Task {
        id: row.get::<_, i64>(0)? as u64,
        board_id: row.get::<_, i64>(1)? as u64,
        title: row.get(2)?,
        body: row.get(3)?,
        status,
        assignee: row.get(5)?,
        priority: row.get::<_, i64>(6)? as i32,
        tags,
        workspace_kind: row.get(8)?,
        workspace_path: workspace_path.map(PathBuf::from),
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
        claimed_at: row.get(12)?,
        claim_ttl_secs: row.get(13)?,
    })
}
