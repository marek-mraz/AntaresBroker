// SPDX-License-Identifier: EUPL-1.2
//! What a write puts into history, cell by cell, under each recording mode.
//!
//! 4.5.7 makes an Attribute's temporal evolution "the sequence of instances
//! of the referred Property during a period of time", each instance placed
//! "at a particular point in time, which is recorded as a Temporal Property
//! of the instance (typically `observedAt`)". Two writes are therefore the
//! same instance when they carry the same value at the same point in time,
//! and different instances when either moves — and a write carrying no
//! Temporal Property has no point in time of its own, which is what
//! `ANTARES_TEMPORAL_RECORD` decides the fate of.
//!
//! Every row below is asserted on the default axis and, where the mode makes
//! a difference, on `?timeproperty=modifiedAt` as well: an instance the gate
//! dropped is on NO axis, not merely off the default one.

use antares_api::{AppState, TemporalRecord};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

const ATTR: &str = "speed";
const T0: &str = "2026-01-01T09:00:00Z";
const T1: &str = "2026-01-01T10:00:00Z";
const T2: &str = "2026-01-01T11:00:00Z";

/// The table runs on whatever backend the harness names: `AppState::new`
/// composes from `ANTARES_TEST_STORE`, so CI puts the same rows through the
/// in-memory and the durable redb store without a second copy of the file.
fn state(mode: TemporalRecord) -> AppState {
    let mut st = AppState::new("me".into());
    st.temporal_record = mode;
    antares_api::wire(&mut st);
    st
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
        .oneshot(b.body(body).expect("request"))
        .await
        .expect("response");
    let status = resp.status();
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// One Property instance, with or without an `observedAt`.
fn prop(value: i64, observed_at: Option<&str>) -> Value {
    let mut v = json!({"type": "Property", "value": value});
    if let Some(t) = observed_at {
        v["observedAt"] = t.into();
    }
    v
}

/// The instances of `ATTR` in the Temporal Evolution of one Entity, on the
/// named axis. An absent Attribute is zero instances, not an error: 4.5.7
/// says the evolution IS the sequence, and an empty sequence is a valid one.
async fn instances(st: &AppState, id: &str, timeproperty: Option<&str>) -> Vec<Value> {
    let q = match timeproperty {
        // A `timeproperty` alone is not a temporal query, so the request
        // carries the open interval that selects the whole evolution.
        Some(tp) => format!("?timerel=after&timeAt={T0}&timeproperty={tp}"),
        None => String::new(),
    };
    let (status, body) = req(
        st,
        "GET",
        &format!("/ngsi-ld/v1/temporal/entities/{id}{q}"),
        None,
    )
    .await;
    if status == StatusCode::NOT_FOUND {
        return Vec::new();
    }
    assert_eq!(status, 200, "temporal read of {id}: {body}");
    match body.get(ATTR) {
        None => Vec::new(),
        Some(Value::Array(a)) => a.clone(),
        Some(one) => vec![one.clone()],
    }
}

fn ids_of(instances: &[Value]) -> Vec<String> {
    instances
        .iter()
        .filter_map(|i| i.get("instanceId").and_then(Value::as_str))
        .map(str::to_owned)
        .collect()
}

/// One request against the row's Entity: method, path suffix under the
/// entity, body. The Entity itself is created by the harness with no
/// Attributes of its own.
type Step = (&'static str, String, Option<Value>);

/// One row of the recording matrix: a name, the writes that build it, and
/// the instance count each mode must end with on the default axis.
struct Cell {
    row: u32,
    what: &'static str,
    writes: fn() -> Vec<Step>,
    all: usize,
    observed: usize,
}

/// 5.6.3 Append Attributes — how an Attribute first appears on an Entity.
fn append(v: Value) -> Step {
    ("POST", "/attrs".to_owned(), Some(json!({ATTR: v})))
}

/// 5.6.19 Replace Attribute — a later write of one that already exists.
fn put(v: Value) -> Step {
    ("PUT", format!("/attrs/{ATTR}"), Some(v))
}

fn patch(v: Value) -> Step {
    ("PATCH", format!("/attrs/{ATTR}"), Some(v))
}

/// The table. Every row states its rule in `what`; a row whose behaviour
/// differs from its counts is a defect in the recorder, never a number to
/// adjust here.
const MATRIX: &[Cell] = &[
    Cell {
        row: 1,
        what: "an attribute created with no observedAt has no point in time of its own",
        writes: || vec![append(prop(1, None))],
        all: 1,
        observed: 0,
    },
    Cell {
        row: 2,
        what: "an attribute created with observedAt is one instance in either mode",
        writes: || vec![append(prop(1, Some(T1)))],
        all: 1,
        observed: 1,
    },
    Cell {
        row: 3,
        what: "re-writing the same value with no observedAt changes nothing at all",
        writes: || vec![append(prop(1, None)), put(prop(1, None))],
        all: 1,
        observed: 0,
    },
    Cell {
        row: 4,
        what: "a new value with no observedAt is a new instance under `all` only",
        writes: || vec![append(prop(1, None)), put(prop(2, None))],
        all: 2,
        observed: 0,
    },
    Cell {
        row: 5,
        what: "the same value at the same observedAt is the same instance",
        writes: || vec![append(prop(1, Some(T1))), put(prop(1, Some(T1)))],
        all: 1,
        observed: 1,
    },
    Cell {
        row: 6,
        what: "a corrected value at the same observedAt replaces that instance",
        writes: || vec![append(prop(1, Some(T1))), put(prop(2, Some(T1)))],
        all: 1,
        observed: 1,
    },
    Cell {
        row: 7,
        what: "the same value at a new observedAt is a new instance",
        writes: || vec![append(prop(1, Some(T1))), put(prop(1, Some(T2)))],
        all: 2,
        observed: 2,
    },
    Cell {
        row: 8,
        what: "a new value at a new observedAt is a new instance",
        writes: || vec![append(prop(1, Some(T1))), put(prop(2, Some(T2)))],
        all: 2,
        observed: 2,
    },
    Cell {
        row: 9,
        what: "an observedAt older than the last one is recorded, not refused",
        writes: || vec![append(prop(1, Some(T1))), put(prop(2, Some(T0)))],
        all: 2,
        observed: 2,
    },
    Cell {
        // 4.5.2.2 lists `unitCode` among the Property's own members, beside
        // `value` and `observedAt`, and 4.5.7 records each instance as "an
        // instance of the Property (as mandated by clause 4.5.2)". A Property
        // whose unit changed is therefore a different instance, and under a
        // mode that records every write, dropping it would lose the unit
        // change from the history entirely.
        row: 10,
        what: "a changed unit is a changed Property, not merely changed metadata",
        writes: || {
            vec![
                append(prop(1, None)),
                patch(json!({"type": "Property", "value": 1, "unitCode": "KMH"})),
            ]
        },
        all: 2,
        observed: 0,
    },
    Cell {
        row: 12,
        what: "two instances at distinct observedAt in one write are two instances",
        writes: || {
            vec![append(json!([
                {"type": "Property", "value": 1, "observedAt": T1, "datasetId": "urn:ds:a"},
                {"type": "Property", "value": 2, "observedAt": T2, "datasetId": "urn:ds:b"},
            ]))]
        },
        all: 2,
        observed: 2,
    },
];

/// Build one row's entity and return its id.
async fn run_cell(st: &AppState, cell: &Cell) -> String {
    let id = format!("urn:ngsi-ld:Matrix:{}", cell.row);
    let (status, body) = req(
        st,
        "POST",
        "/ngsi-ld/v1/entities",
        Some(json!({"id": id, "type": "Device"})),
    )
    .await;
    assert_eq!(status, 201, "row {}: entity: {body}", cell.row);
    for (method, suffix, payload) in (cell.writes)() {
        let (status, body) = req(
            st,
            method,
            &format!("/ngsi-ld/v1/entities/{id}{suffix}"),
            payload,
        )
        .await;
        assert!(
            status.is_success(),
            "row {} ({}): {method} {suffix} answered {status}: {body}",
            cell.row,
            cell.what
        );
    }
    id
}

/// `ANTARES_TEMPORAL_RECORD=all`: every write is a point in time, whether or
/// not the producer supplied one.
#[tokio::test(flavor = "multi_thread")]
async fn the_recording_matrix_under_all() {
    let st = state(TemporalRecord::All);
    for cell in MATRIX {
        let id = run_cell(&st, cell).await;
        let got = instances(&st, &id, None).await;
        assert_eq!(
            got.len(),
            cell.all,
            "row {} ({}): expected {} instance(s) under `all`, got {}: {got:?}",
            cell.row,
            cell.what,
            cell.all,
            got.len()
        );
    }
}

/// `ANTARES_TEMPORAL_RECORD=observed`: only a write that carries its own
/// Temporal Property becomes history.
#[tokio::test(flavor = "multi_thread")]
async fn the_recording_matrix_under_observed() {
    let st = state(TemporalRecord::Observed);
    for cell in MATRIX {
        let id = run_cell(&st, cell).await;
        let got = instances(&st, &id, None).await;
        assert_eq!(
            got.len(),
            cell.observed,
            "row {} ({}): expected {} instance(s) under `observed`, got {}: {got:?}",
            cell.row,
            cell.what,
            cell.observed,
            got.len()
        );
    }
}

/// Rows 5 and 6 turn on identity, not on counting: 4.5.7 asks systems to
/// maintain an `instanceId` per instance, and two writes at one point in
/// time are one instance — so the id must survive a correction of the value
/// rather than a second id appearing beside it.
#[tokio::test(flavor = "multi_thread")]
async fn one_point_in_time_keeps_one_instance_id() {
    let st = state(TemporalRecord::All);
    let id = "urn:ngsi-ld:Matrix:identity";
    let (status, _) = req(
        &st,
        "POST",
        "/ngsi-ld/v1/entities",
        Some(json!({"id": id, "type": "Device"})),
    )
    .await;
    assert_eq!(status, 201);

    let (status, body) = req(
        &st,
        "POST",
        &format!("/ngsi-ld/v1/entities/{id}/attrs"),
        Some(json!({ATTR: prop(1, Some(T1))})),
    )
    .await;
    assert!(status.is_success(), "append: {status} {body}");
    let first = ids_of(&instances(&st, id, None).await);
    assert_eq!(first.len(), 1, "one write, one instance: {first:?}");

    // same value, same instant: nothing new, nothing renamed
    let (status, _) = req(
        &st,
        "PUT",
        &format!("/ngsi-ld/v1/entities/{id}/attrs/{ATTR}"),
        Some(prop(1, Some(T1))),
    )
    .await;
    assert!(status.is_success());
    assert_eq!(ids_of(&instances(&st, id, None).await), first, "row 5");

    // corrected value, same instant: the instance is replaced, not added to
    let (status, _) = req(
        &st,
        "PUT",
        &format!("/ngsi-ld/v1/entities/{id}/attrs/{ATTR}"),
        Some(prop(7, Some(T1))),
    )
    .await;
    assert!(status.is_success());
    let after = instances(&st, id, None).await;
    assert_eq!(
        ids_of(&after),
        first,
        "row 6: the instance id is the point in time"
    );
    assert_eq!(after.len(), 1, "row 6 added an instance: {after:?}");
    assert_eq!(
        after[0]["value"], 7,
        "row 6 kept the stale value: {after:?}"
    );
}

/// A write the gate dropped is on NO axis. Asking for `modifiedAt` — a
/// Temporal Property every write does set — must not resurrect an instance
/// that was never recorded, or `observed` would only be hiding history
/// rather than declining to keep it.
#[tokio::test(flavor = "multi_thread")]
async fn a_gated_write_is_absent_from_every_axis() {
    let st = state(TemporalRecord::Observed);
    let id = "urn:ngsi-ld:Matrix:axes";
    let (status, _) = req(
        &st,
        "POST",
        "/ngsi-ld/v1/entities",
        Some(json!({"id": id, "type": "Device"})),
    )
    .await;
    assert_eq!(status, 201);
    // row 4 under `observed`: two values, neither observed
    let (status, body) = req(
        &st,
        "POST",
        &format!("/ngsi-ld/v1/entities/{id}/attrs"),
        Some(json!({ATTR: prop(1, None)})),
    )
    .await;
    assert!(status.is_success(), "append: {status} {body}");
    let (status, body) = req(
        &st,
        "PUT",
        &format!("/ngsi-ld/v1/entities/{id}/attrs/{ATTR}"),
        Some(prop(2, None)),
    )
    .await;
    assert!(status.is_success(), "replace: {status} {body}");
    assert!(
        instances(&st, id, None).await.is_empty(),
        "default axis kept a gated write"
    );
    assert!(
        instances(&st, id, Some("modifiedAt")).await.is_empty(),
        "the modifiedAt axis resurrected a write that was never recorded"
    );
    // current state is untouched by the gate — the entity still moved
    let (status, body) = req(&st, "GET", &format!("/ngsi-ld/v1/entities/{id}"), None).await;
    assert_eq!(status, 200);
    assert_eq!(
        body[ATTR]["value"], 2,
        "current state lost the write: {body}"
    );
}

/// Row 11. 4.5.7: "In case the Property is deleted, an instance of the
/// Property is recorded with its value set to the URI `urn:ngsi-ld:null` and
/// the deletedAt Temporal Property set." The instances already recorded are
/// not removed — a deletion ends the series, it does not erase it — and the
/// clause states this for the temporal representation itself, with no
/// dependence on whether the deleting request carried an `observedAt`.
#[tokio::test(flavor = "multi_thread")]
async fn deleting_an_attribute_ends_its_series_in_either_mode() {
    for mode in [TemporalRecord::All, TemporalRecord::Observed] {
        let st = state(mode);
        let id = "urn:ngsi-ld:Matrix:deleted";
        let (status, body) = req(
            &st,
            "POST",
            "/ngsi-ld/v1/entities",
            Some(json!({"id": id, "type": "Device", ATTR: prop(7, Some(T1))})),
        )
        .await;
        assert_eq!(status, 201, "{mode:?}: {body}");
        assert_eq!(
            instances(&st, id, None).await.len(),
            1,
            "{mode:?}: the observed write should be one instance"
        );

        let (status, body) = req(
            &st,
            "DELETE",
            &format!("/ngsi-ld/v1/entities/{id}/attrs/{ATTR}"),
            None,
        )
        .await;
        assert_eq!(status, 204, "{mode:?}: delete: {body}");

        let got = instances(&st, id, None).await;
        assert_eq!(
            got.len(),
            2,
            "{mode:?}: the recorded instance and its deletion: {got:?}"
        );
        let deletion = got
            .iter()
            .find(|i| i.get("deletedAt").is_some())
            .unwrap_or_else(|| panic!("{mode:?}: no instance carries deletedAt: {got:?}"));
        assert_eq!(
            deletion["value"], "urn:ngsi-ld:null",
            "{mode:?}: the deletion instance must carry the NGSI-LD Null: {got:?}"
        );
        assert!(
            got.iter().any(|i| i["value"] == 7),
            "{mode:?}: deleting erased the history it should have ended: {got:?}"
        );
    }
}

/// Row 13. 5.7.4 aggregates over INSTANCES, so a value that repeats at
/// distinct points in time counts once per instance. A recorder that deduped
/// by value would leave `totalCount` and `avg` describing a series that was
/// never observed.
#[tokio::test(flavor = "multi_thread")]
async fn aggregation_counts_instances_not_distinct_values() {
    let st = state(TemporalRecord::All);
    let id = "urn:ngsi-ld:Matrix:aggregate";
    let (status, body) = req(
        &st,
        "POST",
        "/ngsi-ld/v1/entities",
        Some(json!({"id": id, "type": "Device"})),
    )
    .await;
    assert_eq!(status, 201, "{body}");
    let (status, body) = req(
        &st,
        "POST",
        &format!("/ngsi-ld/v1/entities/{id}/attrs"),
        Some(json!({ATTR: prop(5, Some(T0))})),
    )
    .await;
    assert!(status.is_success(), "{body}");
    // the SAME value at two further instants
    for t in [T1, T2] {
        let (status, body) = req(
            &st,
            "PUT",
            &format!("/ngsi-ld/v1/entities/{id}/attrs/{ATTR}"),
            Some(prop(5, Some(t))),
        )
        .await;
        assert!(status.is_success(), "{body}");
    }
    assert_eq!(
        instances(&st, id, None).await.len(),
        3,
        "three instants, three instances"
    );

    let (status, body) = req(
        &st,
        "GET",
        &format!(
            "/ngsi-ld/v1/temporal/entities/{id}\
             ?options=aggregatedValues&aggrMethods=totalCount,avg&aggrPeriodDuration=PT0S\
             &attrs={ATTR}&timerel=after&timeAt={T0}"
        ),
        None,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        body[ATTR]["totalCount"][0][0], 3,
        "the repeated value was counted once: {body}"
    );
    assert_eq!(
        body[ATTR]["avg"][0][0].as_f64(),
        Some(5.0),
        "avg over three equal instances is the value itself: {body}"
    );
}
