---
name: iota-src-memory
description: Use when working on persistent memory records, recall buckets, SQLite FTS5 search, TF-IDF embeddings, merge modes, or files under crates/iota-core/src/memory.
triggers:
  - crates/iota-core/src/memory
  - MemoryStore
  - MemoryRecord
  - RecallBuckets
  - MemoryFacet
  - EmbeddingEngine
  - FTS5
---

# memory — Memory Subsystem

Persistent memory store with SQLite FTS5 full-text search and TF-IDF embedding-based semantic search. Implements a 6-bucket classification system.

## Responsibilities

- Insert/update/delete memory records with content-hash deduplication
- 6-bucket recall: identity, preference, strategic, domain, procedural, episodic
- Full-text search via SQLite FTS5
- Semantic similarity search via TF-IDF embeddings
- Merge modes: Auto, Add, Update, None
- Optimized concurrency: pre-calculates embeddings before locking the database connection mutex to avoid blocking during external/HTTP embedding API calls

## Sub-modules

| Module | Purpose |
| :--------| :---------|
| `store` | `MemoryStore` — SQLite-backed CRUD, FTS5, recall, merge |
| `embedding` | `EmbeddingEngine` — TF-IDF tokenization, cosine similarity, blob serialization |

## Key Types

- `MemoryStore` — main store with `insert()`, `recall_buckets()`, `search()`
- `MemoryRecord` — persisted record (id, type, facet, scope, content, confidence, TTL)
- `MemoryType` — Semantic, Episodic, Procedural
- `MemoryFacet` — Identity, Preference, Strategic, Domain
- `MemoryScope` — User, Project, Session, Global
- `RecallBuckets` — 6-bucket classification result
- `EmbeddingEngine` — TF-IDF engine with tokenization and cosine similarity
