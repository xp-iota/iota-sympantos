use anyhow::{Context, Result, bail};
use futures_util::StreamExt;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

/// Maximum size accepted for a downloaded skill file. Skills are
/// human-authored markdown/YAML documents, not large binaries; this bounds
/// memory usage and mitigates a malicious/compromised source attempting to
/// exhaust disk or memory (result.md S-05).
const MAX_SKILL_DOWNLOAD_BYTES: u64 = 10 * 1024 * 1024; // 10 MiB

/// Network timeout for the entire download (connect + body).
const SKILL_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30);

/// Sanitize a candidate file name so it is safe to use as a single path
/// component inside the skill cache directory.
///
/// Rules:
/// - Use only the final path component (strips any directory prefix).
/// - Keep only alphanumeric characters, hyphens, underscores and dots.
/// - Reject names that are empty, consist solely of dots (`..`, `.`), or
///   exceed 128 characters after sanitization.
fn sanitize_file_name(raw: &str) -> Result<String> {
    let raw_path = Path::new(raw);
    if raw_path.is_absolute()
        || raw_path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        bail!("skill file name '{}' must not contain path traversal", raw);
    }
    // Take only the final segment — rejects embedded `/` and `\`.
    let base = raw_path
        .file_name()
        .and_then(|os| os.to_str())
        .unwrap_or(raw);

    // Filter to a safe character set.
    let sanitized: String = base
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect();

    if sanitized.is_empty() {
        bail!("skill file name is empty after sanitization");
    }
    // Reject pure-dot names (`.`, `..`) that are directory references.
    if sanitized.chars().all(|c| c == '.') {
        bail!(
            "skill file name '{}' is a reserved path component",
            sanitized
        );
    }
    if sanitized.len() > 128 {
        bail!(
            "skill file name is too long ({} chars, max 128)",
            sanitized.len()
        );
    }
    Ok(sanitized)
}

/// Metadata recorded alongside each cached skill file, so operators can
/// audit where cached content came from and detect drift/tampering later.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct SkillCacheMetadata {
    source: String,
    sha256: String,
    fetched_at_unix: i64,
    byte_len: u64,
}

pub async fn pull_skill(source: &str, name: Option<&str>) -> Result<PathBuf> {
    pull_skill_with_checksum(source, name, None).await
}

