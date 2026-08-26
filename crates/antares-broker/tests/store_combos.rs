//! Current-state store × temporal store, one real broker process per row.
//! Each row writes an entity with an `observedAt` instance, patches it once,
//! then proves the current state lives ONLY in the store backend and the
//! history ONLY in the temporal backend — a shared-instance regression
//! (temporal pointed at the store instance) fails the row.
//!
//! Memory and file rows need nothing; postgres and timescale rows skip
//! loudly without ANTARES_TEST_DATABASE_URL (same rule as
//! antares-sql/tests/pg*.rs), timescale rows also without the extension.
#![cfg(unix)]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
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
    static NEXT: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(0);
    let base = 21_000 + (std::process::id() % 120) as u16 * 100;
    loop {
        let port = base + NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % 100;
        if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return port;
        }
    }
}

fn tempdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("antares-combo-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tempdir");
    dir
}

fn db_url() -> Option<String> {
    std::env::var("ANTARES_TEST_DATABASE_URL").ok()
}

fn start(port: u16, dir: &Path, store: &str, temporal: &str) -> Broker {
    start_with(port, dir, store, temporal, &[])
}

fn start_with(
    port: u16,
    dir: &Path,
    store: &str,
    temporal: &str,
    extra: &[(&str, &str)],
) -> Broker {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_antares"));
    for (k, v) in extra {
        cmd.env(k, v);
    }
    cmd.env("ANTARES_HTTP_PORT", port.to_string())
        .env("ANTARES_STORE", store)
        .env("ANTARES_TEMPORAL", temporal)
        .env("ANTARES_DATA_DIR", dir)
        .env("ANTARES_ALLOW_SHARED_LOCAL", "1")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    if let Some(url) = db_url() {
        cmd.env("ANTARES_DATABASE_URL", url);
    }
    Broker(cmd.spawn().expect("spawn antares"))
}

fn http(port: u16, request: &str) -> String {
    let mut s = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    s.write_all(request.as_bytes()).expect("write");
    let mut out = String::new();
    s.read_to_string(&mut out).expect("read");
    out
}

