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

#[cfg(unix)]
#[test]
fn sigterm_stops_idle_server_with_open_connection() {
    use std::net::TcpStream;

    struct ChildGuard(std::process::Child);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    let reservation = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = reservation.local_addr().unwrap();
    drop(reservation);
    let mut child = ChildGuard(
        Command::new(env!("CARGO_BIN_EXE_led-server"))
            .env("GRPC_ADDR", addr.to_string())
            .env_remove("EYECATCH_PATH")
            .env_remove("JINGLE_PATH")
            .env_remove("WORKER_TIMEOUT")
            .env("PANEL_ROWS", "32")
            .env("PANEL_COLS", "64")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let startup_deadline = Instant::now() + Duration::from_secs(10);
    // Keep a connection open without sending an HTTP/2 handshake.
    let _connection = loop {
        if let Ok(connection) = TcpStream::connect(addr) {
            break connection;
        }
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "server exited before listening"
        );
        assert!(Instant::now() < startup_deadline, "server did not start");
        std::thread::sleep(Duration::from_millis(20));
    };
    // Give serve_with_shutdown time to install its signal handlers.
    std::thread::sleep(Duration::from_millis(100));
    assert!(Command::new("/bin/kill")
        .args(["-TERM", &child.0.id().to_string()])
        .status()
        .unwrap()
        .success());
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            assert!(status.success(), "shutdown was not graceful: {status}");
            break;
        }
        assert!(Instant::now() < deadline, "server did not stop promptly");
        std::thread::sleep(Duration::from_millis(20));
    }
}
