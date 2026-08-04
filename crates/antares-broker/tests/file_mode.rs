//! `file` mode e2e (tasks.md B8/B10/A2/A4): the real binary, real HTTP, real
//! SIGKILL. Commit-before-ack means an entity acknowledged with 201 MUST
//! survive an immediate `kill -9`; deletes must reach redb too (the Scorpio
//! phantom-409 trap). std-only harness — no test frameworks (`ponytail:`).
#![cfg(unix)]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

fn http(port: u16, request: &str) -> String {
    let mut s = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    s.write_all(request.as_bytes()).expect("write");
    let mut out = String::new();
    s.read_to_string(&mut out).expect("read");
    out
}

fn start(port: u16, dir: &Path, store: &str) -> Child {
    Command::new(env!("CARGO_BIN_EXE_antares"))
        .env("ANTARES_HTTP_PORT", port.to_string())
        .env("ANTARES_STORE", store)
        .env("ANTARES_DATA_DIR", dir)
        .spawn()
        .expect("spawn antares")
}

fn wait_healthy(port: u16) -> String {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) {
            let req = format!("GET /q/health HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n");
            if s.write_all(req.as_bytes()).is_ok() {
                let mut out = String::new();
                let _ = s.read_to_string(&mut out);
                if out.contains("200") {
                    return out;
                }
            }
        }
        assert!(Instant::now() < deadline, "broker on :{port} never got healthy");
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn tempdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("antares-e2e-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tempdir");
    dir
}

#[test]
fn kill_dash_nine_right_after_201_loses_nothing() {
    let dir = tempdir("kill9");
    let port = 21000 + (std::process::id() % 20000) as u16;
    let entity = r#"{"id":"urn:ngsi-ld:Test:kill9","type":"Test"}"#;

    // 1. create, get the 201 ack, SIGKILL immediately — no drain, no grace.
    let mut broker = start(port, &dir, "file");
    let health = wait_healthy(port);
    assert!(health.contains(r#""store":"file""#), "A4 health: {health}");
    let resp = http(
        port,
        &format!(
            "POST /ngsi-ld/v1/entities HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{entity}",
            entity.len()
        ),
    );
    assert!(resp.starts_with("HTTP/1.1 201"), "create: {resp}");
    broker.kill().expect("SIGKILL");
    broker.wait().expect("reap");

    // 2. restart from the same volume: the acked write is there (B3/B8).
    let mut broker = start(port, &dir, "file");
    wait_healthy(port);
    let resp = http(
        port,
        "GET /ngsi-ld/v1/entities/urn:ngsi-ld:Test:kill9 HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    );
    assert!(resp.starts_with("HTTP/1.1 200"), "retrieve after restart: {resp}");
    assert!(resp.contains("urn:ngsi-ld:Test:kill9"));

    // 3. delete, SIGKILL, restart: the delete reached redb — recreate gets
    //    201, not a phantom 409 (B10).
    let resp = http(
        port,
        "DELETE /ngsi-ld/v1/entities/urn:ngsi-ld:Test:kill9 HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    );
    assert!(resp.starts_with("HTTP/1.1 204"), "delete: {resp}");
    broker.kill().expect("SIGKILL");
    broker.wait().expect("reap");

    let mut broker = start(port, &dir, "file");
    wait_healthy(port);
    let resp = http(
        port,
        "GET /ngsi-ld/v1/entities/urn:ngsi-ld:Test:kill9 HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    );
    assert!(resp.starts_with("HTTP/1.1 404"), "deleted stays deleted: {resp}");
    let resp = http(
        port,
        &format!(
            "POST /ngsi-ld/v1/entities HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{entity}",
            entity.len()
        ),
    );
    assert!(resp.starts_with("HTTP/1.1 201"), "no phantom 409: {resp}");

    broker.kill().expect("cleanup kill");
    broker.wait().expect("cleanup reap");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unknown_store_mode_is_fatal() {
    let dir = tempdir("badmode");
    let status = start(23999, &dir, "bogus").wait().expect("wait");
    assert!(!status.success(), "unknown ANTARES_STORE must be fatal (A2)");
    let _ = std::fs::remove_dir_all(&dir);
}
