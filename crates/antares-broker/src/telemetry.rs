//! Tracing + Prometheus metrics + env-gated OTLP span export.
//!
//! Naming follows Prometheus conventions with the `antares_` prefix and
//! unit suffixes. The `metrics` facade is what core crates speak; THIS
//! module is the only place an exporter exists (only the
//! composition root knows). `/q/metrics` renders the Prometheus text
//! format via the closure wired onto AppState.
//!
//! ALL of it sits behind a RUNTIME switch: ANTARES_TELEMETRY=1 installs
//! the recorder, the sampler and (with ANTARES_OTLP_ENDPOINT) the OTLP
//! pipeline at startup; the default constructs NONE of it — `metrics::`
//! macro calls no-op without a recorder (zero allocations), and
//! /q/metrics answers 404. One build, lean by default; flip the
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

/// Is the observability stack switched on for this process? The default is
/// off, so ANY value that is not an explicit off spelling arms it — a knob
/// that recognized only `1|true|on` disabled the whole stack on `TRUE`
/// without a word.
pub fn enabled() -> bool {
    std::env::var("ANTARES_TELEMETRY").is_ok_and(|v| !crate::is_off(&v))
}

/// Strip `user:password@` userinfo (RFC 3986 clause 3.2.1) out of a URL before
/// it is logged. A string without an authority component — no scheme, or an
/// `@` that belongs to the path — comes back byte-identical.
fn redact_url(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_owned();
    };
    let (authority, path) = match rest.find('/') {
        Some(i) => rest.split_at(i),
        None => (rest, ""),
    };
    match authority.rsplit_once('@') {
        Some((_, host)) => format!("{scheme}://{host}{path}"),
        None => url.to_owned(),
    }
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

    // The collector endpoint is a URL and may carry `user:password@` userinfo
    // (RFC 3986 clause 3.2.1); it is logged at startup, so the credential is
    // stripped first.
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
            tracing::info!(endpoint = redact_url(&endpoint), "OTLP span export enabled");
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
        // An endpoint without the switch is a configuration the operator
        // believes is exporting: say so rather than drop it silently. Logged
        // here because the subscriber above is what makes a log visible.
        if let Ok(endpoint) = std::env::var("ANTARES_OTLP_ENDPOINT") {
            tracing::warn!(
                endpoint = redact_url(&endpoint),
                "ANTARES_OTLP_ENDPOINT is set but ANTARES_TELEMETRY is off — \
                 no spans are exported"
            );
        }
        return Ok(None); // the recorder, registry and sampler are never built
    }
    let handle = metrics_exporter_prometheus::PrometheusBuilder::new().install_recorder()?;
    describe();
    Ok(Some(Arc::new(move || handle.render())))
}

/// Metric metadata — Prometheus-convention names (antares_ prefix, unit suffixes).
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
        "1 while this instance drains — a roll is visible on a dashboard"
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
        "file mode: writers queued behind the single redb committer"
    );
    describe_gauge!(
        "antares_limit_rejections_total",
        Unit::Count,
        "bounds-wall rejections, by limit"
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
            // Limit counters live in LimitStats (incremented at rejection
            // sites); exported here so the wall is observable BEFORE users
            // hit it.
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
#[cfg(test)]
mod tests {
    use super::*;

    /// The switch is the whole stack's gate, and the environment is
    /// process-global — every spelling is asserted in ONE test. The DEFAULT
    /// is off, so only an explicit off value may keep it off: a knob that
    /// recognized `1|true|on` alone disabled the whole observability stack on
    /// `TRUE` without a word.
    #[test]
    fn telemetry_switch_is_off_by_default_and_tolerant_of_spelling() {
        std::env::remove_var("ANTARES_TELEMETRY");
        assert!(!enabled(), "the default must allocate nothing");
        for on in ["1", "true", "on", "TRUE", "On", "yes", "1 ", " 1"] {
            std::env::set_var("ANTARES_TELEMETRY", on);
            assert!(enabled(), "ANTARES_TELEMETRY={on:?} must arm the stack");
        }
        for off in ["0", "false", "off", "", " ", "FALSE", "Off", "no", " 0 "] {
            std::env::set_var("ANTARES_TELEMETRY", off);
            assert!(
                !enabled(),
                "ANTARES_TELEMETRY={off:?} must NOT arm the stack"
            );
        }
        std::env::remove_var("ANTARES_TELEMETRY");
    }

    /// A collector endpoint is a URL and may carry `user:password@` userinfo
    /// (RFC 3986 §3.2.1). It is logged at startup, so the credential must be
    /// stripped before it reaches the log.
    #[test]
    fn otlp_endpoint_userinfo_never_reaches_the_log() {
        let redacted = redact_url("http://otel:s3cr3t@collector.internal:4318/v1/traces");
        assert!(
            !redacted.contains("s3cr3t") && !redacted.contains("otel:"),
            "userinfo leaked into the log line: {redacted}"
        );
        assert!(
            redacted.contains("collector.internal:4318/v1/traces"),
            "the useful part of the endpoint must survive: {redacted}"
        );
        // No userinfo: byte-identical, including an '@' that belongs to the
        // path rather than the authority.
        for plain in [
            "http://collector:4318/v1/traces",
            "https://collector:4318/v1/@traces",
            "collector:4318",
            "",
        ] {
            assert_eq!(redact_url(plain), plain, "rewrote a credential-free URL");
        }
    }

    /// Metric label cardinality: the only labelled instrument this module
    /// feeds is the limit-rejection gauge, whose label comes from a fixed key
    /// set in the bounds snapshot — never from a client-controlled string
    /// (a tenant, a URI or a header would blow up the time-series count).
    #[test]
    fn limit_rejection_labels_are_a_closed_identifier_set() {
        let snapshot = antares_api::bounds::LimitStats::default().snapshot();
        let labels: Vec<String> = snapshot
            .as_object()
            .expect("snapshot is an object")
            .keys()
            .filter_map(|k| k.strip_prefix("rejected").map(str::to_owned))
            .collect();
        assert_eq!(
            labels.len(),
            3,
            "the rejection label set changed — keep it closed and bounded: {labels:?}"
        );
        for l in &labels {
            assert!(
                l.chars().all(|c| c.is_ascii_alphanumeric()),
                "label value {l:?} is not a fixed identifier"
            );
        }
    }
}
