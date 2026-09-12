//! Tests for engine pool keying, bounds, and eviction.

use super::{DEFAULT_IDLE_TTL, EngineKey, EnginePool, EvictionReason, canonical_workspace_path};
use crate::config::NimiaConfig;
use std::path::{Path, PathBuf};
use std::time::Duration;

fn pool_with_limits(max_engines: usize, idle_ttl: Duration) -> EnginePool {
    EnginePool::new(NimiaConfig::default(), false, 1000).with_limits(max_engines, idle_ttl)
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("iota-pool-{}-{}", label, uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn equivalent_path_spellings_share_one_engine() {
    let dir = temp_dir("spellings");
    let nested = dir.join("sub");
    std::fs::create_dir_all(&nested).unwrap();

    // `sub/..`, `sub/../sub`, and the canonical path all name one directory.
    let direct = canonical_workspace_path(&nested);
    let via_parent = canonical_workspace_path(&nested.join("..").join("sub"));
    let dotted = canonical_workspace_path(&nested.join(".").join("..").join("sub"));

    assert_eq!(
        direct, via_parent,
        "a path reached through `..` must canonicalize to the same key"
    );
    assert_eq!(direct, dotted);

    let mut pool = pool_with_limits(8, DEFAULT_IDLE_TTL);
    let first = pool.engine_for(nested.clone()).engine;
    let second = pool.engine_for(nested.join("..").join("sub")).engine;
    assert!(
        std::sync::Arc::ptr_eq(&first, &second),
        "equivalent spellings must reuse one engine, not create two"
    );
    assert_eq!(pool.len(), 1);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn symlinked_workspace_shares_the_engine_of_its_target() {
    #[cfg(unix)]
    {
        let dir = temp_dir("symlink");
        let real = dir.join("real");
        std::fs::create_dir_all(&real).unwrap();
        let link = dir.join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let mut pool = pool_with_limits(8, DEFAULT_IDLE_TTL);
        let via_real = pool.engine_for(real.clone()).engine;
        let via_link = pool.engine_for(link.clone()).engine;
        assert!(
            std::sync::Arc::ptr_eq(&via_real, &via_link),
            "a symlink and its target must resolve to one engine"
        );
        assert_eq!(pool.len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[test]
fn distinct_workspaces_get_distinct_engines() {
    let a = temp_dir("distinct-a");
    let b = temp_dir("distinct-b");

    let mut pool = pool_with_limits(8, DEFAULT_IDLE_TTL);
    let engine_a = pool.engine_for(a.clone()).engine;
    let engine_b = pool.engine_for(b.clone()).engine;

    assert!(!std::sync::Arc::ptr_eq(&engine_a, &engine_b));
    assert_eq!(pool.len(), 2);
    let _ = std::fs::remove_dir_all(a);
    let _ = std::fs::remove_dir_all(b);
}

#[test]
fn nonexistent_workspace_still_produces_a_stable_key() {
    // A workspace may not exist yet (fresh checkout); keying must not fail or
    // produce a different key on each call.
    let dir = temp_dir("missing");
    let phantom = dir.join("does-not-exist");

    let first = canonical_workspace_path(&phantom);
    let second = canonical_workspace_path(&phantom);
    assert_eq!(first, second);

    let mut pool = pool_with_limits(8, DEFAULT_IDLE_TTL);
    let engine = pool.engine_for(phantom.clone()).engine;
    let again = pool.engine_for(phantom).engine;
    assert!(std::sync::Arc::ptr_eq(&engine, &again));
    assert_eq!(pool.len(), 1);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn pool_evicts_least_recently_used_at_capacity() {
    let dirs: Vec<PathBuf> = (0..3).map(|i| temp_dir(&format!("lru-{i}"))).collect();
    let mut pool = pool_with_limits(2, DEFAULT_IDLE_TTL);

    let first = pool.engine_for(dirs[0].clone()).engine;
    let _second = pool.engine_for(dirs[1].clone()).engine;
    // Touch the first so the second is now the least recently used.
    let first_again = pool.engine_for(dirs[0].clone()).engine;
    assert!(std::sync::Arc::ptr_eq(&first, &first_again));

    let checkout = pool.engine_for(dirs[2].clone());
    let evicted = checkout
        .evicted
        .as_ref()
        .expect("a third workspace must evict to respect the cap");
    assert_eq!(evicted.reason, EvictionReason::Capacity);
    assert_eq!(
        evicted.key.cwd,
        canonical_workspace_path(&dirs[1]),
        "the least recently used workspace must be the one evicted"
    );
    assert_eq!(pool.len(), 2);

    for dir in dirs {
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[test]
fn touching_a_workspace_protects_it_from_the_next_eviction() {
    // Regression: updating `last_used` before the eviction check made the
    // just-touched entry eligible for eviction, degrading LRU into FIFO.
    let dirs: Vec<PathBuf> = (0..3).map(|i| temp_dir(&format!("touch-{i}"))).collect();
    let mut pool = pool_with_limits(2, DEFAULT_IDLE_TTL);

    pool.engine_for(dirs[0].clone());
    pool.engine_for(dirs[1].clone());
    // Re-touching dirs[0] must make dirs[1] the eviction candidate.
    pool.engine_for(dirs[0].clone());

    let checkout = pool.engine_for(dirs[2].clone());
    let evicted = checkout.evicted.expect("eviction expected");
    assert_eq!(evicted.key.cwd, canonical_workspace_path(&dirs[1]));

    for dir in dirs {
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[test]
fn reaping_removes_only_engines_idle_past_the_ttl() {
    let fresh = temp_dir("idle-fresh");
    let stale = temp_dir("idle-stale");
    let mut pool = pool_with_limits(8, Duration::from_secs(60));

    pool.engine_for(stale.clone());
    pool.engine_for(fresh.clone());

    // Nothing is stale yet.
    let now = std::time::Instant::now();
    assert!(pool.reap_idle(now).is_empty());

    // Advance past the TTL: every engine is now stale.
    let later = now + Duration::from_secs(120);
    let reaped = pool.reap_idle(later);
    assert_eq!(reaped.len(), 2);
    assert!(reaped.iter().all(|e| e.reason == EvictionReason::Idle));
    assert_eq!(pool.len(), 0);

    for dir in [fresh, stale] {
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[test]
fn reaping_never_evicts_more_than_the_idle_set() {
    let dir = temp_dir("idle-partial");
    let mut pool = pool_with_limits(8, Duration::from_secs(600));

    pool.engine_for(dir.clone());
    // A moment later, still well inside the TTL.
    let reaped = pool.reap_idle(std::time::Instant::now() + Duration::from_secs(1));
    assert!(reaped.is_empty());
    assert_eq!(
        pool.len(),
        1,
        "a recently used engine must survive the sweep"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn engine_key_is_orderable_and_stable() {
    let dir = temp_dir("key-order");
    let key_a = EngineKey::new(&dir);
    let key_b = EngineKey::new(&dir);
    assert_eq!(key_a, key_b);

    let other = temp_dir("key-order-other");
    let key_c = EngineKey::new(&other);
    // BTreeMap requires Ord; distinct keys must not compare equal.
    assert_ne!(key_a, key_c);
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_dir_all(other);
}

#[test]
fn limits_are_clamped_to_at_least_one() {
    // A zero cap would make `engine_for` evict on every call, including the
    // entry it is about to insert.
    let pool =
        EnginePool::new(NimiaConfig::default(), false, 1000).with_limits(0, Duration::from_secs(1));
    assert_eq!(pool.max_engines, 1);
}

#[test]
fn canonical_path_of_a_root_like_path_is_absolute() {
    let canonical = canonical_workspace_path(Path::new("."));
    assert!(
        canonical.is_absolute(),
        "a relative workspace must canonicalize to an absolute path, got {canonical:?}"
    );
}
