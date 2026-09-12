use iota_core::acp::AcpBackend;

const BACKEND_CLAUDE: u8 = 1 << 0;
const BACKEND_CODEX: u8 = 1 << 1;
const BACKEND_GEMINI: u8 = 1 << 2;
const BACKEND_HERMES: u8 = 1 << 3;
const BACKEND_OPENCODE: u8 = 1 << 4;
const BACKEND_ALL: u8 =
    BACKEND_CLAUDE | BACKEND_CODEX | BACKEND_GEMINI | BACKEND_HERMES | BACKEND_OPENCODE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SlashAction {
    Help,
    Clear,
    ListBackends,
    SwitchBackend(AcpBackend),
    Model,
    Status,
    Export,
    Quit,
    SubmitToBackend,
    Kanban,
    Memory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ParsedSlashCommand<'a> {
    pub action: SlashAction,
    pub name: &'static str,
    pub args: &'a str,
}

#[derive(Debug, Clone, Copy)]
struct SlashCommandSpec {
    name: &'static str,
    aliases: &'static [&'static str],
    backends: u8,
    action: SlashAction,
}

const COMMAND_SPECS: &[SlashCommandSpec] = &[
    local("?", &[], SlashAction::Help),
    local("backend", &["backends"], SlashAction::ListBackends),
    local("clear", &["new", "reset"], SlashAction::Clear),
    local("model", &["models"], SlashAction::Model),
    local(
        "status",
        &["stats", "usage", "about", "profile"],
        SlashAction::Status,
    ),
    local("export", &["save"], SlashAction::Export),
    local("kanban", &["kb", "k"], SlashAction::Kanban),
    local("quit", &["exit"], SlashAction::Quit),
    spec("q", &[], BACKEND_OPENCODE, SlashAction::Quit),
    // Gemini ACP confirmed: /init (acpCommandHandler.ts InitCommand)
    // Claude Code ACP: NOT confirmed (source is reverse-engineered stubs)
    // Codex ACP: NOT confirmed (no slash handling found in app-server source)
    submit("init", &[], BACKEND_GEMINI),
    // Hermes ACP confirmed: _cmd_help; Gemini ACP confirmed: HelpCommand
    // Claude Code, Codex, OpenCode: /help is a TUI-level command only — cannot
    // be triggered via ACP text prompt → show iota local help for those backends
    submit("help", &[], BACKEND_HERMES | BACKEND_GEMINI),
    spec(
        "help",
        &[],
        BACKEND_CLAUDE | BACKEND_CODEX | BACKEND_OPENCODE,
        SlashAction::Help,
    ),
    // Hermes ACP confirmed: /compact (_cmd_compact)
    // OpenCode ACP confirmed: /compact (explicit case in prompt())
    // Claude Code ACP: empirically fails — LLM receives text and explains the
    //   command instead of executing it (tested; CC source is stubs, ACP server
    //   does NOT intercept slash commands from session/prompt text)
    // Codex ACP: NOT confirmed (no slash handling found in app-server source)
    // Gemini ACP: does NOT handle /compact (no CompressCommand in acpCommandHandler)
    submit("compact", &["compress"], BACKEND_HERMES | BACKEND_OPENCODE),
    local("memory", &["mem"], SlashAction::Memory),
    // Hermes ACP confirmed: /queue (_cmd_queue)
    submit("queue", &["q"], BACKEND_HERMES),
    // Hermes ACP confirmed: /steer (_cmd_steer)
    submit("steer", &[], BACKEND_HERMES),
    // Hermes ACP confirmed: /tools (_cmd_tools)
    submit("tools", &[], BACKEND_HERMES),
    // Gemini ACP confirmed: /restore (acpCommandHandler.ts RestoreCommand)
    submit("rollback", &["restore"], BACKEND_GEMINI),
    // Gemini ACP confirmed: /extensions (acpCommandHandler.ts ExtensionsCommand)
    submit("extensions", &[], BACKEND_GEMINI),
];

const fn local(
    name: &'static str,
    aliases: &'static [&'static str],
    action: SlashAction,
) -> SlashCommandSpec {
    spec(name, aliases, BACKEND_ALL, action)
}

