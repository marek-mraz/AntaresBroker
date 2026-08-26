//! The 2-instance e2e: real binaries, roles split across processes,
//! one shared Postgres + one NATS. Instance A serves the API only; instance B
//! runs matcher+notifier+temporal only. A subscription and an entity created
//! through A must produce a notification matched and delivered BY B (the KV
//! mirror + outbox drain + durable consumer spine, end to end). Temporal
//! auto-recording is synchronous in A's write path (every mode),
//! so the evolution must be readable immediately through A.
//! Plus the ordering-tolerance hook: events injected out of order still
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
                // record request line + headers along with the body so tests
                // can assert WHICH endpoint/path was dialed, not only payloads
                let body = String::from_utf8_lossy(&buf[..]).to_string();
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

    // Ordering tolerance: v5 injected before v4 — the matcher projects
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

/// N≥2 api pods + N≥2 worker pods. A subscription created through api-1
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

/// Postgres arm: a change committed but not yet published survives a
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
        .block_on(antares_sql::store::pg::connect(&db, 5))
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
    // publishes them, the pod dies: exactly the state the outbox must recover from.
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

/// Decode an HTTP response body (Content-Length or chunked) from a raw
/// response string, for the few assertions that must PARSE a payload.
fn response_json(raw: &str) -> Option<serde_json::Value> {
    let (headers, body) = raw.split_once("\r\n\r\n")?;
    let body = if headers
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        let mut out = String::new();
        let mut rest = body;
        loop {
            let (size_line, tail) = rest.split_once("\r\n")?;
            let size = usize::from_str_radix(size_line.trim(), 16).ok()?;
            if size == 0 {
                break;
            }
            out.push_str(tail.get(..size)?);
            rest = tail.get(size + 2..)?; // skip chunk + CRLF
        }
        out
    } else {
        body.to_string()
    };
    serde_json::from_str(&body).ok()
}

