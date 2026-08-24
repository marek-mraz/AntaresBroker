//! The temporal seam's producer side (ADR-0013): the write path pushes
//! `TemporalEvent`s into a per-request buffer; the buffer is drained ONCE
//! per request — after the handler, before the response leaves — so the
//! driver sees the whole request in one `event_list` call and a client
//! reading its own history right after the write always finds it.
//!
//! Outside a request (background jobs, tests without the router) there is
//! no buffer, and a push drains immediately: the seam degrades to today's
//! per-change recording rather than losing events. A driver error in the
//! drain is logged and counted (`/q/health` temporalDrainErrors) — it never
//! changes the response of a write that already committed.

use crate::state::AppState;
use antares_store::TemporalEvent;
use std::sync::atomic::{AtomicU64, Ordering};

static DRAIN_ERRORS: AtomicU64 = AtomicU64::new(0);

/// Drains that failed in the driver (the events of that request are lost;
/// the write itself stood).
pub fn drain_errors() -> u64 {
    DRAIN_ERRORS.load(Ordering::Relaxed)
}

// The buffer rides the request's task; tokio task-locals need the `rt`
// feature the single-threaded wasm build does not carry, so wasm records
// immediately.
#[cfg(not(target_arch = "wasm32"))]
tokio::task_local! {
    static BUFFER: std::cell::RefCell<Vec<TemporalEvent>>;
}

/// Hand one event to the seam: buffered when a request is in flight,
/// drained on the spot otherwise.
pub(crate) fn push(st: &AppState, ev: TemporalEvent) {
    #[cfg(not(target_arch = "wasm32"))]
    let ev = {
        let mut slot = Some(ev);
        let buffered = BUFFER
            .try_with(|b| {
                if let Some(ev) = slot.take() {
                    b.borrow_mut().push(ev);
                }
            })
            .is_ok();
        if buffered {
            return;
        }
        match slot {
            Some(ev) => ev,
            None => return,
        }
    };
    drain(st, vec![ev]);
}

/// The gate chain: an event enters history only if every gate admits it.
/// Gate 1 (value-change) runs in the producer — an unchanged instance never
/// becomes an event (`changed_instances`). Adding a gate = one more entry
/// here; producers and drivers stay untouched.
const GATES: &[fn(&AppState, &TemporalEvent) -> bool] = &[observed_gate];

/// Gate 2: ANTARES_TEMPORAL_RECORD=observed keeps only instances that carry
/// `observedAt` — the spec's own measurement axis (4.5.7: observedAt is
/// the default timeproperty); metadata-shaped writes leave no history.
fn observed_gate(st: &AppState, ev: &TemporalEvent) -> bool {
    !st.record_observed_only || ev.instance.get("observedAt").is_some()
}

/// The consumer side: one `event_list` call per drained batch.
pub(crate) fn drain(st: &AppState, evs: Vec<TemporalEvent>) {
    let evs: Vec<TemporalEvent> = evs
        .into_iter()
        .filter(|ev| GATES.iter().all(|gate| gate(st, ev)))
        .collect();
    if evs.is_empty() {
        return;
    }
    if let Err(e) = st.temporal.event_list(&evs) {
        DRAIN_ERRORS.fetch_add(1, Ordering::Relaxed);
        metrics::counter!("antares_temporal_drain_errors_total").increment(1);
        tracing::warn!(events = evs.len(), "temporal drain failed: {e}");
    }
}

/// Router layer: scopes the buffer over the handler and drains it once the
/// response is built. Drained BEFORE the response is returned so
/// read-your-writes holds at any store latency (the ETSI temporal suites
/// read history straight after the write).
#[cfg(not(target_arch = "wasm32"))]
pub(crate) async fn layer(
    axum::extract::State(st): axum::extract::State<AppState>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let (resp, evs) = BUFFER
        .scope(std::cell::RefCell::new(Vec::new()), async {
            let resp = next.run(req).await;
            (resp, BUFFER.with(|b| b.take()))
        })
        .await;
    drain(&st, evs);
    resp
}

/// No buffer on wasm (pushes drain on the spot): the layer is a pass-through
/// so the router composes identically on both targets.
#[cfg(target_arch = "wasm32")]
pub(crate) async fn layer(
    axum::extract::State(_st): axum::extract::State<AppState>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    next.run(req).await
}
