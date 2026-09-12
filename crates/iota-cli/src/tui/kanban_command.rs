use iota_kanban::{
    AdvancedBridge, CreateTaskRequest, Dispatcher, KanbanStore, Status, TaskFilter, TaskPatch,
    ensure_bridge_available,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, TryLockError};

#[cfg(test)]
pub(super) fn execute(
    args: &str,
    store: &Arc<dyn KanbanStore>,
    default_board: Option<&str>,
) -> Vec<String> {
    execute_with_services(args, store, default_board, None, None, None)
}

#[cfg(test)]
pub(super) fn execute_with_dispatcher(
    args: &str,
    store: &Arc<dyn KanbanStore>,
    default_board: Option<&str>,
    dispatcher: Option<&Arc<Mutex<Dispatcher>>>,
    daemon_active: Option<&Arc<AtomicBool>>,
) -> Vec<String> {
    execute_with_services(args, store, default_board, dispatcher, daemon_active, None)
}

pub(super) fn execute_with_services(
    args: &str,
    store: &Arc<dyn KanbanStore>,
    default_board: Option<&str>,
    dispatcher: Option<&Arc<Mutex<Dispatcher>>>,
    daemon_active: Option<&Arc<AtomicBool>>,
    bridge: Option<&AdvancedBridge>,
) -> Vec<String> {
    let parts: Vec<&str> = args.split_whitespace().collect();
    let subcmd = parts.first().copied().unwrap_or("list");
    match subcmd {
        "list" | "ls" => cmd_list(&parts[1..], store, default_board),
        "boards" => cmd_boards(store),
        "board" => cmd_board(&parts[1..], store, default_board),
        "create" | "new" | "add" => cmd_create(&parts[1..], store, default_board),
        "view" | "columns" => cmd_view(&parts[1..], store, default_board),
        "show" => cmd_show(&parts[1..], store),
        "edit" | "update" => cmd_edit(&parts[1..], store),
        "move" | "mv" | "transition" => cmd_move(&parts[1..], store),
        "comment" | "note" => cmd_comment(&parts[1..], store),
        "assign" => cmd_assign(&parts[1..], store),
        "dispatch" | "run" => cmd_dispatch(&parts[1..], store, dispatcher),
        "daemon" => cmd_daemon(daemon_active),
        "specify" => cmd_specify(&parts[1..], store, bridge),
        "decompose" => cmd_decompose(&parts[1..], store, bridge),
        "help" | "?" => cmd_help(),
        _ => vec![format!(
            "Unknown kanban subcommand: {}. Try /kanban help",
            subcmd
        )],
    }
}

fn cmd_list(
    args: &[&str],
    store: &Arc<dyn KanbanStore>,
    default_board: Option<&str>,
) -> Vec<String> {
    let status_filter = args.first().and_then(|s| s.parse::<Status>().ok());

    let board_id = default_board.and_then(|slug| store.get_board(slug).ok().map(|b| b.id));

    let filter = TaskFilter {
        board_id,
        status: status_filter,
        assignee: None,
        limit: Some(50),
    };

    match store.list_tasks(filter) {
        Ok(tasks) => {
            if tasks.is_empty() {
                return vec!["No tasks found.".to_string()];
            }
            let mut out = vec![format!("Tasks ({}):", tasks.len())];
            for task in &tasks {
                let assignee_str = task
                    .assignee
                    .as_deref()
                    .map(|a| format!(" @{}", a))
                    .unwrap_or_default();
                out.push(format!(
                    "  #{} [{}] {}{}",
                    task.id, task.status, task.title, assignee_str
                ));
            }
            out
        }
        Err(e) => vec![format!("Error listing tasks: {}", e)],
    }
}

