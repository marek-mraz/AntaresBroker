//! Graceful shutdown drain (tasks.md K1; §9.3 `shutdown.rs`).
//!
//! The ORDER is the whole feature, and it exists because of one asymmetry: a
//! load balancer learns this instance is going away only by polling
//! `/q/health`, but the orchestrator kills it on its own schedule. So the
//! health endpoint must go unhealthy while the socket still works, and only
//! then may the socket close.
//!
//! 1. flip `draining` → `/q/health` answers 503 (see `antares_api::health`)
//! 2. keep accepting for `ANTARES_DRAIN_DELAY_MS` — the LB's notice window;
//!    this is the step people skip, and skipping it is what turns a rolling
//!    update into a burst of connection-refused
//! 3. stop accepting
//! 4. wait for in-flight connections, bounded by `ANTARES_DRAIN_DEADLINE_SECS`
//! 5. flush the outbox (F3 — see the note in `drain`)
//! 6. close the pools
//!
//! Operational contract: the container `stopGracePeriod` (compose
//! `stop_grace_period`, K8s `terminationGracePeriodSeconds`) MUST exceed
//! delay + deadline, or the orchestrator turns a drain into a kill. The
//! defaults below (0.5 s + 20 s) fit inside Docker's 10 s default only if the
//! in-flight work is short; K5's manifests set both explicitly.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The LB's notice window: how long to keep serving AFTER going unhealthy.
pub fn drain_delay() -> Duration {
    Duration::from_millis(env_num("ANTARES_DRAIN_DELAY_MS", 500))
}

/// Ceiling on waiting for in-flight requests once the listener is closed.
pub fn drain_deadline() -> Duration {
    Duration::from_secs(env_num("ANTARES_DRAIN_DEADLINE_SECS", 20))
}

fn env_num(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// Resolves on SIGTERM or SIGINT. SIGTERM is the one that matters —
/// it is what every orchestrator sends — and listening only for ctrl_c (the
/// v0 behaviour) meant a `docker stop` or a pod eviction dropped every
/// in-flight request on the floor.
pub async fn signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut term = match signal(SignalKind::terminate()) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("cannot listen for SIGTERM ({e}); ctrl-c only");
                let _ = tokio::signal::ctrl_c().await;
                return;
            }
        };
        tokio::select! {
            _ = term.recv() => tracing::info!("SIGTERM received"),
            _ = tokio::signal::ctrl_c() => tracing::info!("SIGINT received"),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

/// Steps 4–6. Steps 1–3 belong to the accept loop, which owns the listener.
pub async fn drain(inflight: &Arc<AtomicUsize>, store: &antares_sql::store::any::AnyStore) {
    let deadline = drain_deadline();
    let started = Instant::now();
    while inflight.load(Ordering::Relaxed) > 0 {
        if started.elapsed() >= deadline {
            tracing::warn!(
                "drain deadline {deadline:?} hit with {} connection(s) still open — closing anyway",
                inflight.load(Ordering::Relaxed)
            );
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    // Outbox flush (F3) is deliberately absent, not forgotten: the outbox
    // table is written same-tx today (C8) and nothing drains it yet, so there
    // is no buffer to flush. When F3 lands, its drain stops HERE — after the
    // last request has committed its row and before the pool closes.
    store.close().await;
    tracing::info!("drain complete in {:?}", started.elapsed());
}

/// Step 1, so the flip and the log line stay in one place.
pub fn begin(draining: &Arc<AtomicBool>) {
    draining.store(true, Ordering::Relaxed);
    // K12: immediate, not sampler-paced — a roll must be visible on a
    // dashboard for its whole (short) duration.
    metrics::gauge!("antares_draining").set(1.0);
    tracing::info!(
        "draining: /q/health now 503 for {:?} before the listener closes",
        drain_delay()
    );
}