/// Pulls a skill file from `source` (an `https://` URL or a local path) into
/// the local cache, optionally verifying its SHA-256 digest against
/// `expected_sha256` before publishing it.
///
/// Hardening applied (result.md S-05 — remote skill fetch is a supply-chain
/// boundary):
/// - Only `https://` URLs are accepted for network sources; plain `http://`
///   is rejected outright so skill content can never be fetched or tampered
///   with by a network-position attacker performing a downgrade.
/// - The response body is streamed with a hard [`MAX_SKILL_DOWNLOAD_BYTES`]
///   cap and an overall [`SKILL_DOWNLOAD_TIMEOUT`], so a malicious or
///   misbehaving source cannot exhaust memory/disk or hang the caller
///   indefinitely.
/// - If `expected_sha256` is provided, the downloaded bytes are hashed and
///   compared (case-insensitively) before anything is written to the cache;
///   a mismatch aborts the pull with no file written.
/// - The file is written to a temp path and only renamed into the cache
///   directory after the size/checksum checks pass, so a partially
///   downloaded or failed-verification file is never visible under its
///   final name.
/// - A `<file>.meta.json` sidecar records the source URL/path, computed
///   digest, fetch time, and byte length for later audit.
pub async fn pull_skill_with_checksum(
    source: &str,
    name: Option<&str>,
    expected_sha256: Option<&str>,
) -> Result<PathBuf> {
    let home = dirs::home_dir().context("Failed to get home directory")?;
    let cache = home.join(".i6").join("skills").join("registry-cache");
    crate::fs_secure::ensure_dir_owner_only(&cache)
        .with_context(|| format!("Failed to create {}", cache.display()))?;

    // Derive the raw candidate name from the explicit argument or URL tail.
    let raw_name = name
        .map(str::to_string)
        .or_else(|| source.rsplit('/').next().map(str::to_string))
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "skill.md".to_string());

    let file_name = sanitize_file_name(&raw_name)
        .with_context(|| format!("Invalid skill file name derived from source '{}'", source))?;

    let path = cache.join(&file_name);

    // Verify the resolved path is still inside the cache directory (defence in depth).
    let resolved = path.canonicalize().unwrap_or_else(|_| path.clone()); // file may not exist yet — check parent instead
    let resolved_parent = resolved.parent().unwrap_or(&resolved);
    let cache_canonical = cache.canonicalize().unwrap_or_else(|_| cache.clone());
    if !resolved_parent.starts_with(&cache_canonical) {
        bail!(
            "skill file name '{}' would escape the cache directory",
            file_name
        );
    }

    let is_network_source = source.starts_with("http://") || source.starts_with("https://");
    if source.starts_with("http://") {
        bail!(
            "refusing to fetch skill from plain-text http:// source '{}': only https:// is \
             permitted for network skill sources",
            source
        );
    }

    let content_bytes = if is_network_source {
        download_skill_bytes(source).await?
    } else {
        let metadata =
            std::fs::metadata(source).with_context(|| format!("Failed to stat {}", source))?;
        if metadata.len() > MAX_SKILL_DOWNLOAD_BYTES {
            bail!(
                "skill source '{}' is {} bytes, exceeding the {} byte limit",
                source,
                metadata.len(),
                MAX_SKILL_DOWNLOAD_BYTES
            );
        }
        std::fs::read(source).with_context(|| format!("Failed to read {}", source))?
    };

    if let Some(expected) = expected_sha256 {
        let actual = sha256_hex(&content_bytes);
        if !actual.eq_ignore_ascii_case(expected.trim()) {
            bail!(
                "skill content from '{}' failed checksum verification: expected sha256 {}, got {}",
                source,
                expected.trim(),
                actual
            );
        }
    }

    // Write to a temp path and rename over the final path only after all
    // checks above have passed, so a failed/interrupted pull never leaves
    // a partially-written or unverified file visible under the real name.
    crate::fs_secure::atomic_write_secure(&path, &content_bytes)
        .with_context(|| format!("Failed to write {}", path.display()))?;

    let metadata = SkillCacheMetadata {
        source: source.to_string(),
        sha256: sha256_hex(&content_bytes),
        fetched_at_unix: chrono::Utc::now().timestamp(),
        byte_len: content_bytes.len() as u64,
    };
    let meta_path = cache.join(format!("{file_name}.meta.json"));
    let meta_json = serde_json::to_vec_pretty(&metadata)
        .context("Failed to serialize skill cache provenance")?;
    crate::fs_secure::atomic_write_secure(&meta_path, &meta_json)
        .with_context(|| format!("Failed to write provenance {}", meta_path.display()))?;

    Ok(path)
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

async fn download_skill_bytes(source: &str) -> Result<Vec<u8>> {
    let client = reqwest::Client::builder()
        .timeout(SKILL_DOWNLOAD_TIMEOUT)
        .build()
        .context("Failed to build HTTP client for skill download")?;
    let response = client
        .get(source)
        .send()
        .await
        .with_context(|| format!("Failed to request {}", source))?
        .error_for_status()
        .with_context(|| format!("Skill source '{}' returned an error status", source))?;

    if let Some(content_length) = response.content_length()
        && content_length > MAX_SKILL_DOWNLOAD_BYTES
    {
        bail!(
            "skill source '{}' declared Content-Length {} bytes, exceeding the {} byte limit",
            source,
            content_length,
            MAX_SKILL_DOWNLOAD_BYTES
        );
    }

    let mut stream = response.bytes_stream();
    let mut buffer = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.with_context(|| format!("Error while downloading {}", source))?;
        buffer.extend_from_slice(&chunk);
        if buffer.len() as u64 > MAX_SKILL_DOWNLOAD_BYTES {
            bail!(
                "skill source '{}' exceeded the {} byte download limit",
                source,
                MAX_SKILL_DOWNLOAD_BYTES
            );
        }
    }
    Ok(buffer)
}

#[cfg(test)]
#[path = "cache_tests.rs"]
mod cache_tests;