fn cmd_boards(store: &Arc<dyn KanbanStore>) -> Vec<String> {
    match store.list_boards() {
        Ok(boards) => {
            if boards.is_empty() {
                return vec![
                    "No boards. Create one with: /kanban board create <slug> <name>".to_string(),
                ];
            }
            let mut out = vec!["Boards:".to_string()];
            for board in &boards {
                out.push(format!("  {} - {}", board.slug, board.name));
            }
            out
        }
        Err(e) => vec![format!("Error listing boards: {}", e)],
    }
}

fn cmd_board(
    args: &[&str],
    store: &Arc<dyn KanbanStore>,
    default_board: Option<&str>,
) -> Vec<String> {
    let subcmd = args.first().copied().unwrap_or("");
    match subcmd {
        "view" => cmd_view(&args[1..], store, default_board),
        "create" => {
            let slug = match args.get(1) {
                Some(s) => *s,
                None => return vec!["Usage: /kanban board create <slug> <name>".to_string()],
            };
            if args.len() < 3 {
                return vec!["Usage: /kanban board create <slug> <name>".to_string()];
            }
            let name = args[2..].join(" ");
            match store.create_board(slug, &name) {
                Ok(id) => vec![format!("Created board #{}: {} ({})", id, name, slug)],
                Err(e) => vec![format!("Error creating board: {}", e)],
            }
        }
        _ => vec!["Usage: /kanban board create <slug> <name>".to_string()],
    }
}

fn cmd_view(
    args: &[&str],
    store: &Arc<dyn KanbanStore>,
    default_board: Option<&str>,
) -> Vec<String> {
    // Determine which board to show
    let board = if let Some(slug) = args.first().copied().or(default_board) {
        store.get_board(slug).ok()
    } else {
        store
            .list_boards()
            .ok()
            .and_then(|bs| bs.into_iter().next())
    };

    let board = match board {
        Some(b) => b,
        None => {
            return vec![
                "No boards found. Create one with: /kanban board create <slug> <name>".to_string(),
            ];
        }
    };

    // Get all tasks for this board
    let tasks = match store.list_tasks(TaskFilter {
        board_id: Some(board.id),
        limit: Some(200),
        ..Default::default()
    }) {
        Ok(t) => t,
        Err(e) => return vec![format!("Error: {}", e)],
    };

    // Columns to display (skip Archived for brevity)
    let columns: &[Status] = &[
        Status::Triage,
        Status::Todo,
        Status::Ready,
        Status::Running,
        Status::Blocked,
        Status::Done,
    ];

    let grouped: Vec<Vec<&iota_kanban::Task>> = columns
        .iter()
        .map(|status| tasks.iter().filter(|t| t.status == *status).collect())
        .collect();

    // Column width adapts: minimum 11 chars content + 3 border/padding
    let col_width: usize = 12;
    let num_cols = columns.len();
    // Each column occupies col_width+2 chars of content, separated by "│"
    // Layout: "│" + (content + "│") * num_cols
    let inner_width = (col_width + 2) * num_cols + 1; // +1 for leading │

    let mut lines = Vec::new();

    // Top border
    let header_text = format!(" Board: {} ", board.name);
    let border_fill = inner_width.saturating_sub(header_text.len() + 2); // -2 for ┌ and ┐
    lines.push(format!(
        "\u{250c}\u{2500}{}{}┐",
        header_text,
        "\u{2500}".repeat(border_fill)
    ));

    // Column headers with counts
    let mut header_line = String::from("\u{2502}");
    for (i, status) in columns.iter().enumerate() {
        let count = grouped[i].len();
        let label = format!("{}({})", status.as_str().to_uppercase(), count);
        let padded = format!(" {:<width$}", label, width = col_width + 1);
        header_line.push_str(&padded);
        header_line.push('\u{2502}');
    }
    lines.push(header_line);

    // Separator line under headers
    let mut sep_line = String::from("\u{2502}");
    for _i in 0..num_cols {
        let sep = format!(
            " {:<width$}",
            "\u{2500}".repeat(col_width),
            width = col_width + 1
        );
        sep_line.push_str(&sep);
        sep_line.push('\u{2502}');
    }
    lines.push(sep_line);

    // Task rows
    let max_rows = grouped
        .iter()
        .map(|col| col.len())
        .max()
        .unwrap_or(0)
        .min(15);

    for row in 0..max_rows {
        let mut task_line = String::from("\u{2502}");
        for col_tasks in &grouped {
            let cell = if row < col_tasks.len() {
                let t = col_tasks[row];
                let marker = if t.status == Status::Running { "*" } else { "" };
                // Format: " #<id><marker> <title_truncated>"
                let id_part = format!("#{}{}", t.id, marker);
                let avail = col_width.saturating_sub(id_part.len() + 1); // +1 for space between id and title
                let title: String = t.title.chars().take(avail).collect();
                if title.is_empty() {
                    format!(" {}", id_part)
                } else {
                    format!(" {} {}", id_part, title)
                }
            } else {
                String::new()
            };
            let padded = format!("{:<width$}", cell, width = col_width + 2);
            task_line.push_str(&padded);
            task_line.push('\u{2502}');
        }
        lines.push(task_line);
    }

    // If no tasks at all, add an empty row for visual completeness
    if max_rows == 0 {
        let mut empty_line = String::from("\u{2502}");
        for _i in 0..num_cols {
            empty_line.push_str(&format!("{:<width$}", "", width = col_width + 2));
            empty_line.push('\u{2502}');
        }
        lines.push(empty_line);
    }

    // Bottom border
    let bottom_width = inner_width - 2; // subtract ┘ and └
    lines.push(format!(
        "\u{2514}{}\u{2518}",
        "\u{2500}".repeat(bottom_width)
    ));

    lines
}

