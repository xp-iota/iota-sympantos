use anyhow::{Context, Result, bail};
use std::fs;
use std::path::PathBuf;

use super::NimiaConfig;

pub fn config_path() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Failed to get home directory")?;
    Ok(home.join(".i6").join("nimia.yaml"))
}

pub fn read_config() -> Result<NimiaConfig> {
    let path = config_path()?;
    if !path.exists() {
        bail!("Backend config not found: {}", path.display());
    }
    let content =
        fs::read_to_string(&path).with_context(|| format!("Failed to read {}", path.display()))?;
    serde_yaml::from_str(&content).with_context(|| format!("Failed to parse {}", path.display()))
}

/// Persists `config` to `~/.i6/nimia.yaml`.
///
/// The config may contain API keys (result.md S-03), so the write goes
/// through [`crate::fs_secure::atomic_write_secure`]: the parent directory
/// is locked to owner-only (`0700`), the file itself ends up `0600`, and the
/// write is atomic (temp file + rename) so a crash mid-write never leaves a
/// truncated or partially-written config behind. If a config file already
/// exists with looser permissions from before this hardening, the next save
/// tightens it (the temp-file-then-rename always produces a fresh `0600`
/// file, so this happens automatically rather than requiring a separate
/// migration step).
pub fn save_config(config: &NimiaConfig) -> Result<()> {
    let path = config_path()?;
    let content = serde_yaml::to_string(config).context("Failed to encode config")?;
    crate::fs_secure::atomic_write_secure(&path, content.as_bytes())
        .with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}
