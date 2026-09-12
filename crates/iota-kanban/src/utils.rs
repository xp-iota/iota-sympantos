use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

/// Returns the current Unix timestamp in seconds.
pub fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Lock a `std::sync::Mutex` and recover gracefully from a poisoned state.
pub fn lock_or_recover<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|err: PoisonError<MutexGuard<'_, T>>| {
            tracing::warn!("mutex was poisoned by a previous panic; recovering inner value");
            err.into_inner()
        })
}
