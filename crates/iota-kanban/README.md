# iota-sympantos-kanban

`iota-sympantos-kanban` is the event-sourced Kanban task orchestration library used by iota-sympantos. It provides task and board domain types, SQLite persistence, lifecycle validation, dispatcher and worker coordination, shadow workspaces, advanced task decomposition, and cross-node event synchronization.

Published package: [crates.io](https://crates.io/crates/iota-sympantos-kanban) · [docs.rs](https://docs.rs/iota-sympantos-kanban/0.1.0)

## Add the dependency

```toml
[dependencies]
iota-kanban = { package = "iota-sympantos-kanban", version = "0.1.0" }
```

## Core API

```rust
use iota_kanban::{KanbanStore, SqliteKanbanStore};

let store = SqliteKanbanStore::open("kanban.sqlite")?;
```

The public API also exposes task, board, run, comment, and link types, along with dispatcher and event-sync helpers.

## Compatibility

The crate supports Windows, macOS, and Linux. Version 0.1.0 requires Rust 1.95.0 or newer and Rust edition 2024.

`iota-sympantos-core` exposes Kanban-backed MCP tools behind its optional `kanban` feature:

```toml
[dependencies]
iota-core = { package = "iota-sympantos-core", version = "0.1.0", features = ["kanban"] }
```

## License

MIT
