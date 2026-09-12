//! Link CRUD for [`super::SqliteKanbanStore`].

use anyhow::Result;
use rusqlite::params;

use crate::types::{Link, LinkKind, TaskId};

use super::SqliteKanbanStore;

impl SqliteKanbanStore {
    pub(super) fn create_link_on_conn(
        conn: &rusqlite::Connection,
        from: TaskId,
        to: TaskId,
        kind: LinkKind,
    ) -> Result<()> {
        conn.execute(
            "INSERT OR IGNORE INTO links (from_id, to_id, kind) VALUES (?1, ?2, ?3)",
            params![from as i64, to as i64, kind.as_str()],
        )?;
        Ok(())
    }

    pub(super) fn remove_link_on_conn(
        conn: &rusqlite::Connection,
        from: TaskId,
        to: TaskId,
        kind: LinkKind,
    ) -> Result<()> {
        conn.execute(
            "DELETE FROM links WHERE from_id = ?1 AND to_id = ?2 AND kind = ?3",
            params![from as i64, to as i64, kind.as_str()],
        )?;
        Ok(())
    }

    pub(super) fn get_links_impl(&self, id: TaskId) -> Result<Vec<Link>> {
        let conn = self.lock_conn();
        let mut stmt = conn
            .prepare("SELECT from_id, to_id, kind FROM links WHERE from_id = ?1 OR to_id = ?1")?;
        let rows = stmt.query_map(params![id as i64], row_to_link)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }
}

pub(super) fn row_to_link(row: &rusqlite::Row<'_>) -> rusqlite::Result<Link> {
    let kind_str: String = row.get(2)?;
    let kind = kind_str
        .parse::<LinkKind>()
        .map_err(|e| super::parse_err(2, e.to_string()))?;
    Ok(Link {
        from_id: row.get::<_, i64>(0)? as u64,
        to_id: row.get::<_, i64>(1)? as u64,
        kind,
    })
}
