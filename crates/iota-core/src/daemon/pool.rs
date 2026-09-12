//! Engine pool for the daemon service.
//!
//! [`EnginePool`] maintains one [`IotaEngine`] per workspace so ACP subprocess
//! connections are reused across CLI invocations and backend handoff state is
//! shared.
//!
//! # Keying
//!
//! The key is a *canonicalized* workspace path. Without canonicalization the
//! same directory reached by different spellings — a relative path, a symlink,
//! a Windows path differing only in case — would each get a separate engine,
//! duplicating ACP subprocesses and splitting handoff state between them.
//!
//! # Bounds
//!
//! Two independent limits keep the pool from growing without bound:
//!
//! - `max_engines`: hard cap, enforced by evicting the least-recently-used
//!   workspace. Eviction shuts the engine's ACP clients down so child
//!   processes do not leak.
//! - `idle_ttl`: engines untouched for this long are reaped even when the pool
//!   is below its cap, so a daemon that has served many one-off workspaces
//!   releases their subprocesses.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

use crate::config::NimiaConfig;
use crate::engine::IotaEngine;
use crate::resources::LocalResources;

/// Default maximum number of pooled engines.
pub const DEFAULT_MAX_ENGINES: usize = 16;

/// Default idle time after which an engine is reaped.
pub const DEFAULT_IDLE_TTL: Duration = Duration::from_secs(30 * 60);

/// Composite key used to bucket engines by workspace.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct EngineKey {
    pub cwd: PathBuf,
}

impl EngineKey {
    /// Builds a key from a caller-supplied workspace path.
    ///
    /// The path is canonicalized so two spellings of one directory share an
    /// engine. Canonicalization requires the path to exist; when it does not
    /// (or on platforms where it fails, e.g. a network path), the path is
    /// normalized lexically instead — an absolute, dot-segment-free path with
    /// platform-appropriate case handling.
    pub fn new(cwd: &Path) -> Self {
        Self {
            cwd: canonical_workspace_path(cwd),
        }
    }
}

/// Canonicalizes a workspace path for use as a pool key.
///
/// Falls back to lexical normalization when the filesystem cannot resolve the
/// path, so a not-yet-created workspace still gets a stable key rather than
/// causing an error.
pub(crate) fn canonical_workspace_path(cwd: &Path) -> PathBuf {
    if let Ok(canonical) = std::fs::canonicalize(cwd) {
        return normalize_case(canonical);
    }
    let absolute = if cwd.is_absolute() {
        cwd.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|base| base.join(cwd))
            .unwrap_or_else(|_| cwd.to_path_buf())
    };
    normalize_case(lexically_normalize(&absolute))
}

/// Removes `.` and `..` segments without touching the filesystem.
fn lexically_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                // Popping past the root is a no-op, matching path resolution.
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Folds case and strips the Windows verbatim prefix.
#[cfg(windows)]
fn normalize_case(path: PathBuf) -> PathBuf {
    // `canonicalize` on Windows returns a `\\?\` verbatim path, which the rest
    // of the codebase and `LocalResources` do not expect. Strip it, then match
    // Windows' case-insensitive filesystem so `C:\Repo` and `c:\repo` agree.
    let text = path.to_string_lossy();
    let stripped = text.strip_prefix(r"\\?\").unwrap_or(&text).to_string();
    PathBuf::from(stripped.to_lowercase())
}

#[cfg(not(windows))]
fn normalize_case(path: PathBuf) -> PathBuf {
    // Unix filesystems are case-sensitive, so case must NOT be folded here:
    // `/tmp/Repo` and `/tmp/repo` are genuinely different directories.
    path
}

/// Why an engine was removed from the pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvictionReason {
    /// The pool was at `max_engines` and this was the least-recently-used.
    Capacity,
    /// The engine had been idle past `idle_ttl`.
    Idle,
}

impl EvictionReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Capacity => "capacity",
            Self::Idle => "idle",
        }
    }
}

/// An engine removed from the pool, handed back so the caller can shut its ACP
/// clients down outside the pool lock.
pub(crate) struct EvictedEngine {
    pub key: EngineKey,
    pub engine: Arc<Mutex<IotaEngine>>,
    pub reason: EvictionReason,
}

/// Holds one [`IotaEngine`] per workspace.
///
/// Wrapped in `Arc<Mutex<EnginePool>>` by the daemon server loop.
pub(crate) struct EnginePool {
    pub config: NimiaConfig,
    pub show_native: bool,
    pub timeout_ms: u64,
    pub engines: BTreeMap<EngineKey, Arc<Mutex<IotaEngine>>>,
    last_used: BTreeMap<EngineKey, Instant>,
    pub max_engines: usize,
    idle_ttl: Duration,
}

impl EnginePool {
    pub fn new(config: NimiaConfig, show_native: bool, timeout_ms: u64) -> Self {
        let store_config = crate::config::store_config();
        Self {
            config,
            show_native,
            timeout_ms,
            engines: BTreeMap::new(),
            last_used: BTreeMap::new(),
            max_engines: DEFAULT_MAX_ENGINES,
            idle_ttl: DEFAULT_IDLE_TTL,
        }
        .with_limits(
            store_config.engine_pool_max,
            Duration::from_secs(store_config.engine_pool_idle_ttl_secs),
        )
    }

