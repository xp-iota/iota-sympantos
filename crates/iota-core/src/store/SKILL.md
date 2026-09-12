---
name: iota-src-store
description: Use when working on SQLite-backed cache, approval policy persistence, session ledger storage, token observability, execution idempotency, or files under crates/iota-core/src/store.
triggers:
  - crates/iota-core/src/store
  - CacheStore
  - ApprovalStore
  - SessionLedger
  - ObservabilityStore
  - StoreConfig
  - approval policy
  - execution cache
  - token percentiles
  - token usage
---

# store — SQLite Store Layer

SQLite-backed persistence for execution cache, tool approvals, session ledger, and token observability.

## Responsibilities

- Execution lifecycle caching and deduplication (`CacheStore`)
- Tool approval event recording and policy lookup (`ApprovalStore`)
- Session/turn/handoff tracking (`SessionLedger`)
- Token usage persistence, deduplication, and analytics (`ObservabilityStore`)

## Sub-modules

| Module | Purpose |
| :--------| :---------|
| `approvals` | `ApprovalStore` — tool approval events and policy |
| `cache` | `CacheStore` — execution replay and deduplication |
| `ledger` | `SessionLedger` — sessions, backend sessions, turns, handoffs |
| `observability` | `ObservabilityStore` — token usage recording, execution dedup, analytics |

## Key Types

- `CacheStore` — execution caching with idempotency and fencing; config cached at `open()` time
- `ApprovalStore` — tool approval persistence and policy lookup
- `SessionLedger` — session/turn/handoff tracking
- `ObservabilityStore` — token usage events with execution-level best-record deduplication
- `StoreConfig` — configurable retention via `~/.i6/nimia.yaml` (`store:` section)

## Database Paths

| Store(s) | SQLite File |
| :--------| :-----------|
| `CacheStore` + `ObservabilityStore` | `~/.i6/context/events.sqlite` |
| `MemoryStore` | `~/.i6/context/memory.sqlite` |
| `SessionLedger` + `ApprovalStore` | `~/.i6/context/store.sqlite` (merged) |

`SessionLedger` and `ApprovalStore` share `store.sqlite` via `StorePaths::store_db()`. The former `sessions.sqlite` and `approvals.sqlite` files are no longer used; users upgrading must delete them manually.

## Configuration (StoreConfig)

Data retention values are read from `~/.i6/nimia.yaml` once per store `open()`:

```yaml
store:
  cache_retention_days: 30           # completed/failed execution TTL
  cache_running_ttl_secs: 3600       # stale running execution timeout
  observability_retention_days: 90   # token usage event retention
  approvals_max_pending_age_secs: 604800  # pending approval expiry (7 days)
```

All values default to these numbers if the `store:` section is absent.

## Query APIs

### CacheStore
- `begin_execution_with_id()` — atomic insert with fencing token and stale-running cleanup
- `finish_execution()` — status update
- `get_execution_statuses(&[&str])` — batch status query via single `WHERE IN (...)` SQL

### ApprovalStore
- `record_request()` / `record_decision()` — approval audit trail
- `get_pending_requests()` — all requests without a decision
- `get_decision_history(execution_id, limit)` — decision trail, optionally filtered by execution

### SessionLedger
- `ensure_session()` / `turn_increment()` / `publish_handoff()` / `read_handoff()`
- `session_stats(session_id)` → `(turn_count, actual_turns, distinct_backend_count)`
- `get_handoff_history(session_id)` → ordered `(from, to, summary)` list

### ObservabilityStore
- `record_token_usage()` — persists event; validates `computed ≤ provider_total` and logs warn on mismatch
- `recent_token_usage(limit)` / `token_usage_for_execution()` / `token_usage_between(from, to)`
- `token_summary_since(ts)` — backend-grouped means with execution-level deduplication
- `token_percentiles(backend)` → P50/P95/P99 of `normalized_total_tokens` via direct SQL sort

## Concurrency Pattern

All stores use `Arc<Mutex<Connection>>`. `CacheStore` provides a private `lock_conn()` method; other stores call `crate::utils::lock_or_recover()` directly. All connections are opened with `PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;`.

## Token Deduplication Logic

`token_event_score()` selects the canonical record per execution:

| Priority | Condition |
| :----------| :-----------|
| +5 | `provider_reported_total_tokens` is present |
| +4 | `normalized_total_tokens` is present |
| +2 | `source` ≠ `session_update.usage_update` |
| +1 | `input_tokens` present |
| +1 | `output_tokens` present |

Higher score wins; ties resolved by insertion order.