fn cmd_create(
    args: &[&str],
    store: &Arc<dyn KanbanStore>,
    default_board: Option<&str>,
) -> Vec<String> {
    if args.is_empty() {
        return vec!["Usage: /kanban create <title>".to_string()];
    }
    let title = args.join(" ");

    // Resolve board_id: use default_board slug or fall back to first board
    let board_id = if let Some(slug) = default_board {
        match store.get_board(slug) {
            Ok(b) => b.id,
            Err(_) => return vec![format!("Board '{}' not found.", slug)],
        }
    } else {
        match store.list_boards() {
            Ok(boards) if !boards.is_empty() => boards[0].id,
            _ => {
                return vec![
                    "No boards exist. Create one first: /kanban board create <slug> <name>"
                        .to_string(),
                ];
            }
        }
    };

    let req = CreateTaskRequest {
        board_id,
        title: title.clone(),
        body: None,
        status: Some(Status::Triage),
        assignee: None,
        priority: None,
        tags: Vec::new(),
        workspace_kind: None,
        workspace_path: None,
    };

    match store.create_task(req) {
        Ok(id) => vec![format!("Created task #{}: {}", id, title)],
        Err(e) => vec![format!("Error creating task: {}", e)],
    }
}

fn cmd_show(args: &[&str], store: &Arc<dyn KanbanStore>) -> Vec<String> {
    let id_str = match args.first() {
        Some(s) => s.strip_prefix('#').unwrap_or(s),
        None => return vec!["Usage: /kanban show <id>".to_string()],
    };

    let id: u64 = match id_str.parse() {
        Ok(v) => v,
        Err(_) => return vec![format!("Invalid task id: {}", id_str)],
    };

    match store.get_task(id) {
        Ok(task) => {
            let mut out = vec![
                format!("Task #{}", task.id),
                format!("  Title:    {}", task.title),
                format!("  Status:   {}", task.status),
                format!("  Priority: {}", task.priority),
            ];
            if let Some(ref assignee) = task.assignee {
                out.push(format!("  Assignee: @{}", assignee));
            }
            if !task.tags.is_empty() {
                out.push(format!("  Tags:     {}", task.tags.join(", ")));
            }
            if let Some(ref body) = task.body {
                out.push(format!("  Body:     {}", body));
            }
            out
        }
        Err(e) => vec![format!("Error: {}", e)],
    }
}

