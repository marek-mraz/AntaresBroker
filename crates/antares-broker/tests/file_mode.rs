//! `file` mode e2e: the real binary, real HTTP, real
//! SIGKILL. Commit-before-ack means an entity acknowledged with 201 MUST
//! survive an immediate `kill -9`; deletes must reach redb too (the Scorpio
//! phantom-409 trap). std-only harness — no test frameworks.
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

/// Kills the child on drop, so a failed assert never leaks a broker process
/// into the test host (the drain test found this the hard way: its panic left
/// a live broker answering health checks half an hour later).
struct Broker(Child);

impl Drop for Broker {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl std::ops::Deref for Broker {
    type Target = Child;
    fn deref(&self) -> &Child {
        &self.0
    }
}

impl std::ops::DerefMut for Broker {
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.0
    }
}

fn start(port: u16, dir: &Path, store: &str) -> Broker {
    // Harness vars (ANTARES_TEST_*) are inherited from CI but are not broker
    // config; the unknown-key guard reserves that prefix, so no env_remove
    // allowlist to keep in sync here.
    Broker(
        Command::new(env!("CARGO_BIN_EXE_antares"))
            .env("ANTARES_HTTP_PORT", port.to_string())
            .env("ANTARES_STORE", store)
            .env("ANTARES_DATA_DIR", dir)
            .spawn()
            .expect("spawn antares"),
    )
}

fn wait_healthy(port: u16) -> String {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) {
            let req = "GET /q/health HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n".to_string();
            if s.write_all(req.as_bytes()).is_ok() {
                let mut out = String::new();
                let _ = s.read_to_string(&mut out);
                if out.contains("200") {
                    return out;
                }
            }
        }
        assert!(
            Instant::now() < deadline,
            "broker on :{port} never got healthy"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Ask the OS for a free port — racing other parallel test binaries on a
/// pid-derived port was flaky.
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("probe")
        .local_addr()
        .expect("addr")
        .port()
}

fn tempdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("antares-e2e-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tempdir");
    dir
}

