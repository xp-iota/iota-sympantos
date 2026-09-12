//! Memory subsystem — persistent memory store with FTS5 and embedding search.
//!
//! - [`store`] — [`MemoryStore`]: episodic/semantic/procedural memory, 6-bucket recall
//! - [`embedding`] — [`EmbeddingEngine`]: TF-IDF embedding engine for semantic search

pub mod embedding;
pub mod store;

// Re-export primary types for convenience.
pub use store::{
    MemoryFacet, MemoryInsert, MemoryMergeMode, MemoryRecord, MemoryScope, MemorySearchMode,
    MemoryStore, MemoryType, RecallBuckets,
};
