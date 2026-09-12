---
name: iota-src-utils
description: Use when working on shared utility helpers, elapsed timing, timestamp helpers, summary truncation, mutex recovery, or files under crates/iota-core/src/utils.
triggers:
  - crates/iota-core/src/utils
  - elapsed_ms
  - now_ts
  - summarize
  - lock_or_recover
---

# utils — Shared Utilities

Common helper functions used across multiple modules.

## Functions

| Function | Purpose |
| :----------| :---------|
| `elapsed_ms(Instant)` | Wall-clock milliseconds since a start instant |
| `now_ts()` | Current Unix timestamp in seconds |
| `summarize(str, limit)` | Collapse whitespace and truncate with "..." |
| `lock_or_recover(Mutex)` | Lock mutex, recovering gracefully from poisoned state |
