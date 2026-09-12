//! Tests for SQLite pooling, migration, and degraded-write classification.

use crate::store::db::DbPool;
use crate::store::migrations;
use crate::store::{ErrorCategory, degraded_write};

fn temp_db_path(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("iota-store-{}-{}", name, uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("store.sqlite")
}

#[test]
fn migrations_stamp_a_fresh_database() {
    let path = temp_db_path("migrate-fresh");
    let mut conn = crate::store::db::open_db(&path).unwrap();

    assert_eq!(migrations::current_version(&conn).unwrap(), 0);
    let end = migrations::apply(&mut conn, "test").unwrap();

    assert_eq!(end, migrations::target_version());
    assert_eq!(migrations::current_version(&conn).unwrap(), end);
    std::fs::remove_dir_all(path.parent().unwrap()).ok();
}

#[test]
fn migrations_are_idempotent_across_reopens() {
    let path = temp_db_path("migrate-twice");

    let mut conn = crate::store::db::open_db(&path).unwrap();
    let first = migrations::apply(&mut conn, "test").unwrap();
    drop(conn);

    // Reopening an already-migrated database must be a no-op, not an error.
    let mut conn = crate::store::db::open_db(&path).unwrap();
    let second = migrations::apply(&mut conn, "test").unwrap();
    assert_eq!(first, second);
    std::fs::remove_dir_all(path.parent().unwrap()).ok();
}

#[test]
fn migrations_never_downgrade_a_newer_database() {
    let path = temp_db_path("migrate-newer");
    let mut conn = crate::store::db::open_db(&path).unwrap();

    // Simulate a database written by a future build.
    let future = migrations::target_version() + 5;
    conn.execute_batch(&format!("PRAGMA user_version = {future}"))
        .unwrap();

    let end = migrations::apply(&mut conn, "test").unwrap();
    assert_eq!(end, future, "a newer schema must be left untouched");
    std::fs::remove_dir_all(path.parent().unwrap()).ok();
}

#[test]
fn pool_opens_and_serves_concurrent_readers() {
    let path = temp_db_path("pool-readers");
    let pool = DbPool::open(&path).unwrap();

    pool.write("test.create")
        .execute_batch(
            "CREATE TABLE t (v INTEGER NOT NULL); INSERT INTO t (v) VALUES (1), (2), (3);",
        )
        .unwrap();

    // Four threads reading through the pool must all succeed: WAL permits
    // concurrent readers, so checkouts must not serialize into failure.
    let pool = std::sync::Arc::new(pool);
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let pool = std::sync::Arc::clone(&pool);
            std::thread::spawn(move || {
                pool.read("test.count")
                    .query_row("SELECT COUNT(*) FROM t", [], |row| row.get::<_, i64>(0))
                    .unwrap()
            })
        })
        .collect();

    for handle in handles {
        assert_eq!(handle.join().unwrap(), 3);
    }
    std::fs::remove_dir_all(path.parent().unwrap()).ok();
}

#[test]
fn pool_writer_sees_its_own_committed_writes_and_readers_do_too() {
    let path = temp_db_path("pool-visibility");
    let pool = DbPool::open(&path).unwrap();

    pool.write("test.create")
        .execute_batch("CREATE TABLE t (v INTEGER NOT NULL);")
        .unwrap();
    pool.write("test.insert")
        .execute("INSERT INTO t (v) VALUES (?1)", rusqlite::params![7])
        .unwrap();

    let seen = pool
        .read("test.read")
        .query_row("SELECT v FROM t", [], |row| row.get::<_, i64>(0))
        .unwrap();
    assert_eq!(
        seen, 7,
        "a committed write must be visible to pooled readers"
    );
    std::fs::remove_dir_all(path.parent().unwrap()).ok();
}

#[test]
fn error_categories_classify_sqlite_failures() {
    // A constraint violation is the caller's fault and must be distinguishable
    // from an unavailable database, since only the former is worth retrying.
    let constraint = rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CONSTRAINT),
        None,
    );
    let error = anyhow::Error::from(constraint);
    assert_eq!(ErrorCategory::classify(&error), ErrorCategory::Constraint);

    let busy =
        rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY), None);
    assert_eq!(
        ErrorCategory::classify(&anyhow::Error::from(busy)),
        ErrorCategory::Locked
    );

    // A non-SQLite error must not be misreported as a constraint failure.
    let other = anyhow::anyhow!("something else entirely");
    assert_eq!(ErrorCategory::classify(&other), ErrorCategory::Other);
}

#[test]
fn in_memory_pool_shares_one_database() {
    // Each SQLite connection to `:memory:` opens its own private database, so
    // a reader pool would see an empty schema. The pool must collapse to a
    // single connection instead.
    let pool = DbPool::open(std::path::Path::new(":memory:")).unwrap();
    pool.write("test.create")
        .execute_batch("CREATE TABLE t (v INTEGER NOT NULL); INSERT INTO t (v) VALUES (42);")
        .unwrap();

    let seen = pool
        .read("test.read")
        .query_row("SELECT v FROM t", [], |row| row.get::<_, i64>(0))
        .unwrap();
    assert_eq!(seen, 42, "reads must see the schema the writer created");
}

#[test]
fn degraded_write_returns_the_error_without_panicking() {
    let error = anyhow::anyhow!("synthetic auxiliary failure");
    let returned = degraded_write("observability", error, Some("exec-1"), Some("sess-1"));
    assert!(
        returned.to_string().contains("synthetic auxiliary failure"),
        "degraded_write must surface the original error, got: {returned}"
    );
}

