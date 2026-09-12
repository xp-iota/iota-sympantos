use anyhow::{Context, Result, bail};
use std::io::{self, IsTerminal, Read};
use std::path::PathBuf;

use super::{AcpBackend, DEFAULT_TIMEOUT_MS};

pub struct AcpRunOptions {
    pub backend: AcpBackend,
    pub multi_backend: bool,
    pub cwd: PathBuf,
    pub prompt: String,
    pub show_native: bool,
    pub use_daemon: bool,
    pub log_events: bool,
    pub timing: bool,
    pub timeout_ms: u64,
}

pub fn parse_acp_args(args: &[String]) -> Result<AcpRunOptions> {
    let mut backend = AcpBackend::Codex;
    let mut multi_backend = false;
    let mut cwd = std::env::current_dir().context("Failed to get current directory")?;
    let mut show_native = false;
    let mut use_daemon = true;
    let mut log_events = false;
    let mut timing = false;
    let mut timeout_ms = DEFAULT_TIMEOUT_MS;
    let mut prompt_parts = Vec::new();
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "5backend" => {
                multi_backend = true;
            }
            "-b" | "--backend" => {
                index += 1;
                let value = args
                    .get(index)
                    .context("--backend requires a backend name")?;
                backend = AcpBackend::parse(value)?;
            }
            "--cwd" => {
                index += 1;
                let value = args.get(index).context("--cwd requires a path")?;
                cwd = PathBuf::from(value);
            }
            "--show-native" => {
                show_native = true;
            }
            "-d" | "--daemon" | "--require-daemon" => {
                use_daemon = true;
            }
            "--no-daemon" => {
                use_daemon = false;
            }
            "--log-events" => {
                log_events = true;
            }
            "--timing" => {
                timing = true;
            }
            "--timeout-ms" => {
                index += 1;
                let value = args.get(index).context("--timeout-ms requires a value")?;
                timeout_ms = value.parse().context("--timeout-ms must be an integer")?;
                if timeout_ms == 0 {
                    bail!("--timeout-ms must be greater than 0");
                }
            }
            "-h" | "--help" => {
                print_acp_help();
                std::process::exit(0);
            }
            "--" => {
                prompt_parts.extend(args[index + 1..].iter().cloned());
                break;
            }
            value if is_backend_alias(value) && prompt_parts.is_empty() => {
                backend = AcpBackend::parse(value)?;
            }
            value => {
                prompt_parts.push(value.to_string());
            }
        }
        index += 1;
    }

    let prompt = if prompt_parts.is_empty() {
        read_prompt_from_stdin()?
    } else {
        prompt_parts.join(" ")
    };

    if prompt.trim().is_empty() {
        bail!("ACP prompt is empty");
    }

    Ok(AcpRunOptions {
        backend,
        multi_backend,
        cwd,
        prompt,
        show_native,
        use_daemon,
        log_events,
        timing,
        timeout_ms,
    })
}

pub fn print_acp_help() {
    println!(
        "Usage:\n  iota run [backend] [options] <prompt>\n\nOptions:\n  -b, --backend <name>   claude-code | codex | gemini | hermes | opencode\n      --cwd <path>       Working directory for session/new\n      --show-native      Print raw ACP messages to stderr\n  -d, --daemon           Route through daemon (default: on)\n      --no-daemon         Bypass daemon and run directly\n      --log-events       Print normalized runtime log/tool events to stderr\n      --timing           Print route and ACP phase timings to stderr as JSON\n      --timeout-ms <ms>  ACP response timeout (default: 60000)\n  -h, --help             Show this help\n\nExamples:\n  iota run codex \"What is 2+2?\"\n  iota run --no-daemon --timing codex \"What is 2+2?\"\n  iota run --backend gemini --cwd D:\\\\coding\\\\creative \"Summarize this repo\""
    );
}

pub(crate) fn is_backend_alias(value: &str) -> bool {
    AcpBackend::parse(value).is_ok()
}

fn read_prompt_from_stdin() -> Result<String> {
    if io::stdin().is_terminal() {
        bail!("Missing ACP prompt. Pass a prompt argument or pipe text into stdin.");
    }

    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .context("Failed to read prompt from stdin")?;
    Ok(input)
}

#[cfg(test)]
#[path = "parser_tests.rs"]
mod parser_tests;
