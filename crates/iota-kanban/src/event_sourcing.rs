//! Event sourcing layer for the kanban store.
//!
//! Every mutation is recorded as a structured event in the append-only events table.
//! Events can be replayed to rebuild state, enabling multi-node sync.

#![allow(dead_code)]

use serde::{Deserialize, Serialize};

use super::types::*;

// ---------------------------------------------------------------------------
// Event type constants
// ---------------------------------------------------------------------------

pub const EVT_BOARD_CREATED: &str = "board_created";
pub const EVT_TASK_CREATED: &str = "task_created";
pub const EVT_TASK_UPDATED: &str = "task_updated";
pub const EVT_TASK_DELETED: &str = "task_deleted";
pub const EVT_TASK_TRANSITIONED: &str = "task_transitioned";
pub const EVT_LINK_CREATED: &str = "link_created";
pub const EVT_LINK_REMOVED: &str = "link_removed";
pub const EVT_COMMENT_ADDED: &str = "comment_added";
pub const EVT_RUN_STARTED: &str = "run_started";
pub const EVT_RUN_COMPLETED: &str = "run_completed";

// ---------------------------------------------------------------------------
// Payload structs (serialized as JSON into events.payload)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardCreatedPayload {
    pub board_id: BoardId,
    pub slug: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskCreatedPayload {
    pub task_id: TaskId,
    pub board_id: BoardId,
    pub title: String,
    pub body: Option<String>,
    pub status: String,
    pub assignee: Option<String>,
    pub priority: i32,
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskUpdatedPayload {
    pub task_id: TaskId,
    pub patch: TaskPatchPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskPatchPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignee: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_kind: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_path: Option<Option<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDeletedPayload {
    pub task_id: TaskId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskTransitionedPayload {
    pub task_id: TaskId,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkCreatedPayload {
    pub from_id: TaskId,
    pub to_id: TaskId,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkRemovedPayload {
    pub from_id: TaskId,
    pub to_id: TaskId,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommentAddedPayload {
    pub comment_id: CommentId,
    pub task_id: TaskId,
    pub author: String,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunStartedPayload {
    pub run_id: RunId,
    pub task_id: TaskId,
    pub profile: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunCompletedPayload {
    pub run_id: RunId,
    pub task_id: TaskId,
    pub status: String,
    pub exit_code: Option<i32>,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "event_sourcing_tests.rs"]
mod tests;