const fn submit(
    name: &'static str,
    aliases: &'static [&'static str],
    backends: u8,
) -> SlashCommandSpec {
    spec(name, aliases, backends, SlashAction::SubmitToBackend)
}

const fn spec(
    name: &'static str,
    aliases: &'static [&'static str],
    backends: u8,
    action: SlashAction,
) -> SlashCommandSpec {
    SlashCommandSpec {
        name,
        aliases,
        backends,
        action,
    }
}

pub(super) fn parse_slash_command(
    input: &str,
    active_backend: AcpBackend,
) -> Option<ParsedSlashCommand<'_>> {
    let text = input.trim_end();
    if !text.starts_with('/') || input.starts_with(char::is_whitespace) || text.contains('\n') {
        return None;
    }

    let command_line = text.strip_prefix('/')?;
    let (command, args) = command_line
        .split_once(char::is_whitespace)
        .map(|(command, rest)| (command, rest.trim()))
        .unwrap_or((command_line, ""));
    if command.is_empty() {
        return None;
    }

    if args.is_empty()
        && let Ok(backend) = AcpBackend::parse(command)
    {
        return Some(ParsedSlashCommand {
            action: SlashAction::SwitchBackend(backend),
            name: command_name(backend),
            args,
        });
    }

    if command.eq_ignore_ascii_case("backend")
        && !args.is_empty()
        && let Ok(backend) = AcpBackend::parse(args)
    {
        return Some(ParsedSlashCommand {
            action: SlashAction::SwitchBackend(backend),
            name: "backend",
            args,
        });
    }

    let backend_mask = backend_mask(active_backend);
    COMMAND_SPECS
        .iter()
        .find(|spec| spec.matches(command, backend_mask))
        .map(|spec| ParsedSlashCommand {
            action: spec.action,
            name: spec.name,
            args,
        })
}

fn backend_mask(backend: AcpBackend) -> u8 {
    match backend {
        AcpBackend::ClaudeCode => BACKEND_CLAUDE,
        AcpBackend::Codex => BACKEND_CODEX,
        AcpBackend::Gemini => BACKEND_GEMINI,
        AcpBackend::Hermes => BACKEND_HERMES,
        AcpBackend::OpenCode => BACKEND_OPENCODE,
    }
}

fn command_name(backend: AcpBackend) -> &'static str {
    match backend {
        AcpBackend::ClaudeCode => "claude",
        AcpBackend::Codex => "codex",
        AcpBackend::Gemini => "gemini",
        AcpBackend::Hermes => "hermes",
        AcpBackend::OpenCode => "opencode",
    }
}

impl SlashCommandSpec {
    fn matches(&self, command: &str, backend_mask: u8) -> bool {
        self.backends & backend_mask != 0
            && (self.name.eq_ignore_ascii_case(command)
                || self
                    .aliases
                    .iter()
                    .any(|alias| alias.eq_ignore_ascii_case(command)))
    }
}

/// Returns names (and aliases) of slash commands available for `active_backend`
/// whose name starts with `prefix` (case-insensitive). An empty prefix returns
/// all commands for the backend. Results preserve COMMAND_SPECS order and are
/// deduplicated — a name only appears once even if it appears as both a primary
/// name and an alias.
pub(super) fn slash_completions(prefix: &str, active_backend: AcpBackend) -> Vec<&'static str> {
    let mask = backend_mask(active_backend);
    let lc = prefix.to_ascii_lowercase();
    let mut seen: std::collections::HashSet<&'static str> = std::collections::HashSet::new();
    let mut out: Vec<&'static str> = Vec::new();
    for spec in COMMAND_SPECS {
        if spec.backends & mask == 0 {
            continue;
        }
        if spec.name.to_ascii_lowercase().starts_with(lc.as_str()) && seen.insert(spec.name) {
            out.push(spec.name);
        }
        for &alias in spec.aliases {
            if alias.to_ascii_lowercase().starts_with(lc.as_str()) && seen.insert(alias) {
                out.push(alias);
            }
        }
    }
    out
}