fn cmd_move(args: &[&str], store: &Arc<dyn KanbanStore>) -> Vec<String> {
    if args.len() < 2 {
        return vec!["Usage: /kanban move <id> <status>".to_string()];
    }
    let id_str = args[0].strip_prefix('#').unwrap_or(args[0]);
    let id: u64 = match id_str.parse() {
        Ok(v) => v,
        Err(_) => return vec![format!("Invalid task id: {}", id_str)],
    };

    let status = match args[1].parse::<Status>() {
        Ok(s) => s,
        Err(e) => return vec![format!("Invalid status '{}': {}", args[1], e)],
    };

    match store.transition(id, status) {
        Ok(()) => vec![format!("Task #{} -> {}", id, status)],
        Err(e) => vec![format!("Error: {}", e)],
    }
}

fn cmd_edit(args: &[&str], store: &Arc<dyn KanbanStore>) -> Vec<String> {
    if args.len() < 3 {
        return vec![
            "Usage: /kanban edit <id> title <text> | /kanban edit <id> body <text>".to_string(),
        ];
    }
    let id = match parse_task_id_arg(&args[0..1], "Usage: /kanban edit <id> <field> <text>") {
        Ok(id) => id,
        Err(lines) => return lines,
    };
    let value = args[2..].join(" ");
    let patch = match args[1] {
        "title" => TaskPatch {
            title: Some(value.clone()),
            ..Default::default()
        },
        "body" => TaskPatch {
            body: Some(Some(value.clone())),
            ..Default::default()
        },
        field => {
            return vec![format!(
                "Unsupported edit field: {}. Supported fields: title, body",
                field
            )];
        }
    };

    match store.update_task(id, patch) {
        Ok(()) => vec![format!("Task #{} {} updated.", id, args[1])],
        Err(e) => vec![format!("Error updating task #{}: {}", id, e)],
    }
}

fn cmd_comment(args: &[&str], store: &Arc<dyn KanbanStore>) -> Vec<String> {
    if args.len() < 2 {
        return vec!["Usage: /kanban comment <id> <text>".to_string()];
    }
    let id_str = args[0].strip_prefix('#').unwrap_or(args[0]);
    let id: u64 = match id_str.parse() {
        Ok(v) => v,
        Err(_) => return vec![format!("Invalid task id: {}", id_str)],
    };
    let body = args[1..].join(" ");

    match store.add_comment(id, "user", &body) {
        Ok(_) => vec![format!("Comment added to task #{}.", id)],
        Err(e) => vec![format!("Error adding comment: {}", e)],
    }
}

fn cmd_assign(args: &[&str], store: &Arc<dyn KanbanStore>) -> Vec<String> {
    if args.len() < 2 {
        return vec!["Usage: /kanban assign <id> <@assignee>".to_string()];
    }
    let id_str = args[0].strip_prefix('#').unwrap_or(args[0]);
    let id: u64 = match id_str.parse() {
        Ok(v) => v,
        Err(_) => return vec![format!("Invalid task id: {}", id_str)],
    };
    let assignee = args[1].strip_prefix('@').unwrap_or(args[1]);

    let patch = TaskPatch {
        assignee: Some(Some(assignee.to_string())),
        ..Default::default()
    };

    match store.update_task(id, patch) {
        Ok(()) => vec![format!("Task #{} assigned to @{}.", id, assignee)],
        Err(e) => vec![format!("Error assigning task: {}", e)],
    }
}

