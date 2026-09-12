use crate::skill::fun::*;

#[cfg(unix)]
#[test]
fn run_command_times_out_without_waiting_for_child_completion() {
    let started = Instant::now();
    let err = run_command(
        "sh",
        &[OsString::from("-c"), OsString::from("sleep 5")],
        None,
        100,
    )
    .unwrap_err();

    assert!(err.to_string().contains("timed out"));
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[cfg(unix)]
#[test]
fn run_command_rejects_excessive_output_without_buffering_it() {
    let err = run_command(
        "sh",
        &[
            OsString::from("-c"),
            OsString::from("yes x | head -c 70000"),
        ],
        None,
        5_000,
    )
    .unwrap_err();

    assert!(err.to_string().contains("output exceeded"));
}

#[cfg(windows)]
#[test]
fn run_command_times_out_without_waiting_for_child_completion() {
    let started = Instant::now();
    let err = run_command(
        "cmd",
        &[
            OsString::from("/C"),
            OsString::from("ping -n 6 127.0.0.1 >NUL"),
        ],
        None,
        100,
    )
    .unwrap_err();

    assert!(err.to_string().contains("timed out"));
    assert!(started.elapsed() < Duration::from_secs(2));
}

// ---------------------------------------------------------------------------
// Execution isolation
// ---------------------------------------------------------------------------

/// A child must never receive a credential the parent holds.
#[cfg(unix)]
#[test]
fn run_command_does_not_leak_api_keys_into_the_child() {
    // Seed every scrubbed variable with a recognizable sentinel.
    for key in SCRUBBED_ENV_KEYS {
        unsafe {
            std::env::set_var(key, "SENTINEL-SECRET-VALUE");
        }
    }

    let output = run_command(
        "sh",
        &[
            OsString::from("-c"),
            OsString::from("env | grep -c 'SENTINEL-SECRET-VALUE' || true"),
        ],
        None,
        5_000,
    )
    .unwrap();

    for key in SCRUBBED_ENV_KEYS {
        unsafe {
            std::env::remove_var(key);
        }
    }

    assert_eq!(
        output.trim(),
        "0",
        "no scrubbed credential may reach the child environment"
    );
}

/// The child runs in its own process group, so a timeout kills descendants too.
#[cfg(unix)]
#[test]
fn timeout_terminates_the_whole_process_tree() {
    let marker = std::env::temp_dir().join(format!("iota-fun-tree-{}", uuid::Uuid::new_v4()));
    let _ = std::fs::remove_file(&marker);

    // The shell backgrounds a long sleep that would outlive a direct-child-only
    // kill, then waits. If only the shell is killed, the sleep survives.
    let script = format!("(sleep 30; touch {}) & sleep 30", marker.display());
    let started = Instant::now();
    let err = run_command(
        "sh",
        &[OsString::from("-c"), OsString::from(script)],
        None,
        300,
    )
    .unwrap_err();
    assert!(err.to_string().contains("timed out"));
    assert!(started.elapsed() < Duration::from_secs(3));

    // Give the descendant a moment to have written the marker had it survived.
    std::thread::sleep(Duration::from_millis(200));
    assert!(
        !marker.exists(),
        "a grandchild process survived the timeout kill; the whole tree must be terminated"
    );
}

#[test]
fn temp_workspace_stages_sources_and_cleans_up_on_drop() {
    let source_dir = std::env::temp_dir().join(format!("iota-fun-src-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&source_dir).unwrap();
    let source = source_dir.join("sample.txt");
    std::fs::write(&source, "hello").unwrap();

    let workspace_path;
    {
        let workspace = TempWorkspace::prepare(std::slice::from_ref(&source)).unwrap();
        workspace_path = workspace.path().to_path_buf();
        assert!(workspace_path.exists());
        // Sources are staged by file name in a directory distinct from the
        // source tree, so the build cannot write into shipped sources.
        assert_ne!(workspace_path, source_dir);
        assert_eq!(
            std::fs::read_to_string(workspace_path.join("sample.txt")).unwrap(),
            "hello"
        );
    }

    assert!(
        !workspace_path.exists(),
        "the temp workspace must be removed when the execution ends"
    );
    let _ = std::fs::remove_dir_all(source_dir);
}

#[test]
fn temp_workspaces_are_unique_per_execution() {
    let source_dir = std::env::temp_dir().join(format!("iota-fun-uniq-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&source_dir).unwrap();
    let source = source_dir.join("s.txt");
    std::fs::write(&source, "x").unwrap();

    let first = TempWorkspace::prepare(std::slice::from_ref(&source)).unwrap();
    let second = TempWorkspace::prepare(std::slice::from_ref(&source)).unwrap();
    assert_ne!(
        first.path(),
        second.path(),
        "concurrent executions must not share a workspace"
    );
    let _ = std::fs::remove_dir_all(source_dir);
}

#[test]
fn promote_artifact_moves_build_output_into_the_cache() {
    let source_dir = std::env::temp_dir().join(format!("iota-fun-pr-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&source_dir).unwrap();
    let source = source_dir.join("in.txt");
    std::fs::write(&source, "src").unwrap();

    let cache_dir = std::env::temp_dir().join(format!("iota-fun-cache-{}", uuid::Uuid::new_v4()));
    let destination = cache_dir.join("artifact.bin");

    {
        let workspace = TempWorkspace::prepare(std::slice::from_ref(&source)).unwrap();
        std::fs::write(workspace.path().join("artifact.bin"), "compiled").unwrap();
        workspace
            .promote_artifact("artifact.bin", &destination)
            .unwrap();
    }

    assert_eq!(
        std::fs::read_to_string(&destination).unwrap(),
        "compiled",
        "a promoted artifact must survive workspace cleanup"
    );
    let _ = std::fs::remove_dir_all(source_dir);
    let _ = std::fs::remove_dir_all(cache_dir);
}

#[test]
fn unknown_tool_is_reported_as_a_structured_failure() {
    let error = run_tool_structured("fun.nope", &serde_json::json!({})).unwrap_err();
    assert_eq!(error.kind, FunErrorKind::UnknownTool);
    assert!(error.to_string().contains("unknown_tool"));
}

#[test]
fn command_timeout_classifies_as_a_timeout() {
    #[cfg(unix)]
    {
        let error = run_command(
            "sh",
            &[OsString::from("-c"), OsString::from("sleep 5")],
            None,
            100,
        )
        .unwrap_err();
        let classified = classify_command_error(error, FunErrorKind::RunFailed);
        assert_eq!(classified.kind, FunErrorKind::Timeout);
    }
}

#[test]
fn output_limit_classifies_as_an_output_limit() {
    #[cfg(unix)]
    {
        let error = run_command(
            "sh",
            &[
                OsString::from("-c"),
                OsString::from("yes x | head -c 70000"),
            ],
            None,
            5_000,
        )
        .unwrap_err();
        let classified = classify_command_error(error, FunErrorKind::RunFailed);
        assert_eq!(classified.kind, FunErrorKind::OutputLimit);
    }
}

#[test]
fn missing_tool_classifies_as_tool_missing() {
    let error = run_command("iota-fun-definitely-not-a-real-binary", &[], None, 1_000).unwrap_err();
    let classified = classify_command_error(error, FunErrorKind::ToolMissing);
    assert_eq!(classified.kind, FunErrorKind::ToolMissing);
}
