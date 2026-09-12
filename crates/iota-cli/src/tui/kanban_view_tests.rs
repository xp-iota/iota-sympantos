use crate::tui::kanban_view::*;
use iota_kanban::{LinkKind, RunStatus};

fn task(id: TaskId, status: Status, title: &str) -> KanbanTaskSnapshot {
    KanbanTaskSnapshot {
        task: Task {
            id,
            board_id: 1,
            title: title.to_string(),
            body: Some(format!("body {id}")),
            status,
            assignee: None,
            priority: id as i32,
            tags: vec![],
            workspace_kind: None,
            workspace_path: None,
            created_at: id as i64,
            updated_at: id as i64,
            claimed_at: None,
            claim_ttl_secs: 1800,
        },
        links: vec![],
        runs: vec![],
    }
}

fn snapshot() -> KanbanSnapshot {
    KanbanSnapshot {
        board: Some(Board {
            id: 1,
            slug: "dev".to_string(),
            name: "Dev".to_string(),
            created_at: 1,
        }),
        tasks: vec![
            task(1, Status::Todo, "write tests"),
            task(2, Status::Ready, "wire tui"),
            task(3, Status::Done, "review spec"),
        ],
    }
}

#[test]
fn column_view_groups_tasks_and_selects_first_task() {
    let mut state = KanbanViewState::default();
    state.open(Some("dev".to_string()));
    state.selected_column = 1;

    let lines = render_lines(&mut state, &snapshot(), 100, 8).join("\n");

    assert_eq!(state.selected_task_id, Some(1));
    assert!(lines.contains("[todo:1]"));
    assert!(lines.contains("*#1 write tests"));
}

#[test]
fn list_view_selects_and_moves_through_tasks() {
    let snapshot = snapshot();
    let mut state = KanbanViewState {
        active: true,
        mode: KanbanViewMode::List,
        ..Default::default()
    };

    state.select_task_delta(1, &snapshot);

    assert_eq!(state.selected_task_id, Some(2));
}

#[test]
fn graph_view_renders_task_links() {
    let mut snapshot = snapshot();
    snapshot.tasks[0].links.push(Link {
        from_id: 1,
        to_id: 2,
        kind: LinkKind::Blocks,
    });
    let mut state = KanbanViewState {
        active: true,
        mode: KanbanViewMode::Graph,
        ..Default::default()
    };

    let lines = render_lines(&mut state, &snapshot, 100, 8).join("\n");

    assert!(lines.contains("blocks->#2"));
}

#[test]
fn timeline_view_renders_worker_runs() {
    let mut snapshot = snapshot();
    snapshot.tasks[1].runs.push(Run {
        id: "run-1".to_string(),
        task_id: 2,
        profile: "hermes".to_string(),
        status: RunStatus::Completed,
        started_at: 10,
        finished_at: Some(20),
        last_heartbeat: 20,
        exit_code: Some(0),
        output_summary: None,
    });
    let mut state = KanbanViewState {
        active: true,
        mode: KanbanViewMode::Timeline,
        ..Default::default()
    };

    let lines = render_lines(&mut state, &snapshot, 100, 8).join("\n");

    assert!(lines.contains("hermes"));
    assert!(lines.contains("completed"));
}
