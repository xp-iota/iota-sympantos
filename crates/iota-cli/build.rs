fn main() {
    // Set build timestamp
    println!(
        "cargo:rustc-env=BUILD_TIMESTAMP={}",
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    );

    // Check formatting before compilation
    let status = std::process::Command::new("cargo")
        .args(["fmt", "--", "--check"])
        .status();

    match status {
        Ok(s) if s.success() => {}
        _ => {
            eprintln!(
                "error: source code not formatted. Run `cargo fmt` to format before building."
            );
            std::process::exit(1);
        }
    }
}
