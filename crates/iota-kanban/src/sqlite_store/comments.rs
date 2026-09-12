//! Comment CRUD for [`super::SqliteKanbanStore`].

use anyhow::Result;
use rusqlite::params;

use crate::types::{Comment, CommentId, TaskId};
use crate::utils::now_ts;

use super::SqliteKanbanStore;

impl SqliteKanbanStore {
    pub(super) fn add_comment_on_conn(
        conn: &rusqlite::Connection,
        task_id: TaskId,
        author: &str,
        body: &str,
    ) -> Result<CommentId> {
        let now = now_ts();
        let id = Self::new_entity_id_on_conn(conn, "comments")?;
        conn.execute(
            "INSERT INTO comments (id, task_id, author, body, created_at, comment_seq)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                id,
                task_id as i64,
                author,
                body,
                now,
                Self::next_comment_seq(conn)?
            ],
        )?;
        Ok(id as u64)
    }

    /// Local insertion-order counter. Comment `id`s are collision-resistant
    /// random values shared across nodes (see `event_sync`), and `created_at`
    /// is whole seconds, so neither can order two comments added in the same
    /// second. This counter is per-store and monotonic, mirroring how the
    /// `events` table uses its local rowid for ordering.
    pub(super) fn next_comment_seq(conn: &rusqlite::Connection) -> Result<i64> {
        Ok(conn.query_row(
            "SELECT COALESCE(MAX(comment_seq), 0) + 1 FROM comments",
            [],
            |row| row.get(0),
        )?)
    }

    pub(super) fn list_comments_impl(&self, task_id: TaskId) -> Result<Vec<Comment>> {
        let conn = self.lock_conn();
        // Order by the local insertion counter, falling back to `created_at`
        // for rows written before the `comment_seq` migration.
        let mut stmt = conn.prepare(
            "SELECT id, task_id, author, body, created_at
             FROM comments WHERE task_id = ?1
             ORDER BY COALESCE(comment_seq, created_at), created_at",
        )?;
        let rows = stmt.query_map(params![task_id as i64], row_to_comment)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }
}

pub(super) fn row_to_comment(row: &rusqlite::Row<'_>) -> rusqlite::Result<Comment> {
    Ok(Comment {
        id: row.get::<_, i64>(0)? as u64,
        task_id: row.get::<_, i64>(1)? as u64,
        author: row.get(2)?,
        body: row.get(3)?,
        created_at: row.get(4)?,
    })
}
