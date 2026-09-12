# iota-desktop

Tauri desktop workbench for iota-sympantos. It is a local GUI for the same
runtime used by the CLI and TUI, with React handling presentation and Tauri
bridging into the daemon-first Rust backend.

The desktop app is daemon-first:

- React renders the chat-first workbench, config editor, right-side inspector,
  observability summaries, and read-only memory/context workspace.
- Tauri commands connect to the local iota daemon over the desktop JSON-line
  protocol instead of creating `IotaEngine` directly.
- The daemon owns `EnginePool`, `IotaEngine`, ACP processes, scoped approvals,
  config reads/writes, runtime events, observability, and memory/context
  snapshots.
- Tauri also exposes Kanban CRUD commands backed by `SqliteKanbanStore` under
  `~/.i6/kanban/iota.db`; the current React shell does not yet render a Kanban
  board.
- `~/.i6/nimia.yaml` remains the only configuration source.

## Runtime Flow

```text
React ChatWorkbench
  -> src/api.ts invoke/listen wrappers
  -> Tauri commands in src-tauri/src/lib.rs
  -> daemon_client::connect_or_start()
  -> iota __daemon desktop protocol v2
  -> EnginePool / IotaEngine / ACP backend
  -> daemon-message / daemon-client-error window events
  -> turnReducer updates transcript and inspector state
```

The desktop client first tries the normal daemon address from
`IOTA_DAEMON_ADDR` or `127.0.0.1:47661`. If that connection fails, it switches
to `IOTA_DESKTOP_DAEMON_ADDR` or `127.0.0.1:47662`; if needed, it autostarts
`iota __daemon` using `IOTA_CLI_PATH`, a sibling `iota` binary, or `PATH`.

## Main Files

| File | Role |
| :--- | :--- |
| `src/components/ChatWorkbench.tsx` | Main shell: daemon status, backend selector, chat transcript, prompt form, config view, resizable inspector |
| `src/components/RightInspector.tsx` | Turn details, approval actions, cancellation, observability, memory/context tabs |
| `src/components/MemoryContextWorkspace.tsx` | Read-only memory bucket and runtime context capsule browser |
| `src/components/ConfigPanel.tsx` | Daemon-backed backend model editor with masked API key state |
| `src/turnReducer.ts` | Frontend state machine for daemon stream messages and runtime events |
| `src/api.ts` | Tauri command and event binding layer |
| `src-tauri/src/lib.rs` | Tauri command registration, Kanban store setup, command handlers |
| `src-tauri/src/daemon_client.rs` | TCP daemon handshake, autostart, streaming reader, Tauri event emission |

## Development

```bash
cd crates/iota-desktop
npm install
npm run dev:clean
```

`dev:clean` stops existing `iota __daemon` processes before launching Tauri dev.
It also builds the current workspace `iota` CLI and exports `IOTA_CLI_PATH`, so
the Tauri daemon client autostarts the matching daemon instead of a stale binary
from `PATH`.

For frontend-only checks, use `npm run dev:frontend`. Tauri development expects
Vite on port `1420` and HMR on `1421` when `TAURI_DEV_HOST` is set.

## Verification

```bash
cargo test -p iota-core daemon
cargo test -p iota-desktop
cd crates/iota-desktop && npm test && npm run build
```

Manual MVP acceptance is tracked in `../../docs/desktop-mvp-acceptance.md`.

The generated technical guide for the whole project is [docs/iota book.md](../../docs/iota%20book.md).
