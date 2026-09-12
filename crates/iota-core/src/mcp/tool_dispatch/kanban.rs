//! `iota_kanban_create_task`, `iota_kanban_list_tasks`, and
//! `iota_kanban_ready_task` tool implementations (behind the `kanban`
//! feature).

use serde_json::{Value, json};

use iota_kanban::{Board, CreateTaskRequest, KanbanStore, Status, Task, TaskFilter, TaskId};

use super::{McpTool, ToolContext, required_string};

pub(super) struct KanbanCreateTaskTool;
impl McpTool for KanbanCreateTaskTool {
    fn name(&self) -> &'static str {
        "iota_kanban_create_task"
    }

    fn description(&self) -> &'static str {
        "Create a task in iota's Kanban DB. Generated tasks auto-promote triage/todo to ready by default so the dispatcher can claim them. Set auto_ready=false to keep a raw idea in triage/todo."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["title"],
            "properties": {
                "title": {"type": "string"},
                "body": {"type": "string"},
                "status": {"type": "string", "enum": ["triage", "todo", "ready", "running", "blocked", "done", "archived"], "description": "Default triage"},
                "assignee": {"type": "string", "description": "Hermes profile name, e.g. research-agent"},
                "priority": {"type": "integer"},
                "tags": {"type": "array", "items": {"type": "string"}},
                "auto_ready": {"type": "boolean", "description": "Default true. When true, triage/todo tasks are legally promoted to ready after creation."},
                "board_slug": {"type": "string"},
                "board_name": {"type": "string"},
                "workspace_kind": {"type": "string"},
                "workspace_path": {"type": "string"}
            }
        })
    }

    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<Value, String> {
        dispatch_kanban_create_task(ctx, args)
    }
}

pub(super) struct KanbanListTasksTool;
impl McpTool for KanbanListTasksTool {
    fn name(&self) -> &'static str {
        "iota_kanban_list_tasks"
    }

    fn description(&self) -> &'static str {
        "List tasks from iota's Kanban DB for verification after creating or dispatching tasks."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "status": {"type": "string", "enum": ["triage", "todo", "ready", "running", "blocked", "done", "archived"]},
                "assignee": {"type": "string"},
                "limit": {"type": "integer"}
            }
        })
    }

    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<Value, String> {
        dispatch_kanban_list_tasks(ctx, args)
    }
}

pub(super) struct KanbanReadyTaskTool;
impl McpTool for KanbanReadyTaskTool {
    fn name(&self) -> &'static str {
        "iota_kanban_ready_task"
    }

    fn description(&self) -> &'static str {
        "Move an existing iota Kanban task to ready so the desktop dispatcher can execute it."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["task_id"],
            "properties": {
                "task_id": {"type": "integer"}
            }
        })
    }

    fn execute(&self, ctx: &ToolContext, args: &Value) -> Result<Value, String> {
        dispatch_kanban_ready_task(ctx, args)
    }
}

fn dispatch_kanban_create_task(ctx: &ToolContext, args: &Value) -> Result<Value, String> {
    let kanban = ctx
        .kanban
        .ok_or_else(|| "kanban store is unavailable".to_string())?;
    let title = required_string(args, "title")?;
    let board = resolve_kanban_board(ctx, args)?;
    let status = args
        .get("status")
        .and_then(Value::as_str)
        .map(parse_kanban_status)
        .transpose()?
        .unwrap_or(Status::Triage);
    let auto_ready = args
        .get("auto_ready")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let tags = args
        .get("tags")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    item.as_str()
                        .map(str::to_string)
                        .ok_or_else(|| "tags must be strings".to_string())
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?
        .unwrap_or_default();
    let task_id = kanban
        .create_task(CreateTaskRequest {
            board_id: board.id,
            title: title.to_string(),
            body: args.get("body").and_then(Value::as_str).map(str::to_string),
            status: Some(status),
            assignee: args
                .get("assignee")
                .and_then(Value::as_str)
                .map(str::to_string),
            priority: args
                .get("priority")
                .and_then(Value::as_i64)
                .map(|value| value as i32),
            tags,
            workspace_kind: args
                .get("workspace_kind")
                .and_then(Value::as_str)
                .map(str::to_string),
            workspace_path: args
                .get("workspace_path")
                .and_then(Value::as_str)
                .map(std::path::PathBuf::from),
        })
        .map_err(|err| err.to_string())?;
    let final_status = if auto_ready && matches!(status, Status::Triage | Status::Todo) {
        ready_kanban_task(kanban, task_id)?;
        Status::Ready
    } else {
        status
    };
    Ok(json!({
        "ok": true,
        "task_id": task_id,
        "status": final_status.as_str(),
        "created_status": status.as_str(),
        "board": board,
        "auto_ready": auto_ready,
        "auto_dispatch": final_status == Status::Ready
    }))
}

