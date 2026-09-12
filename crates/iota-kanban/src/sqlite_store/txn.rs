//! Transactional helper for [`super::SqliteKanbanStore`].
//!
//! Every public `KanbanStore` mutation method previously performed the
//! domain write (e.g. `INSERT INTO tasks`) and the event-log append
//! (`INSERT INTO events`) as two separate, independently-locked
//! `Connection::execute` calls. If the process crashed or the connection was
//! interrupted between the two, the database was left with a task that has
//! no corresponding event (breaking replay/sync) or, more subtly, an event
//! append that could observe a different intermediate state than the one
//! the domain write actually produced.
//!
//! [`with_transaction`] runs both writes under one SQLite transaction and
//! one held lock, so a failure at any step rolls back the entire operation
//! — callers never see a partially-applied mutation, and `events_since`
//! never reports an event for a domain change that was rolled back (or vice
//! versa).

use anyhow::Result;
use rusqlite::Connection;

use super::SqliteKanbanStore;

impl SqliteKanbanStore {
    /// Runs `f` inside a single SQLite transaction on this store's
    /// connection, committing on `Ok` and rolling back on `Err`.
    ///
    /// `f` receives the live `&Connection` (already inside `BEGIN`) so it
    /// can freely mix domain-table writes and `events`-table appends; both
    /// become durable together on commit, or neither does on rollback.
    ///
    /// The store's mutex is held for the duration of `f`, matching the
    /// existing single-writer model (`lock_conn()`); this function does not
    /// change the concurrency model, only the atomicity of what happens
    /// while the lock is held.
    pub(super) fn with_transaction<T>(
        &self,
        f: impl FnOnce(&Connection) -> Result<T>,
    ) -> Result<T> {
        let conn = self.lock_conn();
        conn.execute_batch("BEGIN IMMEDIATE")?;
        match f(&conn) {
            Ok(value) => {
                if let Err(error) = conn.execute_batch("COMMIT") {
                    let _ = conn.execute_batch("ROLLBACK");
                    return Err(error.into());
                }
                Ok(value)
            }
            Err(err) => {
                // Best-effort rollback: if ROLLBACK itself fails (e.g. the
                // connection is already broken), the original error `err`
                // is still what gets returned — we do not mask a real
                // failure with a rollback failure.
                let _ = conn.execute_batch("ROLLBACK");
                Err(err)
            }
        }
    }
}

#[cfg(test)]
#[path = "txn_tests.rs"]
mod tests;
