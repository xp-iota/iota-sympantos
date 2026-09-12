//! Raw event-log append/read for [`super::SqliteKanbanStore`].
//!
//! This is the low-level events table accessor used by the [`KanbanStore`]
//! trait's `append_event`/`events_since`. Event *sourcing* (replaying a
//! [`crate::types::KanbanEvent`] to rebuild state) lives in
//! [`super::apply_event`], not here.

use anyhow::Result;
use rusqlite::params;

use crate::types::{EventId, KanbanEvent};
use crate::utils::now_ts;

use super::SqliteKanbanStore;

impl SqliteKanbanStore {
    pub(super) fn append_event_impl(&self, event_type: &str, payload: &str) -> Result<EventId> {
        let conn = self.lock_conn();
        Self::append_event_on_conn(&conn, event_type, payload)
    }

    pub(super) fn append_event_on_conn(
        conn: &rusqlite::Connection,
        event_type: &str,
        payload: &str,
    ) -> Result<EventId> {
        Self::append_event_with_uuid_on_conn(conn, event_type, payload, &uuid::Uuid::new_v4())
    }

    /// Appends an event with an explicit identity.
    ///
    /// Replayed remote events keep their original UUID so the same event
    /// arriving twice is recognized as a duplicate rather than duplicated.
    pub(super) fn append_event_with_uuid_on_conn(
        conn: &rusqlite::Connection,
        event_type: &str,
        payload: &str,
        event_uuid: &uuid::Uuid,
    ) -> Result<EventId> {
        let now = now_ts();
        conn.execute(
            "INSERT INTO events (event_type, payload, created_at, event_uuid) VALUES (?1, ?2, ?3, ?4)",
            params![event_type, payload, now, event_uuid.to_string()],
        )?;
        Ok(conn.last_insert_rowid() as u64)
    }

    pub(super) fn events_since_impl(&self, cursor: EventId) -> Result<Vec<KanbanEvent>> {
        let conn = self.lock_conn();
        let mut stmt = conn.prepare(
            "SELECT id, event_type, payload, created_at, event_uuid
             FROM events WHERE id > ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map(params![cursor as i64], row_to_event)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }
}

pub(super) fn row_to_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<KanbanEvent> {
    let raw_uuid: Option<String> = row.get(4)?;
    Ok(KanbanEvent {
        id: row.get::<_, i64>(0)? as u64,
        // Rows written before the `event_uuid` migration are backfilled, but a
        // NULL must not panic a read; fall back to the nil UUID.
        event_uuid: raw_uuid
            .and_then(|value| uuid::Uuid::parse_str(&value).ok())
            .unwrap_or(uuid::Uuid::nil()),
        event_type: row.get(1)?,
        payload: row.get(2)?,
        created_at: row.get(3)?,
    })
}
