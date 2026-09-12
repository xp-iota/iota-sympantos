//! Board CRUD for [`super::SqliteKanbanStore`].

use anyhow::Result;
use rusqlite::params;

use crate::types::Board;

use super::SqliteKanbanStore;

impl SqliteKanbanStore {
    pub(super) fn list_boards_impl(&self) -> Result<Vec<Board>> {
        let conn = self.lock_conn();
        let mut stmt =
            conn.prepare("SELECT id, slug, name, created_at FROM boards ORDER BY slug")?;
        let rows = stmt.query_map([], row_to_board)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub(super) fn get_board_impl(&self, slug: &str) -> Result<Board> {
        let conn = self.lock_conn();
        conn.query_row(
            "SELECT id, slug, name, created_at FROM boards WHERE slug = ?1",
            params![slug],
            row_to_board,
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => anyhow::anyhow!("board '{}' not found", slug),
            other => other.into(),
        })
    }
}

pub(super) fn row_to_board(row: &rusqlite::Row<'_>) -> rusqlite::Result<Board> {
    Ok(Board {
        id: row.get::<_, i64>(0)? as u64,
        slug: row.get(1)?,
        name: row.get(2)?,
        created_at: row.get(3)?,
    })
}
