//! SQLite connection initialization, pooling, and standard configurations.

use anyhow::{Context, Result};
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

/// Standard pragmas applied to every connection this module opens.
///
/// - WAL: concurrent readers alongside a single writer.
/// - NORMAL: durable enough with WAL, without a full fsync per commit.
/// - busy_timeout: wait rather than fail on a transient write lock.
/// - foreign_keys: enforce declared constraints.
///
/// All four stores (`ledger`, `approvals`, `cache`, `observability`) share one
/// database file, so these must be identical everywhere to avoid connections
/// with divergent locking behavior.
const STANDARD_PRAGMAS: &str = "PRAGMA journal_mode=WAL; \
     PRAGMA synchronous=NORMAL; \
     PRAGMA busy_timeout=5000; \
     PRAGMA foreign_keys=ON;";

/// Ceiling on read connections per database, in addition to the single writer.
///
/// WAL allows any number of concurrent readers but only one writer, so reads
/// scale out and writes stay serialized on the dedicated write connection.
///
/// This is a ceiling, not an eager allocation: readers are opened on demand by
/// [`DbPool::read`]. Every WAL connection costs three file descriptors (the
/// database plus its `-wal`/`-shm` sidecars) and a daemon holds one pool per
/// store per cached workspace, so opening the ceiling up front multiplied
/// descriptor use per store by 5x and exhausted the default macOS limit
/// (`ulimit -n` 256) during parallel runs.
const POOL_READ_CONNECTIONS: usize = 4;

/// Environment override for [`POOL_READ_CONNECTIONS`], clamped to it. `0`
/// disables the reader pool and routes reads through the writer connection,
/// which is the minimum-descriptor configuration.
const READ_CONNECTIONS_ENV: &str = "IOTA_SQLITE_READ_CONNECTIONS";

/// Resolves the reader ceiling for a new pool.
///
/// An unparseable override is ignored with a warning rather than failing the
/// store: a bad environment variable must not make the database unopenable.
fn resolve_read_connections() -> usize {
    match std::env::var(READ_CONNECTIONS_ENV) {
        Err(_) => POOL_READ_CONNECTIONS,
        Ok(raw) => match raw.trim().parse::<usize>() {
            Ok(value) => value.min(POOL_READ_CONNECTIONS),
            Err(_) => {
                tracing::warn!(
                    var = READ_CONNECTIONS_ENV,
                    value = %raw,
                    default = POOL_READ_CONNECTIONS,
                    "ignoring unparseable sqlite reader count override"
                );
                POOL_READ_CONNECTIONS
            }
        },
    }
}

/// A query slower than this is logged as a slow query.
const SLOW_QUERY_THRESHOLD: Duration = Duration::from_millis(250);

/// Opens an SQLite database connection at the specified path and applies
/// [`STANDARD_PRAGMAS`].
///
/// The parent directory and the database file (plus its `-wal`/`-shm`
/// sidecar files once SQLite creates them) are locked to owner-only
/// permissions (`0700`/`0600` on Unix) — these databases can hold
/// conversation/memory content and must not be readable by other local
/// users (result.md S-03).
pub fn open_db(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        crate::fs_secure::create_missing_dir_owner_only(parent)
            .with_context(|| format!("Failed to create parent directory: {}", parent.display()))?;
    }

    let conn = Connection::open(path)
        .with_context(|| format!("Failed to open SQLite database: {}", path.display()))?;

    conn.execute_batch(STANDARD_PRAGMAS).with_context(|| {
        format!(
            "Failed to configure SQLite database pragmas for: {}",
            path.display()
        )
    })?;

    lock_down_db_files(path)?;

    Ok(conn)
}

