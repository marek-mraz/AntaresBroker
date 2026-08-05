//! Antares — NGSI-LD context broker (composition root, §9.3).
//!
//! Config: ANTARES_* env vars only for v0 (antares.toml layering lands with
//! figment in phase 1). Unknown ANTARES_* keys are fatal (§14.3).

mod shutdown;
mod wiring;

use antares_api::AppState;
use antares_bus::LocalBus;

// ANTARES_DATABASE_URL: accepted — the ETSI compose wires one DB per broker —
// consumed when the phase-1 sqlx store lands (§8.2 temporal.store).
const KNOWN_KEYS: &[&str] = &[
    "ANTARES_HTTP_PORT",
    "ANTARES_HOST_ALIAS",
    "ANTARES_ROLES",
    "ANTARES_DATABASE_URL",
    "ANTARES_STORE",
    "ANTARES_DATA_DIR",
    // §16.4 egress: private-range destinations are denied by default; the
    // ETSI/IOP stacks (mock servers on localhost) set this to true.
    "ANTARES_EGRESS_ALLOW_PRIVATE",
    // C9/D4 temporal retention horizon in days; absent = keep forever (a
    // maintenance job must never default to dropping data).
    "ANTARES_TEMPORAL_RETENTION_DAYS",
    // K1 drain: the LB notice window, and the ceiling on waiting for
    // in-flight requests once the listener has closed.
    "ANTARES_DRAIN_DELAY_MS",
    "ANTARES_DRAIN_DEADLINE_SECS",
    // §16.4: optional PEM bundle of extra TLS trust anchors (private CAs,
    // incomplete-chain servers — see error.md). Never disables verification.
    "ANTARES_EXTRA_CA_FILE",
    // F1/§9.2: the bus seam. local (default, single process, all roles) or
    // nats (the JetStream spine — requires a postgres/timescale store and
    // ANTARES_NATS_URL).
    "ANTARES_BUS",
    "ANTARES_NATS_URL",
    // §10 K5: stream/KV replication factor on a clustered JetStream (3 for
    // the reference manifests' R3; default 1 for single-node).
    "ANTARES_NATS_REPLICAS",
    // F3/K9: outbox drain on this pod, on (default) | off. `off` is the
    // crash-drill lever (rows commit but this pod never publishes them —
    // another pod's drain must) and the knob for a dedicated-drainer split.
    "ANTARES_OUTBOX_DRAIN",
];

/// J7 (§6.1): jemalloc with decay-based purging — RSS returns to ~live×1.2
/// when idle; tune via MALLOC_CONF (e.g. dirty_decay_ms). glibc malloc's
/// arena fragmentation is the §2.1 anti-choice.
#[global_allocator]
static ALLOC: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    // Unknown-config-is-fatal (§14.3): catch typos before they become Scorpio's
    // $[quarkus.uuid} class of silent misconfiguration. ANTARES_TEST_* is the
    // reserved harness namespace (ANTARES_TEST_DATABASE_URL, ANTARES_TEST_MQTT_URL,
    // …) — CI exports those for the integration tests, and they land in the env of
    // any broker a test spawns. Reserving the prefix here beats making every
    // spawn site remember an env_remove allowlist.
    for (key, _) in std::env::vars() {
        if key.starts_with("ANTARES_")
            && !key.starts_with("ANTARES_TEST_")
            && !KNOWN_KEYS.contains(&key.as_str())
        {
            return Err(format!("unknown config key {key} (known: {KNOWN_KEYS:?})").into());
        }
    }

    let port: u16 = std::env::var("ANTARES_HTTP_PORT")
        .unwrap_or_else(|_| "9090".into())
        .parse()?;
    let host_alias = std::env::var("ANTARES_HOST_ALIAS").unwrap_or_else(|_| "antares".into());
    let roles = std::env::var("ANTARES_ROLES").unwrap_or_else(|_| "all".into());
    // A2: unknown store mode is fatal BEFORE the runtime spins up.
    let mode = std::env::var("ANTARES_STORE").unwrap_or_else(|_| "memory".into());
    if !["memory", "file", "postgres", "timescale"].contains(&mode.as_str()) {
        return Err(
            format!("unknown ANTARES_STORE={mode} (memory|file|postgres|timescale)").into(),
        );
    }

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let (store, store_mode) = build_store(&mode).await?;
            run(port, host_alias, roles, store, store_mode).await
        })
}

