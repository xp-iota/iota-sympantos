use anyhow::{Context, Result};

pub fn expand_home_path(value: &str) -> Result<String> {
    if value == "~" || value.starts_with("~/") || value.starts_with("~\\") {
        let home = dirs::home_dir().context("Failed to get home directory")?;
        if value == "~" {
            return Ok(home.display().to_string());
        }
        return Ok(home.join(&value[2..]).display().to_string());
    }
    Ok(value.to_string())
}

pub fn normalize_command(command: &str) -> String {
    if cfg!(windows) && command.eq_ignore_ascii_case("npx") {
        "npx.cmd".to_string()
    } else {
        command.to_string()
    }
}