fn dispatch_kanban_list_tasks(ctx: &ToolContext, args: &Value) -> Result<Value, String> {
    let kanban = ctx
        .kanban
        .ok_or_else(|| "kanban store is unavailable".to_string())?;
    let status = args
        .get("status")
        .and_then(Value::as_str)
        .map(parse_kanban_status)
        .transpose()?;
    let tasks = kanban
        .list_tasks(TaskFilter {
            status,
            assignee: args
                .get("assignee")
                .and_then(Value::as_str)
                .map(str::to_string),
            limit: args
                .get("limit")
                .and_then(Value::as_u64)
                .map(|value| value as usize),
            ..Default::default()
        })
        .map_err(|err| err.to_string())?;
    Ok(json!({"tasks": summarize_kanban_tasks(tasks)}))
}

fn dispatch_kanban_ready_task(ctx: &ToolContext, args: &Value) -> Result<Value, String> {
    let kanban = ctx
        .kanban
        .ok_or_else(|| "kanban store is unavailable".to_string())?;
    let task_id = args
        .get("task_id")
        .and_then(Value::as_u64)
        .ok_or_else(|| "task_id is required".to_string())? as TaskId;
    ready_kanban_task(kanban, task_id)?;
    Ok(json!({"ok": true, "task_id": task_id, "status": "ready", "auto_dispatch": true}))
}

fn ready_kanban_task(kanban: &dyn KanbanStore, task_id: TaskId) -> Result<(), String> {
    let task = kanban.get_task(task_id).map_err(|err| err.to_string())?;
    match task.status {
        Status::Ready => Ok(()),
        Status::Triage => {
            kanban
                .transition(task_id, Status::Todo)
                .map_err(|err| err.to_string())?;
            kanban
                .transition(task_id, Status::Ready)
                .map_err(|err| err.to_string())
        }
        Status::Todo | Status::Running | Status::Blocked => kanban
            .transition(task_id, Status::Ready)
            .map_err(|err| err.to_string()),
        Status::Done | Status::Archived => Err(format!(
            "task #{} is {} and cannot be moved to ready",
            task_id, task.status
        )),
    }
}

fn resolve_kanban_board(ctx: &ToolContext, args: &Value) -> Result<Board, String> {
    let kanban = ctx
        .kanban
        .ok_or_else(|| "kanban store is unavailable".to_string())?;
    if let Some(slug) = args.get("board_slug").and_then(Value::as_str) {
        return match kanban.get_board(slug) {
            Ok(board) => Ok(board),
            Err(_) => {
                let name = args
                    .get("board_name")
                    .and_then(Value::as_str)
                    .unwrap_or(slug);
                let board_id = kanban
                    .create_board(slug, name)
                    .map_err(|err| err.to_string())?;
                kanban.get_board(slug).map_err(|err| {
                    format!("created board {board_id}, but failed to read it back: {err}")
                })
            }
        };
    }
    let boards = kanban.list_boards().map_err(|err| err.to_string())?;
    if let Some(board) = boards.into_iter().next() {
        return Ok(board);
    }
    let board_id = kanban
        .create_board("default", "Default")
        .map_err(|err| err.to_string())?;
    kanban
        .get_board("default")
        .map_err(|err| format!("created board {board_id}, but failed to read it back: {err}"))
}

fn summarize_kanban_tasks(tasks: Vec<Task>) -> Vec<Value> {
    tasks
        .into_iter()
        .map(|task| {
            json!({
                "id": task.id,
                "title": task.title,
                "status": task.status.as_str(),
                "assignee": task.assignee,
                "priority": task.priority,
                "tags": task.tags,
            })
        })
        .collect()
}

fn parse_kanban_status(value: &str) -> Result<Status, String> {
    value
        .parse::<Status>()
        .map_err(|_| format!("invalid kanban status {}", value))
}