/// Memory arm: SIGKILL loses EVERYTHING and that is the documented
/// contract of the rung — asserted so the mode's limits stay honest instead
/// of drifting into an assumed durability nobody built.
#[test]
fn memory_mode_sigkill_loses_everything_by_contract() {
    let dir = tempdir("k9mem"); // unused by the store; start() wants a path
    let port = free_port();
    let entity = r#"{"id":"urn:ngsi-ld:Test:k9mem","type":"Test"}"#;
    let mut broker = start(port, &dir, "memory");
    wait_healthy(port);
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

    let _broker = start(port, &dir, "memory");
    wait_healthy(port);
    let resp = http(
        port,
        "GET /ngsi-ld/v1/entities/urn:ngsi-ld:Test:k9mem HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    );
    assert!(
        resp.starts_with("HTTP/1.1 404"),
        "memory mode must lose unpersisted state on kill — anything else means \
         the mode table is lying: {resp}"
    );
}

#[test]
fn kill_dash_nine_right_after_201_loses_nothing() {
    let dir = tempdir("kill9");
    // Ask the OS for a free port (racing other parallel test binaries on a
    // pid-derived port was flaky).
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .expect("probe")
        .local_addr()
        .expect("addr")
        .port();
    let entity = r#"{"id":"urn:ngsi-ld:Test:kill9","type":"Test"}"#;

    // 1. create, get the 201 ack, SIGKILL immediately — no drain, no grace.
    let mut broker = start(port, &dir, "file");
    let health = wait_healthy(port);
    assert!(health.contains(r#""store":"file""#), "health: {health}");
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

    // 2. restart from the same volume: the acked write is there.
    let mut broker = start(port, &dir, "file");
    wait_healthy(port);
    let resp = http(
        port,
        "GET /ngsi-ld/v1/entities/urn:ngsi-ld:Test:kill9 HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    );
    assert!(
        resp.starts_with("HTTP/1.1 200"),
        "retrieve after restart: {resp}"
    );
    assert!(resp.contains("urn:ngsi-ld:Test:kill9"));

    // 3. delete, SIGKILL, restart: the delete reached redb — recreate gets
    //    201, not a phantom 409.
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
    assert!(
        resp.starts_with("HTTP/1.1 404"),
        "deleted stays deleted: {resp}"
    );
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

/// Redb takes an EXCLUSIVE file lock, so `file` mode is
/// single-instance by construction — which is exactly why the manifests pin
/// `strategy: Recreate` for it and never RollingUpdate. The drill asserts the
/// refusal IS the contract: the second process dies with a nameable error,
/// and the FIRST one keeps serving. Never silent corruption, never two
/// writers sharing a volume.
#[test]
fn double_start_refuses_the_lock_instead_of_corrupting() {
    let dir = tempdir("doublestart");
    let port = free_port();
    let mut first = start(port, &dir, "file");
    wait_healthy(port);

    // second process, same data dir, different port: must refuse to start
    let out = Command::new(env!("CARGO_BIN_EXE_antares"))
        .env("ANTARES_HTTP_PORT", free_port().to_string())
        .env("ANTARES_STORE", "file")
        .env("ANTARES_DATA_DIR", &dir)
        .output()
        .expect("run second broker");
    assert!(
        !out.status.success(),
        "a second broker on the same volume must NOT start"
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("Cannot acquire lock"),
        "the refusal must name the lock, not fail obscurely: {err}"
    );

    // the incumbent is untouched — still serving, still writable
    let entity = r#"{"id":"urn:ngsi-ld:Test:lock1","type":"Test"}"#;
    let resp = http(
        port,
        &format!(
            "POST /ngsi-ld/v1/entities HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{entity}",
            entity.len()
        ),
    );
    assert!(
        resp.starts_with("HTTP/1.1 201"),
        "incumbent still writable after the refused start: {resp}"
    );

    first.kill().expect("cleanup kill");
    first.wait().expect("cleanup reap");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Because `file` mode can only ever be recreated (never
/// rolled), the restart gap IS the downtime — so it has to be bounded rather
/// than assumed. Boot rebuild scans the redb tables back into the in-memory
/// maps at a measured 257k entities/s, which puts the gap at process start
/// rather than at the scan.
///
/// Scale note: the gate was first written as "<2 s at 100k entities", but
/// `file` measures ~19 KB RSS per entity, so 100k is ~1.9 GB — past the mode's
/// own documented ~10k ceiling and past the 350 MiB gate. The drill therefore
/// runs at that documented ceiling and reports the implied rate, which is the
/// honest measurement; 100k belongs to a `postgres` box, not to this rung.
#[test]
fn restart_gap_stays_under_the_gate() {
    const SEED: usize = 10_000;
    const BATCH: usize = 1_000; // = MAX_BATCH_ITEMS (the batch bounds wall)

    let dir = tempdir("restartgap");
    let port = free_port();
    let mut broker = start(port, &dir, "file");
    wait_healthy(port);

    for chunk in 0..(SEED / BATCH) {
        let entities: Vec<String> = (0..BATCH)
            .map(|i| {
                let n = chunk * BATCH + i;
                format!(r#"{{"id":"urn:ngsi-ld:Test:gap{n}","type":"Test"}}"#)
            })
            .collect();
        let body = format!("[{}]", entities.join(","));
        let resp = http(
            port,
            &format!(
                "POST /ngsi-ld/v1/entityOperations/create HTTP/1.1\r\nHost: x\r\n\
                 Content-Type: application/json\r\nContent-Length: {}\r\n\
                 Connection: close\r\n\r\n{body}",
                body.len()
            ),
        );
        assert!(
            resp.starts_with("HTTP/1.1 201"),
            "batch {chunk} create: {}",
            &resp[..resp.len().min(120)]
        );
    }

    // Restart cold and time the gap: SIGKILL, so nothing is flushed on the
    // way out and the rebuild has to do the real work.
    broker.kill().expect("SIGKILL");
    broker.wait().expect("reap");
    let t0 = Instant::now();
    let mut broker = start(port, &dir, "file");
    wait_healthy(port);
    let gap = t0.elapsed();

    // the rebuild is correct, not merely fast
    let resp = http(
        port,
        &format!(
            "GET /ngsi-ld/v1/entities/urn:ngsi-ld:Test:gap{} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
            SEED - 1
        ),
    );
    assert!(
        resp.starts_with("HTTP/1.1 200"),
        "last seeded entity survived the restart: {}",
        &resp[..resp.len().min(120)]
    );

    println!(
        "restart gap: {:?} for {SEED} entities ({:.0} entities/s implied)",
        gap,
        SEED as f64 / gap.as_secs_f64()
    );
    assert!(
        gap < Duration::from_secs(2),
        "restart gap {gap:?} exceeds the 2 s gate at {SEED} entities"
    );

    broker.kill().expect("cleanup kill");
    broker.wait().expect("cleanup reap");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The drain ORDER, which is the whole feature. A load balancer only
/// learns an instance is going away by polling `/q/health`, so health must go
/// 503 while the socket STILL WORKS — flipping it at the same moment the
/// listener closes is what turns a rolling update into connection-refused.
/// The window is widened here so the assertion is about ordering, not timing.
#[test]
fn sigterm_flips_health_before_it_closes_the_socket() {
    let dir = tempdir("drain");
    let port = free_port();
    let mut broker = Broker(
        Command::new(env!("CARGO_BIN_EXE_antares"))
            .env("ANTARES_HTTP_PORT", port.to_string())
            .env("ANTARES_STORE", "memory")
            .env("ANTARES_DATA_DIR", &dir)
            .env("ANTARES_DRAIN_DELAY_MS", "2000")
            .spawn()
            .expect("spawn antares"),
    );
    let health = wait_healthy(port);
    assert!(health.contains(r#""status":"UP""#), "before: {health}");

    // `kill` via the shell builtin — this box ships no /bin/kill binary
    // (same family as its missing pgrep/pkill).
    let killed = Command::new("sh")
        .args(["-c", &format!("kill -TERM {}", broker.id())])
        .status()
        .expect("kill -TERM");
    assert!(killed.success(), "SIGTERM not delivered");

    // inside the notice window: unhealthy, but still accepting and serving
    std::thread::sleep(Duration::from_millis(300));
    let resp = http(
        port,
        "GET /q/health HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    );
    assert!(
        resp.starts_with("HTTP/1.1 503"),
        "draining must be 503 so the LB ejects it — a 200 keeps traffic coming: {resp}"
    );
    assert!(
        resp.contains(r#""status":"DRAINING""#),
        "drain body: {resp}"
    );

    // and real API traffic still completes during the window
    let entity = r#"{"id":"urn:ngsi-ld:Test:drain1","type":"Test"}"#;
    let resp = http(
        port,
        &format!(
            "POST /ngsi-ld/v1/entities HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{entity}",
            entity.len()
        ),
    );
    assert!(
        resp.starts_with("HTTP/1.1 201"),
        "in-flight traffic must still be served while draining: {resp}"
    );

    // then it exits cleanly and on its own — no kill needed
    let deadline = Instant::now() + Duration::from_secs(25);
    let status = loop {
        if let Some(s) = broker.try_wait().expect("try_wait") {
            break s;
        }
        assert!(
            Instant::now() < deadline,
            "broker never exited after SIGTERM"
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(
        status.success(),
        "graceful shutdown must exit 0: {status:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unknown_store_mode_is_fatal() {
    let dir = tempdir("badmode");
    let status = start(23999, &dir, "bogus").wait().expect("wait");
    assert!(
        !status.success(),
        "unknown ANTARES_STORE must be fatal"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// An unknown ANTARES_* key is fatal, but ANTARES_TEST_* is the reserved
/// harness namespace. CI exports ANTARES_TEST_DATABASE_URL / ANTARES_TEST_MQTT_URL
/// for the integration tests and they land in the env of every broker a test
/// spawns — reserving the prefix is what keeps that from killing the process.
/// Both halves are asserted here so neither can regress alone: a typo must still
/// be caught, and a harness var must still be ignored.
#[test]
fn unknown_key_is_fatal_but_the_test_prefix_is_reserved() {
    let dir = tempdir("badkey");

    let typo = Command::new(env!("CARGO_BIN_EXE_antares"))
        .env("ANTARES_HTTP_PROT", "9090") // transposed — the Scorpio typo class
        .env("ANTARES_STORE", "bogus")
        .env("ANTARES_DATA_DIR", &dir)
        .output()
        .expect("run broker with a typo'd key");
    let err = String::from_utf8_lossy(&typo.stderr);
    assert!(
        err.contains("unknown config key ANTARES_HTTP_PROT"),
        "a typo'd ANTARES_* key must be fatal and name itself: {err}"
    );

    // Same run, but the only extra var is a harness one: the guard must fall
    // through to the store check rather than dying on the env var.
    let harness = Command::new(env!("CARGO_BIN_EXE_antares"))
        .env("ANTARES_TEST_MQTT_URL", "mqtt://127.0.0.1:1883")
        .env("ANTARES_TEST_DATABASE_URL", "postgresql://x/y")
        .env("ANTARES_STORE", "bogus")
        .env("ANTARES_DATA_DIR", &dir)
        .output()
        .expect("run broker with harness vars");
    let err = String::from_utf8_lossy(&harness.stderr);
    assert!(
        !err.contains("unknown config key"),
        "ANTARES_TEST_* is reserved for the harness, not broker config: {err}"
    );
    assert!(
        err.contains("unknown ANTARES_STORE"),
        "the guard must fall through to the store check: {err}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// The drain must wait on ACTIVE REQUESTS, not on
/// open connections. A load balancer holds idle keep-alive connections to
/// every backend; if those count as in-flight, every api pod burns the FULL
/// drain deadline on every roll doing nothing (measured: the fleet's api
/// rolls always paid the whole ceiling behind haproxy). An idle keep-alive
/// connection must not delay shutdown; a request arriving during the notice
/// window is still served (the ordering test above pins that half).
#[test]
fn idle_keepalive_connection_does_not_stall_the_drain() {
    let dir = tempdir("drain-idle");
    let port = free_port();
    let mut broker = Broker(
        Command::new(env!("CARGO_BIN_EXE_antares"))
            .env("ANTARES_HTTP_PORT", port.to_string())
            .env("ANTARES_STORE", "memory")
            .env("ANTARES_DATA_DIR", &dir)
            // far above the assertion bound: red = the drain waited it out
            .env("ANTARES_DRAIN_DEADLINE_SECS", "15")
            .spawn()
            .expect("spawn antares"),
    );
    wait_healthy(port);

    // an idle keep-alive connection, exactly like haproxy holds to every
    // backend: one served request, socket deliberately kept open
    let mut idle = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    idle.write_all(b"GET /q/health HTTP/1.1\r\nHost: x\r\n\r\n")
        .expect("write");
    let mut buf = [0u8; 4096];
    let n = idle.read(&mut buf).expect("read response");
    assert!(
        n > 0,
        "keep-alive request must be answered before the drain"
    );

    let t0 = Instant::now();
    let killed = Command::new("sh")
        .args(["-c", &format!("kill -TERM {}", broker.id())])
        .status()
        .expect("kill -TERM");
    assert!(killed.success(), "SIGTERM not delivered");

    let deadline = Instant::now() + Duration::from_secs(20);
    let status = loop {
        if let Some(s) = broker.try_wait().expect("try_wait") {
            break s;
        }
        assert!(
            Instant::now() < deadline,
            "broker never exited after SIGTERM"
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    let took = t0.elapsed();
    assert!(
        status.success(),
        "graceful shutdown must exit 0: {status:?}"
    );
    assert!(
        took < Duration::from_secs(6),
        "drain took {took:?} — an idle keep-alive connection stalled it to the deadline"
    );
    drop(idle);
    let _ = std::fs::remove_dir_all(&dir);
}
