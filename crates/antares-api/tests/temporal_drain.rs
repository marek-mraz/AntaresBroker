// SPDX-License-Identifier: EUPL-1.2
//! The temporal seam drains once per request (ADR-0013): a write's history
//! events reach the driver in ONE `event_list` call after the handler ran,
//! a client reading its history straight after the write finds it, and a
//! driver failure in the drain is counted but never alters the response of
//! a write that already committed.

use antares_api::{AppState, TemporalRecord};
use antares_model::{NgsiError, TenantId};
use antares_sql::store::any::AnyStore;
use antares_sql::store::Store;
use antares_store::{filter, MutateFn, TemporalDriver, TemporalEvent};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tower::ServiceExt;

/// Forwards to the fused store, counting drain calls and the events they
/// carried.
struct Counting {
    inner: Arc<AnyStore>,
    calls: AtomicUsize,
    events: AtomicUsize,
}

impl TemporalDriver for Counting {
    fn event_list(&self, evs: &[TemporalEvent]) -> Result<(), NgsiError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.events.fetch_add(evs.len(), Ordering::SeqCst);
        TemporalDriver::event_list(&*self.inner, evs)
    }
    fn temporal_append(
        &self,
        t: &TenantId,
        id: &str,
        shell: &Value,
        additions: &Value,
    ) -> Result<(), NgsiError> {
        TemporalDriver::temporal_append(&*self.inner, t, id, shell, additions)
    }
    fn query_temporal(
        &self,
        t: &TenantId,
        f: &filter::TemporalFilter<'_>,
    ) -> Result<filter::TemporalOutcome, NgsiError> {
        TemporalDriver::query_temporal(&*self.inner, t, f)
    }
    fn get_temporal(
        &self,
        t: &TenantId,
        id: &str,
        f: &filter::TemporalFilter<'_>,
    ) -> Result<Option<Value>, NgsiError> {
        TemporalDriver::get_temporal(&*self.inner, t, id, f)
    }
    fn get(&self, t: &TenantId, id: &str) -> Result<Option<Value>, NgsiError> {
        TemporalDriver::get(&*self.inner, t, id)
    }
    fn create(&self, t: &TenantId, id: &str, doc: Value) -> Result<bool, NgsiError> {
        TemporalDriver::create(&*self.inner, t, id, doc)
    }
    fn upsert(&self, t: &TenantId, id: &str, doc: Value) -> Result<bool, NgsiError> {
        TemporalDriver::upsert(&*self.inner, t, id, doc)
    }
    fn delete(&self, t: &TenantId, id: &str) -> Result<bool, NgsiError> {
        TemporalDriver::delete(&*self.inner, t, id)
    }
    fn list(&self, t: &TenantId) -> Result<Vec<Value>, NgsiError> {
        TemporalDriver::list(&*self.inner, t)
    }
    fn mutate_boxed<'a>(
        &self,
        t: &TenantId,
        id: &str,
        f: MutateFn<'a>,
    ) -> Result<Option<Result<(), ()>>, NgsiError> {
        TemporalDriver::mutate_boxed(&*self.inner, t, id, f)
    }
}

/// A driver whose drain always fails.
struct Failing;

impl TemporalDriver for Failing {
    fn event_list(&self, _evs: &[TemporalEvent]) -> Result<(), NgsiError> {
        Err(NgsiError::InternalError("history store down".into()))
    }
    fn temporal_append(
        &self,
        _t: &TenantId,
        _id: &str,
        _shell: &Value,
        _additions: &Value,
    ) -> Result<(), NgsiError> {
        Err(NgsiError::InternalError("history store down".into()))
    }
    fn query_temporal(
        &self,
        _t: &TenantId,
        _f: &filter::TemporalFilter<'_>,
    ) -> Result<filter::TemporalOutcome, NgsiError> {
        Err(NgsiError::InternalError("history store down".into()))
    }
    fn get_temporal(
        &self,
        _t: &TenantId,
        _id: &str,
        _f: &filter::TemporalFilter<'_>,
    ) -> Result<Option<Value>, NgsiError> {
        Err(NgsiError::InternalError("history store down".into()))
    }
    fn get(&self, _t: &TenantId, _id: &str) -> Result<Option<Value>, NgsiError> {
        Ok(None)
    }
    fn create(&self, _t: &TenantId, _id: &str, _doc: Value) -> Result<bool, NgsiError> {
        Ok(false)
    }
    fn upsert(&self, _t: &TenantId, _id: &str, _doc: Value) -> Result<bool, NgsiError> {
        Ok(false)
    }
    fn delete(&self, _t: &TenantId, _id: &str) -> Result<bool, NgsiError> {
        Ok(false)
    }
    fn list(&self, _t: &TenantId) -> Result<Vec<Value>, NgsiError> {
        Ok(Vec::new())
    }
    fn mutate_boxed<'a>(
        &self,
        _t: &TenantId,
        _id: &str,
        _f: MutateFn<'a>,
    ) -> Result<Option<Result<(), ()>>, NgsiError> {
        Ok(None)
    }
}