/// A small read/write connection pool over one SQLite database file.
///
/// Replaces the previous `Arc<Mutex<Connection>>` held by every store, which
/// serialized *all* access — including reads that WAL would have allowed to run
/// concurrently. Reads now take one of [`POOL_READ_CONNECTIONS`] connections;
/// writes take the single writer connection, since SQLite permits only one
/// writer at a time regardless.
///
/// Blocks the calling thread while waiting, so callers on the async runtime
/// must run pool access inside `spawn_blocking`.
pub struct DbPool {
    path: PathBuf,
    /// The single writer connection, plus its own lock.
    writer: Mutex<Connection>,
    /// Reader slots, each filled the first time [`Self::read`] finds every
    /// already-open reader busy. Unfilled slots hold no descriptors.
    readers: Vec<OnceLock<Mutex<Connection>>>,
    /// Rotates read checkout once every slot is filled, so concurrent readers
    /// do not all block on one lock.
    next_reader: std::sync::atomic::AtomicUsize,
}

impl DbPool {
    /// Opens a pool against `path`, creating the file and parent directory if
    /// needed.
    ///
    /// Runs pending [`super::migrations`] on the writer connection before the
    /// pool is built, so no reader can observe a half-migrated schema.
    ///
    /// In-memory databases get a single connection and no reader pool: each
    /// SQLite connection to `:memory:` opens its *own* private database, so a
    /// reader pool would see an empty schema. Sharing one connection keeps the
    /// `:memory:` path usable for tests and throwaway stores.
    ///
    /// Only the writer connection is opened here. Readers are added lazily
    /// under concurrent read load, so an idle store costs one connection
    /// instead of [`POOL_READ_CONNECTIONS`] + 1.
    pub fn open(path: &Path) -> Result<Self> {
        Self::open_with_read_connections(path, resolve_read_connections())
    }

    /// Returns the process-wide pool for `path`, opening it on first use.
    ///
    /// Several stores map onto the same database file — `ledger` and
    /// `approvals` both use `store.db`, `cache` and `observability` both use
    /// `events.db`. Giving each store its own pool meant several writer
    /// connections to one file, which SQLite serializes with `SQLITE_BUSY`
    /// retries instead of the writer mutex, and doubled the descriptor cost of
    /// every workspace the daemon caches. One pool per file fixes both.
    ///
    /// The registry holds [`Weak`] references, so a pool closes as soon as the
    /// last store using it is dropped; tests that delete their temporary
    /// database keep working.
    ///
    /// In-memory databases are never shared: each connection to `:memory:` gets
    /// its own private database, and two stores that both asked for `:memory:`
    /// are asking for two independent databases.
    pub fn shared(path: &Path) -> Result<Arc<Self>> {
        if is_in_memory(path) {
            return Ok(Arc::new(Self::open(path)?));
        }
        static POOLS: OnceLock<Mutex<HashMap<PathBuf, Weak<DbPool>>>> = OnceLock::new();
        let registry = POOLS.get_or_init(|| Mutex::new(HashMap::new()));
        let key = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());

