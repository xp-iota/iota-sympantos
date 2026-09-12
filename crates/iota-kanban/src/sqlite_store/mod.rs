use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use std::path::Path;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

use super::event_sourcing::*;
use super::store::KanbanStore;
use super::types::*;
use crate::utils::{lock_or_recover, now_ts};

mod boards;
mod comments;
mod events;
mod links;
mod runs;
mod security;
mod tasks;
mod txn;

// --- Schema -------------------------------------------------------------------

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS boards (
    id          INTEGER PRIMARY KEY,
    slug        TEXT    UNIQUE NOT NULL,
    name        TEXT    NOT NULL,
    created_at  INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS tasks (
    id              INTEGER PRIMARY KEY,
    board_id        INTEGER NOT NULL REFERENCES boards(id),
    title           TEXT    NOT NULL,
    body            TEXT,
    status          TEXT    NOT NULL DEFAULT 'triage',
    assignee        TEXT,
    priority        INTEGER NOT NULL DEFAULT 0,
    tags            TEXT    NOT NULL DEFAULT '[]',
    workspace_kind  TEXT,
    workspace_path  TEXT,
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL,
    claimed_at      INTEGER,
    claim_ttl_secs  INTEGER NOT NULL DEFAULT 900
);
CREATE TABLE IF NOT EXISTS links (
    from_id INTEGER NOT NULL,
    to_id   INTEGER NOT NULL,
    kind    TEXT    NOT NULL,
    PRIMARY KEY (from_id, to_id, kind)
);
CREATE TABLE IF NOT EXISTS comments (
    id          INTEGER PRIMARY KEY,
    task_id     INTEGER NOT NULL,
    author      TEXT    NOT NULL,
    body        TEXT    NOT NULL,
    created_at  INTEGER NOT NULL,
    comment_seq INTEGER
);
CREATE TABLE IF NOT EXISTS runs (
    id              TEXT    PRIMARY KEY,
    task_id         INTEGER NOT NULL,
    profile         TEXT    NOT NULL,
    status          TEXT    NOT NULL DEFAULT 'running',
    started_at      INTEGER NOT NULL,
    finished_at     INTEGER,
    last_heartbeat  INTEGER NOT NULL,
    exit_code       INTEGER,
    output_summary  TEXT
);
CREATE TABLE IF NOT EXISTS events (
    id          INTEGER PRIMARY KEY,
    event_type  TEXT    NOT NULL,
    payload     TEXT    NOT NULL,
    created_at  INTEGER NOT NULL,
    event_uuid  TEXT
);
CREATE TABLE IF NOT EXISTS event_sync_cursors (
    source          TEXT    PRIMARY KEY,
    cursor          INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL,
    source_sequence INTEGER
);
-- Event identity is the UUID, not the local `id`: deduplicating an already
-- imported event must be a lookup, and a UNIQUE index makes that O(log n)
-- while also rejecting a genuine uuid collision rather than storing it twice.
CREATE UNIQUE INDEX IF NOT EXISTS idx_events_uuid ON events(event_uuid)
    WHERE event_uuid IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_tasks_board_status ON tasks(board_id, status);
CREATE INDEX IF NOT EXISTS idx_tasks_assignee     ON tasks(assignee);
CREATE INDEX IF NOT EXISTS idx_runs_task          ON runs(task_id);
CREATE INDEX IF NOT EXISTS idx_comments_task      ON comments(task_id);
";

// --- Struct -------------------------------------------------------------------

#[derive(Clone)]
pub struct SqliteKanbanStore {
    conn: Arc<Mutex<Connection>>,
    event_tx: broadcast::Sender<KanbanUiEvent>,
}

impl SqliteKanbanStore {
    pub fn open(path: &Path) -> Result<Self> {
        security::prepare_sqlite_path(path)?;
        let conn = Connection::open(path)
            .with_context(|| format!("opening kanban db {}", path.display()))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             PRAGMA busy_timeout=5000;
             PRAGMA foreign_keys=ON;",
        )?;
        conn.execute_batch(SCHEMA)?;
        Self::migrate(&conn)?;
        security::secure_sqlite_files(path)?;
        let (event_tx, _) = broadcast::channel(64);
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            event_tx,
        })
    }

    /// Applies additive schema migrations to databases created by an earlier
    /// version. `CREATE TABLE IF NOT EXISTS` in [`SCHEMA`] only covers fresh
    /// databases, so every column added after the first release needs an
    /// explicit `ALTER TABLE` here.
    ///
    /// Each step must be idempotent: it may run against a database that
    /// already has the change.
    fn migrate(conn: &Connection) -> Result<()> {
        if !Self::column_exists(conn, "comments", "comment_seq")? {
            conn.execute_batch("ALTER TABLE comments ADD COLUMN comment_seq INTEGER")?;
            // Backfill from `created_at` so pre-existing rows keep a stable
            // relative order.
            conn.execute_batch(
                "UPDATE comments SET comment_seq = created_at WHERE comment_seq IS NULL",
            )?;
        }
        if !Self::column_exists(conn, "events", "event_uuid")? {
            conn.execute_batch("ALTER TABLE events ADD COLUMN event_uuid TEXT")?;
            // Pre-existing events predate cross-node identity; assign each a
            // UUID now so they can be deduplicated like any other event.
            // Reading the rowids first avoids mutating the table while a
            // cursor over it is open.
            let ids: Vec<i64> = {
                let mut stmt = conn.prepare("SELECT id FROM events WHERE event_uuid IS NULL")?;
                let rows = stmt.query_map([], |row| row.get::<_, i64>(0))?;
                rows.collect::<rusqlite::Result<Vec<_>>>()?
            };
            for id in ids {
                conn.execute(
                    "UPDATE events SET event_uuid = ?2 WHERE id = ?1",
                    params![id, uuid::Uuid::new_v4().to_string()],
                )?;
            }
        }
        if !Self::column_exists(conn, "event_sync_cursors", "source_sequence")? {
            conn.execute_batch(
                "ALTER TABLE event_sync_cursors ADD COLUMN source_sequence INTEGER",
            )?;
            // The legacy single `cursor` column was a global event id; carry it
            // over as the initial sequence so existing syncs resume rather than
            // re-importing.
            conn.execute_batch(
                "UPDATE event_sync_cursors SET source_sequence = cursor WHERE source_sequence IS NULL",
            )?;
        }
        // The unique index lives here as well as in SCHEMA: older databases
        // gain the column through the ALTER above but not the index, and an
        // unindexed dedup check would scan the whole log per imported event.
        conn.execute_batch(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_events_uuid ON events(event_uuid)
                 WHERE event_uuid IS NOT NULL",
        )?;
        Ok(())
    }

    fn column_exists(conn: &Connection, table: &str, column: &str) -> Result<bool> {
        let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            if row.get::<_, String>(1)? == column {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn lock_conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        lock_or_recover(&self.conn)
    }

    /// Subscribe to real-time UI events emitted when store mutations occur.
    pub fn subscribe(&self) -> broadcast::Receiver<KanbanUiEvent> {
        self.event_tx.subscribe()
    }

    /// Bytes already imported from `source`, as a per-source sequence number.
    ///
    /// This is **not** a global event id. Each producer numbers its own events
    /// from its own log, so the cursor is only meaningful paired with the
    /// source it came from — comparing sequences across sources is meaningless.
    pub fn sync_cursor(&self, source: &str) -> Result<EventId> {
        let conn = self.lock_conn();
        let cursor = conn
            .query_row(
                "SELECT source_sequence FROM event_sync_cursors WHERE source = ?1",
                params![source],
                |row| row.get::<_, Option<i64>>(0),
            )
            .unwrap_or(None);
        Ok(cursor.unwrap_or(0).max(0) as EventId)
    }

    pub fn set_sync_cursor(&self, source: &str, cursor: EventId) -> Result<()> {
        let conn = self.lock_conn();
        Self::set_sync_cursor_on_conn(&conn, source, cursor)
    }

    /// Replay a sequence of events against this store, rebuilding state.
    /// Used for syncing a remote node's events into this store.
    /// Returns the number of applied events. A failed event aborts the replay so
    /// callers never acknowledge a cursor that contains an unapplied event.
    #[allow(dead_code)]
    pub fn replay_events(&self, events: &[KanbanEvent]) -> Result<usize> {
        self.with_transaction(|conn| {
            let mut applied = 0;
            for event in events {
                Self::apply_event_on_conn(conn, event).with_context(|| {
                    format!(
                        "failed to replay event {} ({})",
                        event.event_uuid, event.event_type
                    )
                })?;
                applied += 1;
            }
            Ok(applied)
        })
    }

    /// Atomically imports a remote event bundle: replays every new event,
    /// appends each to this store's own event log, and advances the sync
    /// cursor — all inside a single SQLite transaction.
    ///
    /// Previously (result.md S-04) these three steps ran as independent
    /// `KanbanStore` trait calls, each opening and releasing the store's
    /// lock separately. A crash or error partway through could replay some
    /// events but not append them (or vice versa), or advance the cursor
    /// past events that were never actually applied — silently losing or
    /// duplicating data on the next sync. Running all of it under one
    /// `with_transaction` means the whole import either fully commits or
    /// fully rolls back.
    ///
    /// Idempotent by construction: an event already present (matched on its
    /// immutable `event_uuid`) is skipped rather than replayed, so re-importing
    /// an overlapping bundle — the normal case when two nodes sync repeatedly —
    /// applies each event exactly once.
    pub fn import_event_bundle_atomic(
        &self,
        new_events: &[KanbanEvent],
        source: &str,
        source_sequence: EventId,
    ) -> Result<usize> {
        self.with_transaction(|conn| {
            let mut applied = 0;
            for event in new_events {
                if Self::event_uuid_exists_on_conn(conn, &event.event_uuid)? {
                    continue;
                }
                Self::apply_event_on_conn(conn, event).with_context(|| {
                    format!(
                        "failed to replay event {} ({}) during bundle import",
                        event.event_uuid, event.event_type
                    )
                })?;
                // Preserve the producer's UUID so the same event arriving in a
                // later bundle is recognized as already-applied.
                Self::append_event_with_uuid_on_conn(
                    conn,
                    &event.event_type,
                    &event.payload,
                    &event.event_uuid,
                )?;
                applied += 1;
            }
            Self::set_sync_cursor_on_conn(conn, source, source_sequence)?;
            Ok(applied)
        })
    }

    fn event_uuid_exists_on_conn(conn: &Connection, event_uuid: &uuid::Uuid) -> Result<bool> {
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM events WHERE event_uuid = ?1)",
            params![event_uuid.to_string()],
            |row| row.get(0),
        )?;
        Ok(exists)
    }

    fn set_sync_cursor_on_conn(
        conn: &Connection,
        source: &str,
        source_sequence: EventId,
    ) -> Result<()> {
        anyhow::ensure!(
            !source.trim().is_empty() && source.len() <= 256,
            "kanban sync source must be 1..=256 bytes"
        );
        let sequence = i64::try_from(source_sequence)
            .context("kanban sync source sequence exceeds SQLite range")?;
        let now = now_ts();
        conn.execute(
            "INSERT INTO event_sync_cursors (source, cursor, source_sequence, updated_at)
             VALUES (?1, ?2, ?2, ?3)
             ON CONFLICT(source) DO UPDATE SET
                source_sequence = MAX(COALESCE(event_sync_cursors.source_sequence, 0), excluded.source_sequence),
                cursor = MAX(event_sync_cursors.cursor, excluded.cursor),
                updated_at = excluded.updated_at",
            params![source, sequence, now],
        )?;
        Ok(())
    }

    fn new_entity_id_on_conn(conn: &Connection, table: &str) -> Result<i64> {
        // IDs must stay collision-resistant even for in-memory stores: tests
        // and local simulations open two independent stores and sync events
        // between them, and sequential per-store IDs would make the same id
        // refer to different entities on each node. See `event_sync`.
        for _ in 0..32 {
            let bytes = uuid::Uuid::new_v4().into_bytes();
            let id = (u64::from_be_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]) & i64::MAX as u64) as i64;
            if id == 0 {
                continue;
            }
            let sql = match table {
                "boards" => "SELECT EXISTS(SELECT 1 FROM boards WHERE id = ?1)",
                "tasks" => "SELECT EXISTS(SELECT 1 FROM tasks WHERE id = ?1)",
                "comments" => "SELECT EXISTS(SELECT 1 FROM comments WHERE id = ?1)",
                _ => anyhow::bail!("unsupported entity ID table '{table}'"),
            };
            let exists: bool = conn.query_row(sql, params![id], |row| row.get(0))?;
            if !exists {
                return Ok(id);
            }
        }
        anyhow::bail!("failed to allocate a collision-free ID for {table}")
    }

    fn remote_entity_id(id: u64, label: &str) -> Result<i64> {
        let id =
            i64::try_from(id).with_context(|| format!("{label} exceeds SQLite integer range"))?;
        anyhow::ensure!(id > 0, "{label} must be greater than zero");
        Ok(id)
    }

    fn apply_event_on_conn(conn: &Connection, event: &KanbanEvent) -> Result<()> {
        match event.event_type.as_str() {
            EVT_BOARD_CREATED => {
                let p: BoardCreatedPayload = serde_json::from_str(&event.payload)?;
                let board_id = Self::remote_entity_id(p.board_id, "board id")?;
                conn.execute(
                    "INSERT INTO boards (id, slug, name, created_at) VALUES (?1, ?2, ?3, ?4)",
                    params![board_id, p.slug, p.name, event.created_at],
                )?;
                Ok(())
            }
            EVT_TASK_CREATED => {
                let p: TaskCreatedPayload = serde_json::from_str(&event.payload)?;
                let task_id = Self::remote_entity_id(p.task_id, "task id")?;
                let board_id = Self::remote_entity_id(p.board_id, "board id")?;
                let status: Status = p.status.parse()?;
                let tags_json = serde_json::to_string(&p.tags)?;
                conn.execute(
                    "INSERT INTO tasks
                     (id, board_id, title, body, status, assignee, priority, tags,
                      workspace_kind, workspace_path, created_at, updated_at, claim_ttl_secs)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11, 900)",
                    params![
                        task_id,
                        board_id,
                        p.title,
                        p.body,
                        status.as_str(),
                        p.assignee,
                        p.priority,
                        tags_json,
                        p.workspace_kind,
                        p.workspace_path,
                        event.created_at,
                    ],
                )?;
                Ok(())
            }
            EVT_TASK_UPDATED => {
                let p: TaskUpdatedPayload = serde_json::from_str(&event.payload)?;
                Self::remote_entity_id(p.task_id, "task id")?;
                let patch = TaskPatch {
                    title: p.patch.title,
                    body: p.patch.body,
                    status: p.patch.status.map(|status| status.parse()).transpose()?,
                    assignee: p.patch.assignee,
                    priority: p.patch.priority,
                    tags: p.patch.tags,
                    workspace_kind: p.patch.workspace_kind,
                    workspace_path: p
                        .patch
                        .workspace_path
                        .map(|path| path.map(std::path::PathBuf::from)),
                };
                Self::update_task_on_conn(conn, p.task_id, patch)
            }
            EVT_TASK_DELETED => {
                let p: TaskDeletedPayload = serde_json::from_str(&event.payload)?;
                Self::remote_entity_id(p.task_id, "task id")?;
                Self::delete_task_on_conn(conn, p.task_id)
            }
            EVT_TASK_TRANSITIONED => {
                let p: TaskTransitionedPayload = serde_json::from_str(&event.payload)?;
                let task_id = Self::remote_entity_id(p.task_id, "task id")?;
                let declared_from: Status = p.from.parse()?;
                let actual_from: String = conn.query_row(
                    "SELECT status FROM tasks WHERE id = ?1",
                    params![task_id],
                    |row| row.get(0),
                )?;
                anyhow::ensure!(
                    actual_from == declared_from.as_str(),
                    "task {} transition expected {}, found {}",
                    p.task_id,
                    declared_from.as_str(),
                    actual_from
                );
                let to: Status = p.to.parse()?;
                Self::transition_on_conn(conn, p.task_id, to)
            }
            EVT_LINK_CREATED => {
                let p: LinkCreatedPayload = serde_json::from_str(&event.payload)?;
                Self::remote_entity_id(p.from_id, "link source task id")?;
                Self::remote_entity_id(p.to_id, "link target task id")?;
                let kind: LinkKind = p.kind.parse()?;
                Self::create_link_on_conn(conn, p.from_id, p.to_id, kind)
            }
            EVT_LINK_REMOVED => {
                let p: LinkRemovedPayload = serde_json::from_str(&event.payload)?;
                Self::remote_entity_id(p.from_id, "link source task id")?;
                Self::remote_entity_id(p.to_id, "link target task id")?;
                let kind: LinkKind = p.kind.parse()?;
                Self::remove_link_on_conn(conn, p.from_id, p.to_id, kind)
            }
            EVT_COMMENT_ADDED => {
                let p: CommentAddedPayload = serde_json::from_str(&event.payload)?;
                let comment_id = Self::remote_entity_id(p.comment_id, "comment id")?;
                let task_id = Self::remote_entity_id(p.task_id, "task id")?;
                // `event.created_at` is the *remote* node's clock, which can
                // tie with or lag local rows. Order by the local counter so
                // replayed comments interleave deterministically.
                conn.execute(
                    "INSERT INTO comments (id, task_id, author, body, created_at, comment_seq)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        comment_id,
                        task_id,
                        p.author,
                        p.body,
                        event.created_at,
                        Self::next_comment_seq(conn)?,
                    ],
                )?;
                Ok(())
            }
            EVT_RUN_STARTED => {
                let p: RunStartedPayload = serde_json::from_str(&event.payload)?;
                let task_id = Self::remote_entity_id(p.task_id, "task id")?;
                conn.execute(
                    "INSERT INTO runs
                     (id, task_id, profile, status, started_at, last_heartbeat)
                     VALUES (?1, ?2, ?3, 'running', ?4, ?4)",
                    params![p.run_id, task_id, p.profile, event.created_at],
                )?;
                Ok(())
            }
            EVT_RUN_COMPLETED => {
                let p: RunCompletedPayload = serde_json::from_str(&event.payload)?;
                let task_id = Self::remote_entity_id(p.task_id, "task id")?;
                let actual_task_id: i64 = conn.query_row(
                    "SELECT task_id FROM runs WHERE id = ?1",
                    params![&p.run_id],
                    |row| row.get(0),
                )?;
                anyhow::ensure!(
                    actual_task_id == task_id,
                    "run '{}' belongs to a different task",
                    p.run_id
                );
                let status: RunStatus = p.status.parse()?;
                Self::complete_run_on_conn(conn, &p.run_id, status, p.exit_code)
            }
            other => anyhow::bail!("unsupported kanban event type '{other}'"),
        }
    }
}

