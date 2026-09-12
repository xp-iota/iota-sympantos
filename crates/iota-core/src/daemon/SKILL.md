---
name: iota-src-daemon
description: Use when working on the internal TCP daemon, EnginePool reuse by cwd, daemon prompt protocol, warm ACP clients, or files under crates/iota-core/src/daemon.
triggers:
  - crates/iota-core/src/daemon
  - EnginePool
  - DaemonPromptRequest
  - DaemonPromptResponse
  - __daemon
  - 127.0.0.1:47661
---

# daemon — Background Daemon

TCP server on `127.0.0.1:47661` that keeps `IotaEngine` instances alive across CLI invocations, eliminating cold-start overhead.

## Responsibilities

- Accept JSON prompt requests over TCP
- Maintain an `EnginePool` keyed by working directory
- Reuse warm ACP backend connections
- Auto-start on first `--daemon` CLI call
- Provide two local JSON-line APIs: legacy CLI request/response and desktop streaming turns

## Security / trust boundary

The TCP listener is loopback-only and authenticates all sensitive requests with
an owner-only, CSPRNG-generated token from `~/.i6/daemon.token`. Legacy
prompt/warm requests carry the token; desktop clients present it in the `Hello`
handshake, which authenticates the connection before any sensitive message is
processed. Tokens are compared in constant time, and authentication failures
plus sensitive operations are emitted through structured audit logging without
including token values. TCP JSON lines are not transport encryption: keep the
endpoint on loopback, and use an additional controlled encrypted transport if
an operator deliberately exposes it beyond the host.

## Sub-modules

| Module | Purpose |
| :--------| :---------|
| `pool` | `EnginePool` — per-cwd engine instance management |
| `proto` | `DaemonPromptRequest` / `DaemonPromptResponse` and desktop wire types |
| `desktop` | `handle_desktop_connection` — streams text chunks, events, and routes approvals |

## Key Types

- `EnginePool` — maps cwd → `IotaEngine` with warm ACP clients
- `DaemonPromptRequest` — inbound prompt (backend, cwd, prompt, timeout)
- `DaemonPromptResponse` — result (ok, output, error, timing)
- `DaemonClientMessage` — desktop client command (start turn, cancel turn, getConfig, saveBackendModel, respondApproval)
- `DaemonServerMessage` — desktop streaming server event (helloAccepted, textChunk, turnEvent, approvalRequested)
