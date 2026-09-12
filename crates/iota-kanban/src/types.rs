use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

// ---------------------------------------------------------------------------
// Type aliases
// ---------------------------------------------------------------------------

pub type TaskId = u64;
pub type BoardId = u64;
pub type RunId = String;
pub type CommentId = u64;
#[allow(dead_code)]
pub type EventId = u64;

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Triage,
    Todo,
    Ready,
    Running,
    Blocked,
    Done,
    Archived,
}

impl Status {
    pub fn as_str(&self) -> &'static str {
        match self {
            Status::Triage => "triage",
            Status::Todo => "todo",
            Status::Ready => "ready",
            Status::Running => "running",
            Status::Blocked => "blocked",
            Status::Done => "done",
            Status::Archived => "archived",
        }
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for Status {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "triage" => Ok(Status::Triage),
            "todo" => Ok(Status::Todo),
            "ready" => Ok(Status::Ready),
            "running" => Ok(Status::Running),
            "blocked" => Ok(Status::Blocked),
            "done" => Ok(Status::Done),
            "archived" => Ok(Status::Archived),
            _ => anyhow::bail!("unknown status: {}", s),
        }
    }
}

// ---------------------------------------------------------------------------
// RunStatus
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Running,
    Completed,
    Failed,
    TimedOut,
    Cancelled,
}

impl RunStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            RunStatus::Running => "running",
            RunStatus::Completed => "completed",
            RunStatus::Failed => "failed",
            RunStatus::TimedOut => "timed_out",
            RunStatus::Cancelled => "cancelled",
        }
    }
}

impl fmt::Display for RunStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for RunStatus {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "running" => Ok(RunStatus::Running),
            "completed" => Ok(RunStatus::Completed),
            "failed" => Ok(RunStatus::Failed),
            "timed_out" => Ok(RunStatus::TimedOut),
            "cancelled" => Ok(RunStatus::Cancelled),
            _ => anyhow::bail!("unknown run status: {}", s),
        }
    }
}

// ---------------------------------------------------------------------------
// LinkKind
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkKind {
    Parent,
    Blocks,
    Related,
}

impl LinkKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            LinkKind::Parent => "parent",
            LinkKind::Blocks => "blocks",
            LinkKind::Related => "related",
        }
    }
}

impl fmt::Display for LinkKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for LinkKind {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "parent" => Ok(LinkKind::Parent),
            "blocks" => Ok(LinkKind::Blocks),
            "related" => Ok(LinkKind::Related),
            _ => anyhow::bail!("unknown link kind: {}", s),
        }
    }
}

// ---------------------------------------------------------------------------
// Structs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Board {
    pub id: BoardId,
    pub slug: String,
    pub name: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub board_id: BoardId,
    pub title: String,
    pub body: Option<String>,
    pub status: Status,
    pub assignee: Option<String>,
    pub priority: i32,
    pub tags: Vec<String>,
    pub workspace_kind: Option<String>,
    pub workspace_path: Option<PathBuf>,
    pub created_at: i64,
    pub updated_at: i64,
    pub claimed_at: Option<i64>,
    pub claim_ttl_secs: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CreateTaskRequest {
    pub board_id: BoardId,
    pub title: String,
    pub body: Option<String>,
    pub status: Option<Status>,
    pub assignee: Option<String>,
    pub priority: Option<i32>,
    pub tags: Vec<String>,
    pub workspace_kind: Option<String>,
    pub workspace_path: Option<PathBuf>,
}

/// Partial update for a Task. `None` means "don't change this field".
/// For nullable fields, `Some(None)` means "set to null/clear".
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct TaskPatch {
    pub title: Option<String>,
    pub body: Option<Option<String>>,
    pub status: Option<Status>,
    pub assignee: Option<Option<String>>,
    pub priority: Option<i32>,
    pub tags: Option<Vec<String>>,
    pub workspace_kind: Option<Option<String>>,
    pub workspace_path: Option<Option<PathBuf>>,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct TaskFilter {
    pub board_id: Option<BoardId>,
    pub status: Option<Status>,
    pub assignee: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Link {
    pub from_id: TaskId,
    pub to_id: TaskId,
    pub kind: LinkKind,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Comment {
    pub id: CommentId,
    pub task_id: TaskId,
    pub author: String,
    pub body: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Run {
    pub id: RunId,
    pub task_id: TaskId,
    pub profile: String,
    pub status: RunStatus,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub last_heartbeat: i64,
    pub exit_code: Option<i32>,
    pub output_summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
pub struct KanbanEvent {
    /// Local log position in *this* store's `events` table.
    ///
    /// This is a per-store insertion counter, not a global one: two nodes will
    /// independently assign the same `id` to different events. It orders this
    /// node's own log and nothing more — never use it as an event's identity
    /// or as a cross-node cursor.
    pub id: EventId,
    /// Immutable identity of the event, stable across every node that
    /// replicates it. This is what deduplication keys on.
    ///
    /// Defaults to a fresh UUID when absent so events read back from stores
    /// written before this field existed still deserialize.
    #[serde(default = "default_event_uuid")]
    pub event_uuid: uuid::Uuid,
    pub event_type: String,
    pub payload: String,
    pub created_at: i64,
}

fn default_event_uuid() -> uuid::Uuid {
    uuid::Uuid::nil()
}

// ---------------------------------------------------------------------------
// UI Events (broadcast channel for real-time TUI updates)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum KanbanUiEvent {
    TaskCreated {
        id: TaskId,
        title: String,
    },
    TaskStatusChanged {
        id: TaskId,
        from: Status,
        to: Status,
    },
    TaskUpdated {
        id: TaskId,
    },
    TaskDeleted {
        id: TaskId,
    },
    CommentAdded {
        task_id: TaskId,
        comment_id: CommentId,
    },
    RunStarted {
        task_id: TaskId,
        run_id: RunId,
    },
    RunCompleted {
        task_id: TaskId,
        run_id: RunId,
        status: RunStatus,
    },
}