// --- Trait impl --------------------------------------------------------------

impl KanbanStore for SqliteKanbanStore {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn create_board(&self, slug: &str, name: &str) -> Result<BoardId> {
        self.with_transaction(|conn| {
            let now = now_ts();
            let id = Self::new_entity_id_on_conn(conn, "boards")?;
            conn.execute(
                "INSERT INTO boards (id, slug, name, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![id, slug, name, now],
            )?;
            let id = id as u64;
            let payload = serde_json::to_string(&BoardCreatedPayload {
                board_id: id,
                slug: slug.to_string(),
                name: name.to_string(),
            })?;
            Self::append_event_on_conn(conn, EVT_BOARD_CREATED, &payload)?;
            Ok(id)
        })
    }
    fn list_boards(&self) -> Result<Vec<Board>> {
        self.list_boards_impl()
    }
    fn get_board(&self, slug: &str) -> Result<Board> {
        self.get_board_impl(slug)
    }
    fn create_task(&self, req: CreateTaskRequest) -> Result<TaskId> {
        // Capture the fields the event payload needs before `req` is moved
        // into `create_task_on_conn`.
        let title = req.title.clone();
        let body = req.body.clone();
        let board_id = req.board_id;
        let status = req.status.unwrap_or(Status::Triage).as_str().to_string();
        let assignee = req.assignee.clone();
        let priority = req.priority.unwrap_or(0);
        let tags = req.tags.clone();
        let workspace_kind = req.workspace_kind.clone();
        let workspace_path = req
            .workspace_path
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned());
        // SECURITY/CORRECTNESS (result.md S-04): the domain write (INSERT
        // INTO tasks) and the event-log append (INSERT INTO events) must
        // be atomic — a crash between them previously could leave a task
        // with no corresponding event (breaking sync/replay) or vice
        // versa. `with_transaction` runs both under one BEGIN/COMMIT and
        // one held lock.
        let id = self.with_transaction(|conn| {
            let id = Self::create_task_on_conn(conn, req)?;
            let payload = serde_json::to_string(&TaskCreatedPayload {
                task_id: id,
                board_id,
                title: title.clone(),
                body,
                status,
                assignee,
                priority,
                tags,
                workspace_kind,
                workspace_path,
            })?;
            Self::append_event_on_conn(conn, EVT_TASK_CREATED, &payload)?;
            Ok(id)
        })?;
        let _ = self.event_tx.send(KanbanUiEvent::TaskCreated { id, title });
        Ok(id)
    }
    fn get_task(&self, id: TaskId) -> Result<Task> {
        self.get_task_impl(id)
    }
    fn update_task(&self, id: TaskId, patch: TaskPatch) -> Result<()> {
        let patch_payload = TaskPatchPayload {
            title: patch.title.clone(),
            body: patch.body.clone(),
            status: patch.status.map(|s| s.as_str().to_string()),
            assignee: patch.assignee.clone(),
            priority: patch.priority,
            tags: patch.tags.clone(),
            workspace_kind: patch.workspace_kind.clone(),
            workspace_path: patch.workspace_path.as_ref().map(|path| {
                path.as_ref()
                    .map(|path| path.to_string_lossy().into_owned())
            }),
        };
        self.with_transaction(|conn| {
            Self::update_task_on_conn(conn, id, patch)?;
            let payload = serde_json::to_string(&TaskUpdatedPayload {
                task_id: id,
                patch: patch_payload,
            })?;
            Self::append_event_on_conn(conn, EVT_TASK_UPDATED, &payload)?;
            Ok(())
        })?;
        let _ = self.event_tx.send(KanbanUiEvent::TaskUpdated { id });
        Ok(())
    }
    fn list_tasks(&self, filter: TaskFilter) -> Result<Vec<Task>> {
        self.list_tasks_impl(filter)
    }
    fn delete_task(&self, id: TaskId) -> Result<()> {
        self.with_transaction(|conn| {
            Self::delete_task_on_conn(conn, id)?;
            let payload = serde_json::to_string(&TaskDeletedPayload { task_id: id })?;
            Self::append_event_on_conn(conn, EVT_TASK_DELETED, &payload)?;
            Ok(())
        })?;
        let _ = self.event_tx.send(KanbanUiEvent::TaskDeleted { id });
        Ok(())
    }
    fn transition(&self, id: TaskId, to: Status) -> Result<()> {
        let from = self.with_transaction(|conn| {
            let from_text: String = conn.query_row(
                "SELECT status FROM tasks WHERE id = ?1",
                params![id as i64],
                |row| row.get(0),
            )?;
            let from: Status = from_text.parse()?;
            Self::transition_on_conn(conn, id, to)?;
            let payload = serde_json::to_string(&TaskTransitionedPayload {
                task_id: id,
                from: from.as_str().to_string(),
                to: to.as_str().to_string(),
            })?;
            Self::append_event_on_conn(conn, EVT_TASK_TRANSITIONED, &payload)?;
            Ok(from)
        })?;
        let _ = self
            .event_tx
            .send(KanbanUiEvent::TaskStatusChanged { id, from, to });
        Ok(())
    }
    fn create_link(&self, from: TaskId, to: TaskId, kind: LinkKind) -> Result<()> {
        self.with_transaction(|conn| {
            Self::create_link_on_conn(conn, from, to, kind)?;
            let payload = serde_json::to_string(&LinkCreatedPayload {
                from_id: from,
                to_id: to,
                kind: kind.as_str().to_string(),
            })?;
            Self::append_event_on_conn(conn, EVT_LINK_CREATED, &payload)?;
            Ok(())
        })
    }
    fn remove_link(&self, from: TaskId, to: TaskId, kind: LinkKind) -> Result<()> {
        self.with_transaction(|conn| {
            Self::remove_link_on_conn(conn, from, to, kind)?;
            let payload = serde_json::to_string(&LinkRemovedPayload {
                from_id: from,
                to_id: to,
                kind: kind.as_str().to_string(),
            })?;
            Self::append_event_on_conn(conn, EVT_LINK_REMOVED, &payload)?;
            Ok(())
        })
    }
    fn get_links(&self, id: TaskId) -> Result<Vec<Link>> {
        self.get_links_impl(id)
    }
    fn add_comment(&self, task_id: TaskId, author: &str, body: &str) -> Result<CommentId> {
        let comment_id = self.with_transaction(|conn| {
            let comment_id = Self::add_comment_on_conn(conn, task_id, author, body)?;
            let payload = serde_json::to_string(&CommentAddedPayload {
                comment_id,
                task_id,
                author: author.to_string(),
                body: body.to_string(),
            })?;
            Self::append_event_on_conn(conn, EVT_COMMENT_ADDED, &payload)?;
            Ok(comment_id)
        })?;
        let _ = self.event_tx.send(KanbanUiEvent::CommentAdded {
            task_id,
            comment_id,
        });
        Ok(comment_id)
    }
    fn list_comments(&self, task_id: TaskId) -> Result<Vec<Comment>> {
        self.list_comments_impl(task_id)
    }
    fn create_run(&self, task_id: TaskId, profile: &str) -> Result<RunId> {
        let run_id = self.with_transaction(|conn| {
            let run_id = Self::create_run_on_conn(conn, task_id, profile)?;
            let payload = serde_json::to_string(&RunStartedPayload {
                run_id: run_id.clone(),
                task_id,
                profile: profile.to_string(),
            })?;
            Self::append_event_on_conn(conn, EVT_RUN_STARTED, &payload)?;
            Ok(run_id)
        })?;
        let _ = self.event_tx.send(KanbanUiEvent::RunStarted {
            task_id,
            run_id: run_id.clone(),
        });
        Ok(run_id)
    }
    fn complete_run(&self, run_id: &str, status: RunStatus, exit_code: Option<i32>) -> Result<()> {
        let task_id = self.with_transaction(|conn| {
            let task_id: i64 = conn.query_row(
                "SELECT task_id FROM runs WHERE id = ?1",
                params![run_id],
                |row| row.get(0),
            )?;
            Self::complete_run_on_conn(conn, run_id, status, exit_code)?;
            let payload = serde_json::to_string(&RunCompletedPayload {
                run_id: run_id.to_string(),
                task_id: task_id as u64,
                status: status.as_str().to_string(),
                exit_code,
            })?;
            Self::append_event_on_conn(conn, EVT_RUN_COMPLETED, &payload)?;
            Ok(task_id as u64)
        })?;
        let _ = self.event_tx.send(KanbanUiEvent::RunCompleted {
            task_id,
            run_id: run_id.to_string(),
            status,
        });
        Ok(())
    }
    fn heartbeat(&self, run_id: &str) -> Result<()> {
        self.heartbeat_impl(run_id)
    }
    fn get_runs(&self, task_id: TaskId) -> Result<Vec<Run>> {
        self.get_runs_impl(task_id)
    }
    fn append_event(&self, event_type: &str, payload: &str) -> Result<EventId> {
        self.append_event_impl(event_type, payload)
    }
    fn events_since(&self, cursor: EventId) -> Result<Vec<KanbanEvent>> {
        self.events_since_impl(cursor)
    }
}

fn parse_err(col: usize, msg: String) -> rusqlite::Error {
    #[derive(Debug)]
    struct E(String);
    impl std::fmt::Display for E {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(&self.0)
        }
    }
    impl std::error::Error for E {}
    rusqlite::Error::FromSqlConversionFailure(col, rusqlite::types::Type::Text, Box::new(E(msg)))
}

#[cfg(test)]
#[path = "../sqlite_store_tests.rs"]
mod sqlite_store_tests;
