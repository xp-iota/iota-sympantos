use super::*;

#[cfg(unix)]
#[test]
fn ensure_dir_owner_only_sets_0700() {
    use std::os::unix::fs::PermissionsExt;
    let base = std::env::temp_dir().join(format!(
        "iota-fs-secure-test-{}",
        generate_csprng_token_hex(8)
    ));
    ensure_dir_owner_only(&base).expect("create dir");
    let meta = fs::metadata(&base).expect("metadata");
    assert_eq!(meta.permissions().mode() & 0o777, 0o700);
    fs::remove_dir_all(&base).ok();
}

#[cfg(unix)]
#[test]
fn ensure_dir_owner_only_tightens_existing_dir() {
    use std::os::unix::fs::PermissionsExt;
    let base = std::env::temp_dir().join(format!(
        "iota-fs-secure-test-{}",
        generate_csprng_token_hex(8)
    ));
    fs::create_dir_all(&base).expect("create dir");
    fs::set_permissions(&base, fs::Permissions::from_mode(0o777)).expect("loosen perms");
    ensure_dir_owner_only(&base).expect("tighten dir");
    let meta = fs::metadata(&base).expect("metadata");
    assert_eq!(meta.permissions().mode() & 0o777, 0o700);
    fs::remove_dir_all(&base).ok();
}

#[cfg(unix)]
#[test]
fn atomic_write_secure_sets_0600_and_writes_contents() {
    use std::os::unix::fs::PermissionsExt;
    let base = std::env::temp_dir().join(format!(
        "iota-fs-secure-test-{}",
        generate_csprng_token_hex(8)
    ));
    let file_path = base.join("secret.txt");
    atomic_write_secure(&file_path, b"hello secure world").expect("write");
    let meta = fs::metadata(&file_path).expect("metadata");
    assert_eq!(meta.permissions().mode() & 0o777, 0o600);
    let contents = fs::read(&file_path).expect("read back");
    assert_eq!(contents, b"hello secure world");
    fs::remove_dir_all(&base).ok();
}

#[test]
fn atomic_write_secure_overwrites_existing_file_completely() {
    let base = std::env::temp_dir().join(format!(
        "iota-fs-secure-test-{}",
        generate_csprng_token_hex(8)
    ));
    let file_path = base.join("secret.txt");
    atomic_write_secure(&file_path, b"first version, quite long indeed").expect("write 1");
    atomic_write_secure(&file_path, b"v2").expect("write 2");
    let contents = fs::read(&file_path).expect("read back");
    assert_eq!(contents, b"v2");
    fs::remove_dir_all(&base).ok();
}

#[test]
fn no_leftover_temp_files_after_atomic_write() {
    let base = std::env::temp_dir().join(format!(
        "iota-fs-secure-test-{}",
        generate_csprng_token_hex(8)
    ));
    let file_path = base.join("secret.txt");
    atomic_write_secure(&file_path, b"data").expect("write");
    let entries: Vec<_> = fs::read_dir(&base)
        .expect("read dir")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(entries, vec!["secret.txt".to_string()]);
    fs::remove_dir_all(&base).ok();
}

#[test]
fn constant_time_eq_matches_equal_slices() {
    assert!(constant_time_eq(b"same-secret-token", b"same-secret-token"));
}

#[test]
fn constant_time_eq_rejects_different_slices() {
    assert!(!constant_time_eq(b"same-secret-token", b"other-secret-tok"));
}

#[test]
fn constant_time_eq_rejects_different_lengths() {
    assert!(!constant_time_eq(b"short", b"a-much-longer-value"));
}

#[test]
fn generate_csprng_token_hex_has_expected_length_and_varies() {
    let a = generate_csprng_token_hex(32);
    let b = generate_csprng_token_hex(32);
    assert_eq!(a.len(), 64); // 32 bytes -> 64 hex chars
    assert_ne!(
        a, b,
        "two independently generated tokens should not collide"
    );
}
