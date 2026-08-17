//! Graceful shutdown drain.
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
//! 5. flush the outbox (see the note in `drain`)
//! 6. close the pools
//!
//! Operational contract: the container `stopGracePeriod` (compose
//! `stop_grace_period`, K8s `terminationGracePeriodSeconds`) MUST exceed
//! delay + deadline, or the orchestrator turns a drain into a kill. The
//! defaults below (0.5 s + 20 s) fit inside Docker's 10 s default only if the
//! in-flight work is short; the reference manifests set both explicitly.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The LB's notice window: how long to keep serving AFTER going unhealthy.
pub fn drain_delay() -> Result<Duration, String> {
    env_num("ANTARES_DRAIN_DELAY_MS", 500).map(Duration::from_millis)
}

/// Ceiling on waiting for in-flight requests once the listener is closed.
pub fn drain_deadline() -> Result<Duration, String> {
    env_num("ANTARES_DRAIN_DEADLINE_SECS", 20).map(Duration::from_secs)
}

/// Absent = the documented default; present-but-unparsable is fatal. A
/// misread drain window silently running at the default is the same class of
/// misconfiguration as an unknown key, and is refused the same way. Zero is a
/// real choice on both knobs (no notice window / close at once).
fn env_num(key: &str, default: u64) -> Result<u64, String> {
    match std::env::var(key) {
        Err(std::env::VarError::NotPresent) => Ok(default),
        Err(e) => Err(format!("{key} is unreadable: {e}")),
        Ok(v) => v
            .parse::<u64>()
            .map_err(|e| format!("{key} must be a non-negative integer, got {v:?} ({e})")),
    }
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
/// The deadline is passed in, not read here: the composition root parses every
/// config value once, at startup, so a garbage window fails before serving.
pub async fn drain(
    inflight: &Arc<AtomicUsize>,
    store: &antares_sql::store::any::AnyStore,
    deadline: Duration,
    flush_outbox: bool,
) {
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
    // The outbox is drained by a background task on the api pods. Stopping
    // here — after the last request has committed its row, before the pool
    // closes — gives that task the chance to publish what is still pending,
    // so a rolling update does not leave events sitting in the table until
    // another pod's fallback poll finds them. Stores without an outbox
    // (memory, file) answer an empty page and fall straight through.
    while flush_outbox {
        match store.outbox_peek(1) {
            Ok(rows) if rows.is_empty() => break,
            Ok(_) => {}
            Err(e) => {
                tracing::warn!("outbox flush gave up: {e}");
                break;
            }
        }
        if started.elapsed() >= deadline {
            tracing::warn!("drain deadline {deadline:?} hit with outbox rows still pending");
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    store.close().await;
    tracing::info!("drain complete in {:?}", started.elapsed());
}

/// Step 1, so the flip and the log line stay in one place.
pub fn begin(draining: &Arc<AtomicBool>, delay: Duration) {
    draining.store(true, Ordering::Relaxed);
    // Immediate, not sampler-paced — a roll must be visible on a
    // dashboard for its whole (short) duration.
    metrics::gauge!("antares_draining").set(1.0);
    tracing::info!(
        "draining: /q/health now 503 for {delay:?} before the listener closes"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both drain knobs in ONE test: the environment is process-global, so
    /// parsing them from parallel test threads would race.
    ///
    /// Contract: absent = the documented default; present-but-unparsable is
    /// FATAL, never a silent default — a misread timeout is exactly the class
    /// of misconfiguration the unknown-key policy exists to catch.
    #[test]
    fn drain_knobs_default_when_absent_and_refuse_garbage() {
        std::env::remove_var("ANTARES_DRAIN_DELAY_MS");
        std::env::remove_var("ANTARES_DRAIN_DEADLINE_SECS");
        assert_eq!(drain_delay().expect("absent"), Duration::from_millis(500));
        assert_eq!(drain_deadline().expect("absent"), Duration::from_secs(20));

        std::env::set_var("ANTARES_DRAIN_DELAY_MS", "2000");
        std::env::set_var("ANTARES_DRAIN_DEADLINE_SECS", "10");
        assert_eq!(drain_delay().expect("set"), Duration::from_millis(2000));
        assert_eq!(drain_deadline().expect("set"), Duration::from_secs(10));

        // Zero is a real choice on both knobs (no notice window / close at
        // once), so it must NOT be rejected with the garbage.
        std::env::set_var("ANTARES_DRAIN_DELAY_MS", "0");
        std::env::set_var("ANTARES_DRAIN_DEADLINE_SECS", "0");
        assert_eq!(drain_delay().expect("zero"), Duration::ZERO);
        assert_eq!(drain_deadline().expect("zero"), Duration::ZERO);

        for bad in ["soon", "", "-1", "2.5", "500ms", "99999999999999999999999", " 5"] {
            std::env::set_var("ANTARES_DRAIN_DELAY_MS", bad);
            let err = drain_delay()
                .expect_err(&format!("ANTARES_DRAIN_DELAY_MS={bad:?} must be fatal"));
            assert!(
                err.contains("ANTARES_DRAIN_DELAY_MS"),
                "the error must name the key: {err}"
            );
        }
        std::env::set_var("ANTARES_DRAIN_DELAY_MS", "500");
        for bad in ["soon", "", "-1", "20.0"] {
            std::env::set_var("ANTARES_DRAIN_DEADLINE_SECS", bad);
            let err = drain_deadline()
                .expect_err(&format!("ANTARES_DRAIN_DEADLINE_SECS={bad:?} must be fatal"));
            assert!(err.contains("ANTARES_DRAIN_DEADLINE_SECS"), "{err}");
        }
        std::env::remove_var("ANTARES_DRAIN_DELAY_MS");
        std::env::remove_var("ANTARES_DRAIN_DEADLINE_SECS");
    }

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime")
    }

    fn mem_store() -> antares_sql::store::any::AnyStore {
        antares_sql::store::any::AnyStore::Mem(antares_sql::store::Store::default())
    }

    /// Nothing in flight = nothing to wait for: the drain must not sit out
    /// its deadline, and the outbox flush must not hang a store that has no
    /// outbox (memory/file).
    #[test]
    fn drain_returns_at_once_when_nothing_is_in_flight() {
        let inflight = Arc::new(AtomicUsize::new(0));
        let store = mem_store();
        let started = Instant::now();
        rt().block_on(drain(&inflight, &store, Duration::from_secs(20), true));
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "an idle drain waited {:?} — it must not burn the deadline",
            started.elapsed()
        );
    }

    /// The deadline is a CEILING, not a promise: a connection that never
    /// finishes must not hold the process open forever.
    #[test]
    fn drain_gives_up_at_the_deadline_with_a_stuck_connection() {
        let inflight = Arc::new(AtomicUsize::new(1)); // never released
        let store = mem_store();
        let started = Instant::now();
        rt().block_on(drain(&inflight, &store, Duration::from_millis(300), false));
        let waited = started.elapsed();
        assert!(
            waited >= Duration::from_millis(300),
            "the drain must actually wait for in-flight work: {waited:?}"
        );
        assert!(
            waited < Duration::from_secs(3),
            "the drain must give up AT the deadline, not later: {waited:?}"
        );
    }

    /// Step 1 is the flag the health endpoint reads; nothing else may flip it.
    #[test]
    fn begin_flips_the_health_flag() {
        let draining = Arc::new(AtomicBool::new(false));
        assert!(!draining.load(Ordering::Relaxed));
        begin(&draining, Duration::from_millis(500));
        assert!(draining.load(Ordering::Relaxed), "/q/health must go 503");
    }
}