/// The 5-role × 2-replica fleet (docker-compose-roles.yml shape): every
/// role-PAIR claim asserted with its negative —
/// duplicated consumers must not duplicate work.
///   matcher×2 + notifier×2: one change → exactly ONE notification (the four
///     pods share the "matcher" durable; a duplicate means two consumers
///     processed one change);
///   matcher pair ticking intervals: firings single-winner by row-lock claim
///     — a doubled rate means the claim is broken;
///   registry×2 present: the registration mirror stays consistent across
///     BOTH api pods (a CSR registered via api-1 makes api-2 dial the source);
///   temporal×2 present: auto-recording is synchronous in the api write path
///     — a write through either api pod records exactly ONE instance;
///   worker negative: worker pods serve ops endpoints only, never the API.
#[test]
fn role_pairs_exactly_once_semantics() {
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
    let tenant = format!("pair{run}");

    let api1 = free_port();
    let api2 = free_port();
    let _a1 = start(api1, "api", &db, &nats);
    let _a2 = start(api2, "api", &db, &nats);
    let worker_roles = [
        "matcher", "matcher", "notifier", "notifier", "temporal", "temporal", "registry",
        "registry",
    ];
    let workers: Vec<(u16, Broker)> = worker_roles
        .iter()
        .map(|r| {
            let p = free_port();
            (p, start(p, r, &db, &nats))
        })
        .collect();
    wait_healthy(api1);
    wait_healthy(api2);
    for (p, _) in &workers {
        wait_healthy(*p);
        wait_for("worker /q/ready", 30, || {
            http(*p, "GET", "/q/ready", None, None).starts_with("HTTP/1.1 200")
        });
    }

    // Negative: a worker pod must NOT serve the NGSI-LD surface
    let (m1, _) = workers[0];
    let r = http(
        m1,
        "POST",
        "/ngsi-ld/v1/entities",
        Some(&tenant),
        Some(r#"{"id":"urn:x:1","type":"X"}"#),
    );
    assert!(
        !r.contains("HTTP/1.1 2"),
        "worker pod accepted an API write: {r}"
    );

    // ---- exactly-once through matcher×2 + notifier×2 ----
    let (rx_port, seen) = receiver();
    let sub = format!(
        r#"{{"id":"urn:ngsi-ld:Subscription:pair:{run}","type":"Subscription",
            "entities":[{{"type":"PairProbe"}}],
            "notification":{{"endpoint":{{"uri":"http://127.0.0.1:{rx_port}/notify"}}}}}}"#
    );
    let resp = http(
        api1,
        "POST",
        "/ngsi-ld/v1/subscriptions",
        Some(&tenant),
        Some(&sub),
    );
    assert!(resp.starts_with("HTTP/1.1 201"), "sub: {resp}");
    let eid = format!("urn:ngsi-ld:PairProbe:{run}");
    let resp = http(
        api2,
        "POST",
        "/ngsi-ld/v1/entities",
        Some(&tenant),
        Some(&format!(
            r#"{{"id":"{eid}","type":"PairProbe","temperature":{{"type":"Property","value":21}}}}"#
        )),
    );
    assert!(resp.starts_with("HTTP/1.1 201"), "entity: {resp}");
    wait_for("the pair notification", 30, || {
        seen.lock().expect("seen").iter().any(|b| b.contains(&eid))
    });
    // settle long enough for any duplicate consumer to have fired too
    std::thread::sleep(Duration::from_secs(5));
    let hits = seen
        .lock()
        .expect("seen")
        .iter()
        .filter(|b| b.contains(&eid))
        .count();
    assert_eq!(
        hits, 1,
        "expected exactly ONE notification from the matcher/notifier pairs, got {hits}"
    );

    // ---- interval firings single-winner across the ticking pair ----
    let iv_sub_id = format!("urn:ngsi-ld:Subscription:pairiv:{run}");
    let sub = format!(
        r#"{{"id":"{iv_sub_id}","type":"Subscription","timeInterval":2,
            "entities":[{{"type":"PairProbe"}}],
            "notification":{{"endpoint":{{"uri":"http://127.0.0.1:{rx_port}/notify"}}}}}}"#
    );
    let resp = http(
        api1,
        "POST",
        "/ngsi-ld/v1/subscriptions",
        Some(&tenant),
        Some(&sub),
    );
    assert!(resp.starts_with("HTTP/1.1 201"), "interval sub: {resp}");
    std::thread::sleep(Duration::from_secs(9));
    let firings = seen
        .lock()
        .expect("seen")
        .iter()
        .filter(|b| b.contains(&iv_sub_id))
        .count();
    assert!(
        (2..=6).contains(&firings),
        "timeInterval=2 over 9 s must fire ~4 times single-winner; {firings} means \
         the pair double-fires or intervals stopped"
    );

    // ---- registration mirror consistent across BOTH api pods ----
    let (src_port, src_seen) = receiver();
    let ftype = format!("FedProbe{run}");
    let csr = format!(
        r#"{{"id":"urn:ngsi-ld:ContextSourceRegistration:pair:{run}",
            "type":"ContextSourceRegistration",
            "information":[{{"entities":[{{"type":"{ftype}"}}]}}],
            "endpoint":"http://127.0.0.1:{src_port}"}}"#
    );
    let resp = http(
        api1,
        "POST",
        "/ngsi-ld/v1/csourceRegistrations",
        Some(&tenant),
        Some(&csr),
    );
    assert!(resp.starts_with("HTTP/1.1 201"), "csr via api-1: {resp}");
    // api-2's mirror learns the CSR via the ANTARES_REGISTRY broadcast; the
    // query is retried until the dial proves it (the mirror lag is the race)
    wait_for("api-2 dials the registered source", 30, || {
        let _ = http(
            api2,
            "GET",
            &format!("/ngsi-ld/v1/entities?type={ftype}"),
            Some(&tenant),
            None,
        );
        src_seen
            .lock()
            .expect("src")
            .iter()
            .any(|b| b.contains("/entities") && b.contains(&ftype))
    });

    // ---- temporal auto-recording exactly once per write ----
    let tid = format!("urn:ngsi-ld:PairTemporal:{run}");
    let resp = http(
        api1,
        "POST",
        "/ngsi-ld/v1/entities",
        Some(&tenant),
        Some(&format!(
            r#"{{"id":"{tid}","type":"PairTemporal","temperature":{{"type":"Property","value":1}}}}"#
        )),
    );
    assert!(resp.starts_with("HTTP/1.1 201"), "temporal create: {resp}");
    let resp = http(
        api2,
        "POST",
        &format!("/ngsi-ld/v1/entities/{tid}/attrs"),
        Some(&tenant),
        Some(r#"{"temperature":{"type":"Property","value":2}}"#),
    );
    assert!(resp.starts_with("HTTP/1.1 204"), "temporal update: {resp}");
    // both instances readable immediately (synchronous recording), and
    // EXACTLY two — a third means a temporal pod double-recorded a write
    wait_for("two temporal instances, no duplicates", 30, || {
        let raw = http(
            api1,
            "GET",
            &format!("/ngsi-ld/v1/temporal/entities/{tid}"),
            Some(&tenant),
            None,
        );
        if !raw.starts_with("HTTP/1.1 200") {
            return false;
        }
        let Some(doc) = response_json(&raw) else {
            return false;
        };
        match doc.get("temperature") {
            Some(serde_json::Value::Array(a)) => a.len() == 2,
            _ => false,
        }
    });
    // negative re-check after a settle: the count must STAY 2
    std::thread::sleep(Duration::from_secs(3));
    let raw = http(
        api1,
        "GET",
        &format!("/ngsi-ld/v1/temporal/entities/{tid}"),
        Some(&tenant),
        None,
    );
    let doc = response_json(&raw).expect("temporal body parses");
    let n = doc
        .get("temperature")
        .and_then(serde_json::Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    assert_eq!(
        n, 2,
        "temporal instances must stay exactly 2 (create + update); {n} means a \
         temporal/worker pod re-recorded a write"
    );
}

// ---------- NATS outage drill ----------

/// A stoppable TCP proxy in front of the real NATS server: killing it (and
/// hard-shutting its live connections) IS the outage from the broker's
/// viewpoint; re-arming on the same port is the restart. The real NATS
/// server is never touched.
struct Proxy {
    port: u16,
    alive: Arc<std::sync::atomic::AtomicBool>,
    conns: Arc<Mutex<Vec<TcpStream>>>,
}

fn start_proxy(port: u16, upstream: String) -> Proxy {
    let alive = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let conns: Arc<Mutex<Vec<TcpStream>>> = Arc::default();
    let listener = TcpListener::bind(("127.0.0.1", port)).expect("proxy bind");
    let (a, c) = (alive.clone(), conns.clone());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            if !a.load(std::sync::atomic::Ordering::SeqCst) {
                break; // listener drops here — the port frees for a restart
            }
            let Ok(client) = stream else { continue };
            let Ok(server) = TcpStream::connect(&*upstream) else {
                continue;
            };
            {
                let mut held = c.lock().expect("conns");
                held.push(client.try_clone().expect("clone"));
                held.push(server.try_clone().expect("clone"));
            }
            let (mut cr, cw) = (client.try_clone().expect("clone"), client);
            let (mut sr, sw) = (server.try_clone().expect("clone"), server);
            let mut sw2 = sw;
            std::thread::spawn(move || {
                let _ = std::io::copy(&mut cr, &mut sw2);
                let _ = sw2.shutdown(std::net::Shutdown::Both);
            });
            let mut cw2 = cw;
            std::thread::spawn(move || {
                let _ = std::io::copy(&mut sr, &mut cw2);
                let _ = cw2.shutdown(std::net::Shutdown::Both);
            });
        }
    });
    Proxy { port, alive, conns }
}

