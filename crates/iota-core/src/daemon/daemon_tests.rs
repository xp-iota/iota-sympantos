use crate::daemon::*;

#[test]
fn warm_count_only_increments_for_newly_started_backend() {
    let mut warmed = 0;

    record_warm_result(&mut warmed, false);
    record_warm_result(&mut warmed, true);

    assert_eq!(warmed, 1);
}

// `guard_daemon_bind_addr` reads a process-wide env var, so these tests must
// not run concurrently with each other (or with anything else touching
// `IOTA_DAEMON_ALLOW_NON_LOOPBACK`). A dedicated mutex serializes them; each
// test clears the var on both entry and exit so failures do not leak state
// into unrelated tests.
static ENV_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn guard_allows_ipv4_loopback_by_default() {
    let _lock = ENV_GUARD.lock().unwrap();
    unsafe {
        std::env::remove_var("IOTA_DAEMON_ALLOW_NON_LOOPBACK");
    }
    assert!(guard_daemon_bind_addr("127.0.0.1:47661").is_ok());
}

#[test]
fn guard_allows_ipv6_loopback_by_default() {
    let _lock = ENV_GUARD.lock().unwrap();
    unsafe {
        std::env::remove_var("IOTA_DAEMON_ALLOW_NON_LOOPBACK");
    }
    assert!(guard_daemon_bind_addr("[::1]:47661").is_ok());
}

#[test]
fn guard_rejects_wildcard_address_by_default() {
    let _lock = ENV_GUARD.lock().unwrap();
    unsafe {
        std::env::remove_var("IOTA_DAEMON_ALLOW_NON_LOOPBACK");
    }
    let err = guard_daemon_bind_addr("0.0.0.0:47661").unwrap_err();
    assert!(err.to_string().contains("non-loopback"));
}

#[test]
fn guard_rejects_lan_address_by_default() {
    let _lock = ENV_GUARD.lock().unwrap();
    unsafe {
        std::env::remove_var("IOTA_DAEMON_ALLOW_NON_LOOPBACK");
    }
    let err = guard_daemon_bind_addr("192.168.1.10:47661").unwrap_err();
    assert!(err.to_string().contains("non-loopback"));
}

#[test]
fn guard_rejects_unparseable_host_by_default() {
    let _lock = ENV_GUARD.lock().unwrap();
    unsafe {
        std::env::remove_var("IOTA_DAEMON_ALLOW_NON_LOOPBACK");
    }
    // A bare hostname is not verifiably loopback and must fail closed.
    let err = guard_daemon_bind_addr("my-host:47661").unwrap_err();
    assert!(err.to_string().contains("non-loopback"));
}

#[test]
fn guard_allows_non_loopback_with_explicit_opt_in() {
    let _lock = ENV_GUARD.lock().unwrap();
    unsafe {
        std::env::set_var("IOTA_DAEMON_ALLOW_NON_LOOPBACK", "1");
    }
    let result = guard_daemon_bind_addr("0.0.0.0:47661");
    unsafe {
        std::env::remove_var("IOTA_DAEMON_ALLOW_NON_LOOPBACK");
    }
    assert!(result.is_ok());
}
