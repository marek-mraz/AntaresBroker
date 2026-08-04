//! Antares — NGSI-LD context broker (composition root, §9.3).
//!
//! Config: ANTARES_* env vars only for v0 (antares.toml layering lands with
//! figment in phase 1). Unknown ANTARES_* keys are fatal (§14.3).

use antares_api::AppState;
use antares_bus::LocalBus;
use std::time::Instant;

// ANTARES_DATABASE_URL: accepted — the ETSI compose wires one DB per broker —
// consumed when the phase-1 sqlx store lands (§8.2 temporal.store).
const KNOWN_KEYS: &[&str] = &[
    "ANTARES_HTTP_PORT",
    "ANTARES_HOST_ALIAS",
    "ANTARES_ROLES",
    "ANTARES_DATABASE_URL",
    "ANTARES_STORE",
    "ANTARES_DATA_DIR",
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    // Unknown-config-is-fatal (§14.3): catch typos before they become Scorpio's
    // $[quarkus.uuid} class of silent misconfiguration.
    for (key, _) in std::env::vars() {
        if key.starts_with("ANTARES_") && !KNOWN_KEYS.contains(&key.as_str()) {
            return Err(format!("unknown config key {key} (known: {KNOWN_KEYS:?})").into());
        }
    }

    let port: u16 = std::env::var("ANTARES_HTTP_PORT")
        .unwrap_or_else(|_| "9090".into())
        .parse()?;
    let host_alias = std::env::var("ANTARES_HOST_ALIAS").unwrap_or_else(|_| "antares".into());
    let roles = std::env::var("ANTARES_ROLES").unwrap_or_else(|_| "all".into());
    let (store, store_mode) = build_store()?;

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(run(port, host_alias, roles, store, store_mode))
}

/// ANTARES_STORE → store construction (A2/A3): unknown value fatal; `file`
/// requires ANTARES_DATA_DIR (never a default inside the image, B5);
/// postgres/timescale accepted for the CI matrix but their backends land with
/// tasks.md §C/§D — until then they run the in-memory store, loudly.
fn build_store() -> Result<(antares_sql::store::Store, String), String> {
    let mode = std::env::var("ANTARES_STORE").unwrap_or_else(|_| "memory".into());
    match mode.as_str() {
        "memory" => Ok((antares_sql::store::Store::default(), "memory".into())),
        "file" => {
            let dir = std::env::var("ANTARES_DATA_DIR").map_err(|_| {
                "ANTARES_STORE=file requires ANTARES_DATA_DIR (a mounted volume — data \
                 must never live inside the image)"
                    .to_owned()
            })?;
            let dir = std::path::PathBuf::from(dir);
            warn_if_not_mount_point(&dir);
            Ok((antares_sql::store::Store::open_file(&dir)?, "file".into()))
        }
        "postgres" | "timescale" => {
            eprintln!(
                "WARN: ANTARES_STORE={mode} backend is not implemented yet (tasks.md §C/§D) — \
                 running the in-memory store"
            );
            Ok((antares_sql::store::Store::default(), "memory".into()))
        }
        other => Err(format!(
            "unknown ANTARES_STORE={other} (memory|file|postgres|timescale)"
        )),
    }
}

/// B5: warn when the data dir shares a device with its parent — i.e. it is
/// not a mount point, so the redb file dies with the container.
#[cfg(unix)]
fn warn_if_not_mount_point(dir: &std::path::Path) {
    use std::os::unix::fs::MetadataExt;
    let _ = std::fs::create_dir_all(dir);
    if let (Ok(md), Some(Ok(parent_md))) = (
        std::fs::metadata(dir),
        dir.parent().map(std::fs::metadata),
    ) {
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
    store: antares_sql::store::Store,
    store_mode: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let _bus = LocalBus::new(1024); // consumers attach as phases land
    tracing::info!(port, %host_alias, %roles, %store_mode, "starting antares (v0 skeleton)");

    // Trailing-slash tolerance: Table 6.2-1 spells collection resources with a
    // trailing '/'; normalize before routing.
    let state = AppState::with_store(host_alias, std::sync::Arc::new(store), store_mode);
    antares_api::notify::wire(&state); // matcher + notifier + interval firing
    let app = tower::Layer::layer(
        &tower_http::normalize_path::NormalizePathLayer::trim_trailing_slash(),
        antares_api::router(state),
    );

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;
    tracing::info!("listening on http://0.0.0.0:{port}");
    // Manual serve loop: the ETSI suite reads response headers case-sensitively
    // ("Location"), so HTTP/1 responses are written with title-case headers.
    loop {
        let (stream, _) = tokio::select! {
            r = listener.accept() => r?,
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("shutting down");
                return Ok(());
            }
        };
        let app = app.clone();
        tokio::spawn(async move {
            let svc = hyper::service::service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
                let mut app = app.clone();
                async move {
                    tower::Service::call(&mut app, req.map(axum::body::Body::new)).await
                }
            });
            let mut builder =
                hyper_util::server::conn::auto::Builder::new(hyper_util::rt::TokioExecutor::new());
            builder.http1().title_case_headers(true);
            let _ = builder
                .serve_connection(hyper_util::rt::TokioIo::new(stream), svc)
                .await;
        });
    }
}
