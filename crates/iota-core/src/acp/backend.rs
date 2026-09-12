use anyhow::{Result, bail};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AcpBackend {
    ClaudeCode,
    Codex,
    Gemini,
    Hermes,
    OpenCode,
}

pub const ALL_BACKENDS: [AcpBackend; 5] = [
    AcpBackend::ClaudeCode,
    AcpBackend::Codex,
    AcpBackend::Gemini,
    AcpBackend::Hermes,
    AcpBackend::OpenCode,
];

impl AcpBackend {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "claude" | "claude-code" | "claudecode" => Ok(Self::ClaudeCode),
            "codex" => Ok(Self::Codex),
            "gemini" | "gemini-cli" => Ok(Self::Gemini),
            "hermes" | "hermes-agent" => Ok(Self::Hermes),
            "opencode" | "open-code" => Ok(Self::OpenCode),
            other => bail!(
                "Unknown ACP backend '{}'. Expected one of: claude-code, codex, gemini, hermes, opencode",
                other
            ),
        }
    }

    pub fn command(self) -> (&'static str, Vec<&'static str>) {
        let npx = if cfg!(windows) { "npx.cmd" } else { "npx" };
        match self {
            Self::ClaudeCode => (
                npx,
                vec!["-y", "@agentclientprotocol/claude-agent-acp@0.32.0"],
            ),
            Self::Codex => (npx, vec!["-y", "@zed-industries/codex-acp@0.12.0"]),
            Self::Gemini => (npx, vec!["-y", "@google/gemini-cli@0.41.2", "--acp"]),
            Self::Hermes => ("hermes", vec!["acp"]),
            Self::OpenCode => (npx, vec!["-y", "opencode-ai@1.14.40", "acp"]),
        }
    }
}

impl fmt::Display for AcpBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
            Self::Gemini => "gemini",
            Self::Hermes => "hermes",
            Self::OpenCode => "opencode",
        };
        f.write_str(value)
    }
}

#[cfg(test)]
#[path = "backend_tests.rs"]
mod backend_tests;
