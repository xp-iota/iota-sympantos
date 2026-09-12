use crate::types::{Comment, Run, RunStatus, Status, Task};
use crate::worker::{
    WorkerHandle, build_worker_context, configure_process_tree_root, write_context_or_kill,
};
use std::process::{Command, Stdio};
use std::time::Instant;

fn make_task() -> Task {
    Task {
        id: 1,
        board_id: 1,
        title: "Fix the bug".to_string(),
        body: Some("There is a nasty bug to fix.".to_string()),
        status: Status::Ready,
        assignee: None,
        priority: 0,
        tags: vec![],
        workspace_kind: None,
        workspace_path: None,
        created_at: 0,
        updated_at: 0,
        claimed_at: None,
        claim_ttl_secs: 900,
    }
}

#[test]
fn build_context_includes_task_and_comments() {
    let task = make_task();
    let comments = vec![Comment {
        id: 1,
        task_id: 1,
        author: "alice".to_string(),
        body: "Please fix urgently".to_string(),
        created_at: 0,
    }];
    let ctx = build_worker_context(&task, &comments, &[]);
    assert!(ctx.contains("Fix the bug"), "should include title");
    assert!(
        ctx.contains("There is a nasty bug to fix."),
        "should include body"
    );
    assert!(ctx.contains("alice"), "should include comment author");
    assert!(
        ctx.contains("Please fix urgently"),
        "should include comment body"
    );
}

#[test]
fn build_context_includes_prior_runs() {
    let task = make_task();
    let runs = vec![Run {
        id: "run-001".to_string(),
        task_id: 1,
        profile: "default".to_string(),
        status: RunStatus::Failed,
        started_at: 0,
        finished_at: None,
        last_heartbeat: 0,
        exit_code: Some(1),
        output_summary: Some("Hit an error".to_string()),
    }];
    let ctx = build_worker_context(&task, &[], &runs);
    assert!(
        ctx.contains("Prior attempts"),
        "should include prior attempts section"
    );
    assert!(ctx.contains("run-001"), "should include run id");
    assert!(ctx.contains("default"), "should include profile");
    assert!(ctx.contains("failed"), "should include status");
    assert!(ctx.contains("Hit an error"), "should include summary");
}

#[test]
fn kill_stops_child_process_tree() {
    let tmp = std::env::temp_dir().join(format!("iota-worker-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();
    let child_pid_path = tmp.join("child.pid");

    let (program, args): (&str, Vec<String>) = if cfg!(windows) {
        (
            "powershell",
            vec![
                "-NoProfile".into(),
                "-WindowStyle".into(),
                "Hidden".into(),
                "-Command".into(),
                format!(
                    "$p = Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-WindowStyle','Hidden','-Command','Start-Sleep -Seconds 30' -PassThru; Set-Content -Path '{}' -Value $p.Id; Wait-Process -Id $p.Id",
                    child_pid_path.display()
                ),
            ],
        )
    } else {
        (
            "sh",
            vec![
                "-c".into(),
                format!("sleep 30 & echo $! > '{}'; wait", child_pid_path.display()),
            ],
        )
    };

    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    configure_process_tree_root(&mut command);
    let child = command.spawn().unwrap();
    let mut handle = WorkerHandle {
        run_id: "run".to_string(),
        child,
        started_at: Instant::now(),
    };

    let child_pid = read_pid_file_with_retry(&child_pid_path);

    handle.kill().unwrap();
    let _ = handle.child.wait();
    std::thread::sleep(std::time::Duration::from_millis(200));

    assert!(
        !process_exists(&child_pid),
        "worker kill should terminate descendant process {}",
        child_pid
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn spawn_write_failure_kills_process_tree() {
    let tmp = std::env::temp_dir().join(format!("iota-worker-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();
    let child_pid_path = tmp.join("child.pid");

    let (program, args): (&str, Vec<String>) = if cfg!(windows) {
        (
            "powershell",
            vec![
                "-NoProfile".into(),
                "-WindowStyle".into(),
                "Hidden".into(),
                "-Command".into(),
                format!(
                    "$p = Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-WindowStyle','Hidden','-Command','Start-Sleep -Seconds 30' -PassThru; Set-Content -Path '{}' -Value $p.Id; Wait-Process -Id $p.Id",
                    child_pid_path.display()
                ),
            ],
        )
    } else {
        (
            "sh",
            vec![
                "-c".into(),
                format!("sleep 30 & echo $! > '{}'; wait", child_pid_path.display()),
            ],
        )
    };

    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    configure_process_tree_root(&mut command);
    let mut child = command.spawn().unwrap();

    let child_pid = read_pid_file_with_retry(&child_pid_path);

    let result = write_context_or_kill(&mut child, "x", |_child, _context| {
        Err(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "test write failure",
        ))
    });

    assert!(result.is_err());
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert!(
        !process_exists(&child_pid),
        "write failure cleanup should terminate descendant process {}",
        child_pid
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

fn process_exists(pid: &str) -> bool {
    if cfg!(windows) {
        std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!(
                    "if (Get-Process -Id {} -ErrorAction SilentlyContinue) {{ exit 0 }} else {{ exit 1 }}",
                    pid
                ),
            ])
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    } else {
        if let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            let state = stat.split_whitespace().nth(2);
            if state == Some("Z") {
                return false;
            }
        }
        std::process::Command::new("kill")
            .args(["-0", pid])
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }
}

fn read_pid_file_with_retry(path: &std::path::Path) -> String {
    let deadline = Instant::now() + std::time::Duration::from_secs(5);
    while Instant::now() < deadline {
        if path.exists() {
            match std::fs::read_to_string(path) {
                Ok(content) => {
                    let trimmed = content.trim().to_string();
                    if !trimmed.is_empty() {
                        return trimmed;
                    }
                }
                Err(_) => {
                    // Ignore sharing violations or file lock issues temporarily and retry
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    std::fs::read_to_string(path)
        .expect("Failed to read PID file after retries")
        .trim()
        .to_string()
}