#[test]
fn idle_pool_holds_no_reader_connections() {
    // Every WAL connection costs three descriptors, and a daemon holds one pool
    // per store per cached workspace. An idle pool must therefore cost exactly
    // one connection, not POOL_READ_CONNECTIONS + 1.
    let path = temp_db_path("pool-idle");
    let pool = DbPool::open(&path).unwrap();
    assert_eq!(
        pool.open_read_connections(),
        0,
        "opening a pool must not pre-open readers"
    );
    std::fs::remove_dir_all(path.parent().unwrap()).ok();
}

#[test]
fn sequential_reads_reuse_a_single_reader_connection() {
    let path = temp_db_path("pool-sequential");
    let pool = DbPool::open(&path).unwrap();
    pool.write("test.create")
        .execute_batch("CREATE TABLE t (v INTEGER NOT NULL); INSERT INTO t (v) VALUES (1);")
        .unwrap();

    for _ in 0..8 {
        let seen = pool
            .read("test.read")
            .query_row("SELECT v FROM t", [], |row| row.get::<_, i64>(0))
            .unwrap();
        assert_eq!(seen, 1);
    }

    assert_eq!(
        pool.open_read_connections(),
        1,
        "reads that never overlap must not each open their own connection"
    );
    std::fs::remove_dir_all(path.parent().unwrap()).ok();
}

#[test]
fn overlapping_reads_open_additional_readers() {
    let path = temp_db_path("pool-overlap");
    let pool = DbPool::open(&path).unwrap();
    pool.write("test.create")
        .execute_batch("CREATE TABLE t (v INTEGER NOT NULL); INSERT INTO t (v) VALUES (1);")
        .unwrap();

    // Hold two reads at once: the second must not block on the first, which
    // means a second connection was opened on demand.
    let first = pool.read("test.first");
    let second = pool.read("test.second");
    assert_eq!(
        pool.open_read_connections(),
        2,
        "a concurrent read must get its own connection"
    );
    drop(first);
    drop(second);
    std::fs::remove_dir_all(path.parent().unwrap()).ok();
}

#[test]
fn reader_pool_can_be_disabled_by_override() {
    // Serialized-but-working reads are the correct degraded mode when a host is
    // descriptor constrained, so the override must be honored end to end.
    let path = temp_db_path("pool-disabled");
    let pool = crate::store::db::DbPool::open_with_read_connections(&path, 0).unwrap();
    pool.write("test.create")
        .execute_batch("CREATE TABLE t (v INTEGER NOT NULL); INSERT INTO t (v) VALUES (5);")
        .unwrap();

    let seen = pool
        .read("test.read")
        .query_row("SELECT v FROM t", [], |row| row.get::<_, i64>(0))
        .unwrap();
    assert_eq!(seen, 5, "reads must still work without a reader pool");
    assert_eq!(pool.open_read_connections(), 0);
    std::fs::remove_dir_all(path.parent().unwrap()).ok();
}

#[test]
fn shared_pool_is_reused_for_one_database_file() {
    // `ledger` and `approvals` both open `store.db`, `cache` and
    // `observability` both open `events.db`. Each store opening its own pool
    // meant two writer connections per file and twice the descriptors.
    let path = temp_db_path("pool-shared");
    let first = DbPool::shared(&path).unwrap();
    let second = DbPool::shared(&path).unwrap();
    assert!(
        std::sync::Arc::ptr_eq(&first, &second),
        "two stores on one database file must share a pool"
    );

    let other = temp_db_path("pool-shared-other");
    let third = DbPool::shared(&other).unwrap();
    assert!(
        !std::sync::Arc::ptr_eq(&first, &third),
        "different database files must not share a pool"
    );
    std::fs::remove_dir_all(path.parent().unwrap()).ok();
    std::fs::remove_dir_all(other.parent().unwrap()).ok();
}

#[test]
fn shared_pool_registry_holds_no_strong_reference() {
    // A daemon evicting a workspace must actually release the descriptors, so
    // the registry may only hold a weak reference: the last store dropping its
    // pool must close the connections.
    let path = temp_db_path("pool-shared-drop");
    let first = DbPool::shared(&path).unwrap();
    assert_eq!(
        std::sync::Arc::strong_count(&first),
        1,
        "the registry must not keep a pool alive after its stores are dropped"
    );

    let second = DbPool::shared(&path).unwrap();
    assert_eq!(
        std::sync::Arc::strong_count(&first),
        2,
        "a second store on the same file shares the live pool"
    );
    drop(second);
    assert_eq!(std::sync::Arc::strong_count(&first), 1);
    std::fs::remove_dir_all(path.parent().unwrap()).ok();
}

#[test]
fn in_memory_pools_are_never_shared() {
    // Each connection to `:memory:` is its own private database, so two stores
    // asking for `:memory:` are asking for two independent databases.
    let first = DbPool::shared(std::path::Path::new(":memory:")).unwrap();
    let second = DbPool::shared(std::path::Path::new(":memory:")).unwrap();
    assert!(!std::sync::Arc::ptr_eq(&first, &second));

    first
        .write("test.create")
        .execute_batch("CREATE TABLE t (v INTEGER NOT NULL);")
        .unwrap();
    assert!(
        second
            .read("test.read")
            .query_row("SELECT COUNT(*) FROM t", [], |row| row.get::<_, i64>(0))
            .is_err(),
        "in-memory stores must stay isolated from each other"
    );
}