        let mut guard = crate::utils::lock_or_recover(registry);
        if let Some(existing) = guard.get(&key).and_then(Weak::upgrade) {
            return Ok(existing);
        }
        let pool = Arc::new(Self::open(path)?);
        guard.insert(key, Arc::downgrade(&pool));
        // Drop entries whose pool has closed so the registry cannot grow
        // without bound in a long-running daemon.
        guard.retain(|_, weak| weak.strong_count() > 0);
        Ok(pool)
    }

    /// [`Self::open`] with an explicit reader ceiling, bypassing the
    /// [`READ_CONNECTIONS_ENV`] override.
    ///
    /// Tests use this instead of mutating the environment, which is not sound
    /// while other test threads are reading it.
    pub fn open_with_read_connections(path: &Path, read_connections: usize) -> Result<Self> {
        let mut writer = open_db(path)?;
        super::migrations::apply(&mut writer, &path.display().to_string())?;

        let reader_slots = if is_in_memory(path) {
            0
        } else {
            read_connections
        };
        let mut readers = Vec::with_capacity(reader_slots);
        for _ in 0..reader_slots {
            readers.push(OnceLock::new());
        }
        Ok(Self {
            path: path.to_path_buf(),
            writer: Mutex::new(writer),
            readers,
            next_reader: std::sync::atomic::AtomicUsize::new(0),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Takes a read connection for the lifetime of the returned guard.
    ///
    /// Descriptor use tracks actual read concurrency: an already-open idle
    /// reader is reused, a new one is opened only when every open reader is
    /// busy, and once all slots are filled callers block on a rotating slot.
    /// If a new reader cannot be opened (descriptor exhaustion, for instance)
    /// the read is served by the writer connection instead of failing.
    pub fn read(&self, label: &'static str) -> PooledConn<'_> {
        if self.readers.is_empty() {
            // Reader pool disabled (in-memory database, or count overridden to
            // zero): serve reads from the writer so they still work.
            return self.write(label);
        }

        // Prefer an open, idle reader.
        for slot in &self.readers {
            if let Some(reader) = slot.get()
                && let Some(guard) = try_lock_or_recover(reader)
            {
                return self.reader_conn(guard, label, Duration::ZERO);
            }
        }

        // Every open reader is busy: fill the next empty slot.
        for slot in &self.readers {
            if slot.get().is_some() {
                continue;
            }
            match open_db(&self.path) {
                Ok(conn) => {
                    let _ = slot.set(Mutex::new(conn));
                }
                Err(err) => {
                    tracing::warn!(
                        store = %self.path.display(),
                        statement = label,
                        error = %err,
                        "failed to open an additional sqlite reader; serving read from the writer connection"
                    );
                    crate::telemetry::metrics::get().record_db_reader_fallback("open_failed");
                    return self.write(label);
                }
            }
            if let Some(reader) = slot.get()
                && let Some(guard) = try_lock_or_recover(reader)
            {
                return self.reader_conn(guard, label, Duration::ZERO);
            }
        }

        // All slots are filled and busy: wait on one, rotating so concurrent
        // callers spread across readers.
        let index = self
            .next_reader
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            % self.readers.len();
        let waited = Instant::now();
        let guard = crate::utils::lock_or_recover(
            self.readers[index]
                .get()
                .expect("reader slot filled above stays filled"),
        );
        let waited = waited.elapsed();
        self.reader_conn(guard, label, waited)
    }

    /// Number of reader connections currently open, for metrics and tests.
    pub fn open_read_connections(&self) -> usize {
        self.readers
            .iter()
            .filter(|slot| slot.get().is_some())
            .count()
    }

    fn reader_conn<'a>(
        &'a self,
        guard: std::sync::MutexGuard<'a, Connection>,
        label: &'static str,
        waited: Duration,
    ) -> PooledConn<'a> {
        PooledConn {
            inner: PooledConnInner::Reader(guard),
            store: &self.path,
            label,
            kind: "read",
            started: Instant::now(),
            waited,
        }
    }

    /// Takes the writer connection for the lifetime of the returned guard.
    ///
    /// SQLite permits a single writer per database, so this serializes all
    /// writes; reads proceed concurrently on [`Self::read`].
    pub fn write(&self, label: &'static str) -> PooledConn<'_> {
        let waited = Instant::now();
        let guard = crate::utils::lock_or_recover(&self.writer);
        PooledConn {
            inner: PooledConnInner::Writer(guard),
            store: &self.path,
            label,
            kind: "write",
            started: Instant::now(),
            waited: waited.elapsed(),
        }
    }
}

/// A checked-out connection. Dereferences to [`Connection`], so callers use it
/// exactly like the `MutexGuard<Connection>` it replaces.
pub struct PooledConn<'a> {
    inner: PooledConnInner<'a>,
    /// Borrowed from the pool, which outlives every guard it hands out.
    store: &'a Path,
    label: &'static str,
    kind: &'static str,
    started: Instant,
    waited: Duration,
}

enum PooledConnInner<'a> {
    Writer(std::sync::MutexGuard<'a, Connection>),
    Reader(std::sync::MutexGuard<'a, Connection>),
}

impl std::ops::Deref for PooledConn<'_> {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        match &self.inner {
            PooledConnInner::Writer(g) => g,
            PooledConnInner::Reader(g) => g,
        }
    }
}

