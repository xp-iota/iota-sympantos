---
name: iota-src-cli
description: Use when working on iota CLI commands, argument dispatch, run/check/bench/memory/skill commands, daemon routing, or files under crates/iota-cli/src/cli.
triggers:
  - crates/iota-cli/src/cli
  - iota run
  - iota check
  - bench-cold
  - bench-warm
  - iota mcp
  - context-mcp
  - fun-mcp
---

# cli — Command Dispatch

Top-level CLI entry point. Parses arguments and routes to subcommand handlers.

## Commands

| Command | Handler | Description |
| :---------| :---------| :-------------|
| (default) | `tui::run()` | Interactive TUI mode |
| `run <backend> <prompt>` | `run_cmd` | Single-shot prompt execution |
| `check [--daemon]` | `info_cmd` | Backend health/info JSON output |
| `bench <cold|warm> [rounds]` | `daemon_cmd` | Cold/warm latency benchmark |
| `bench-cold [rounds]` | `daemon_cmd` | Compatibility cold-start benchmark |
| `bench-warm [rounds]` | `daemon_cmd` | Compatibility warm-connection benchmark |
| `mcp <context|fun>` | — | Spawn iota-context or iota-fun MCP stdio server |
| `context-mcp` | — | Compatibility alias for `iota mcp context` |
| `fun-mcp` | — | Compatibility alias for `iota mcp fun` |
| `skill pull <src>` | `skill_cmd` | Pull remote skill definition |
| `observability ...` | `observability_cmd` | Query local token data, metrics, Loki logs, or Jaeger traces |
| `logs/trace` | `observability_cmd` | Top-level observability aliases |
| `kanban ...` | `kanban_cmd` | Kanban board/task/dispatch/sync commands |
| `__daemon` | `daemon_cmd` | Internal daemon entry point |

## Sub-modules

| Module | Purpose |
| :--------| :---------|
| `daemon_cmd` | Daemon lifecycle, cold/warm/daemon benchmarks |
| `info_cmd` | `check` command — backend info aggregation |
| `observability_cmd` | Logs and trace query commands |
| `run_cmd` | Single-shot `run` command execution |
| `skill_cmd` | Skill pull/cache management |
| `kanban_cmd` | Kanban CLI command handling |
