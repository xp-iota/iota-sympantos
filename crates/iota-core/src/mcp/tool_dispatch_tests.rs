use crate::mcp::tool_dispatch::*;
#[cfg(feature = "kanban")]
use iota_kanban::{KanbanStore, SqliteKanbanStore, Status};
use serde_json::json;

#[test]
fn memory_scope_id_defaults_with_workspace() {
    let workspace = std::path::Path::new("/tmp/iota-project");
    assert_eq!(
        default_memory_scope_id(&MemoryScope::User, &json!({}), workspace),
        "local-user"
    );
    assert_eq!(
        default_memory_scope_id(&MemoryScope::Project, &json!({}), workspace),
        workspace.display().to_string()
    );
    assert_eq!(
        default_memory_scope_id(
            &MemoryScope::Session,
            &json!({"source_session_id": "session-1"}),
            workspace
        ),
        "session-1"
    );
    assert_eq!(
        default_memory_scope_id(
            &MemoryScope::Session,
            &json!({"session_id": "s1"}),
            workspace
        ),
        "s1"
    );
    assert_eq!(
        default_memory_scope_id(&MemoryScope::Global, &json!({}), workspace),
        "global"
    );
}

#[test]
fn confidence_validation() {
    assert_eq!(
        required_confidence(&json!({})).unwrap_err(),
        "confidence is required"
    );
    assert_eq!(
        required_confidence(&json!({"confidence": 1.5})).unwrap_err(),
        "confidence must be between 0 and 1"
    );
    assert_eq!(
        required_confidence(&json!({"confidence": "0.75"})).unwrap(),
        0.75
    );
    assert_eq!(
        required_confidence(&json!({"confidence": 0.9})).unwrap(),
        0.9
    );
}

#[test]
fn memory_shape_validation() {
    assert_eq!(
        validate_memory_shape(MemoryType::Semantic, None).unwrap_err(),
        "semantic memory requires a facet"
    );
    assert_eq!(
        validate_memory_shape(MemoryType::Procedural, Some(MemoryFacet::Domain)).unwrap_err(),
        "only semantic memory may set facet"
    );
    validate_memory_shape(MemoryType::Semantic, Some(MemoryFacet::Domain)).unwrap();
    validate_memory_shape(MemoryType::Episodic, None).unwrap();
}

#[test]
fn is_known_tool_recognizes_iota_tools() {
    assert!(is_known_tool("iota_memory_search"));
    assert!(is_known_tool("iota_memory_write"));
    assert!(is_known_tool("iota_skill_search"));
    assert!(is_known_tool("iota_skill_load"));
    assert!(is_known_tool("iota_session_summary"));
    assert!(is_known_tool("iota_handoff_publish"));
    assert!(is_known_tool("iota_handoff_read"));
    #[cfg(feature = "kanban")]
    {
        assert!(is_known_tool("iota_kanban_create_task"));
        assert!(is_known_tool("iota_kanban_list_tasks"));
        assert!(is_known_tool("iota_kanban_ready_task"));
    }
    assert!(!is_known_tool("external_tool"));
    assert!(!is_known_tool("iota_unknown"));
}

#[test]
#[cfg(feature = "kanban")]
fn kanban_create_task_auto_promotes_generated_tasks_to_ready() {
    let store = SqliteKanbanStore::open(std::path::Path::new(":memory:")).unwrap();
    let workspace = std::path::Path::new("/tmp/iota-project");
    let skills = crate::skill::SkillRegistry::load(workspace, &[]);
    let ctx = ToolContext {
        memory: None,
        ledger: None,
        kanban: Some(&store),
        skills: &skills,
        workspace,
    };

    let result = dispatch_tool(
        &ctx,
        "iota_kanban_create_task",
        &json!({
            "title": "Research Agent - TinyFish trending to Supabase",
            "assignee": "research-agent",
            "tags": ["research", "supabase"]
        }),
    )
    .unwrap();

    let task_id = result["task_id"].as_u64().unwrap();
    let task = store.get_task(task_id).unwrap();
    assert_eq!(task.status, Status::Ready);
    assert_eq!(task.assignee.as_deref(), Some("research-agent"));
    assert_eq!(result["created_status"], "triage");
    assert_eq!(result["auto_ready"], true);
    assert_eq!(result["auto_dispatch"], true);
}

#[test]
#[cfg(feature = "kanban")]
fn kanban_create_task_can_keep_manual_triage_when_auto_ready_is_false() {
    let store = SqliteKanbanStore::open(std::path::Path::new(":memory:")).unwrap();
    let workspace = std::path::Path::new("/tmp/iota-project");
    let skills = crate::skill::SkillRegistry::load(workspace, &[]);
    let ctx = ToolContext {
        memory: None,
        ledger: None,
        kanban: Some(&store),
        skills: &skills,
        workspace,
    };

    let result = dispatch_tool(
        &ctx,
        "iota_kanban_create_task",
        &json!({
            "title": "Raw idea for later grooming",
            "auto_ready": false
        }),
    )
    .unwrap();

    let task_id = result["task_id"].as_u64().unwrap();
    let task = store.get_task(task_id).unwrap();
    assert_eq!(task.status, Status::Triage);
    assert_eq!(result["status"], "triage");
    assert_eq!(result["auto_ready"], false);
    assert_eq!(result["auto_dispatch"], false);
}

#[test]
#[cfg(feature = "kanban")]
fn kanban_create_task_allows_explicit_ready_for_dispatch() {
    let store = SqliteKanbanStore::open(std::path::Path::new(":memory:")).unwrap();
    let workspace = std::path::Path::new("/tmp/iota-project");
    let skills = crate::skill::SkillRegistry::load(workspace, &[]);
    let ctx = ToolContext {
        memory: None,
        ledger: None,
        kanban: Some(&store),
        skills: &skills,
        workspace,
    };

    let result = dispatch_tool(
        &ctx,
        "iota_kanban_create_task",
        &json!({
            "title": "Research Agent - TinyFish trending to Supabase",
            "status": "ready",
            "assignee": "research-agent",
            "tags": ["research", "supabase"]
        }),
    )
    .unwrap();

    let task_id = result["task_id"].as_u64().unwrap();
    let task = store.get_task(task_id).unwrap();
    assert_eq!(task.status, Status::Ready);
    assert_eq!(task.assignee.as_deref(), Some("research-agent"));
    assert_eq!(result["auto_dispatch"], true);
}

#[test]
#[cfg(feature = "kanban")]
fn kanban_ready_task_manually_promotes_triage_through_todo() {
    let store = SqliteKanbanStore::open(std::path::Path::new(":memory:")).unwrap();
    let workspace = std::path::Path::new("/tmp/iota-project");
    let skills = crate::skill::SkillRegistry::load(workspace, &[]);
    let ctx = ToolContext {
        memory: None,
        ledger: None,
        kanban: Some(&store),
        skills: &skills,
        workspace,
    };
    let task_id = store
        .create_task(iota_kanban::CreateTaskRequest {
            board_id: store.create_board("manual", "Manual").unwrap(),
            title: "Manual ready promotion".to_string(),
            body: None,
            status: Some(Status::Triage),
            assignee: None,
            priority: None,
            tags: Vec::new(),
            workspace_kind: None,
            workspace_path: None,
        })
        .unwrap();

    let result =
        dispatch_tool(&ctx, "iota_kanban_ready_task", &json!({"task_id": task_id})).unwrap();

    assert_eq!(result["status"], "ready");
    assert_eq!(store.get_task(task_id).unwrap().status, Status::Ready);
}
