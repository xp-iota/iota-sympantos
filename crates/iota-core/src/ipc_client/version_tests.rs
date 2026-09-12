use crate::ipc_client::version::{VersionNegotiationError, negotiate_version};

#[test]
fn negotiates_the_highest_mutually_supported_version() {
    assert_eq!(negotiate_version(2, 3, 2, 3), Ok(3));
}

#[test]
fn backward_compatible_client_without_a_range_still_negotiates() {
    // A v2-only client sends protocol_version=2 with no explicit range,
    // i.e. client_min == client_max == 2.
    assert_eq!(negotiate_version(2, 2, 2, 3), Ok(2));
}

#[test]
fn rejects_a_client_range_entirely_below_the_server_minimum() {
    let err = negotiate_version(0, 1, 2, 3).unwrap_err();
    assert_eq!(
        err,
        VersionNegotiationError {
            client_min: 0,
            client_max: 1,
            server_min: 2,
            server_max: 3,
        }
    );
}

#[test]
fn rejects_a_client_range_entirely_above_the_server_maximum() {
    assert!(negotiate_version(5, 6, 2, 3).is_err());
}

#[test]
fn negotiated_version_is_capped_by_the_narrower_of_the_two_maximums() {
    assert_eq!(negotiate_version(2, 10, 2, 3), Ok(3));
}
