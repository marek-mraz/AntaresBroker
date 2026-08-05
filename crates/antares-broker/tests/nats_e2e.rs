//! F9/F10 — the 2-instance e2e: real binaries, roles split across processes,
//! one shared Postgres + one NATS. Instance A serves the API only; instance B
//! runs matcher+notifier+temporal only. A subscription and an entity created
//! through A must produce a notification matched and delivered BY B (the KV
//! mirror + outbox drain + durable consumer spine, end to end). Temporal
//! auto-recording is synchronous in A's write path (every mode — K8 lesson),
//! so the evolution must be readable immediately through A.
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
    start_env(port, roles, db, nats, &[])
}

fn start_env(port: u16, roles: &str, db: &str, nats: &str, extra: &[(&str, &str)]) -> Broker {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_antares"));
    cmd.env("ANTARES_HTTP_PORT", port.to_string())
        .env("ANTARES_STORE", "postgres")
        .env("ANTARES_DATABASE_URL", db)
        .env("ANTARES_BUS", "nats")
        .env("ANTARES_NATS_URL", nats)
        .env("ANTARES_ROLES", roles)
        .env("ANTARES_EGRESS_ALLOW_PRIVATE", "true");
    for (k, v) in extra {
        cmd.env(k, v);
    }
    Broker(cmd.spawn().expect("spawn antares"))
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

/// One test at a time: the tests share one database and one NATS, and a
/// sibling test's api pod (drain ON) would legitimately publish the sigkill
/// drill's deliberately-unpublished outbox rows — the product recovering
/// rows is exactly what makes the parallel run flaky.
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

#[test]
fn roles_split_across_two_instances_notifies_and_records() {
    let _serial = serial();
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

    // temporal evolution recorded synchronously in A's write path
    wait_for("the temporal doc", 30, || {
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

/// K4: N≥2 api pods + N≥2 worker pods. A subscription created through api-1
/// matches an entity created through api-2 (any pod, one broker), and after
/// one worker dies with SIGKILL the shared durable rebalances — notifications
/// keep flowing without it.
#[test]
fn api_pods_interchangeable_and_worker_group_survives_a_kill() {
    let _serial = serial();
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
    let tenant = format!("k4x{run}");

    let api1 = free_port();
    let api2 = free_port();
    let w1 = free_port();
    let w2 = free_port();
    let _a1 = start(api1, "api", &db, &nats);
    let _a2 = start(api2, "api", &db, &nats);
    let mut worker1 = start(w1, "matcher,notifier,temporal", &db, &nats);
    let _worker2 = start(w2, "matcher,notifier,temporal", &db, &nats);
    for p in [api1, api2, w1, w2] {
        wait_healthy(p);
    }

    let (rx_port, seen) = receiver();
    let sub = format!(
        r#"{{"id":"urn:ngsi-ld:Subscription:k4:{run}","type":"Subscription",
            "entities":[{{"type":"K4Probe"}}],
            "notification":{{"endpoint":{{"uri":"http://127.0.0.1:{rx_port}/notify"}}}}}}"#
    );
    let resp = http(
        api1,
        "POST",
        "/ngsi-ld/v1/subscriptions",
        Some(&tenant),
        Some(&sub),
    );
    assert!(resp.starts_with("HTTP/1.1 201"), "sub via api-1: {resp}");

    // entity through THE OTHER api pod — pods must be interchangeable
    let e1 = format!("urn:ngsi-ld:K4Probe:{run}:1");
    let resp = http(
        api2,
        "POST",
        "/ngsi-ld/v1/entities",
        Some(&tenant),
        Some(&format!(
            r#"{{"id":"{e1}","type":"K4Probe","temperature":{{"type":"Property","value":1}}}}"#
        )),
    );
    assert!(resp.starts_with("HTTP/1.1 201"), "entity via api-2: {resp}");
    wait_for("notification for the cross-api entity", 30, || {
        seen.lock().expect("seen").iter().any(|b| b.contains(&e1))
    });

    // kill one worker ungracefully; the durable's share must rebalance
    worker1.0.kill().expect("SIGKILL worker-1");
    let _ = worker1.0.wait();
    let e2 = format!("urn:ngsi-ld:K4Probe:{run}:2");
    let resp = http(
        api1,
        "POST",
        "/ngsi-ld/v1/entities",
        Some(&tenant),
        Some(&format!(
            r#"{{"id":"{e2}","type":"K4Probe","temperature":{{"type":"Property","value":2}}}}"#
        )),
    );
    assert!(
        resp.starts_with("HTTP/1.1 201"),
        "entity after kill: {resp}"
    );
    wait_for("notification with one worker dead", 60, || {
        seen.lock().expect("seen").iter().any(|b| b.contains(&e2))
    });
}

/// K9 (postgres arm): a change committed but not yet published survives a
/// SIGKILL — the outbox republishes it on restart. The kill loop retries
/// until it catches the drain with rows still pending (outbox_peek > 0 at
/// the moment of death), so the assertion is about the crash window itself,
/// never about winning a race.
#[test]
fn sigkill_between_commit_and_publish_republishes_from_outbox() {
    let _serial = serial();
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
    let tenant = format!("k9pg{run}");

    // a store handle of our own, to observe the outbox table
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("rt");
    let pool = rt
        .block_on(antares_sql::pg::connect(&db, 5))
        .expect("connect");
    let store =
        antares_sql::store::any::AnyStore::Pg(antares_sql::store::any::PgBackend::new(pool));

    let worker_port = free_port();
    let _worker = start(worker_port, "matcher,notifier,temporal", &db, &nats);
    wait_healthy(worker_port);
    let (rx_port, seen) = receiver();

    // The victim pod's own drain is OFF — the deterministic stand-in for a
    // crash in the commit→publish window (with the drain nudge the live race
    // is ~1 ms wide and unwinnable from outside). Rows commit, nothing
    // publishes them, the pod dies: exactly the state F3 must recover from.
    let api_port = free_port();
    let mut api = start_env(
        api_port,
        "api",
        &db,
        &nats,
        &[("ANTARES_OUTBOX_DRAIN", "off")],
    );
    wait_healthy(api_port);
    let sub = format!(
        r#"{{"id":"urn:ngsi-ld:Subscription:k9:{run}","type":"Subscription",
            "entities":[{{"type":"K9Probe"}}],
            "notification":{{"endpoint":{{"uri":"http://127.0.0.1:{rx_port}/notify"}}}}}}"#
    );
    let resp = http(
        api_port,
        "POST",
        "/ngsi-ld/v1/subscriptions",
        Some(&tenant),
        Some(&sub),
    );
    assert!(resp.starts_with("HTTP/1.1 201"), "sub: {resp}");
    // burst of acked writes, then die with every event still unpublished
    let caught: Vec<String> = (0..20)
        .map(|i| format!("urn:ngsi-ld:K9Probe:{run}:0:{i}"))
        .collect();
    for id in &caught {
        let resp = http(
            api_port,
            "POST",
            "/ngsi-ld/v1/entities",
            Some(&tenant),
            Some(&format!(
                r#"{{"id":"{id}","type":"K9Probe","temperature":{{"type":"Property","value":9}}}}"#
            )),
        );
        assert!(resp.starts_with("HTTP/1.1 201"), "create {id}: {resp}");
    }
    api.0.kill().expect("SIGKILL api");
    let _ = api.0.wait();
    let pending = store.outbox_peek(1000).expect("peek").len();
    assert!(
        pending >= caught.len(),
        "expected every acked write unpublished at death, found {pending}"
    );
    eprintln!("caught {pending} unpublished rows at death");

    // a fresh api pod restarts the drain: every acked write must notify
    let api_port = free_port();
    let _api = start(api_port, "api", &db, &nats);
    wait_healthy(api_port);
    wait_for("all caught writes republished from the outbox", 60, || {
        let seen = seen.lock().expect("seen");
        caught.iter().all(|id| seen.iter().any(|b| b.contains(id)))
    });
    // and the outbox drains to empty — nothing wedged
    wait_for("outbox drained", 30, || {
        store.outbox_peek(1).expect("peek").is_empty()
    });
}
