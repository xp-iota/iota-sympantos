---
name: iota-src-engine
description: Use when working on IotaEngine orchestration, prompt turns, ACP client reuse, context injection, memory writes, session ledger, handoffs, or files under crates/iota-core/src/engine.
triggers:
  - crates/iota-core/src/engine
  - IotaEngine
  - AcpClientKey
  - run_with_timing
  - session ledger
  - backend handoff
---

# engine — IotaEngine Orchestration

Core orchestration layer that manages ACP client pools, executes prompt turns, and coordinates memory, skills, ledger, and handoffs.

## Responsibilities

- Create and reuse ACP client sessions per (backend, cwd)
- Execute prompt turns with context injection and event collection
- Persist turn results to session ledger and episodic memory
- Handle backend handoff (switching between backends mid-session)
- Manage event loop for streaming prompt execution

## Sub-modules

| Module | Purpose |
| :--------| :---------|
| `memory_ops` | Memory extraction, classification, recall, and injection |
| `prompt` | Core prompt execution flow with early-return paths |
| `session_ledger` | Session/turn persistence and backend handoff records |
| `telemetry` | Runtime event recording into traces, logs, and metrics |

## Key Types

- `IotaEngine` — main orchestrator holding ACP clients, stores, config
- `AcpClientKey` — `(AcpBackend, PathBuf)` key for ACP client pool reuse
