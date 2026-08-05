//! F9/F10 — the 2-instance e2e: real binaries, roles split across processes,
//! one shared Postgres + one NATS. Instance A serves the API only; instance B
//! runs matcher+notifier+temporal only. A subscription and an entity created
//! through A must produce a notification matched and delivered BY B (the KV
//! mirror + outbox drain + durable consumer spine, end to end), and B's
//! recorder must materialize the temporal evolution A no longer writes.
//! Plus the §3.1 ordering-tolerance hook: events injected out of order still
//! both notify (the matcher projects no state).
//!
//! Env-gated on BOTH ANTARES_TEST_DATABASE_URL and ANTARES_TEST_NATS_URL.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

struct Broker(Child);

impl Drop for Broker {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("probe")
        .local_addr()
        .expect("addr")
        .port()
}

fn start(port: u16, roles: &str, db: &str, nats: &str) -> Broker {
    Broker(
        Command::new(env!("CARGO_BIN_EXE_antares"))
            .env("ANTARES_HTTP_PORT", port.to_string())
            .env("ANTARES_STORE", "postgres")
            .env("ANTARES_DATABASE_URL", db)
            .env("ANTARES_BUS", "nats")
            .env("ANTARES_NATS_URL", nats)
            .env("ANTARES_ROLES", roles)
            .env("ANTARES_EGRESS_ALLOW_PRIVATE", "true")
            .spawn()
            .expect("spawn antares"),
    )
}

fn wait_healthy(port: u16) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let r = http(port, "GET", "/q/health", None, None);
        if r.starts_with("HTTP/1.1 200") {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "broker on :{port} never got healthy"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Minimal HTTP/1.1 client (stdlib only, the file_mode.rs pattern).
fn http(port: u16, method: &str, path: &str, tenant: Option<&str>, body: Option<&str>) -> String {
    let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) else {
        return String::new();
    };
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n");
    if let Some(t) = tenant {
        req.push_str(&format!("NGSILD-Tenant: {t}\r\n"));
    }
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

/// Notification receiver: records every POSTed body, answers 200.
fn receiver() -> (u16, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind receiver");
    let port = listener.local_addr().expect("addr").port();
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let sink = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let sink = sink.clone();
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                // headers first
                while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    match stream.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(n) => buf.extend_from_slice(&chunk[..n]),
                        Err(_) => break,
                    }
                }
                let header_end = buf
                    .windows(4)
                    .position(|w| w == b"\r\n\r\n")
                    .map(|p| p + 4)
                    .unwrap_or(buf.len());
                let headers = String::from_utf8_lossy(&buf[..header_end]).to_string();
                let want: usize = headers
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse().unwrap_or(0))
                    })
                    .unwrap_or(0);
                while buf.len() - header_end < want {
                    match stream.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(n) => buf.extend_from_slice(&chunk[..n]),
                        Err(_) => break,
                    }
                }
                let body = String::from_utf8_lossy(&buf[header_end..]).to_string();
                sink.lock().expect("sink").push(body);
                let _ = stream.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
            });
        }
    });
    (port, seen)
}

fn wait_for<F: Fn() -> bool>(what: &str, secs: u64, f: F) {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while !f() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[test]
fn roles_split_across_two_instances_notifies_and_records() {
    let (Ok(db), Ok(nats)) = (
        std::env::var("ANTARES_TEST_DATABASE_URL"),
        std::env::var("ANTARES_TEST_NATS_URL"),
    ) else {
        eprintln!("SKIP: ANTARES_TEST_DATABASE_URL / ANTARES_TEST_NATS_URL not set");
        return;
    };
    let run = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_millis();
    let tenant = format!("e2e{run}");

    let api_port = free_port();
    let worker_port = free_port();
    let _api = start(api_port, "api", &db, &nats);
    let _worker = start(worker_port, "matcher,notifier,temporal", &db, &nats);
    wait_healthy(api_port);
    wait_healthy(worker_port);

    let (rx_port, seen) = receiver();

    // subscription created on A → KV → B's mirror
    let sub = format!(
        r#"{{"id":"urn:ngsi-ld:Subscription:e2e:{run}","type":"Subscription",
            "entities":[{{"type":"RoleSplit"}}],
            "notification":{{"endpoint":{{"uri":"http://127.0.0.1:{rx_port}/notify"}}}}}}"#
    );
    let resp = http(
        api_port,
        "POST",
        "/ngsi-ld/v1/subscriptions",
        Some(&tenant),
        Some(&sub),
    );
    assert!(resp.starts_with("HTTP/1.1 201"), "sub create: {resp}");

    // entity created on A → outbox → drain → NATS → B's matcher → receiver
    let eid = format!("urn:ngsi-ld:RoleSplit:{run}");
    let ent = format!(
        r#"{{"id":"{eid}","type":"RoleSplit","temperature":{{"type":"Property","value":21}}}}"#
    );
    let resp = http(
        api_port,
        "POST",
        "/ngsi-ld/v1/entities",
        Some(&tenant),
        Some(&ent),
    );
    assert!(resp.starts_with("HTTP/1.1 201"), "entity create: {resp}");

    wait_for("the cross-instance notification", 30, || {
        seen.lock().expect("seen").iter().any(|b| b.contains(&eid))
    });

    // temporal evolution recorded by B's recorder (A skips local recording)
    wait_for("the recorder's temporal doc", 30, || {
        http(
            api_port,
            "GET",
            &format!("/ngsi-ld/v1/temporal/entities/{eid}"),
            Some(&tenant),
            None,
        )
        .starts_with("HTTP/1.1 200")
    });

    // §3.1 ordering tolerance: v5 injected before v4 — the matcher projects
    // no state, so BOTH fire. (Direct publish, bypassing the outbox.)
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("rt");
    rt.block_on(async {
        let bus = antares_bus::nats::NatsBus::connect(&nats)
            .await
            .expect("bus");
        let mk = |version: i64| antares_bus::ChangeEvent {
            tenant: antares_model::TenantId::new(&tenant).expect("tenant"),
            entity_id: antares_model::EntityId::new(&format!("urn:ooo:{run}:{version}"))
                .expect("id"),
            types: vec!["https://uri.etsi.org/ngsi-ld/default-context/RoleSplit".into()],
            op: antares_bus::ChangeOp::Update,
            changed_attrs: vec![],
            payload: Some(serde_json::json!({
                "id": format!("urn:ooo:{run}:{version}"),
                "type": ["https://uri.etsi.org/ngsi-ld/default-context/RoleSplit"],
                "createdAt": "2026-08-05T00:00:00Z", "modifiedAt": "2026-08-05T00:00:00Z",
                "https://uri.etsi.org/ngsi-ld/default-context/temperature":
                    [{"type": "Property", "value": version}]
            })),
            prev_payload: None,
            version,
            incarnation: "2026-08-05T00:00:00Z".into(),
            seq: 9_000_000 + version,
            payload_ref: None,
            prev_payload_ref: None,
        };
        bus.publish(&mk(5)).await.expect("publish v5");
        bus.publish(&mk(4)).await.expect("publish v4 late");
    });
    wait_for("both out-of-order notifications", 30, || {
        let seen = seen.lock().expect("seen");
        seen.iter().any(|b| b.contains(&format!("urn:ooo:{run}:5")))
            && seen.iter().any(|b| b.contains(&format!("urn:ooo:{run}:4")))
    });
}
