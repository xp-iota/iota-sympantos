use anyhow::Result;
use std::any::Any;

use super::types::*;

#[allow(dead_code)]
pub trait KanbanStore: Send + Sync + Any {
    fn as_any(&self) -> &dyn Any;

    fn create_board(&self, slug: &str, name: &str) -> Result<BoardId>;
    fn list_boards(&self) -> Result<Vec<Board>>;
    fn get_board(&self, slug: &str) -> Result<Board>;

    fn create_task(&self, req: CreateTaskRequest) -> Result<TaskId>;
    fn get_task(&self, id: TaskId) -> Result<Task>;
    fn update_task(&self, id: TaskId, patch: TaskPatch) -> Result<()>;
    fn list_tasks(&self, filter: TaskFilter) -> Result<Vec<Task>>;
    fn delete_task(&self, id: TaskId) -> Result<()>;

    fn transition(&self, id: TaskId, to: Status) -> Result<()>;

    fn create_link(&self, from: TaskId, to: TaskId, kind: LinkKind) -> Result<()>;
    fn remove_link(&self, from: TaskId, to: TaskId, kind: LinkKind) -> Result<()>;
    fn get_links(&self, id: TaskId) -> Result<Vec<Link>>;

    fn add_comment(&self, task_id: TaskId, author: &str, body: &str) -> Result<CommentId>;
    fn list_comments(&self, task_id: TaskId) -> Result<Vec<Comment>>;

    fn create_run(&self, task_id: TaskId, profile: &str) -> Result<RunId>;
    fn complete_run(&self, run_id: &str, status: RunStatus, exit_code: Option<i32>) -> Result<()>;
    fn heartbeat(&self, run_id: &str) -> Result<()>;
    fn get_runs(&self, task_id: TaskId) -> Result<Vec<Run>>;

    fn append_event(&self, event_type: &str, payload: &str) -> Result<EventId>;
    fn events_since(&self, cursor: EventId) -> Result<Vec<KanbanEvent>>;
}
