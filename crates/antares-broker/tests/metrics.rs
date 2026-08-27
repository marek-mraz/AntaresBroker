// SPDX-License-Identifier: EUPL-1.2
//! The observability surface, proven against the real binary.
//! /q/metrics serves the Prometheus text format with antares_-prefixed,
//! unit-suffixed instruments and the counters actually move.
//! The stack is a runtime switch (ANTARES_TELEMETRY=1, default OFF —
//! nothing telemetry-shaped is allocated); this spawns the broker with
//! the switch ON, plus proves the off-default answers 404.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

struct Broker(Child);

impl Drop for Broker {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn free_port() -> u16 {
    // Bind-0 / close / rebind races the other test processes for the same
    // port (seen as AddrInUse on the spawned broker). Each process draws from
    // its own pid-keyed range instead, still bind-probed, and stays below the
    // ephemeral range (32768+) so outbound connections never land on it.
    static NEXT: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(0);
    let base = 20_000 + (std::process::id() % 120) as u16 * 100;
    loop {
        let port = base + NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % 100;
        if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return port;
        }
    }
}

fn http(port: u16, method: &str, path: &str, body: Option<&str>) -> String {
    let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) else {
        return String::new();
    };
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n");
    match body {
        Some(b) => req.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{b}",
            b.len()
        )),
        None => req.push_str("\r\n"),
    }
    let _ = s.write_all(req.as_bytes());
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    out
}

#[test]
fn q_metrics_off_by_default_costs_nothing_and_404s() {
    let port = free_port();
    let _broker = Broker(
        Command::new(env!("CARGO_BIN_EXE_antares"))
            .env("ANTARES_HTTP_PORT", port.to_string())
            .spawn()
            .expect("spawn antares"),
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    while !http(port, "GET", "/q/health", None).starts_with("HTTP/1.1 200") {
        assert!(Instant::now() < deadline, "broker never got healthy");
        std::thread::sleep(Duration::from_millis(200));
    }
    let metrics = http(port, "GET", "/q/metrics", None);
    assert!(
        metrics.starts_with("HTTP/1.1 404"),
        "switch off must mean no recorder and a 404: {metrics}"
    );
}

#[test]
fn q_metrics_serves_prometheus_text_and_counters_move() {
    let port = free_port();
    let _broker = Broker(
        Command::new(env!("CARGO_BIN_EXE_antares"))
            .env("ANTARES_HTTP_PORT", port.to_string())
            .env("ANTARES_TELEMETRY", "1")
            .spawn()
            .expect("spawn antares"),
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    while !http(port, "GET", "/q/health", None).starts_with("HTTP/1.1 200") {
        assert!(Instant::now() < deadline, "broker never got healthy");
        std::thread::sleep(Duration::from_millis(200));
    }

    // traffic that must show up in the counters
    let create = http(
        port,
        "POST",
        "/ngsi-ld/v1/entities",
        Some(r#"{"id":"urn:ngsi-ld:Sensor:1","type":"Sensor","p":{"type":"Property","value":1}}"#),
    );
    assert!(create.starts_with("HTTP/1.1 201"), "create: {create}");

    let metrics = http(port, "GET", "/q/metrics", None);
    assert!(
        metrics.starts_with("HTTP/1.1 200"),
        "metrics endpoint: {metrics}"
    );
    // antares_ prefix + unit suffixes, and the request counter moved
    // (at least the create above and this scrape's own health probes).
    assert!(
        metrics.contains("antares_http_requests_total"),
        "request counter missing:\n{metrics}"
    );
    assert!(
        metrics.contains("antares_http_request_duration_seconds"),
        "duration histogram missing"
    );
    assert!(
        metrics.contains("antares_uptime_seconds") || metrics.contains("antares_draining"),
        "sampler gauges missing (uptime/draining)"
    );
    let post_count = metrics
        .lines()
        .find(|l| l.starts_with("antares_http_requests_total") && l.contains("POST"))
        .and_then(|l| l.rsplit(' ').next()?.parse::<f64>().ok())
        .unwrap_or(0.0);
    assert!(post_count >= 1.0, "POST counter did not move:\n{metrics}");
}
