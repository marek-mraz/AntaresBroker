// SPDX-License-Identifier: EUPL-1.2
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
//! 5. wait for the outbox to empty — same deadline, whatever step 4 left of it
//!    (see the note in `drain`)
//! 6. close the pools
//!
//! The two numbers are for different jobs and are easy to confuse: the delay
//! is the LB's notice window in MILLIseconds (default 2000), the deadline is
//! the ceiling on in-flight work in SECONDS (default 20). Steps 4 and 5 share
//! that one deadline.
//!
//! Operational contract: the container `stopGracePeriod` (compose
//! `stop_grace_period`, K8s `terminationGracePeriodSeconds`) MUST exceed
//! delay + deadline, or the orchestrator turns a drain into a kill. The
//! defaults below (2 s + 20 s) overrun Docker's 10 s default as soon as the
//! in-flight work is not short; the reference manifests set both explicitly.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The LB's notice window (default 2 s): how long to keep serving AFTER going
/// unhealthy, sized so a load balancer's health poll actually observes the 503
/// before the socket goes. It is NOT the in-flight ceiling — that is
/// `drain_deadline`, and it is 10× longer.
pub fn drain_delay() -> Result<Duration, String> {
    env_num("ANTARES_DRAIN_DELAY_MS", 2000).map(Duration::from_millis)
}

