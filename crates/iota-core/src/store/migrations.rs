//! Versioned schema migrations for the `~/.i6/context` store databases.
//!
//! Before this module every store created its tables with a bare
//! `CREATE TABLE IF NOT EXISTS`, which silently does nothing when a table
//! already exists — so any column added after a database was first created
//! would be missing on existing installs, with no way to detect it. There was
//! also no record of which schema version a file was at.
//!
//! Migrations here are driven by `PRAGMA user_version`:
//!
//! - Version `0` means "not yet migrated" (a database from before this
//!   module, or a brand-new file).
//! - Each step runs in its own transaction and bumps `user_version` only
//!   after its statements succeed, so a failed step leaves the version
//!   untouched and the next open retries it.
//! - Steps must be idempotent, since a crash between the statements and the
//!   version bump replays them.
//!
//! The store APIs and on-disk file locations are unchanged; this only makes
//! existing schema evolution explicit and observable.

use anyhow::{Context, Result};
use rusqlite::Connection;

/// A single schema step.
struct Migration {
    /// Version this step brings the database *to*.
    version: i32,
    /// Human-readable summary, logged when applied.
    description: &'static str,
    /// Statements to run. Must be idempotent.
    statements: &'static str,
}

/// Ordered migration steps.
///
/// Step 1 is the baseline: it pins the schema that `CREATE TABLE IF NOT
/// EXISTS` had already been creating implicitly, so existing databases are
/// stamped with a known version instead of staying at 0 forever.
const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    description: "baseline context store schema",
    statements: "CREATE TABLE IF NOT EXISTS schema_meta (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );",
}];

/// Current schema version this build expects.
pub fn target_version() -> i32 {
    MIGRATIONS.last().map(|m| m.version).unwrap_or(0)
}

/// Reads the database's recorded schema version.
pub fn current_version(conn: &Connection) -> Result<i32> {
    conn.query_row("PRAGMA user_version", [], |row| row.get(0))
        .context("Failed to read PRAGMA user_version")
}

/// Brings `conn` up to [`target_version`], applying only the missing steps.
///
/// Returns the version the database ended at. A database already at or beyond
/// the target is left untouched — this never downgrades, so a newer build's
/// database opened by an older binary is not corrupted.
pub fn apply(conn: &mut Connection, label: &str) -> Result<i32> {
    let start = current_version(conn)?;
    let target = target_version();

    if start > target {
        tracing::warn!(
            store = label,
            found = start,
            expected = target,
            "store schema is newer than this build; leaving it untouched"
        );
        return Ok(start);
    }

    for migration in MIGRATIONS.iter().filter(|m| m.version > start) {
        let tx = conn
            .transaction()
            .with_context(|| format!("Failed to begin migration to v{}", migration.version))?;
        tx.execute_batch(migration.statements).with_context(|| {
            format!(
                "Migration to v{} ({}) failed for store {}",
                migration.version, migration.description, label
            )
        })?;
        // `PRAGMA user_version` does not accept a bound parameter, and the
        // value is a compile-time constant from MIGRATIONS, not user input.
        tx.execute_batch(&format!("PRAGMA user_version = {}", migration.version))
            .with_context(|| format!("Failed to record schema version v{}", migration.version))?;
        tx.commit()
            .with_context(|| format!("Failed to commit migration to v{}", migration.version))?;
        tracing::info!(
            store = label,
            version = migration.version,
            description = migration.description,
            "applied store schema migration"
        );
    }

    let end = current_version(conn)?;
    if end != start {
        tracing::info!(
            store = label,
            from = start,
            to = end,
            "store schema migrated"
        );
    }
    Ok(end)
}