/// ANTARES_STORE → store construction (A2/A3): `file` requires
/// ANTARES_DATA_DIR (never a default inside the image, B5); postgres and
/// timescale require ANTARES_DATABASE_URL, connect ONE shared pool, run the
/// embedded migrations at start (§C-i) and serve from the Pg backend (C13).
async fn build_store(
    mode: &str,
) -> Result<(antares_sql::store::any::AnyStore, String), Box<dyn std::error::Error>> {
    use antares_sql::store::any::{AnyStore, PgBackend};
    use antares_sql::store::Store;
    match mode {
        "file" => {
            let dir = std::env::var("ANTARES_DATA_DIR").map_err(|_| {
                "ANTARES_STORE=file requires ANTARES_DATA_DIR (a mounted volume — data \
                 must never live inside the image)"
            })?;
            let dir = std::path::PathBuf::from(dir);
            warn_if_not_mount_point(&dir);
            Ok((AnyStore::Mem(Store::open_file(&dir)?), "file".into()))
        }
        "postgres" | "timescale" => {
            let url = std::env::var("ANTARES_DATABASE_URL")
                .map_err(|_| format!("ANTARES_STORE={mode} requires ANTARES_DATABASE_URL"))?;
            // The DB container may still be booting — bounded retry, then die.
            let mut last = String::new();
            for _ in 0..30 {
                match antares_sql::pg::connect(&url, 20).await {
                    Ok(pool) => {
                        let ts = antares_sql::maintenance::timescale_present(&pool).await?;
                        // D3: never silently fall back — timescale mode without
                        // the extension is a config error, not a downgrade.
                        if mode == "timescale" && !ts {
                            return Err("ANTARES_STORE=timescale but the timescaledb extension \
                                 is not CREATEd in this database — install it (CREATE EXTENSION \
                                 timescaledb) or use ANTARES_STORE=postgres"
                                .into());
                        }
                        if mode == "postgres" && ts {
                            tracing::info!(
                                "timescaledb extension detected: attr_instances runs as a \
                                 hypertable (§8.2 auto-detection); the plain-mode partition \
                                 job stands down"
                            );
                        }
                        tracing::info!(
                            "ANTARES_STORE={mode}: pool up, migrations applied, serving \
                             from postgres (temporal: {})",
                            if ts { "timescale" } else { "plain partitions" }
                        );
                        return Ok((AnyStore::Pg(PgBackend::new(pool)), mode.to_owned()));
                    }
                    Err(e) => {
                        last = e.to_string();
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    }
                }
            }
            Err(format!("ANTARES_STORE={mode}: database not reachable after 30 s: {last}").into())
        }
        _ => Ok((AnyStore::Mem(Store::default()), "memory".into())),
    }
}

/// B5: warn when the data dir shares a device with its parent — i.e. it is
/// not a mount point, so the redb file dies with the container.
#[cfg(unix)]
fn warn_if_not_mount_point(dir: &std::path::Path) {
    use std::os::unix::fs::MetadataExt;
    let _ = std::fs::create_dir_all(dir);
    if let (Ok(md), Some(Ok(parent_md))) =
        (std::fs::metadata(dir), dir.parent().map(std::fs::metadata))
    {
        if md.dev() == parent_md.dev() {
            eprintln!(
                "WARN: ANTARES_DATA_DIR {} is not a mount point — data will be lost when \
                 the container is removed",
                dir.display()
            );
        }
    }
}

#[cfg(not(unix))]
fn warn_if_not_mount_point(_dir: &std::path::Path) {}

