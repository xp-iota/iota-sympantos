use crate::ipc_client::autostart::{
    AutostartError, locate_sidecar_binary_with_override, wait_for_port,
};
use std::net::{TcpListener, ToSocketAddrs};
use std::time::Duration;

#[test]
fn wait_for_port_succeeds_once_a_listener_is_bound() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local addr");
    wait_for_port(addr, Duration::from_millis(500), Duration::from_millis(10))
        .expect("listener is already accepting connections");
}

#[test]
fn wait_for_port_times_out_when_nothing_is_listening() {
    // Port 9 is the historical "discard" service; on CI sandboxes nothing
    // binds it, so connections should reliably fail/timeout.
    let addr = "127.0.0.1:9"
        .to_socket_addrs()
        .expect("parse addr")
        .next()
        .expect("at least one addr");
    let result = wait_for_port(addr, Duration::from_millis(150), Duration::from_millis(20));
    assert!(matches!(result, Err(AutostartError::Timeout { .. })));
}

#[test]
fn locate_sidecar_binary_prefers_an_explicit_override_when_it_exists() {
    let dir =
        std::env::temp_dir().join(format!("iota-core-ipc-client-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let binary_path = dir.join("fake-sidecar");
    std::fs::write(&binary_path, b"#!/bin/sh\n").expect("write fake binary");

    let found = locate_sidecar_binary_with_override(
        Some(binary_path.as_os_str()),
        "IOTA_CORE_TEST_SIDECAR_PATH",
        "fake-sidecar-not-used",
    );
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(found.unwrap(), binary_path);
}

#[test]
fn locate_sidecar_binary_errors_when_nothing_matches() {
    let result = locate_sidecar_binary_with_override(
        None,
        "IOTA_CORE_TEST_SIDECAR_PATH_MISSING",
        "definitely-not-a-real-binary-xyz",
    );
    assert!(matches!(result, Err(AutostartError::BinaryNotFound(_))));
}