fn kill_proxy(p: &Proxy) {
    p.alive.store(false, std::sync::atomic::Ordering::SeqCst);
    for s in p.conns.lock().expect("conns").drain(..) {
        let _ = s.shutdown(std::net::Shutdown::Both);
    }
    // unblock accept() so the listener thread exits and releases the port
    let _ = TcpStream::connect(("127.0.0.1", p.port));
}

/// Kill NATS mid-run: /q/health flips `bus.connected` to false while the
/// API keeps serving writes; on restart the client reconnects (reconnects
/// counter increments), the outbox drains what queued during the outage, and
/// subscription notifications resume — no panic, no lost event.
#[test]
fn nats_outage_flips_health_and_recovers() {
    let _serial = serial();
    let (Ok(db), Ok(nats)) = (
        std::env::var("ANTARES_TEST_DATABASE_URL"),
        std::env::var("ANTARES_TEST_NATS_URL"),
    ) else {
        eprintln!("SKIP: ANTARES_TEST_DATABASE_URL / ANTARES_TEST_NATS_URL not set");
        return;
    };
    let upstream = nats
        .trim_start_matches("nats://")
        .trim_end_matches('/')
        .to_string();
    let run = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_millis();
    let tenant = format!("outage{run}");
    let etype = format!("OutageVeh{run}");

    let proxy_port = free_port();
    let proxy = start_proxy(proxy_port, upstream.clone());
    let port = free_port();
    let _broker = start(port, "all", &db, &format!("nats://127.0.0.1:{proxy_port}"));
    wait_healthy(port);
    wait_for("health reports bus connected", 15, || {
        http(port, "GET", "/q/health", None, None).contains("\"connected\":true")
    });

    // a subscription + a first entity prove the notify chain BEFORE the outage
    let (recv_port, seen) = receiver();
    let sub = format!(
        r#"{{"id":"urn:ngsi-ld:Subscription:outage{run}","type":"Subscription","entities":[{{"type":"{etype}"}}],"notification":{{"endpoint":{{"uri":"http://127.0.0.1:{recv_port}/notify"}}}}}}"#
    );
    let r = http(
        port,
        "POST",
        "/ngsi-ld/v1/subscriptions",
        Some(&tenant),
        Some(&sub),
    );
    assert!(r.starts_with("HTTP/1.1 201"), "subscription: {r}");
    let entity = |n: u32| {
        format!(
            r#"{{"id":"urn:ngsi-ld:{etype}:{n}","type":"{etype}","speed":{{"type":"Property","value":{n}}}}}"#
        )
    };
    let r = http(
        port,
        "POST",
        "/ngsi-ld/v1/entities",
        Some(&tenant),
        Some(&entity(1)),
    );
    assert!(r.starts_with("HTTP/1.1 201"), "create 1: {r}");
    wait_for("the pre-outage notification", 20, || {
        seen.lock()
            .expect("seen")
            .iter()
            .any(|b| b.contains(":1\""))
    });

    // ---- outage ----
    kill_proxy(&proxy);
    wait_for("health flips bus.connected to false", 20, || {
        let h = http(port, "GET", "/q/health", None, None);
        h.starts_with("HTTP/1.1 200") && h.contains("\"connected\":false")
    });
    // the API keeps serving while the bus is down (the write lands in the
    // outbox; nothing may panic or 5xx)
    let r = http(
        port,
        "POST",
        "/ngsi-ld/v1/entities",
        Some(&tenant),
        Some(&entity(2)),
    );
    assert!(
        r.starts_with("HTTP/1.1 201"),
        "the API must keep serving during the outage: {r}"
    );
    assert!(
        !seen
            .lock()
            .expect("seen")
            .iter()
            .any(|b| b.contains(":2\"")),
        "no notification can arrive while the bus is down"
    );

    // ---- restart ----
    let _proxy2 = start_proxy(proxy_port, upstream);
    wait_for("reconnect visible on health", 30, || {
        let h = http(port, "GET", "/q/health", None, None);
        h.contains("\"connected\":true") && !h.contains("\"reconnects\":0")
    });
    // the outage-time write drains from the outbox — at-least-once holds
    wait_for("the outage-time notification after reconnect", 30, || {
        seen.lock()
            .expect("seen")
            .iter()
            .any(|b| b.contains(":2\""))
    });
    // and new work flows end to end again
    let r = http(
        port,
        "POST",
        "/ngsi-ld/v1/entities",
        Some(&tenant),
        Some(&entity(3)),
    );
    assert!(r.starts_with("HTTP/1.1 201"), "create 3: {r}");
    wait_for("the post-restart notification", 30, || {
        seen.lock()
            .expect("seen")
            .iter()
            .any(|b| b.contains(":3\""))
    });
    let h = http(port, "GET", "/q/health", None, None);
    assert!(h.starts_with("HTTP/1.1 200"), "no panic, still UP: {h}");
}

