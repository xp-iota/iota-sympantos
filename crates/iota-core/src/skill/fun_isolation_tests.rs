//! End-to-end check that iota-fun runs from a temp workspace and does not
//! write into the shipped source tree.

use crate::skill::fun::*;

/// Runs a tool that needs no external toolchain (Python) and confirms the
/// shipped source directory gained no build artifacts.
#[test]
fn python_execution_succeeds_and_leaves_sources_untouched() {
    let root = fun_root().expect("iota-fun sources must be locatable from the test cwd");
    let python_dir = root.join("python");
    let before = directory_state(&python_dir);

    let started = std::time::Instant::now();
    let output = run_tool("fun.python", &serde_json::json!({}))
        .expect("fun.python should succeed when python3 is installed");
    assert!(!output.trim().is_empty(), "expected a generated number");

    let after = directory_state(&python_dir);
    assert_eq!(
        before, after,
        "running fun.python must not create or modify files in the shipped \
         source tree; all writes belong in the temp workspace"
    );
    // A trivial script should finish quickly; a temp-workspace round trip must
    // not introduce a stall.
    assert!(
        started.elapsed() < std::time::Duration::from_secs(30),
        "fun.python took {:?}",
        started.elapsed()
    );
}

/// Snapshot of a directory: sorted file names plus sizes.
fn directory_state(dir: &std::path::Path) -> Vec<(String, u64)> {
    let mut entries: Vec<(String, u64)> = std::fs::read_dir(dir)
        .expect("reading source directory")
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let metadata = entry.metadata().ok()?;
            if !metadata.is_file() {
                return None;
            }
            Some((
                entry.file_name().to_string_lossy().to_string(),
                metadata.len(),
            ))
        })
        .collect();
    entries.sort();
    entries
}