fn counting_state() -> (AppState, Arc<Counting>) {
    let store = Arc::new(AnyStore::Mem(Store::default()));
    let counting = Arc::new(Counting {
        inner: store.clone(),
        calls: AtomicUsize::new(0),
        events: AtomicUsize::new(0),
    });
    let mut st = AppState::with_drivers(
        "me".into(),
        store,
        counting.clone(),
        antares_sql::StoreMode::Memory,
    );
    antares_api::notify::wire(&mut st);
    (st, counting)
}

async fn req(st: &AppState, method: &str, path: &str, body: Option<Value>) -> (StatusCode, Value) {
    let mut b = Request::builder().method(method).uri(path);
    let body = match body {
        Some(v) => {
            let s = v.to_string();
            b = b
                .header("Content-Type", "application/json")
                .header("Content-Length", s.len());
            Body::from(s)
        }
        None => Body::empty(),
    };
    let resp = antares_api::router(st.clone())
        .oneshot(b.body(body).expect("req"))
        .await
        .expect("resp");
    let status = resp.status();
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn entity(n: u32) -> Value {
    json!({"id": format!("urn:ngsi-ld:D:{n}"), "type": "D",
           "speed": {"type": "Property", "value": n, "observedAt": "2026-01-01T00:00:00Z"},
           "heading": {"type": "Property", "value": n * 10}})
}

#[tokio::test(flavor = "multi_thread")]
async fn a_batch_write_drains_in_one_driver_call() {
    let (st, c) = counting_state();
    let (status, body) = req(
        &st,
        "POST",
        "/ngsi-ld/v1/entityOperations/upsert",
        Some(json!([entity(1), entity(2), entity(3)])),
    )
    .await;
    assert_eq!(status, 201, "{body}");
    assert_eq!(
        c.calls.load(Ordering::SeqCst),
        1,
        "one drain per request, not per entity"
    );
    assert_eq!(
        c.events.load(Ordering::SeqCst),
        6,
        "three entities x two attribute instances, every one an event"
    );
    // read-your-writes: the history exists the moment the response is out
    let (status, body) = req(
        &st,
        "GET",
        "/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:D:2",
        None,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["speed"][0]["value"], 2, "{body}");
    assert_eq!(body["heading"][0]["value"], 20, "{body}");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unchanged_re_put_produces_no_events() {
    let (st, c) = counting_state();
    let (status, _) = req(&st, "POST", "/ngsi-ld/v1/entities", Some(entity(7))).await;
    assert_eq!(status, 201);
    let after_create = c.events.load(Ordering::SeqCst);
    assert_eq!(after_create, 2);
    // gate 1: identical values again — the diff finds nothing, the seam
    // sees no event and the driver no call
    let (status, _) = req(
        &st,
        "POST",
        "/ngsi-ld/v1/entityOperations/upsert",
        Some(json!([entity(7)])),
    )
    .await;
    assert!(status.is_success(), "{status}");
    assert_eq!(
        c.events.load(Ordering::SeqCst),
        after_create,
        "no event for an unchanged value"
    );
    assert_eq!(
        c.calls.load(Ordering::SeqCst),
        1,
        "an empty buffer is not drained"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn reads_never_drain() {
    let (st, c) = counting_state();
    let (status, _) = req(&st, "GET", "/ngsi-ld/v1/entities?type=D", None).await;
    assert_eq!(status, 200);
    assert_eq!(c.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failing_drain_keeps_the_2xx_and_is_counted() {
    let store = Arc::new(AnyStore::Mem(Store::default()));
    let mut st = AppState::with_drivers(
        "me".into(),
        store,
        Arc::new(Failing),
        antares_sql::StoreMode::Memory,
    );
    antares_api::notify::wire(&mut st);
    let before = antares_api::history::drain_errors();
    let (status, body) = req(&st, "POST", "/ngsi-ld/v1/entities", Some(entity(9))).await;
    assert_eq!(
        status, 201,
        "the write stood; the drain failing after it is not the client's problem: {body}"
    );
    assert_eq!(body, Value::Null, "no error body rides a 201");
    assert_eq!(
        antares_api::history::drain_errors(),
        before + 1,
        "the failure is counted exactly once"
    );
    let (status, body) = req(&st, "GET", "/ngsi-ld/v1/entities/urn:ngsi-ld:D:9", None).await;
    assert_eq!(status, 200, "current state is intact: {body}");
    let (status, health) = req(&st, "GET", "/q/health", None).await;
    assert_eq!(status, 200);
    assert!(
        health["temporalDrainErrors"]
            .as_u64()
            .is_some_and(|n| n >= 1),
        "the counter is visible in /q/health: {health}"
    );
}

/// Gate 2 (ANTARES_TEMPORAL_RECORD=observed): the instance with observedAt
/// enters history, the one without does not — while current state keeps
/// both and its modifiedAt still moves.
#[tokio::test(flavor = "multi_thread")]
async fn observed_mode_records_only_observed_instances() {
    let (mut st, c) = counting_state();
    st.temporal_record = TemporalRecord::Observed;
    let (status, _) = req(&st, "POST", "/ngsi-ld/v1/entities", Some(entity(11))).await;
    assert_eq!(status, 201);
    assert_eq!(
        c.events.load(Ordering::SeqCst),
        1,
        "heading carries no observedAt and is gated out before the driver"
    );
    let (status, body) = req(
        &st,
        "GET",
        "/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:D:11",
        None,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["speed"][0]["value"], 11, "{body}");
    assert!(
        body.get("heading").is_none(),
        "no history for a never-observed attribute: {body}"
    );
    // a metadata-only change still updates current state, still no history
    let (status, _) = req(
        &st,
        "PATCH",
        "/ngsi-ld/v1/entities/urn:ngsi-ld:D:11/attrs/heading",
        Some(json!({"type": "Property", "value": 999})),
    )
    .await;
    assert_eq!(status, 204);
    let (status, body) = req(
        &st,
        "GET",
        "/ngsi-ld/v1/entities/urn:ngsi-ld:D:11?options=sysAttrs",
        None,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["heading"]["value"], 999, "current state moved: {body}");
    assert!(body["heading"]["modifiedAt"].is_string(), "{body}");
    assert_eq!(c.events.load(Ordering::SeqCst), 1, "still gated out");
    let (_, body) = req(
        &st,
        "GET",
        "/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:D:11",
        None,
    )
    .await;
    assert!(body.get("heading").is_none(), "{body}");
}

/// The default (`all`) records both instances — the gate is off, not
/// merely lenient.
#[tokio::test(flavor = "multi_thread")]
async fn all_mode_records_unobserved_instances_too() {
    let (st, c) = counting_state();
    assert_eq!(
        st.temporal_record,
        TemporalRecord::All,
        "all is the default"
    );
    let (status, _) = req(&st, "POST", "/ngsi-ld/v1/entities", Some(entity(12))).await;
    assert_eq!(status, 201);
    assert_eq!(c.events.load(Ordering::SeqCst), 2);
    let (_, body) = req(
        &st,
        "GET",
        "/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:D:12",
        None,
    )
    .await;
    assert_eq!(body["heading"][0]["value"], 120, "{body}");
}

/// `none`: nothing the entity endpoints do reaches the driver — the
/// observed instance included — while current state and the temporal
/// API's own write path keep working.
#[tokio::test(flavor = "multi_thread")]
async fn none_mode_records_nothing() {
    let (mut st, c) = counting_state();
    st.temporal_record = TemporalRecord::None;
    let (status, _) = req(&st, "POST", "/ngsi-ld/v1/entities", Some(entity(13))).await;
    assert_eq!(status, 201);
    let (status, _) = req(
        &st,
        "PATCH",
        "/ngsi-ld/v1/entities/urn:ngsi-ld:D:13/attrs/speed",
        Some(json!({"type": "Property", "value": 14, "observedAt": "2026-01-01T00:00:01Z"})),
    )
    .await;
    assert_eq!(status, 204);
    let (status, _) = req(
        &st,
        "POST",
        "/ngsi-ld/v1/entityOperations/upsert",
        Some(json!([entity(15), entity(16)])),
    )
    .await;
    assert!(status == 201 || status == 204, "{status}");
    assert_eq!(
        c.calls.load(Ordering::SeqCst),
        0,
        "the driver was never called"
    );
    assert_eq!(c.events.load(Ordering::SeqCst), 0);
    let (status, body) = req(&st, "GET", "/ngsi-ld/v1/entities/urn:ngsi-ld:D:13", None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["speed"]["value"], 14, "current state moved: {body}");
    let (status, body) = req(
        &st,
        "GET",
        "/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:D:13",
        None,
    )
    .await;
    assert_eq!(
        status, 404,
        "no history exists for an entity nobody recorded: {body}"
    );
    // the temporal API's own write path is not the gate's business
    let (status, body) = req(
        &st,
        "POST",
        "/ngsi-ld/v1/temporal/entities",
        Some(json!({
            "id": "urn:ngsi-ld:D:14",
            "type": "Device",
            "speed": [{"type": "Property", "value": 1, "observedAt": "2026-01-01T00:00:00Z"}]
        })),
    )
    .await;
    assert!(status == 201 || status == 204, "{status} {body}");
    let (status, body) = req(
        &st,
        "GET",
        "/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:D:14",
        None,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["speed"][0]["value"], 1, "{body}");
}

/// The env spelling accepted by the broker — every mode, and a typo is a
/// startup error rather than a silent `all`.
#[test]
fn temporal_record_parses_every_mode() {
    assert_eq!("all".parse::<TemporalRecord>(), Ok(TemporalRecord::All));
    assert_eq!(
        "observed".parse::<TemporalRecord>(),
        Ok(TemporalRecord::Observed)
    );
    assert_eq!("none".parse::<TemporalRecord>(), Ok(TemporalRecord::None));
    let Err(err) = "observedAt".parse::<TemporalRecord>() else {
        panic!("a typo must not parse")
    };
    assert!(err.contains("all|observed|none"), "{err}");
}
