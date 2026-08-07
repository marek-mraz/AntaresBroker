//! K12 (§9.1): tracing + Prometheus metrics + env-gated OTLP span export.
//!
//! Naming follows Prometheus conventions with the `antares_` prefix and
//! unit suffixes. The `metrics` facade is what core crates speak; THIS
//! module is the only place an exporter exists (§9.2 — only the
//! composition root knows). `/q/metrics` renders the Prometheus text
//! format via the closure wired onto AppState.
//!
//! ALL of it sits behind a RUNTIME switch: ANTARES_TELEMETRY=1 installs
//! the recorder, the sampler and (with ANTARES_OTLP_ENDPOINT) the OTLP
//! pipeline at startup; the default constructs NONE of it — `metrics::`
//! macro calls no-op without a recorder (zero allocations), and
//! /q/metrics answers 404. One build, lean by default (§2.1); flip the
//! env and restart where a dashboard actually scrapes.
//!
//! OTLP: set ANTARES_OTLP_ENDPOINT (e.g. http://collector:4318/v1/traces)
//! and spans flow out over OTLP/HTTP; unset (the default) costs nothing.
//! tokio-console: cargo feature `console` + RUSTFLAGS="--cfg tokio_unstable"
//! (the layer only arms when BOTH are present — an --all-features build
//! without the RUSTFLAGS must not panic at startup).

use std::sync::Arc;

/// What /q/metrics renders through — exporter type erased so `main` builds
/// identically with and without the `telemetry` feature.
pub type MetricsRender = Arc<dyn Fn() -> String + Send + Sync>;

/// Is the observability stack switched on for this process?
pub fn enabled() -> bool {
    matches!(
        std::env::var("ANTARES_TELEMETRY").as_deref(),
        Ok("1" | "true" | "on")
    )
}

/// Install the tracing subscriber stack and (ANTARES_TELEMETRY=1) the
/// Prometheus recorder. Call once, before the runtime spins up anything
/// measurable. Returns the /q/metrics render closure, or None when the
/// switch is off — in which case nothing telemetry-shaped is allocated.
pub fn init() -> Result<Option<MetricsRender>, Box<dyn std::error::Error>> {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let env_filter =
        tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());
    let fmt = tracing_subscriber::fmt::layer();

    // Env-gated OTLP span pipeline — needs the switch AND an endpoint.
    let otlp = match std::env::var("ANTARES_OTLP_ENDPOINT") {
        Ok(endpoint) if enabled() => {
            use opentelemetry::trace::TracerProvider as _;
            use opentelemetry_otlp::WithExportConfig as _;
            let exporter = opentelemetry_otlp::SpanExporter::builder()
                .with_http()
                .with_endpoint(endpoint.clone())
                .build()?;
            let provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
                .with_batch_exporter(exporter)
                .with_resource(
                    opentelemetry_sdk::Resource::builder()
                        .with_service_name("antares")
                        .build(),
                )
                .build();
            let tracer = provider.tracer("antares");
            tracing::info!(endpoint, "OTLP span export enabled");
            Some(tracing_opentelemetry::layer().with_tracer(tracer))
        }
        _ => None,
    };

    #[cfg(all(feature = "console", tokio_unstable))]
    let console = Some(console_subscriber::spawn());
    #[cfg(not(all(feature = "console", tokio_unstable)))]
    let console: Option<tracing_subscriber::layer::Identity> = None;

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt)
        .with(otlp)
        .with(console)
        .init();

    if !enabled() {
        return Ok(None); // the recorder, registry and sampler are never built
    }
    let handle = metrics_exporter_prometheus::PrometheusBuilder::new().install_recorder()?;
    describe();
    Ok(Some(Arc::new(move || handle.render())))
}

/// Metric metadata — names per §9.1 (antares_ prefix, unit suffixes).
fn describe() {
    use metrics::{describe_counter, describe_gauge, describe_histogram, Unit};
    describe_counter!(
        "antares_http_requests_total",
        Unit::Count,
        "HTTP requests served, by method and status class"
    );
    describe_histogram!(
        "antares_http_request_duration_seconds",
        Unit::Seconds,
        "HTTP request service time"
    );
    describe_counter!(
        "antares_notifications_sent_total",
        Unit::Count,
        "notifications delivered successfully, by sink scheme"
    );
    describe_counter!(
        "antares_notifications_failed_total",
        Unit::Count,
        "notification deliveries that failed, by sink scheme"
    );
    describe_histogram!(
        "antares_change_lag_seconds",
        Unit::Seconds,
        "bus=nats: change-event age (stream publish -> matcher processing)"
    );
    describe_gauge!(
        "antares_draining",
        Unit::Count,
        "1 while this instance drains (K1) — a roll is visible on a dashboard"
    );
    describe_gauge!(
        "antares_uptime_seconds",
        Unit::Seconds,
        "seconds since process start"
    );
    describe_gauge!(
        "antares_memory_allocated_bytes",
        Unit::Bytes,
        "jemalloc allocated (live) bytes"
    );
    describe_gauge!(
        "antares_memory_resident_bytes",
        Unit::Bytes,
        "jemalloc resident bytes (RSS ~ live x1.2 is the section-2.1 target)"
    );
    describe_gauge!(
        "antares_commit_queue_depth",
        Unit::Count,
        "file mode: writers queued behind the single redb committer (B13)"
    );
    describe_gauge!(
        "antares_limit_rejections_total",
        Unit::Count,
        "I2 bounds-wall rejections, by limit"
    );
}

/// The 5 s gauge sampler: process-level state that has no natural
/// increment site. Spawned once per process from `run`. With the switch
/// off there is no recorder to feed — no task is spawned at all.
pub fn spawn_sampler(state: antares_api::AppState) {
    if !enabled() {
        return;
    }
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(5));
        loop {
            tick.tick().await;
            metrics::gauge!("antares_uptime_seconds").set(state.started.elapsed().as_secs_f64());
            metrics::gauge!("antares_draining").set(f64::from(u8::from(
                state.draining.load(std::sync::atomic::Ordering::Relaxed),
            )));
            if let Some(mem) = &state.mem_stats {
                let m = mem();
                if let Some(a) = m.get("allocatedBytes").and_then(serde_json::Value::as_u64) {
                    metrics::gauge!("antares_memory_allocated_bytes").set(a as f64);
                }
                if let Some(r) = m.get("residentBytes").and_then(serde_json::Value::as_u64) {
                    metrics::gauge!("antares_memory_resident_bytes").set(r as f64);
                }
            }
            if let Some((depth, _peak)) = state.store.commit_queue() {
                metrics::gauge!("antares_commit_queue_depth").set(depth as f64);
            }
            // I2 counters live in LimitStats (incremented at rejection
            // sites); exported here so the wall is observable BEFORE users
            // hit it (§16.3).
            if let Some(map) = state.limits.snapshot().as_object() {
                for (key, n) in map {
                    if let Some(limit) = key.strip_prefix("rejected") {
                        if let Some(n) = n.as_u64() {
                            metrics::gauge!(
                                "antares_limit_rejections_total",
                                "limit" => limit.to_owned()
                            )
                            .set(n as f64);
                        }
                    }
                }
            }
        }
    });
}