async fn run(
    port: u16,
    host_alias: String,
    roles: String,
    store: antares_sql::store::any::AnyStore,
    store_mode: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let _bus = LocalBus::new(1024); // local-mode ring (in-process hook path)
    let roles = wiring::Roles::parse(&roles)?;
    // F1/§9.2 bus seam: local (default) or nats. Unknown value fatal (§14.3).
    let bus_mode = std::env::var("ANTARES_BUS").unwrap_or_else(|_| "local".into());
    match bus_mode.as_str() {
        "local" => {
            // §9.2: bus=local means ONE process running every role — a role
            // split without a shared bus would silently drop whole concerns.
            if !roles.all() {
                return Err(
                    "ANTARES_BUS=local requires all roles in one process (ANTARES_ROLES=all); \
                     role splits need ANTARES_BUS=nats"
                        .into(),
                );
            }
        }
        "nats" => {
            if !matches!(store_mode.as_str(), "postgres" | "timescale") {
                return Err(format!(
                    "ANTARES_BUS=nats requires a shared store (ANTARES_STORE=postgres|timescale); \
                     {store_mode} state is per-process and cannot back multiple instances"
                )
                .into());
            }
        }
        other => return Err(format!("unknown ANTARES_BUS={other} (local|nats)").into()),
    }
    tracing::info!(port, %store_mode, %bus_mode, ?roles, "starting antares");

    // Trailing-slash tolerance: Table 6.2-1 spells collection resources with a
    // trailing '/'; normalize before routing.
    let mut state = AppState::with_store(host_alias, std::sync::Arc::new(store), store_mode);
    // J7: heap stats on /q/health (allocated/resident bytes via jemalloc-ctl)
    state.mem_stats = Some(std::sync::Arc::new(|| {
        use tikv_jemalloc_ctl::{epoch, stats};
        let _ = epoch::advance();
        serde_json::json!({
            "allocatedBytes": stats::allocated::read().unwrap_or(0),
            "residentBytes": stats::resident::read().unwrap_or(0),
        })
    }));
    if bus_mode == "nats" {
        // F1..F8: outbox producer + drain, KV/registry mirrors, durable
        // consumers per role, topology asserted before traffic (F7).
        let url = std::env::var("ANTARES_NATS_URL")
            .map_err(|_| "ANTARES_BUS=nats requires ANTARES_NATS_URL")?;
        wiring::wire_nats(&mut state, &url, roles).await?;
    } else {
        antares_api::notify::wire(&mut state); // in-process matcher + notifier + interval firing
    }

    // J2: Cached-@context write-through + boot preload — fetched contexts
    // are persisted as kind='Cached' rows and reloaded on start, so the
    // parsed-context cache and the 5.13 listing survive a restart.
    {
        let store = state.store.clone();
        state
            .loader
            .set_cache_writer(Box::new(move |url, ctx_value| {
                let id = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, url.as_bytes());
                let doc = serde_json::json!({
                    "url": url,
                    "localId": id.to_string(),
                    "kind": "Cached",
                    "createdAt": antares_api::state::now_iso(),
                    "body": {"@context": ctx_value},
                });
                if let Err(e) = store.context_put(&id.to_string(), doc) {
                    tracing::warn!("@context write-through failed for {url}: {e}");
                }
            }));
        for row in state.store.context_list().unwrap_or_default() {
            if row.get("kind").and_then(|v| v.as_str()) != Some("Cached") {
                continue;
            }
            let (Some(url), Some(id), Some(created)) = (
                row.get("url").and_then(|v| v.as_str()),
                row.get("localId").and_then(|v| v.as_str()),
                row.get("createdAt").and_then(|v| v.as_str()),
            ) else {
                continue;
            };
            if let Some(v) = row.pointer("/body/@context") {
                state.loader.seed_cached(url, id, created, v.clone()).await;
            }
        }
    }

    // C9/D4: temporal maintenance — plain-mode partition pre-creation and the
    // (opt-in) retention horizon, single-winner via SKIP LOCKED (§3.1.6).
    if let antares_sql::store::any::AnyStore::Pg(p) = state.store.as_ref() {
        let pool = p.docs.pool().clone();
        let retention: Option<i64> = std::env::var("ANTARES_TEMPORAL_RETENTION_DAYS")
            .ok()
            .map(|v| {
                v.parse()
                    .map_err(|_| "ANTARES_TEMPORAL_RETENTION_DAYS must be an integer")
            })
            .transpose()?;
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(15 * 60));
            loop {
                tick.tick().await; // first tick is immediate: partitions at boot
                match antares_sql::maintenance::temporal_maintenance(&pool, retention).await {
                    Ok(msg) => tracing::debug!("temporal maintenance: {msg}"),
                    Err(e) => tracing::warn!("temporal maintenance failed: {e}"),
                }
            }
        });
    }
    // K1: handles the drain needs, taken before `state` is consumed by the
    // router — the flag the health endpoint reads, and the store whose pools
    // close last.
    let draining = state.draining.clone();
    let store_for_drain = state.store.clone();
    let app = tower::Layer::layer(
        &tower_http::normalize_path::NormalizePathLayer::trim_trailing_slash(),
        antares_api::router(state),
    );

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;
    tracing::info!("listening on http://0.0.0.0:{port}");
    // K1: count open connections so the drain can wait for them. Incremented
    // before the task is spawned — incrementing inside the task would race the
    // drain's first check and let a just-accepted connection be missed.
    let inflight = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    // K1: the signal future is created ONCE and polled by reference. Written
    // inline in the select, it would be dropped and re-created on every
    // accepted connection — and a SIGTERM landing in that drop-to-recreate
    // window is lost for good (tokio signal streams do not replay events from
    // before their creation). Under health-check polling that window is hit
    // constantly, which is exactly how the drain test caught it.
    let mut sigterm = std::pin::pin!(shutdown::signal());
    // Manual serve loop: the ETSI suite reads response headers case-sensitively
    // ("Location"), so HTTP/1 responses are written with title-case headers.
    loop {
        let (stream, _) = tokio::select! {
            r = listener.accept() => r?,
            _ = &mut sigterm => {
                // 1+2: unhealthy FIRST, then keep serving for the LB's notice
                // window — still inside this select, so connections arriving
                // during it are accepted normally.
                shutdown::begin(&draining);
                let until = tokio::time::Instant::now() + shutdown::drain_delay();
                loop {
                    tokio::select! {
                        r = listener.accept() => {
                            let (stream, _) = r?;
                            serve(stream, app.clone(), inflight.clone());
                        }
                        _ = tokio::time::sleep_until(until) => break,
                    }
                }
                // 3–6: listener dropped, in-flight drained, pools closed.
                drop(listener);
                shutdown::drain(&inflight, &store_for_drain).await;
                tracing::info!("shutting down");
                return Ok(());
            }
        };
        serve(stream, app.clone(), inflight.clone());
    }
}

/// The served app: the router under trailing-slash normalization.
type App = tower_http::normalize_path::NormalizePath<axum::Router>;

/// One accepted connection. Split out of the accept loop so the drain's
/// notice window serves connections with identical behaviour, and so the
/// in-flight counter is incremented in exactly one place.
fn serve(
    stream: tokio::net::TcpStream,
    app: App,
    inflight: std::sync::Arc<std::sync::atomic::AtomicUsize>,
) {
    inflight.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    tokio::spawn(async move {
        let svc = hyper::service::service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
            let mut app = app.clone();
            async move { tower::Service::call(&mut app, req.map(axum::body::Body::new)).await }
        });
        let mut builder =
            hyper_util::server::conn::auto::Builder::new(hyper_util::rt::TokioExecutor::new());
        builder.http1().title_case_headers(true);
        let _ = builder
            .serve_connection(hyper_util::rt::TokioIo::new(stream), svc)
            .await;
        inflight.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    });
}