    /// Overrides the pool bounds. Used by configuration and by tests.
    pub fn with_limits(mut self, max_engines: usize, idle_ttl: Duration) -> Self {
        // A zero cap would evict on every checkout, including the entry about
        // to be inserted, so the pool would never hold anything.
        self.max_engines = max_engines.max(1);
        self.idle_ttl = idle_ttl;
        self
    }

    /// Returns (or creates) the engine for `cwd`, evicting if the pool is full.
    ///
    /// The evicted engine is *not* shut down here — the caller does that outside
    /// the pool lock, since draining ACP clients is async and can block.
    pub(crate) fn engine_for(&mut self, cwd: PathBuf) -> PoolCheckout {
        let key = EngineKey::new(&cwd);
        let mut evicted = None;

        // Evict before touching `last_used`, so an entry that is about to be
        // evicted can never be the one we just marked as most-recently-used.
        if !self.engines.contains_key(&key)
            && self.engines.len() >= self.max_engines
            && let Some(oldest) = self.least_recently_used()
        {
            evicted = self.remove(&oldest, EvictionReason::Capacity);
        }

        self.last_used.insert(key.clone(), Instant::now());
        let canonical_cwd = key.cwd.clone();
        let engine = self
            .engines
            .entry(key)
            .or_insert_with(|| {
                Arc::new(Mutex::new(IotaEngine::create_session_with_resources(
                    self.config.clone(),
                    LocalResources::from_workspace(canonical_cwd.clone()),
                    self.show_native,
                    self.timeout_ms,
                    Some(&canonical_cwd),
                )))
            })
            .clone();

        PoolCheckout { engine, evicted }
    }

    /// Removes (but does not return) every engine idle past the TTL.
    ///
    /// Called periodically by the daemon so long-lived processes release ACP
    /// subprocesses for workspaces nobody is using any more.
    pub(crate) fn reap_idle(&mut self, now: Instant) -> Vec<EvictedEngine> {
        let stale: Vec<EngineKey> = self
            .last_used
            .iter()
            .filter(|(_, used_at)| now.duration_since(**used_at) >= self.idle_ttl)
            .map(|(key, _)| key.clone())
            .collect();
        stale
            .into_iter()
            .filter_map(|key| self.remove(&key, EvictionReason::Idle))
            .collect()
    }

    fn least_recently_used(&self) -> Option<EngineKey> {
        self.last_used
            .iter()
            .min_by_key(|(_, used_at)| **used_at)
            .map(|(key, _)| key.clone())
    }

    fn remove(&mut self, key: &EngineKey, reason: EvictionReason) -> Option<EvictedEngine> {
        let engine = self.engines.remove(key)?;
        self.last_used.remove(key);
        crate::telemetry::metrics::get().record_pool_eviction(reason.as_str());
        tracing::info!(
            workspace = %key.cwd.display(),
            reason = reason.as_str(),
            pool_size = self.engines.len(),
            "evicted engine from pool"
        );
        Some(EvictedEngine {
            key: key.clone(),
            engine,
            reason,
        })
    }

    pub fn all_engines(&self) -> Vec<Arc<Mutex<IotaEngine>>> {
        self.engines.values().cloned().collect()
    }

    /// Number of engines currently pooled.
    ///
    /// Reported as a metric by the daemon's idle reaper, which is also where a
    /// shrinking pool becomes visible.
    pub fn len(&self) -> usize {
        self.engines.len()
    }

    /// Records the current pool occupancy as a gauge.
    ///
    /// Called periodically by the daemon's idle reaper, which is also where a
    /// shrinking pool becomes visible.
    pub fn record_pool_size(&self) {
        crate::telemetry::metrics::get().record_pool_size(self.len());
    }

    pub fn config(&self) -> NimiaConfig {
        self.config.clone()
    }

    pub async fn replace_config(&mut self, config: NimiaConfig) {
        self.config = config.clone();
        for engine in self.engines.values() {
            let mut engine_guard = engine.lock().await;
            engine_guard.update_config(config.clone()).await;
        }
    }
}

/// The result of asking the pool for an engine: the engine, plus any engine
/// evicted to make room for it.
pub(crate) struct PoolCheckout {
    pub engine: Arc<Mutex<IotaEngine>>,
    pub evicted: Option<EvictedEngine>,
}

impl PoolCheckout {
    /// Shuts down the evicted engine's ACP clients, if one was evicted.
    ///
    /// Separate from checkout so the (async, potentially slow) shutdown runs
    /// outside the pool lock.
    pub(crate) async fn shutdown_evicted(&self) {
        if let Some(evicted) = &self.evicted {
            let mut engine = evicted.engine.lock().await;
            let closed = engine.open_client_count();
            engine.shutdown_open_clients().await;
            tracing::info!(
                workspace = %evicted.key.cwd.display(),
                reason = evicted.reason.as_str(),
                closed_clients = closed,
                "shut down evicted engine ACP clients"
            );
        }
    }
}

#[cfg(test)]
#[path = "pool_tests.rs"]
mod pool_tests;
