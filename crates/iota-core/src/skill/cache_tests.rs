use crate::skill::cache::*;

#[test]
fn accepts_normal_names() {
    assert!(sanitize_file_name("my-skill.md").is_ok());
    assert!(sanitize_file_name("skill_v2.yaml").is_ok());
    assert!(sanitize_file_name("Skill123").is_ok());
}

#[test]
fn rejects_path_traversal() {
    assert!(sanitize_file_name("../../.bashrc").is_err());
    assert!(sanitize_file_name("..").is_err());
    assert!(sanitize_file_name(".").is_err());
}

#[test]
fn strips_directory_prefix() {
    // Path::file_name extracts only the final component.
    let name = sanitize_file_name("subdir/skill.md").unwrap();
    assert_eq!(name, "skill.md");
}

#[test]
fn replaces_unsafe_chars() {
    let name = sanitize_file_name("my skill (v2)!.md").unwrap();
    assert!(!name.contains(' '));
    assert!(!name.contains('('));
    assert!(!name.contains(')'));
    assert!(!name.contains('!'));
}

#[test]
fn rejects_empty_and_too_long() {
    assert!(sanitize_file_name("").is_err());
    let long = "a".repeat(129);
    assert!(sanitize_file_name(&long).is_err());
}

// ---------------------------------------------------------------------------
// S-05 regression tests: skill fetch supply-chain hardening.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn rejects_plain_http_source() {
    let result = pull_skill("http://example.com/skill.md", None).await;
    assert!(result.is_err());
    let message = result.unwrap_err().to_string();
    assert!(
        message.contains("https://") || message.contains("http://"),
        "error should explain the https-only requirement: {message}"
    );
}

#[tokio::test]
async fn local_path_with_wrong_checksum_is_rejected() {
    let dir = std::env::temp_dir().join(format!("iota-skill-cache-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let source_path = dir.join("source-skill.md");
    std::fs::write(&source_path, b"skill content").unwrap();

    let result = pull_skill_with_checksum(
        source_path.to_str().unwrap(),
        Some("local-checksum-test.md"),
        Some("0000000000000000000000000000000000000000000000000000000000000000"),
    )
    .await;
    assert!(result.is_err());
    assert!(
        result.unwrap_err().to_string().contains("checksum"),
        "expected a checksum verification failure"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn local_path_with_correct_checksum_succeeds() {
    use sha2::{Digest, Sha256};

    let dir = std::env::temp_dir().join(format!("iota-skill-cache-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let source_path = dir.join("source-skill-ok.md");
    let content = b"skill content that matches its digest";
    std::fs::write(&source_path, content).unwrap();

    let mut hasher = Sha256::new();
    hasher.update(content);
    let digest = hex::encode(hasher.finalize());

    let result = pull_skill_with_checksum(
        source_path.to_str().unwrap(),
        Some("local-checksum-ok-test.md"),
        Some(&digest),
    )
    .await;
    assert!(result.is_ok(), "expected success: {:?}", result.err());

    std::fs::remove_dir_all(&dir).ok();
}
