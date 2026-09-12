use super::*;

#[test]
fn is_sensitive_request_flags_known_types() {
    assert!(is_sensitive_request("start_turn"));
    assert!(is_sensitive_request("respond_approval"));
    assert!(is_sensitive_request("cancel_turn"));
    assert!(is_sensitive_request("get_config"));
    assert!(is_sensitive_request("save_backend_model"));
    assert!(is_sensitive_request("get_observability_summary"));
    assert!(is_sensitive_request("get_memory_context_snapshot"));
    assert!(is_sensitive_request("warm"));
}

#[test]
fn is_sensitive_request_excludes_handshake_and_liveness() {
    assert!(!is_sensitive_request("hello"));
    assert!(!is_sensitive_request("ping"));
}

#[test]
fn verify_token_accepts_matching_token() {
    let token = generate_csprng_token_hex(MIN_TOKEN_BYTES);
    assert_eq!(verify_token(&token, &token), AuthOutcome::Authenticated);
}

#[test]
fn verify_token_rejects_mismatched_token() {
    let expected = generate_csprng_token_hex(MIN_TOKEN_BYTES);
    let presented = generate_csprng_token_hex(MIN_TOKEN_BYTES);
    assert!(!verify_token(&presented, &expected).is_authenticated());
}

#[test]
fn verify_token_rejects_short_server_token() {
    let outcome = verify_token("abc", "abc");
    assert!(!outcome.is_authenticated());
    match outcome {
        AuthOutcome::Rejected(msg) => assert!(msg.contains("minimum length")),
        _ => panic!("expected rejection"),
    }
}

#[test]
fn load_or_create_token_generates_min_length_token() {
    let _env_lock = test_token_path_env_lock();
    let dir = std::env::temp_dir().join(format!("iota-auth-test-{}", generate_csprng_token_hex(8)));
    let token_file = dir.join("daemon.token");
    // SAFETY: test-local env var scoping is inherently racy under parallel
    // test execution; this crate's test suite does not currently run
    // daemon::auth tests concurrently with other env-var-sensitive tests in
    // the same process, and each test uses a unique temp path.
    unsafe {
        std::env::set_var("IOTA_DAEMON_TOKEN_PATH", &token_file);
    }
    let token = load_or_create_token().expect("create token");
    assert_eq!(token.len(), MIN_TOKEN_BYTES * 2);
    let reloaded = load_or_create_token().expect("reload token");
    assert_eq!(token, reloaded, "token must be stable across reloads");
    unsafe {
        std::env::remove_var("IOTA_DAEMON_TOKEN_PATH");
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[cfg(unix)]
#[test]
fn load_or_create_token_file_is_0600() {
    use std::os::unix::fs::PermissionsExt;
    let _env_lock = test_token_path_env_lock();
    let dir = std::env::temp_dir().join(format!("iota-auth-test-{}", generate_csprng_token_hex(8)));
    let token_file = dir.join("daemon.token");
    unsafe {
        std::env::set_var("IOTA_DAEMON_TOKEN_PATH", &token_file);
    }
    let _ = load_or_create_token().expect("create token");
    let meta = std::fs::metadata(&token_file).expect("metadata");
    assert_eq!(meta.permissions().mode() & 0o777, 0o600);
    unsafe {
        std::env::remove_var("IOTA_DAEMON_TOKEN_PATH");
    }
    std::fs::remove_dir_all(&dir).ok();
}