/// 5.2.34 cooldown across api pods (fleet regression, IOP_EXT_ERR_01_06
/// red): the per-registration cooldown stamped after a failed forward must
/// be visible to EVERY api pod — round-robin otherwise re-dials the failed
/// source from the pod that never saw the failure. The stamp rides the
/// ANTARES_REGISTRY broadcast (seconds-scale state; deliberately not
/// persisted). Negative: the source records exactly ONE dial.
#[test]
fn cooldown_stamp_is_shared_across_api_pods() {
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
    let tenant = format!("cool{run}");
    let etype = format!("CoolProbe{run}");

    // a source that ACCEPTS and never answers: every forward is
    // timeout-class, and accepted connections are the dial count
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind source");
    let src_port = listener.local_addr().expect("addr").port();
    let dials = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let n = dials.clone();
    std::thread::spawn(move || {
        let mut held = Vec::new();
        for stream in listener.incoming() {
            let Ok(s) = stream else { continue };
            n.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            held.push(s); // keep the socket open, never reply
        }
    });

    let api1 = free_port();
    let api2 = free_port();
    let _a1 = start(api1, "api", &db, &nats);
    let _a2 = start(api2, "api", &db, &nats);
    wait_healthy(api1);
    wait_healthy(api2);

    let csr = format!(
        r#"{{"id":"urn:ngsi-ld:ContextSourceRegistration:cool:{run}",
            "type":"ContextSourceRegistration",
            "information":[{{"entities":[{{"type":"{etype}"}}]}}],
            "management":{{"timeout":500,"cooldown":20000}},
            "endpoint":"http://127.0.0.1:{src_port}"}}"#
    );
    let resp = http(
        api1,
        "POST",
        "/ngsi-ld/v1/csourceRegistrations",
        Some(&tenant),
        Some(&csr),
    );
    assert!(resp.starts_with("HTTP/1.1 201"), "csr: {resp}");
    // both pods' registration mirrors must know the CSR before the queries
    std::thread::sleep(Duration::from_millis(500));

    // pod 1 dials, times out (~500 ms), stamps the cooldown
    let r = http(
        api1,
        "GET",
        &format!("/ngsi-ld/v1/entities?type={etype}"),
        Some(&tenant),
        None,
    );
    assert!(r.starts_with("HTTP/1.1 200"), "query via api1: {r}");
    wait_for("the failed dial to be recorded", 10, || {
        dials.load(std::sync::atomic::Ordering::SeqCst) == 1
    });
    std::thread::sleep(Duration::from_millis(300)); // stamp broadcast settle

    // pod 2, inside the window: must fail fast WITHOUT contacting the source
    let r = http(
        api2,
        "GET",
        &format!("/ngsi-ld/v1/entities?type={etype}"),
        Some(&tenant),
        None,
    );
    assert!(r.starts_with("HTTP/1.1 200"), "query via api2: {r}");
    std::thread::sleep(Duration::from_millis(300));
    let total = dials.load(std::sync::atomic::Ordering::SeqCst);
    assert_eq!(
        total, 1,
        "the cooldown must suppress pod 2's dial — {total} dials means the \
         stamp stayed per-process"
    );
}