/// The real shutdown deadline (default 20 s): the ceiling on waiting for
/// in-flight work — connections, then the outbox — once the listener is
/// closed. Both share it; it does not extend the notice window above.
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
    pending_changes: &AtomicUsize,
    store: &dyn antares_store::CurrentStateDriver,
    temporal: &dyn antares_store::TemporalDriver,
    deadline: Duration,
    flush_outbox: bool,
) {
    let started = Instant::now();
    // A request is not over when its response is: the remote leg of a
    // distributed subscription, an initial Context Source notification and a
    // forwarded notification all run as tasks after the 2xx, and a stop that
    // dropped them left the subscription chain half-built on every roll.
    // The matcher queue is part of a request too: a change accepted before
    // the listener closed still owes its notifications (5.8.6).
    while inflight.load(Ordering::Relaxed) > 0
        || antares_api::background_tasks() > 0
        || pending_changes.load(Ordering::SeqCst) > 0
    {
        if started.elapsed() >= deadline {
            tracing::warn!(
                "drain deadline {deadline:?} hit with {} connection(s), {} task(s) and {} change batch(es) still open — closing anyway",
                inflight.load(Ordering::Relaxed),
                antares_api::background_tasks(),
                pending_changes.load(Ordering::SeqCst)
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
    //
    // Two residuals, stated rather than papered over. The table is shared, so
    // this waits for rows OTHER pods are still producing too, and under
    // sustained write load it therefore runs to the deadline; and the deadline
    // is the same one step 4 just spent, so a slow in-flight wait can leave
    // the flush no time at all. Either way the rows stay committed — the
    // fallback poll on a surviving pod publishes them — so the ceiling costs
    // latency, never an event.
    if flush_outbox {
        loop {
            match store.outbox_peek(1).await {
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
    }
    store.close().await;
    // Both seams, because the temporal half may be a store of its own
    // (`ANTARES_TEMPORAL` naming a second backend) with its own pool. When
    // one instance serves both, the second call lands on an already-closed
    // pool and does nothing.
    temporal.close().await;
    tracing::info!("drain complete in {:?}", started.elapsed());
}

/// Step 1, so the flip and the log line stay in one place.
pub fn begin(draining: &Arc<AtomicBool>, delay: Duration) {
    draining.store(true, Ordering::Relaxed);
    // Immediate, not sampler-paced — a roll must be visible on a
    // dashboard for its whole (short) duration.
    metrics::gauge!("antares_draining").set(1.0);
    tracing::info!("draining: /q/health now 503 for {delay:?} before the listener closes");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A temporal driver that only records whether it was closed; every
    /// operation answers "no temporal store", the same shape as `NoTemporal`.
    struct CountsCloses(Arc<AtomicUsize>);

    impl CountsCloses {
        fn off<T>() -> Result<T, antares_model::NgsiError> {
            Err(antares_model::NgsiError::OperationNotSupported(
                "test driver".into(),
            ))
        }
    }

    #[async_trait::async_trait]
    impl antares_store::TemporalDriver for CountsCloses {
        async fn close(&self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
        async fn temporal_append(
            &self,
            _t: &antares_model::TenantId,
            _id: &str,
            _shell: &serde_json::Value,
            _add: &serde_json::Value,
        ) -> Result<(), antares_model::NgsiError> {
            Self::off()
        }
        async fn query_temporal(
            &self,
            _t: &antares_model::TenantId,
            _f: &antares_store::filter::TemporalFilter<'_>,
        ) -> Result<antares_store::filter::TemporalOutcome, antares_model::NgsiError> {
            Self::off()
        }
        async fn get_temporal(
            &self,
            _t: &antares_model::TenantId,
            _id: &str,
            _f: &antares_store::filter::TemporalFilter<'_>,
        ) -> Result<Option<serde_json::Value>, antares_model::NgsiError> {
            Self::off()
        }
        async fn get(
            &self,
            _t: &antares_model::TenantId,
            _id: &str,
        ) -> Result<Option<serde_json::Value>, antares_model::NgsiError> {
            Self::off()
        }
        async fn create(
            &self,
            _t: &antares_model::TenantId,
            _id: &str,
            _d: serde_json::Value,
        ) -> Result<bool, antares_model::NgsiError> {
            Self::off()
        }
        async fn upsert(
            &self,
            _t: &antares_model::TenantId,
            _id: &str,
            _d: serde_json::Value,
        ) -> Result<bool, antares_model::NgsiError> {
            Self::off()
        }
        async fn delete(
            &self,
            _t: &antares_model::TenantId,
            _id: &str,
        ) -> Result<bool, antares_model::NgsiError> {
            Self::off()
        }
        async fn list(
            &self,
            _t: &antares_model::TenantId,
        ) -> Result<Vec<serde_json::Value>, antares_model::NgsiError> {
            Self::off()
        }
        async fn mutate_boxed<'a>(
            &self,
            _t: &antares_model::TenantId,
            _id: &str,
            _f: antares_store::MutateFn<'a>,
        ) -> Result<Option<Result<(), ()>>, antares_model::NgsiError> {
            Self::off()
        }
    }

    /// The drain closes BOTH driver seams. With `ANTARES_TEMPORAL` naming a
    /// backend of its own the temporal half is a second store holding its own
    /// connection pool; closing only the current-state store left that pool
    /// open for process teardown to sever, with whatever it still owed
    /// in flight.
    #[tokio::test(flavor = "multi_thread")]
    async fn drain_closes_the_temporal_driver_too() {
        let closes = Arc::new(AtomicUsize::new(0));
        let temporal = CountsCloses(Arc::clone(&closes));
        let store = antares_sql::store::any::AnyStore::Mem(antares_sql::store::Store::default());
        drain(
            &Arc::new(AtomicUsize::new(0)),
            &AtomicUsize::new(0),
            &store,
            &temporal,
            Duration::from_millis(50),
            false,
        )
        .await;
        assert_eq!(
            closes.load(Ordering::SeqCst),
            1,
            "the temporal driver must be closed by the drain, not by process exit"
        );
    }

    /// The book states these two defaults, and their SUM is an operator
    /// contract: the container stop grace period has to exceed it or the
    /// orchestrator turns a drain into a kill. Nothing tied the stated
    /// numbers to the ones the binary uses, and the delay drifted to a value
    /// 1.5 s shorter than the truth in three chapters at once — a grace
    /// period sized from the book would then have been under the real drain.
    /// `dev/check-env-docs.sh` proves each variable is documented; this
    /// proves the documented numbers are the ones that run.
    #[test]
    fn the_documented_drain_defaults_are_the_ones_the_binary_uses() {
        let book = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/src/configuration.md"
        ))
        .expect("the configuration chapter");
        let stated = |var: &str| -> String {
            let row = book
                .lines()
                .find(|l| l.starts_with(&format!("| `{var}` |")))
                .unwrap_or_else(|| panic!("{var} has no row in the configuration table"));
            row.split('|')
                .nth(2)
                .expect("the default column")
                .trim()
                .trim_matches('`')
                .to_owned()
        };
        std::env::remove_var("ANTARES_DRAIN_DELAY_MS");
        std::env::remove_var("ANTARES_DRAIN_DEADLINE_SECS");
        assert_eq!(
            stated("ANTARES_DRAIN_DELAY_MS"),
            drain_delay().expect("absent").as_millis().to_string(),
            "the book's notice window is not the compiled one"
        );
        assert_eq!(
            stated("ANTARES_DRAIN_DEADLINE_SECS"),
            drain_deadline().expect("absent").as_secs().to_string(),
            "the book's in-flight ceiling is not the compiled one"
        );

        // The shipped compose files justify their stop_grace_period against
        // the same two defaults, in a comment an operator copies the number
        // out of. Both said 0.5 s long after the delay became 2 s.
        let secs = drain_delay().expect("absent").as_secs_f64();
        let want = format!(
            "drain delay ({} s)",
            if secs.fract() == 0.0 {
                format!("{secs:.0}")
            } else {
                format!("{secs}")
            }
        );
        for name in ["docker-compose-ha.yml", "docker-compose-roles.yml"] {
            let path = format!(
                concat!(env!("CARGO_MANIFEST_DIR"), "/../../compose-files/{}"),
                name
            );
            let text = std::fs::read_to_string(&path).expect("the compose file");
            assert!(
                text.contains(&want),
                "{name} does not justify stop_grace_period against \"{want}\""
            );
        }
    }

    /// Both drain knobs in ONE test: the environment is process-global, so
    /// parsing them from parallel test threads would race.
    ///
    /// Contract: absent = the documented default; present-but-unparsable is
    /// FATAL, never a silent default — a misread timeout is exactly the class
    /// of misconfiguration the unknown-key policy exists to catch. The two
    /// defaults are different numbers for different jobs: 2000 MILLIseconds of
    /// LB notice, 20 SECONDS of in-flight ceiling.
    #[test]
    fn drain_knobs_default_when_absent_and_refuse_garbage() {
        std::env::remove_var("ANTARES_DRAIN_DELAY_MS");
        std::env::remove_var("ANTARES_DRAIN_DEADLINE_SECS");
        assert_eq!(drain_delay().expect("absent"), Duration::from_millis(2000));
        assert_eq!(drain_deadline().expect("absent"), Duration::from_secs(20));
        assert_ne!(
            drain_delay().expect("absent"),
            drain_deadline().expect("absent"),
            "the notice window and the in-flight ceiling are not the same number"
        );

        std::env::set_var("ANTARES_DRAIN_DELAY_MS", "750");
        std::env::set_var("ANTARES_DRAIN_DEADLINE_SECS", "10");
        assert_eq!(drain_delay().expect("set"), Duration::from_millis(750));
        assert_eq!(drain_deadline().expect("set"), Duration::from_secs(10));

        // Zero is a real choice on both knobs (no notice window / close at
        // once), so it must NOT be rejected with the garbage.
        std::env::set_var("ANTARES_DRAIN_DELAY_MS", "0");
        std::env::set_var("ANTARES_DRAIN_DEADLINE_SECS", "0");
        assert_eq!(drain_delay().expect("zero"), Duration::ZERO);
        assert_eq!(drain_deadline().expect("zero"), Duration::ZERO);

        for bad in [
            "soon",
            "",
            "-1",
            "2.5",
            "500ms",
            "99999999999999999999999",
            " 5",
        ] {
            std::env::set_var("ANTARES_DRAIN_DELAY_MS", bad);
            let err =
                drain_delay().expect_err(&format!("ANTARES_DRAIN_DELAY_MS={bad:?} must be fatal"));
            assert!(
                err.contains("ANTARES_DRAIN_DELAY_MS"),
                "the error must name the key: {err}"
            );
        }
        std::env::set_var("ANTARES_DRAIN_DELAY_MS", "2000");
        for bad in ["soon", "", "-1", "20.0"] {
            std::env::set_var("ANTARES_DRAIN_DEADLINE_SECS", bad);
            let err = drain_deadline().expect_err(&format!(
                "ANTARES_DRAIN_DEADLINE_SECS={bad:?} must be fatal"
            ));
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

    /// A request's follow-up work (the remote leg of a distributed
    /// subscription, a forwarded notification) runs after its response; the
    /// drain waits for it like it waits for the request itself.
    #[test]
    fn drain_waits_for_request_born_tasks() {
        rt().block_on(async {
            let inflight = Arc::new(AtomicUsize::new(0));
            let done = Arc::new(AtomicBool::new(false));
            let d = done.clone();
            antares_api::spawn(async move {
                tokio::time::sleep(Duration::from_millis(300)).await;
                d.store(true, Ordering::SeqCst);
            });
            let store = mem_store();
            drain(
                &inflight,
                &AtomicUsize::new(0),
                &store,
                &antares_store::NoTemporal,
                Duration::from_secs(5),
                false,
            )
            .await;
            assert!(
                done.load(Ordering::SeqCst),
                "drain returned before the task finished"
            );
            assert_eq!(antares_api::background_tasks(), 0);
        });
    }

    /// Nothing in flight = nothing to wait for: the drain must not sit out
    /// its deadline, and the outbox flush must not hang a store that has no
    /// outbox (memory/file).
    #[test]
    fn drain_returns_at_once_when_nothing_is_in_flight() {
        let inflight = Arc::new(AtomicUsize::new(0));
        let store = mem_store();
        let started = Instant::now();
        rt().block_on(drain(
            &inflight,
            &AtomicUsize::new(0),
            &store,
            &antares_store::NoTemporal,
            Duration::from_secs(20),
            true,
        ));
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
        rt().block_on(drain(
            &inflight,
            &AtomicUsize::new(0),
            &store,
            &antares_store::NoTemporal,
            Duration::from_millis(300),
            false,
        ));
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

    /// The outbox flush must WAIT while rows are still pending, and only when
    /// it is asked to: a pod whose own drain is off publishes nothing, so
    /// waiting there would only burn the deadline. Needs a live database —
    /// the outbox table exists on the Pg arm alone, so the memory store can
    /// never exercise the wait (it answers an empty page and falls through).
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs a live database (ANTARES_TEST_DATABASE_URL)"]
    async fn outbox_flush_waits_for_pending_rows_and_only_when_asked() {
        use antares_sql::store::any::{AnyStore, PgBackend};
        use antares_sql::store::Kind;
        let url = std::env::var("ANTARES_TEST_DATABASE_URL")
            .expect("ANTARES_TEST_DATABASE_URL: this test is asked for by name where a DB exists");
        // a nested fn, not a closure: connecting is awaited, and an async
        // closure is not a stable language feature
        async fn connect(url: &str) -> AnyStore {
            AnyStore::Pg(PgBackend::new(
                antares_sql::store::pg::connect(url, 5)
                    .await
                    .expect("connect+migrate"),
            ))
        }
        let run = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_millis();
        let tenant = antares_model::TenantId::new(&format!("drain{run}")).expect("tenant");
        let id = format!("urn:ngsi-ld:DrainProbe:{run}");
        let inflight = Arc::new(AtomicUsize::new(0));

        // One committed-but-unpublished row: outbox on, then a write. Nothing
        // publishes it here — a unit test wires no bus drain task.
        let store = connect(&url).await;
        store.set_outbox(true);
        store
            .create(
                &tenant,
                Kind::Entity,
                &id,
                serde_json::json!({"id": id.as_str(), "type": "DrainProbe"}),
            )
            .await
            .expect("write");
        let mine: Vec<i64> = store
            .outbox_peek(500)
            .await
            .expect("peek")
            .into_iter()
            .filter(|(_, t, _)| t == tenant.as_str())
            .map(|(seq, ..)| seq)
            .collect();
        assert!(
            !mine.is_empty(),
            "the write enqueued no outbox row — the rest of this test would prove nothing"
        );

        // Not asked to flush: the pending row may not delay the close at all.
        let t0 = Instant::now();
        drain(
            &inflight,
            &AtomicUsize::new(0),
            &store,
            &antares_store::NoTemporal,
            Duration::from_millis(400),
            false,
        )
        .await;
        let closed = t0.elapsed();
        assert!(
            closed < Duration::from_millis(250),
            "flush_outbox=false waited {closed:?} on a row it never intended to publish"
        );

        // Asked to flush: the rows stay pending, so the flush must hold the
        // process to its deadline rather than exit on top of them.
        let store = connect(&url).await;
        let t0 = Instant::now();
        drain(
            &inflight,
            &AtomicUsize::new(0),
            &store,
            &antares_store::NoTemporal,
            Duration::from_millis(400),
            true,
        )
        .await;
        let waited = t0.elapsed();
        assert!(
            waited >= Duration::from_millis(400),
            "the flush returned after {waited:?} with rows still pending"
        );
        assert!(
            waited < Duration::from_secs(5),
            "the flush must give up AT the deadline, not later: {waited:?}"
        );

        // Leave the shared table as it was found: ack only our own seqs (a
        // blanket ack would delete another test's pending rows) and drop the
        // probe entity with the outbox off, so the delete enqueues nothing.
        let store = connect(&url).await;
        store
            .delete(&tenant, Kind::Entity, &id)
            .await
            .expect("probe cleanup");
        store.outbox_ack(&mine).await.expect("outbox cleanup");
        assert!(
            store
                .outbox_peek(500)
                .await
                .expect("peek")
                .into_iter()
                .all(|(_, t, _)| t != tenant.as_str()),
            "the test left its own rows in the shared outbox"
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