impl std::ops::DerefMut for PooledConn<'_> {
    fn deref_mut(&mut self) -> &mut Connection {
        match &mut self.inner {
            PooledConnInner::Writer(g) => g,
            PooledConnInner::Reader(g) => g,
        }
    }
}

impl Drop for PooledConn<'_> {
    fn drop(&mut self) {
        // The guard's lifetime is the window the caller used the connection,
        // so this is the closest proxy for statement duration available without
        // wrapping every call site.
        let held = self.started.elapsed();
        if self.waited > SLOW_QUERY_THRESHOLD || held > SLOW_QUERY_THRESHOLD {
            let metrics = crate::telemetry::metrics::get();
            if self.waited > SLOW_QUERY_THRESHOLD {
                tracing::warn!(
                    store = %self.store.display(),
                    statement = self.label,
                    kind = self.kind,
                    waited_ms = self.waited.as_millis() as u64,
                    "sqlite connection lock wait exceeded threshold"
                );
                metrics.record_db_lock_wait(self.waited.as_secs_f64(), self.label, self.kind);
            }
            if held > SLOW_QUERY_THRESHOLD {
                tracing::warn!(
                    store = %self.store.display(),
                    statement = self.label,
                    kind = self.kind,
                    held_ms = held.as_millis() as u64,
                    "slow sqlite statement"
                );
                metrics.record_db_slow_query(held.as_secs_f64(), self.label, self.kind);
            }
        }
    }
}

/// Locks `mutex` without blocking, recovering a lock poisoned by an earlier
/// panic the same way [`crate::utils::lock_or_recover`] does.
///
/// Returns `None` only when the lock is genuinely held by another caller.
fn try_lock_or_recover<T>(mutex: &Mutex<T>) -> Option<std::sync::MutexGuard<'_, T>> {
    match mutex.try_lock() {
        Ok(guard) => Some(guard),
        Err(std::sync::TryLockError::Poisoned(err)) => {
            tracing::warn!("mutex was poisoned by a previous panic; recovering inner value");
            Some(err.into_inner())
        }
        Err(std::sync::TryLockError::WouldBlock) => None,
    }
}

/// Whether `path` names an in-memory SQLite database rather than a file.
///
/// Also true for `file::memory:?cache=shared` URIs, which SQLite treats as
/// in-memory.
fn is_in_memory(path: &Path) -> bool {
    let text = path.to_string_lossy();
    text.is_empty() || text == ":memory:" || text.starts_with("file::memory:")
}

/// Applies owner-only permissions to the main database and any WAL/SHM
/// sidecars already created by SQLite. Permission failures are fatal because
/// continuing would expose sensitive local data contrary to the store's
/// security contract.
fn lock_down_db_files(path: &Path) -> Result<()> {
    for candidate in db_sidecar_paths(path) {
        if candidate.exists() {
            crate::fs_secure::set_file_owner_only(&candidate).with_context(|| {
                format!(
                    "Failed to lock down SQLite file permissions: {}",
                    candidate.display()
                )
            })?;
        }
    }
    Ok(())
}

fn db_sidecar_paths(path: &Path) -> Vec<std::path::PathBuf> {
    let mut paths = vec![path.to_path_buf()];
    let file_name = path.file_name().and_then(|n| n.to_str());
    if let (Some(parent), Some(file_name)) = (path.parent(), file_name) {
        paths.push(parent.join(format!("{file_name}-wal")));
        paths.push(parent.join(format!("{file_name}-shm")));
    }
    paths
}

#[cfg(test)]
#[path = "db_tests.rs"]
mod db_tests;
