// SPDX-License-Identifier: EUPL-1.2
//! 4.5.5.1: the datasetId "is of datatype URI, or equal to the JSON-LD
//! keyword `@none`", and "if no datasetId is provided, or `"datasetId":
//! "@none"` is supplied, it is considered as the default Attribute
//! instance". A default instance never carries a datasetId of its own —
//! "the datasetId of the default Attribute instance is never explicitly
//! included in responses" — so every path that selects one instance by the
//! client's datasetId has to read `@none` as "the one without a datasetId".
//!
//! Instance members reach that rule through 5.5.7 expansion, which drops an
//! `@none` before the document is stored. The `?datasetId=` parameter of
//! Delete Attribute (5.6.5.4) and Delete Attribute from the Temporal
//! Evolution (5.6.13.4) does not pass through expansion, and matched the
//! literal string against instances that never carry it.

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

const ID: &str = "urn:ngsi-ld:Vehicle:ds1";
const DS: &str = "urn:ngsi-ld:Dataset:a";

async fn call(st: &AppState, method: &str, path: &str, body: &str) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(method)
        .uri(format!("/ngsi-ld/v1/{path}"))
        .header("Content-Type", "application/json")
        .header("Content-Length", body.len())
        .body(Body::from(body.to_owned()))
        .expect("request");
    let res = antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("response");
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .expect("body");
    let parsed = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, parsed)
}

/// One `speed` with two instances: the default one and one in a dataset.
async fn seeded(instances: Value) -> AppState {
    let mut st = AppState::new("me".into());
    antares_api::wire(&mut st).await;
    let body = json!({"id": ID, "type": "Vehicle", "speed": instances}).to_string();
    let (status, b) = call(&st, "POST", "entities", &body).await;
    assert_eq!(status, StatusCode::CREATED, "seed: {b}");
    st
}

/// The `speed` instances as a list — one surviving instance is served as a
/// single Attribute, several as an array (4.5.5.1).
async fn speed(st: &AppState) -> Vec<Value> {
    let (status, doc) = call(st, "GET", &format!("entities/{ID}"), "").await;
    assert_eq!(status, StatusCode::OK, "{doc}");
    match doc["speed"].clone() {
        Value::Array(a) => a,
        Value::Null => Vec::new(),
        one => vec![one],
    }
}

fn both() -> Value {
    json!([
        {"type": "Property", "value": 10},
        {"type": "Property", "value": 20, "datasetId": DS}
    ])
}

#[tokio::test(flavor = "multi_thread")]
async fn delete_attribute_reads_none_as_the_default_instance() {
    let st = seeded(both()).await;
    let (status, body) = call(
        &st,
        "DELETE",
        &format!("entities/{ID}/attrs/speed?datasetId=@none"),
        "",
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let left = speed(&st).await;
    assert_eq!(left.len(), 1, "one instance survives the delete: {left:?}");
    assert_eq!(
        left[0]["datasetId"], DS,
        "the default instance was not the one deleted: {left:?}"
    );
    assert_eq!(left[0]["value"], 20, "{left:?}");
}

/// The other half of the rule: a datasetId that names a dataset leaves "an
/// instance without a datasetId untouched", so normalizing `@none` must not
/// make every parameter match the default instance.
#[tokio::test(flavor = "multi_thread")]
async fn a_dataset_id_still_selects_only_its_own_instance() {
    let st = seeded(both()).await;
    let (status, body) = call(
        &st,
        "DELETE",
        &format!("entities/{ID}/attrs/speed?datasetId={DS}"),
        "",
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let left = speed(&st).await;
    assert_eq!(left.len(), 1, "one instance survives the delete: {left:?}");
    assert_eq!(
        left[0].get("datasetId"),
        None,
        "the default instance was deleted instead: {left:?}"
    );
    assert_eq!(left[0]["value"], 10, "{left:?}");
}

/// The same rule on the Temporal Evolution (5.6.13.4), whose `?datasetId=`
/// parameter selects the instance set to delete.
#[tokio::test(flavor = "multi_thread")]
async fn temporal_delete_attribute_reads_none_as_the_default_instance() {
    let mut st = AppState::new("me".into());
    antares_api::wire(&mut st).await;
    let body = json!({"id": ID, "type": "Vehicle", "speed": [
        {"type": "Property", "value": 10, "observedAt": "2026-01-01T09:00:00Z"},
        {"type": "Property", "value": 20, "datasetId": DS,
         "observedAt": "2026-01-01T09:00:00Z"}
    ]})
    .to_string();
    let (status, b) = call(&st, "POST", "temporal/entities", &body).await;
    assert!(status.is_success(), "seed: {status} {b}");
    let (status, body) = call(
        &st,
        "DELETE",
        &format!("temporal/entities/{ID}/attrs/speed?datasetId=@none"),
        "",
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (status, doc) = call(
        &st,
        "GET",
        &format!("temporal/entities/{ID}?timerel=after&timeAt=1970-01-01T00:00:00Z"),
        "",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{doc}");
    let left: Vec<&Value> = doc["speed"]
        .as_array()
        .map(|a| a.iter().collect())
        .unwrap_or_else(|| vec![&doc["speed"]]);
    assert!(
        left.iter().all(|i| i["datasetId"] == DS),
        "the default instance set survived: {doc}"
    );
    assert!(!left.is_empty(), "the dataset instance set was deleted too");
}
