// SPDX-License-Identifier: EUPL-1.2
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
use antares_model::TenantId;
use antares_store::{TemporalDriverExt as _, TemporalEvent};
use serde_json::{Map, Value};
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
    static CHANGES: std::cell::RefCell<Vec<crate::mirror::Change>>;
}

/// Buffer one entity change for the request in flight so the matcher
/// receives the whole request at once. Handed back when no request is in
/// flight — the caller then gives it to the matcher on the spot.
pub(crate) fn buffer_change(change: crate::mirror::Change) -> Option<crate::mirror::Change> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut slot = Some(change);
        let _ = CHANGES.try_with(|b| {
            if let Some(c) = slot.take() {
                b.borrow_mut().push(c);
            }
        });
        slot
    }
    #[cfg(target_arch = "wasm32")]
    Some(change)
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

/// Gate 2: ANTARES_TEMPORAL_RECORD. `all` admits everything; `observed`
/// keeps only instances that carry `observedAt` — the spec's own
/// measurement axis (4.5.7: observedAt is the default timeproperty), so
/// metadata-shaped writes leave no history; `none` admits nothing.
fn observed_gate(st: &AppState, ev: &TemporalEvent) -> bool {
    use crate::state::TemporalRecord::*;
    match st.temporal_record {
        All => true,
        Observed => ev.instance.get("observedAt").is_some(),
        None => false,
    }
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
    let (resp, evs, changes) = BUFFER
        .scope(std::cell::RefCell::new(Vec::new()), async {
            CHANGES
                .scope(std::cell::RefCell::new(Vec::new()), async {
                    let resp = next.run(req).await;
                    (resp, BUFFER.with(|b| b.take()), CHANGES.with(|c| c.take()))
                })
                .await
        })
        .await;
    drain(&st, evs);
    if !changes.is_empty() {
        if let Some(flush) = &st.change_flush {
            flush(changes);
        }
    }
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

/// delete_temporal_on_core_delete: entity deletion removes its temporal
/// representation too (suite configuration parity). Skipped on bus=nats
/// api pods — the recorder applies the entityDeleted fence instead.
pub fn mirror_delete_entity(st: &AppState, tenant: &TenantId, id: &str) {
    if !st.record_locally() {
        return;
    }
    if let Err(e) = st.temporal.delete(tenant, id) {
        tracing::warn!("temporal mirror delete failed: {e}");
    }
}

/// 4.5.7/4.5.8: "In case the Property is deleted, an instance of the
/// Property is recorded with its value set to the URI "urn:ngsi-ld:null"
/// and the deletedAt Temporal Property set" (object for a Relationship;
/// typed null shapes for the LanguageProperty/JsonProperty/Vocab/List
/// subtypes). Each recorded instance carries an instanceId — the clause
/// SHOULD that makes 5.6.14/5.6.15 selective modification possible.
pub fn mirror_delete_attr(
    st: &AppState,
    tenant: &TenantId,
    id: &str,
    attr_iri: &str,
    dataset_id: Option<&str>,
    ts: &str,
) -> bool {
    let mut had = false;
    let r = st.temporal.mutate(tenant, id, |doc| {
        // The mirror writes nothing into a document the temporal driver
        // handed back in a shape the contract forbids; `had` stays false and
        // the caller reports that nothing was mirrored.
        let Some(target) = doc.as_object_mut() else {
            return Ok::<(), std::convert::Infallible>(());
        };
        if attr_iri == "scope" {
            // scope deletion: temporal scope becomes an instance array with
            // value [] (the 020_19/020_20 shape)
            had = true;
            let inst = serde_json::json!({
                "type": "Property",
                "value": [],
                "instanceId": format!("urn:ngsi-ld:Instance:{}", uuid::Uuid::new_v4()),
                "deletedAt": ts,
            });
            match target.get_mut("scope").and_then(Value::as_array_mut) {
                Some(arr) if arr.first().is_some_and(|i| i.is_object()) => arr.push(inst),
                _ => {
                    target.insert("scope".into(), Value::Array(vec![inst]));
                }
            }
            return Ok::<(), std::convert::Infallible>(());
        }
        if let Some(arr) = target.get_mut(attr_iri).and_then(Value::as_array_mut) {
            if arr.is_empty() {
                return Ok(());
            }
            had = true;
            let atype = arr
                .first()
                .and_then(|i| i.get("type"))
                .and_then(Value::as_str)
                .unwrap_or("Property")
                .to_owned();
            let mut inst = Map::new();
            inst.insert("type".into(), Value::String(atype.clone()));
            let null = Value::String("urn:ngsi-ld:null".into());
            match atype.as_str() {
                "Relationship" => {
                    inst.insert("object".into(), null);
                }
                "LanguageProperty" => {
                    inst.insert(
                        "languageMap".into(),
                        serde_json::json!({"@none": "urn:ngsi-ld:null"}),
                    );
                }
                "JsonProperty" => {
                    inst.insert("json".into(), null);
                }
                "VocabProperty" => {
                    inst.insert("vocab".into(), null);
                }
                "ListProperty" => {
                    inst.insert("valueList".into(), null);
                }
                "ListRelationship" => {
                    inst.insert("objectList".into(), null);
                }
                _ => {
                    inst.insert("value".into(), null);
                }
            }
            if let Some(ds) = dataset_id {
                inst.insert("datasetId".into(), Value::String(ds.to_owned()));
            }
            inst.insert(
                "instanceId".into(),
                Value::String(format!("urn:ngsi-ld:Instance:{}", uuid::Uuid::new_v4())),
            );
            inst.insert("deletedAt".into(), Value::String(ts.to_owned()));
            arr.push(Value::Object(inst));
        }
        Ok(())
    });
    if let Err(e) = r {
        tracing::warn!("temporal attr mirror failed: {e}");
    }
    had
}
