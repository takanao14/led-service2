#![cfg(feature = "emulator")]

use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn occupied_port_causes_nonzero_exit() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("reserve a port");
    let addr = listener.local_addr().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_led-server"))
        .env("GRPC_ADDR", addr.to_string())
        .env_remove("EYECATCH_PATH")
        .env_remove("JINGLE_PATH")
        .env_remove("WORKER_TIMEOUT")
        .env("PANEL_ROWS", "32")
        .env("PANEL_COLS", "64")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start server");

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if child.try_wait().expect("check server status").is_some() {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let output = child.wait_with_output().expect("reap server");
            panic!(
                "server did not exit after bind failure: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    let output = child.wait_with_output().expect("collect server output");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "bind failure must not exit successfully"
    );
    assert!(
        stderr.contains("gRPC server failed at"),
        "unexpected failure: {stderr}"
    );
    assert!(
        stderr.contains(&addr.to_string()),
        "missing listen address: {stderr}"
    );
}
