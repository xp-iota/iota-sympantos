# iota-desktop MVP Acceptance Runbook

This runbook verifies the daemon-first desktop baseline. Do not paste API keys, tokens, raw `nimia.yaml`, or secret-bearing protocol payloads into logs or screenshots.

## Prerequisites

- `~/.i6/nimia.yaml` exists and is the only config source.
- At least one backend is enabled and configured with a valid model and API key.
- `iota` is available in `PATH`, or `IOTA_CLI_PATH` points to the CLI binary for desktop daemon autostart.
- Desktop dependencies are installed with `npm install` in `crates/iota-desktop`.
- Existing daemon processes are acceptable. The desktop app should connect to the configured daemon or autostart a fallback daemon.

## Automated Gates

Run from the repository root unless noted:

```bash
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p iota-cli -- check
cd crates/iota-desktop && npm test && npm run build
```

Expected:

- All commands exit successfully.
- `iota check` does not print API keys or tokens.
- Desktop tests cover turn reducer behavior, layout structure, and memory/context workspace mounting. Config and observability are still mainly covered through build/type checks and manual scenarios.

## Manual Scenarios

### 1. Launch And Daemon Connection

```bash
cd crates/iota-desktop
npm run dev:clean
```

Expected:

- The app opens to the chat workbench.
- The daemon status becomes connected after config loads.
- The desktop backend first tries the normal daemon address, then the desktop fallback address.
- If no daemon is running, it autostarts `iota __daemon` via `IOTA_CLI_PATH`, sibling binary, or `PATH`.
- No secret values are shown in the UI or terminal logs.

### 2. Backend Readiness

Steps:

1. Open the backend selector.
2. Select a configured backend.
3. Select an intentionally unavailable backend if one exists.

Expected:

- Ready backend allows prompt submission.
- Unavailable backend shows a clear reason such as missing API key, missing adapter command, disabled backend, or invalid config.
- Send stays disabled for unavailable backends.

### 3. Config Panel

Steps:

1. Open the Config view.
2. Review provider, model name, base URL, and API key state.
3. Save a harmless model field change or re-save an existing value.

Expected:

- Config is loaded through daemon `GetConfig`.
- API keys are masked as configured/missing and are never displayed literally.
- Save uses daemon `SaveBackendModel` and preserves `~/.i6/nimia.yaml` semantics.
- Backend checks refresh after save.
- Hermes behavior remains unchanged; desktop does not set or override `HERMES_HOME`.

### 4. Successful Prompt Streaming

Steps:

1. Select a ready backend.
2. Submit `Say hello in one short sentence.`

Expected:

- A turn appears immediately.
- Assistant text streams through `TextChunk`, or appears on `TurnCompleted` if the backend only sends final text.
- Runtime events appear in the right inspector.
- Prompt submission is disabled while the active turn is running and re-enabled after completion.

### 5. Inspector Details

Steps:

1. Select a completed turn.
2. Review the right inspector.

Expected:

- Timing summary appears when available.
- Token usage appears when the backend reports usage.
- Tool calls and tool results appear when emitted.
- Runtime events remain visible and scrollable.
- Large JSON payloads do not break layout.

### 6. Approval Approve And Deny

Steps:

1. Use a prompt/backend/tool combination known to request permission.
2. Approve one request.
3. Run another permission-triggering prompt and deny it.

Expected:

- Approval request shows tool name and params.
- Approve sends daemon `RespondApproval { approved: true }`.
- Deny sends daemon `RespondApproval { approved: false }`.
- Lost or closed approval streams fail closed and do not auto-approve.
- Turn state leaves waiting approval after terminal completion, failure, or cancellation.

### 7. Cancellation

Steps:

1. Start a longer prompt.
2. Click Interrupt Execution.

Expected:

- Desktop sends daemon `CancelTurn`.
- Turn is marked cancelled in transcript and inspector.
- Partial text and events remain visible.
- Prompt submission unlocks after cancellation.

### 8. Daemon Disconnect

Steps:

1. Start a running prompt.
2. Stop the daemon process or interrupt the stream.

Expected:

- Frontend receives `daemon-client-error`.
- Active turn is marked failed while preserving partial text/events.
- Daemon status shows error.
- App remains usable after restart/reconnect where possible.

### 9. Memory And Context Workspace

Steps:

1. Open the right inspector.
2. Switch to the Memory tab, then the Context tab.
3. Toggle workspace/all scope mode.
4. Inspect memory buckets and context preview.

Expected:

- Desktop sends daemon `GetMemoryContextSnapshot`.
- Six memory buckets are shown: identity, preference, strategic, domain, procedural, episodic.
- Runtime context preview shows section names, character counts, budgets, and capsule text when available.
- Snapshot errors are visible without crashing the app.

### 10. Kanban Workspace

Steps:

1. Open the right inspector and select the **Kanban** tab.
2. Create or select a board, then create a task.
3. Transition the task through a legal status, add a comment, and refresh the workspace.

Expected:

- `RightInspector` mounts `KanbanWorkspace`, which reads and refreshes boards, task details, links, runs, and comments through Tauri commands.
- Tauri commands use `SqliteKanbanStore` under `~/.i6/kanban/iota.db`.
- State transitions obey the Kanban state machine and the refreshed UI shows the resulting state.

### 11. CLI Compatibility

Run after desktop use:

```bash
cargo run -p iota-cli -- check --daemon
cargo run -p iota-cli -- run --daemon hermes "ping"
```

Expected:

- Legacy daemon request/response still works.
- Desktop protocol version 2 did not break CLI prompt and warm paths.
- Output does not expose secrets.

## Acceptance Result

Record the date, OS, backend used, and result when executing the runbook.

| Date | OS | Backend | Result | Notes |
| :--- | :--- | :--- | :--- | :--- |
| 2026-05-24 | macOS | Gemini | partial pass | Launch and daemon connection verified. Existing old daemon on `127.0.0.1:47661` caused EOF; desktop fallback autostart on `127.0.0.1:47662` addressed this. Full prompt, approval, cancellation, config-save, memory/context, and Kanban walkthroughs still need interactive execution. |

## Non-Blocking Follow-Ups

| Item | Scope |
| :--- | :--- |
| Full prompt/approval/cancellation walkthrough | Desktop MVP acceptance |
| Cross-platform manual run on Windows/macOS/Linux | Desktop MVP acceptance |
| Memory/context snapshot UX review with non-empty stores | Desktop memory/context |