fn wait_healthy(port: u16) -> String {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) {
            let req = "GET /q/health HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n";
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

fn get(port: u16, path: &str) -> String {
    http(
        port,
        &format!("GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n"),
    )
}

fn send(port: u16, method: &str, path: &str, body: &str) -> String {
    http(
        port,
        &format!(
            "{method} {path} HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    )
}

fn id(row: &str) -> String {
    format!("urn:ngsi-ld:Combo:{row}")
}

/// Create with one observed instance, patch to a second one.
fn write_twice(port: u16, row: &str) {
    let id = id(row);
    let entity = format!(
        r#"{{"id":"{id}","type":"Combo","speed":{{"type":"Property","value":1,"observedAt":"2026-01-01T00:00:00Z"}}}}"#
    );
    let resp = send(port, "POST", "/ngsi-ld/v1/entities", &entity);
    assert!(resp.starts_with("HTTP/1.1 201"), "create: {resp}");
    let patch = r#"{"speed":{"type":"Property","value":2,"observedAt":"2026-01-01T00:00:01Z"}}"#;
    let resp = send(
        port,
        "PATCH",
        &format!("/ngsi-ld/v1/entities/{id}/attrs"),
        patch,
    );
    assert!(resp.starts_with("HTTP/1.1 204"), "patch: {resp}");
}

fn current_state_present(port: u16, row: &str) -> bool {
    let resp = get(port, &format!("/ngsi-ld/v1/entities/{}", id(row)));
    assert!(
        resp.starts_with("HTTP/1.1 200") || resp.starts_with("HTTP/1.1 404"),
        "retrieve: {resp}"
    );
    resp.starts_with("HTTP/1.1 200")
}

/// The history holds both observed instants, or nothing.
fn history(port: u16, row: &str) -> String {
    get(port, &format!("/ngsi-ld/v1/temporal/entities/{}", id(row)))
}

fn history_present(port: u16, row: &str) -> bool {
    let resp = history(port, row);
    let both = resp.contains("2026-01-01T00:00:00Z") && resp.contains("2026-01-01T00:00:01Z");
    assert!(
        (resp.starts_with("HTTP/1.1 200") && both) || resp.starts_with("HTTP/1.1 404"),
        "temporal retrieve must be the full history or 404: {resp}"
    );
    resp.starts_with("HTTP/1.1 200")
}

fn assert_health_names(health: &str, store: &str, temporal: &str) {
    assert!(
        health.contains(&format!(r#""store":"{store}""#)),
        "health: {health}"
    );
    assert!(
        health.contains(&format!(r#""temporal":"{temporal}""#)),
        "health: {health}"
    );
}

/// Restart-survival for the two local backends: `memory` loses on
/// `kill -9`, `file` keeps (redb committed before the ack).
fn survives(mode: &str) -> bool {
    mode == "file"
}

/// Rows whose backends are both local: write, `kill -9`, restart from the
/// same data dir, then each half is present exactly when its backend is
/// `file`.
fn local_row(store: &str, temporal: &str) {
    let row = format!("{store}-{temporal}");
    let dir = tempdir(&row);
    let port = free_port();
    let mut broker = start(port, &dir, store, temporal);
    assert_health_names(&wait_healthy(port), store, temporal);
    write_twice(port, &row);
    assert!(current_state_present(port, &row));
    assert!(history_present(port, &row));
    broker.0.kill().expect("SIGKILL");
    broker.0.wait().expect("reap");

    let _broker = start(port, &dir, store, temporal);
    wait_healthy(port);
    assert_eq!(
        current_state_present(port, &row),
        survives(store),
        "current state after kill -9 must follow the {store} store"
    );
    assert_eq!(
        history_present(port, &row),
        survives(temporal),
        "history after kill -9 must follow the {temporal} temporal backend"
    );
}

#[test]
fn memory_memory() {
    local_row("memory", "memory");
}

#[test]
fn file_file() {
    local_row("file", "file");
}

#[test]
fn memory_file() {
    local_row("memory", "file");
}

#[test]
fn file_memory() {
    local_row("file", "memory");
}

/// `none`: nothing is recorded and temporal reads answer
/// OperationNotSupported 422 (Table 6.3.2-1).
fn none_row(store: &str) {
    let row = format!("{store}-none");
    let dir = tempdir(&row);
    let port = free_port();
    let _broker = start(port, &dir, store, "none");
    assert_health_names(&wait_healthy(port), store, "none");
    write_twice(port, &row);
    assert!(current_state_present(port, &row));
    let resp = history(port, &row);
    assert!(
        resp.starts_with("HTTP/1.1 422"),
        "temporal read under none: {resp}"
    );
    assert!(resp.contains("OperationNotSupported"), "{resp}");
    assert!(!resp.contains("2026-01-01T00:00:00Z"), "{resp}");
}

#[test]
fn memory_none() {
    none_row("memory");
}

// ---- postgres / timescale rows ---------------------------------------------

/// Row counts in the two backend tables for one entity, read under the
/// default tenant's RLS scope exactly as the broker reads them.
async fn pg_rows(url: &str, row: &str) -> (i64, i64) {
    use antares_sql::sqlx;
    let pool = antares_sql::store::pg::connect(url, 2)
        .await
        .expect("connect");
    let mut tx = pool.begin().await.expect("tx");
    let tenant = antares_model::TenantId::new(antares_model::TenantId::DEFAULT).expect("tenant");
    antares_sql::store::pg::set_tenant(&mut tx, &tenant)
        .await
        .expect("set_tenant");
    let entities: i64 = sqlx::query_scalar("SELECT count(*) FROM entities WHERE id = $1")
        .bind(id(row))
        .fetch_one(&mut *tx)
        .await
        .expect("entities count");
    let instances: i64 =
        sqlx::query_scalar("SELECT count(*) FROM attr_instances WHERE entity_id = $1")
            .bind(id(row))
            .fetch_one(&mut *tx)
            .await
            .expect("instances count");
    (entities, instances)
}

fn pg_rows_blocking(url: &str, row: &str) -> (i64, i64) {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("rt")
        .block_on(pg_rows(url, row))
}

fn has_timescale(url: &str) -> bool {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("rt")
        .block_on(async {
            let pool = antares_sql::store::pg::connect(url, 2)
                .await
                .expect("connect");
            antares_sql::store::pg::maintenance::detect_temporal_backend(&pool)
                .await
                .expect("detect")
                == antares_sql::store::pg::maintenance::TemporalBackend::Hypertable
        })
}

macro_rules! require_db {
    () => {
        match db_url() {
            Some(url) => url,
            None => {
                eprintln!("SKIP: ANTARES_TEST_DATABASE_URL not set");
                return;
            }
        }
    };
}

/// Rows with a database half: the pg tables hold exactly the half that
/// backend serves; the local half is proven by `kill -9` as in `local_row`.
/// `expect_db` = (entity row present, instance rows present).
fn pg_row(url: &str, store: &str, temporal: &str) {
    let row = format!("{store}-{temporal}");
    let dir = tempdir(&row);
    let port = free_port();
    let mut broker = start(port, &dir, store, temporal);
    assert_health_names(&wait_healthy(port), store, temporal);
    write_twice(port, &row);
    assert!(current_state_present(port, &row));
    let db_store = matches!(store, "postgres" | "timescale");
    let db_temporal = matches!(temporal, "postgres" | "timescale");
    if temporal == "none" {
        let resp = history(port, &row);
        assert!(
            resp.starts_with("HTTP/1.1 422"),
            "temporal read under none: {resp}"
        );
    } else {
        assert!(history_present(port, &row));
    }
    let (entities, instances) = pg_rows_blocking(url, &row);
    assert_eq!(entities, i64::from(db_store), "entities rows for {row}");
    assert_eq!(
        instances,
        if db_temporal { 2 } else { 0 },
        "attr_instances rows for {row}"
    );

    broker.0.kill().expect("SIGKILL");
    broker.0.wait().expect("reap");
    let _broker = start(port, &dir, store, temporal);
    wait_healthy(port);
    assert_eq!(
        current_state_present(port, &row),
        db_store || survives(store),
        "current state after kill -9 must follow the {store} store"
    );
    if temporal != "none" {
        assert_eq!(
            history_present(port, &row),
            db_temporal || survives(temporal),
            "history after kill -9 must follow the {temporal} temporal backend"
        );
    }
    // leave the database as found: the entity delete mirrors into the
    // temporal half (5.6.6), so one call clears both tables.
    let resp = send(
        port,
        "DELETE",
        &format!("/ngsi-ld/v1/entities/{}", id(&row)),
        "",
    );
    assert!(
        resp.starts_with("HTTP/1.1 204") || resp.starts_with("HTTP/1.1 404"),
        "{resp}"
    );
    if resp.starts_with("HTTP/1.1 404") && db_temporal {
        // the memory entity died with the process; its history did not
        let resp = send(
            port,
            "DELETE",
            &format!("/ngsi-ld/v1/temporal/entities/{}", id(&row)),
            "",
        );
        assert!(resp.starts_with("HTTP/1.1 204"), "{resp}");
    }
    assert_eq!(
        pg_rows_blocking(url, &row),
        (0, 0),
        "rows left behind for {row}"
    );
}

#[test]
fn postgres_postgres() {
    let url = require_db!();
    pg_row(&url, "postgres", "postgres");
}

#[test]
fn memory_postgres() {
    let url = require_db!();
    pg_row(&url, "memory", "postgres");
}

#[test]
fn postgres_memory() {
    let url = require_db!();
    pg_row(&url, "postgres", "memory");
}

#[test]
fn file_postgres() {
    let url = require_db!();
    pg_row(&url, "file", "postgres");
}

#[test]
fn postgres_file() {
    let url = require_db!();
    pg_row(&url, "postgres", "file");
}

#[test]
fn postgres_none() {
    let url = require_db!();
    pg_row(&url, "postgres", "none");
}

#[test]
fn postgres_timescale() {
    let url = require_db!();
    if !has_timescale(&url) {
        eprintln!("SKIP: timescaledb extension not created in the test database");
        return;
    }
    pg_row(&url, "postgres", "timescale");
}

#[test]
fn file_timescale() {
    let url = require_db!();
    if !has_timescale(&url) {
        eprintln!("SKIP: timescaledb extension not created in the test database");
        return;
    }
    pg_row(&url, "file", "timescale");
}

/// Retention applies to the temporal half wherever it lives: a `file`
/// store with `postgres` history still gets the maintenance job, and an
/// instance older than the horizon is pruned while the entity stays.
#[test]
fn file_postgres_retention_prunes_the_temporal_half() {
    let url = require_db!();
    let row = "file-postgres-retention";
    let dir = tempdir(row);
    let port = free_port();
    let _broker = start_with(
        port,
        &dir,
        "file",
        "postgres",
        &[
            ("ANTARES_TEMPORAL_RETENTION_DAYS", "1"),
            ("ANTARES_SWEEP_SECS", "1"),
        ],
    );
    wait_healthy(port);
    write_twice(port, row);
    let deadline = Instant::now() + Duration::from_secs(20);
    let pruned = loop {
        let (entities, instances) = pg_rows_blocking(&url, row);
        assert_eq!(
            entities, 0,
            "the file store holds the entity, not the database"
        );
        if instances == 0 {
            break true;
        }
        if Instant::now() > deadline {
            break false;
        }
        std::thread::sleep(Duration::from_millis(500));
    };
    assert!(
        pruned,
        "instances older than the retention horizon were never pruned"
    );
    assert!(
        current_state_present(port, row),
        "retention must not touch current state"
    );
    let resp = send(
        port,
        "DELETE",
        &format!("/ngsi-ld/v1/entities/{}", id(row)),
        "",
    );
    assert!(resp.starts_with("HTTP/1.1 204"), "{resp}");
}