fn cmd_dispatch(
    args: &[&str],
    store: &Arc<dyn KanbanStore>,
    dispatcher: Option<&Arc<Mutex<Dispatcher>>>,
) -> Vec<String> {
    // If a specific task ID is given, move it to ready first
    if let Some(id_str) = args.first() {
        let id_str = id_str.strip_prefix('#').unwrap_or(id_str);
        if let Ok(id) = id_str.parse::<u64>() {
            match store.get_task(id) {
                Ok(task) => {
                    if let Err(message) = ready_task_for_dispatch(store, id, task.status) {
                        return vec![message];
                    }
                }
                Err(_) => return vec![format!("Task #{} not found", id)],
            }
        }
    }

    if let Some(dispatcher) = dispatcher {
        let mut dispatcher = match dispatcher.try_lock() {
            Ok(dispatcher) => dispatcher,
            Err(TryLockError::WouldBlock) => {
                return vec![
                    "Dispatch already running in the background; try again after it finishes."
                        .to_string(),
                ];
            }
            Err(TryLockError::Poisoned(err)) => {
                eprintln!(
                    "[iota] warning: kanban dispatcher mutex was poisoned; recovering inner value"
                );
                err.into_inner()
            }
        };
        return match dispatcher.tick(store.as_ref()) {
            Ok(report) => {
                let mut lines = Vec::new();
                if report.spawned > 0 {
                    lines.push(format!("Dispatched {} task(s)", report.spawned));
                }
                if report.completed > 0 {
                    lines.push(format!("{} task(s) completed", report.completed));
                }
                if report.timed_out > 0 {
                    lines.push(format!("{} task(s) timed out", report.timed_out));
                }
                if report.spawn_failures > 0 {
                    lines.push(format!("{} spawn failure(s)", report.spawn_failures));
                }
                if report.reclaimed > 0 {
                    lines.push(format!("{} reclaimed", report.reclaimed));
                }
                if lines.is_empty() {
                    lines.push("No ready tasks to dispatch.".to_string());
                }
                let active = dispatcher.active_worker_count();
                if active > 0 {
                    lines.push(format!("Active workers: {}", active));
                }
                lines
            }
            Err(e) => vec![format!("Dispatcher error: {}", e)],
        };
    }

    // Fallback when no dispatcher is available: just list ready tasks
    let filter = TaskFilter {
        board_id: None,
        status: Some(Status::Ready),
        assignee: None,
        limit: Some(20),
    };

    match store.list_tasks(filter) {
        Ok(tasks) => {
            if tasks.is_empty() {
                return vec!["No ready tasks to dispatch.".to_string()];
            }
            let mut out = vec![format!("Ready tasks ({}):", tasks.len())];
            for task in &tasks {
                let assignee_str = task
                    .assignee
                    .as_deref()
                    .map(|a| format!(" @{}", a))
                    .unwrap_or_default();
                out.push(format!("  #{} {}{}", task.id, task.title, assignee_str));
            }
            out
        }
        Err(e) => vec![format!("Error listing tasks: {}", e)],
    }
}

fn ready_task_for_dispatch(
    store: &Arc<dyn KanbanStore>,
    id: u64,
    status: Status,
) -> Result<(), String> {
    match status {
        Status::Ready => Ok(()),
        Status::Triage => {
            store
                .transition(id, Status::Todo)
                .map_err(|e| format!("Cannot ready task #{}: {}", id, e))?;
            store
                .transition(id, Status::Ready)
                .map_err(|e| format!("Cannot ready task #{}: {}", id, e))
        }
        Status::Todo | Status::Running | Status::Blocked => store
            .transition(id, Status::Ready)
            .map_err(|e| format!("Cannot ready task #{}: {}", id, e)),
        Status::Done | Status::Archived => Err(format!(
            "Task #{} is {} — must be triage, todo, ready, running, or blocked to dispatch",
            id, status
        )),
    }
}

fn cmd_daemon(daemon_active: Option<&Arc<AtomicBool>>) -> Vec<String> {
    let Some(flag) = daemon_active else {
        return vec!["Daemon control not available in this context.".to_string()];
    };
    let was_active = flag.load(Ordering::Relaxed);
    let new_active = !was_active;
    flag.store(new_active, Ordering::Relaxed);
    if new_active {
        vec!["Kanban daemon started (auto-dispatch every 30s)".to_string()]
    } else {
        vec!["Kanban daemon stopped".to_string()]
    }
}

