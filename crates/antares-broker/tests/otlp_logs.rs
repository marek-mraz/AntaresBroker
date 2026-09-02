// SPDX-License-Identifier: EUPL-1.2
//! OTLP log export: with ANTARES_TELEMETRY on and ANTARES_OTLP_ENDPOINT set,
//! log records leave over OTLP/HTTP to the `v1/logs` twin of the traces
//! endpoint, carrying the same resource; a collector nobody answers never
//! slows a request, because the export runs off the request path.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

mod common;
use common::free_port;

struct Broker(Child);
impl Drop for Broker {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn start(port: u16, endpoint: &str) -> Broker {
    Broker(
        Command::new(env!("CARGO_BIN_EXE_antares"))
            .env("ANTARES_HTTP_PORT", port.to_string())
            .env("ANTARES_STORE", "memory")
            .env("ANTARES_TELEMETRY", "1")
            .env("ANTARES_OTLP_ENDPOINT", endpoint)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn antares"),
    )
}

fn http(port: u16, path: &str) -> String {
    let mut s = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    s.write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").as_bytes())
        .expect("write");
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    out
}

fn wait_healthy(port: u16) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) {
            if s.write_all(b"GET /q/health HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
                .is_ok()
            {
                let mut out = String::new();
                let _ = s.read_to_string(&mut out);
                if out.contains("200") {
                    return;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    panic!("broker on :{port} never got healthy");
}

type Seen = Arc<Mutex<Vec<(String, Vec<u8>)>>>;

/// A bare OTLP/HTTP collector: records (request line, body) and answers 200.
fn collector() -> (u16, Seen) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let seen: Seen = Arc::default();
    let s = seen.clone();
    std::thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            let s = s.clone();
            std::thread::spawn(move || {
                let mut conn = conn;
                let _ = conn.set_read_timeout(Some(Duration::from_secs(5)));
                let mut buf = Vec::new();
                let mut chunk = [0u8; 8192];
                let (mut head_end, mut len) = (None, 0usize);
                loop {
                    let n = match conn.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => n,
                    };
                    buf.extend_from_slice(&chunk[..n]);
                    if head_end.is_none() {
                        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            head_end = Some(i + 4);
                            let head = String::from_utf8_lossy(&buf[..i]).to_string();
                            len = head
                                .lines()
                                .find_map(|l| {
                                    l.to_ascii_lowercase()
                                        .strip_prefix("content-length:")
                                        .and_then(|v| v.trim().parse().ok())
                                })
                                .unwrap_or(0);
                        }
                    }
                    if let Some(h) = head_end {
                        if buf.len() >= h + len {
                            break;
                        }
                    }
                }
                let h = head_end.unwrap_or(buf.len());
                let line = String::from_utf8_lossy(&buf[..h])
                    .lines()
                    .next()
                    .unwrap_or("")
                    .to_string();
                s.lock().expect("lock").push((line, buf[h..].to_vec()));
                let _ = conn.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
            });
        }
    });
    (port, seen)
}

/// A collector that accepts and then says nothing, holding every connection
/// open. A closed port answers RST at once, so it never exercises an
/// exporter waiting on a response — this does. The counter is how the test
/// knows the exporter really is stuck on it.
fn black_hole() -> (u16, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let hits = Arc::new(AtomicUsize::new(0));
    let h = hits.clone();
    std::thread::spawn(move || {
        let mut held = Vec::new();
        for conn in listener.incoming().flatten() {
            h.fetch_add(1, Ordering::Relaxed);
            held.push(conn);
        }
    });
    (port, hits)
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// A log record written after startup reaches the collector at `v1/logs`
/// as protobuf carrying the message text and the `antares` service name;
/// spans keep going to `v1/traces`.
#[test]
fn log_records_reach_the_collector_next_to_the_spans() {
    let (cport, seen) = collector();
    let port = free_port();
    let _b = start(port, &format!("http://127.0.0.1:{cport}/v1/traces"));
    wait_healthy(port);
    let deadline = Instant::now() + Duration::from_secs(20);
    let logs = loop {
        let got: Vec<_> = seen
            .lock()
            .expect("lock")
            .iter()
            .filter(|(line, _)| line.starts_with("POST /v1/logs "))
            .map(|(_, body)| body.clone())
            .collect();
        if got.iter().any(|b| contains(b, b"starting antares")) {
            break got;
        }
        assert!(
            Instant::now() < deadline,
            "no log export within 20 s: {:?}",
            seen.lock()
                .expect("lock")
                .iter()
                .map(|(l, b)| (l.clone(), b.len()))
                .collect::<Vec<_>>()
        );
        std::thread::sleep(Duration::from_millis(250));
    };
    assert!(
        logs.iter()
            .any(|b| contains(b, b"service.name") && contains(b, b"antares")),
        "the resource travels with the logs, service.name and all"
    );
    assert!(
        !logs
            .iter()
            .any(|b| contains(b, b"ResourceSpans") || contains(b, b"v1/traces")),
        "log bodies are not span bodies"
    );
}

/// A collector nobody answers costs the request path nothing: the export
/// queue is bounded and drained off-path, so /q/health stays fast.
#[test]
fn a_dead_collector_never_slows_a_request() {
    let (dead, hits) = black_hole();
    let port = free_port();
    let _b = start(port, &format!("http://127.0.0.1:{dead}/v1/traces"));
    wait_healthy(port);
    // Measure the request path while the exporter is actually blocked on
    // the collector, not before it has tried.
    let deadline = Instant::now() + Duration::from_secs(20);
    while hits.load(Ordering::Relaxed) == 0 {
        assert!(
            Instant::now() < deadline,
            "the exporter never dialled the collector"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    for _ in 0..5 {
        let t = Instant::now();
        let out = http(port, "/q/health");
        assert!(out.contains("200"), "{out}");
        assert!(
            t.elapsed() < Duration::from_millis(1500),
            "{:?}",
            t.elapsed()
        );
    }
}
