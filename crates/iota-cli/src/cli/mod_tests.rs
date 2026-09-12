use crate::cli::*;

#[test]
fn parse_rounds_skips_daemon_flags() {
    let args = vec!["--daemon".to_string(), "5".to_string()];
    assert_eq!(parse_rounds(&args), Some(5));
}
