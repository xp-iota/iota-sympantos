---
name: iota-desktop
description: Use when working on the Tauri desktop workbench, React components, Tauri command bindings, daemon IPC messages, or files under crates/iota-desktop.
triggers:
  - crates/iota-desktop
  - desktop
  - tauri
  - react
---

# iota-desktop — Tauri Desktop GUI Workbench

Desktop graphical user interface for iota-sympantos combining a React frontend and a Tauri backend. It connects to the local TCP daemon for model execution, configuration, approvals, observability, and memory/context snapshots. The Rust side also exposes Kanban store commands, while the current React UI focuses on chat, config, observability, memory, and runtime context inspection.

## Responsibilities

- **Chat & Control Interface**: A React frontend chat client supporting backend selection, streaming responses, active-turn selection, cancellation, and approval actions.
- **Daemon Client**: A Tauri backend component connecting to the `iota` daemon TCP server to send prompts, check backend readiness, save model settings, and manage active turns.
- **Inspector Workspace**: A resizable right-side panel for turn timing, token usage, tool calls, runtime events, observability summaries, persistent memory buckets, and runtime context capsules.
- **Config Editing**: Reads and writes backend model fields through daemon APIs while masking API keys and preserving `~/.i6/nimia.yaml` as the only config source.
- **Kanban Command Surface**: Connects to the event-sourced `SqliteKanbanStore` directly inside Rust commands for board/task/comment operations; no React Kanban board is currently mounted.

## Structure

```
crates/iota-desktop/
├── src/                     # React Frontend
│   ├── components/          # React layout and widget components
│   ├── App.tsx              # Root application view mounting ChatWorkbench
│   ├── api.ts               # JavaScript bindings calling Tauri commands
│   ├── types.ts             # Shared frontend domain types
│   └── turnReducer.ts       # State machine for turns and stream chunks
└── src-tauri/               # Tauri Rust Backend
    ├── src/
    │   ├── lib.rs           # Tauri command definitions and state setup
    │   ├── daemon_client.rs # TCP stream client to connect to local daemon
    │   └── main.rs          # Application entry point
    └── tauri.conf.json      # Tauri package metadata and devUrl configs
```

## Key Tauri Commands

- `get_config` / `save_backend_model` — Read/write masked backend model settings via daemon.
- `submit_prompt` / `cancel_turn` — Execute prompts or cancel turns asynchronously over the desktop stream protocol.
- `handle_approval` — Respond (allow/deny) to pending tool calls.
- `get_observability_summary` — Retrieve aggregated token usage statistics.
- `get_memory_context_snapshot` — Retrieve six-bucket memory and the latest runtime context capsule.
- `current_workspace` — Return the current Tauri process working directory.
- `list_boards` / `list_tasks` / `create_task` / `transition_task` — direct Kanban SQLite operations.

## Frontend State Notes

- `ChatWorkbench` owns shell state: selected backend, config snapshot, backend checks, observability summary, workspace path, daemon status, active view, backend menu, and inspector width.
- `turnReducer` owns turn state only. It appends streamed text, folds `RuntimeEvent` values into usage/tool/approval views, preserves partial output on failures, and ignores late daemon errors for terminal turns.
- `RightInspector` hosts three tabs: `Observability`, `Memory`, and `Context`. `MemoryContextWorkspace` is intentionally mounted inside the inspector rather than the central workbench.