fn parse_task_id_arg(args: &[&str], usage: &str) -> Result<u64, Vec<String>> {
    let Some(id_str) = args.first() else {
        return Err(vec![usage.to_string()]);
    };
    let id_str = id_str.strip_prefix('#').unwrap_or(id_str);
    id_str
        .parse::<u64>()
        .map_err(|_| vec![format!("Invalid task id: {}", id_str)])
}

fn cmd_specify(
    args: &[&str],
    store: &Arc<dyn KanbanStore>,
    bridge: Option<&AdvancedBridge>,
) -> Vec<String> {
    let id = match parse_task_id_arg(args, "Usage: /kanban specify <id>") {
        Ok(id) => id,
        Err(lines) => return lines,
    };
    let Some(bridge) = bridge else {
        return vec!["Advanced kanban bridge is not available.".to_string()];
    };
    if let Err(e) = ensure_bridge_available(bridge) {
        return vec![e.to_string()];
    }

    match bridge.specify(id, store.as_ref()) {
        Ok(result) => vec![format!(
            "Specified task #{} ({} chars).",
            result.task_id,
            result.spec_body.chars().count()
        )],
        Err(e) => vec![format!("Specify failed for task #{}: {}", id, e)],
    }
}

fn cmd_decompose(
    args: &[&str],
    store: &Arc<dyn KanbanStore>,
    bridge: Option<&AdvancedBridge>,
) -> Vec<String> {
    let id = match parse_task_id_arg(args, "Usage: /kanban decompose <id>") {
        Ok(id) => id,
        Err(lines) => return lines,
    };
    let Some(bridge) = bridge else {
        return vec!["Advanced kanban bridge is not available.".to_string()];
    };
    if let Err(e) = ensure_bridge_available(bridge) {
        return vec![e.to_string()];
    }

    match bridge.decompose(id, store.as_ref()) {
        Ok(result) => vec![format!(
            "Decomposed task #{} into {} child task(s): {}",
            result.parent_id,
            result.child_ids.len(),
            result
                .child_ids
                .iter()
                .map(|id| format!("#{id}"))
                .collect::<Vec<_>>()
                .join(", ")
        )],
        Err(e) => vec![format!("Decompose failed for task #{}: {}", id, e)],
    }
}

fn cmd_help() -> Vec<String> {
    vec![
        "Kanban commands:".to_string(),
        "  /kanban tab [board]        - Open interactive Kanban tab".to_string(),
        "  /kanban close              - Close interactive Kanban tab".to_string(),
        "  /kanban list [status]       - List tasks (optionally filter by status)".to_string(),
        "  /kanban view [board]        - Show board as column view".to_string(),
        "  /kanban boards              - List all boards".to_string(),
        "  /kanban board create <slug> <name> - Create a new board".to_string(),
        "  /kanban create <title>      - Create a new task".to_string(),
        "  /kanban show <id>           - Show task details".to_string(),
        "  /kanban edit <id> title|body <text> - Edit task title or body".to_string(),
        "  /kanban move <id> <status>  - Transition task to a new status".to_string(),
        "  /kanban comment <id> <text> - Add a comment to a task".to_string(),
        "  /kanban assign <id> <user>  - Assign a task to a user".to_string(),
        "  /kanban dispatch [id]       - Tick dispatcher (or ready+dispatch one task)".to_string(),
        "  /kanban daemon              - Toggle auto-dispatch daemon (30s interval)".to_string(),
        "  /kanban specify <id>        - Expand a task into a detailed spec via hermes".to_string(),
        "  /kanban decompose <id>      - Break a task into child tasks via hermes".to_string(),
        "  /kanban help                - Show this help".to_string(),
        "".to_string(),
        "Statuses: triage, todo, ready, running, blocked, done, archived".to_string(),
        "Aliases: /kb, /k | view aliases: columns, board view".to_string(),
    ]
}

#[cfg(test)]
#[path = "kanban_command_tests.rs"]
mod kanban_command_tests;
