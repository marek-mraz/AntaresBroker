// SPDX-License-Identifier: EUPL-1.2
//! 4.5.7: a temporal Property instance is the Property "at a particular point
//! in time … recorded as a Temporal Property of the instance (typically
//! observedAt)". A Core-API re-send carrying the same observedAt for the same
//! attribute and datasetId therefore corrects THAT instance instead of
//! appending a second one; an instance without observedAt has no such point
//! in time and stays append-only.

use antares_api::AppState;
use antares_sql::store::any::AnyStore;
use antares_sql::store::Store;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;

fn state() -> AppState {
    let store = Arc::new(AnyStore::Mem(Store::default()));
    let mut st = AppState::with_drivers(
        "me".into(),
        store.clone(),
        store,
        antares_sql::StoreMode::Memory,
    );
    antares_api::notify::wire(&mut st);
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

fn instances(doc: &Value, attr: &str) -> Vec<Value> {
    match doc.get(attr) {
        Some(Value::Array(a)) => a.clone(),
        Some(v) => vec![v.clone()],
        None => vec![],
    }
}

const ID: &str = "urn:ngsi-ld:D:1";
const T1: &str = "2026-01-01T00:00:00Z";
const T2: &str = "2026-01-01T00:01:00Z";

#[tokio::test(flavor = "multi_thread")]
async fn a_resend_with_the_same_observed_at_corrects_the_instance() {
    let st = state();
    let (s, b) = req(
        &st,
        "POST",
        "/ngsi-ld/v1/entities",
        Some(json!({"id": ID, "type": "D",
            "speed": {"type": "Property", "value": 1, "observedAt": T1},
            "heading": {"type": "Property", "value": 10}})),
    )
    .await;
    assert_eq!(s, 201, "{b}");
    let patch = |v: Value| json!({"speed": {"type": "Property", "value": v, "observedAt": T1}});
    let (s, b) = req(
        &st,
        "PATCH",
        &format!("/ngsi-ld/v1/entities/{ID}/attrs"),
        Some(patch(json!(2))),
    )
    .await;
    assert_eq!(s, 204, "{b}");
    let (s, doc) = req(
        &st,
        "GET",
        &format!("/ngsi-ld/v1/temporal/entities/{ID}"),
        None,
    )
    .await;
    assert_eq!(s, 200, "{doc}");
    let speed = instances(&doc, "speed");
    assert_eq!(
        speed.len(),
        1,
        "same observedAt = same instance, not two: {doc}"
    );
    assert_eq!(
        speed[0]["value"], 2,
        "the corrected value replaced the first: {doc}"
    );

    // a new point in time is a new instance
    let (s, _) = req(
        &st,
        "PATCH",
        &format!("/ngsi-ld/v1/entities/{ID}/attrs"),
        Some(json!({"speed": {"type": "Property", "value": 3, "observedAt": T2}})),
    )
    .await;
    assert_eq!(s, 204);
    let (_, doc) = req(
        &st,
        "GET",
        &format!("/ngsi-ld/v1/temporal/entities/{ID}"),
        None,
    )
    .await;
    let speed = instances(&doc, "speed");
    assert_eq!(speed.len(), 2, "{doc}");
    let ids: std::collections::BTreeSet<_> = speed
        .iter()
        .map(|i| i["instanceId"].as_str().unwrap_or("").to_owned())
        .collect();
    assert_eq!(
        ids.len(),
        2,
        "distinct instances carry distinct instanceIds: {doc}"
    );

    // no observedAt: no point in time to key on, every change appends
    for v in [11, 12] {
        let (s, _) = req(
            &st,
            "PATCH",
            &format!("/ngsi-ld/v1/entities/{ID}/attrs"),
            Some(json!({"heading": {"type": "Property", "value": v}})),
        )
        .await;
        assert_eq!(s, 204);
    }
    let (_, doc) = req(
        &st,
        "GET",
        &format!("/ngsi-ld/v1/temporal/entities/{ID}"),
        None,
    )
    .await;
    assert_eq!(instances(&doc, "heading").len(), 3, "{doc}");
}

#[tokio::test(flavor = "multi_thread")]
async fn dataset_ids_keep_their_own_instances_at_one_observed_at() {
    let st = state();
    let (s, b) = req(
        &st,
        "POST",
        "/ngsi-ld/v1/entities",
        Some(json!({"id": ID, "type": "D",
            "speed": [
                {"type": "Property", "value": 1, "observedAt": T1},
                {"type": "Property", "value": 5, "observedAt": T1, "datasetId": "urn:ngsi-ld:ds:gps"}]})),
    )
    .await;
    assert_eq!(s, 201, "{b}");
    let (_, doc) = req(
        &st,
        "GET",
        &format!("/ngsi-ld/v1/temporal/entities/{ID}"),
        None,
    )
    .await;
    let speed = instances(&doc, "speed");
    assert_eq!(
        speed.len(),
        2,
        "two datasets, two instances at the same instant: {doc}"
    );
    assert_ne!(speed[0]["instanceId"], speed[1]["instanceId"], "{doc}");
}
