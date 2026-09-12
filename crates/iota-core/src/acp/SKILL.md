---
name: iota-src-acp
description: Use when working on ACP backend processes, JSON-RPC wire handling, session lifecycle, permissions, backend aliases, streaming responses, or files under crates/iota-core/src/acp.
triggers:
  - crates/iota-core/src/acp
  - AcpBackend
  - AcpClient
  - ACP
  - session/new
  - session/prompt
  - mcpServers
---

# acp — ACP Protocol Layer

JSON-RPC 2.0 protocol driver for communicating with AI coding assistant backends over stdin/stdout.

## Responsibilities

- Spawn backend processes (`npx`, `hermes acp`) with `kill_on_drop(true)`
- Drive the ACP lifecycle: `initialize → session/new → session/prompt → session/update → session/complete`
- Parse streaming JSON-RPC responses with line-delimited wire protocol
- Handle permission requests (auto-approve iota tools, delegate others)
- Manage backend enumeration and command resolution

## Sub-modules

| Module | Purpose |
| :--------| :---------|
| `backend` | `AcpBackend` enum, `ALL_BACKENDS`, command/alias resolution |
| `client` | `AcpClient` — process spawn, session creation, prompt execution |
| `message` | Response parsing: text extraction, tool calls, permissions |
| `parser` | CLI argument parsing (`AcpRunOptions`) |
| `permission` | Approval routing — auto-approve iota tools, queue others |
| `session` | `session/new` parameter rendering, `mcpServers` shape |
| `stream_reader` | Streaming update collection and conversion to runtime events |
| `types` | Shared types: `AcpPromptOutput`, `AcpPromptTiming`, `AcpStartupTiming` |
| `util` | Helpers: `elapsed_ms`, `should_forward_backend_stderr` |
| `wire` | Line reading, JSON parsing, response ID matching |

## Key Types

- `AcpBackend` — enum: Claude, Codex, Gemini, Hermes, OpenCode
- `AcpClient` — one live backend process with stdin/stdout pipes
- `AcpPromptOutput` — collected events + timing from a single prompt turn
