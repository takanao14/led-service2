use std::process::Command;

#[test]
fn version_exits_before_runtime_initialization() {
    let output = Command::new(env!("CARGO_BIN_EXE_led-server"))
        .arg("--version")
        .env("GRPC_ADDR", "invalid")
        .env("PANEL_ROWS", "invalid")
        .env("EYECATCH_PATH", "/nonexistent/eyecatch.gif")
        .output()
        .expect("run version command");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        concat!("led-server ", env!("CARGO_PKG_VERSION"), "\n")
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn invalid_probe_url_fails_without_runtime_configuration() {
    let output = Command::new(env!("CARGO_BIN_EXE_led-server"))
        .args(["--check", "http://[invalid"])
        .env("PANEL_ROWS", "invalid")
        .output()
        .expect("run probe command");
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stderr).contains("PANEL_ROWS"));
}
