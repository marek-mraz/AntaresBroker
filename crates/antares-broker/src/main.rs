//! Antares — NGSI-LD context broker (composition root, §9.3).
//!
//! Config: ANTARES_* env vars only for v0 (antares.toml layering lands with
//! figment in phase 1). Unknown ANTARES_* keys are fatal (§14.3).

use antares_api::AppState;
use antares_bus::LocalBus;
use std::time::Instant;

const KNOWN_KEYS: &[&str] = &["ANTARES_HTTP_PORT", "ANTARES_HOST_ALIAS", "ANTARES_ROLES"];

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

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(run(port, host_alias, roles))
}

async fn run(
    port: u16,
    host_alias: String,
    roles: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let _bus = LocalBus::new(1024); // consumers attach as phases land
    tracing::info!(port, %host_alias, %roles, "starting antares (v0 skeleton)");

    let app = antares_api::router(AppState {
        started: Instant::now(),
        host_alias,
    });

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;
    tracing::info!("listening on http://0.0.0.0:{port}");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
            tracing::info!("shutting down");
        })
        .await?;
    Ok(())
}
