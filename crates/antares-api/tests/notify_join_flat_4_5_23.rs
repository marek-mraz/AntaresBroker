// SPDX-License-Identifier: EUPL-1.2
//! 4.5.23.3 Flattened Linked Entity Representation on the notification
//! path. Table 5.2.12-1 gives a Subscription's `notification` a `join`
//! member — `"flat"`, `"inline"` or `"@none"`, defaulting to `"@none"` —
//! and a `joinLevel` that only applies to the first two.
//!
//! What "flat" obliges: "the Context Broker response shall always consist
//! of an array of Entities. This array will consist of both Linking
//! Entities and Linked Entities … appended to the array", and a target
//! already in the array is not appended again ("unless a URI has been
//! previously encountered"). The inline form is the other half of 4.5.23
//! and is asserted in `notify_projection_4_21`; this file is the flat
//! form, where the Linked Entity is a sibling in `data` rather than a
//! Sub-Attribute of the Relationship that reached it.

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

/// The programmatic egress override rather than the environment variable:
/// a sibling test reading the environment while another rewrote it saw the
/// policy missing and refused the loopback forward.
fn allow_private() {
    antares_jsonld::allow_private_egress(true);
}

async fn send(st: &AppState, path: &str, doc: Value) -> (StatusCode, String) {
    let body = doc.to_string();
    let req = Request::builder()
        .method("POST")
        .uri(format!("/ngsi-ld/v1/{path}"))
        .header("Content-Type", "application/json")
        .header("Content-Length", body.len())
        .body(Body::from(body))
        .expect("req");
    let resp = antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("resp");
    let status = resp.status();
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

async fn capture_server() -> (String, tokio::sync::mpsc::Receiver<Value>) {
    let (tx, rx) = tokio::sync::mpsc::channel::<Value>(4);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let app = axum::Router::new().route(
        "/notify",
        axum::routing::post(move |body: axum::body::Bytes| {
            let tx = tx.clone();
            async move {
                let v: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
                let _ = tx.send(v).await;
                StatusCode::OK
            }
        }),
    );
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    (format!("http://{addr}/notify"), rx)
}

async fn seed_device(st: &AppState, id: &str) {
    let (status, body) = send(
        st,
        "entities",
        json!({"id": id, "type": "Device",
               "model": {"type": "Property", "value": "X100"}}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

async fn subscribe_flat(st: &AppState) -> tokio::sync::mpsc::Receiver<Value> {
    let (uri, rx) = capture_server().await;
    let (status, body) = send(
        st,
        "subscriptions",
        json!({"type": "Subscription", "entities": [{"type": "Vehicle"}],
        "notification": {"endpoint": {"uri": uri}, "join": "flat", "joinLevel": 1}}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    rx
}

async fn fire(st: &AppState, vehicle: Value, mut rx: tokio::sync::mpsc::Receiver<Value>) -> Value {
    let (status, body) = send(st, "entities", vehicle).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    tokio::time::timeout(
        std::time::Duration::from_secs(5 * antares_api::state::slow_factor()),
        rx.recv(),
    )
    .await
    .expect("notification within 5s")
    .expect("one notification")
}

fn ids(n: &Value) -> Vec<String> {
    n["data"]
        .as_array()
        .expect("data is an array")
        .iter()
        .map(|e| e["id"].as_str().unwrap_or_default().to_owned())
        .collect()
}

/// 4.5.23.3: the notification carries the Linking Entity and the Linked
/// Entity as siblings in `data`, each a whole Entity, and the Linking
/// Entity is not repeated behind its own Relationship.
#[tokio::test(flavor = "multi_thread")]
async fn clause_4_5_23_3_flat_join_appends_the_linked_entity_to_data() {
    allow_private();
    let mut st = AppState::new("me".into());
    // the notification pipeline is wired, not implied: without this the
    // Subscription is stored and nothing ever delivers
    antares_api::wire(&mut st).await;
    seed_device(&st, "urn:ngsi-ld:Device:flat1").await;
    let rx = subscribe_flat(&st).await;
    let n = fire(
        &st,
        json!({"id": "urn:ngsi-ld:Vehicle:flat1", "type": "Vehicle",
               "speed": {"type": "Property", "value": 42},
               "refDevice": {"type": "Relationship",
                             "object": "urn:ngsi-ld:Device:flat1"}}),
        rx,
    )
    .await;

    assert_eq!(
        ids(&n),
        vec!["urn:ngsi-ld:Vehicle:flat1", "urn:ngsi-ld:Device:flat1"],
        "the Linking Entity, then the Linked Entity: {n}"
    );
    let linked = &n["data"][1];
    assert_eq!(
        linked["type"], "Device",
        "a whole Entity, not a fragment: {n}"
    );
    assert_eq!(linked["model"]["value"], "X100", "{n}");
    // flat is the other representation, so the Relationship must NOT also
    // carry the inline Sub-Attribute 4.5.23.2 defines
    assert!(
        n["data"][0]["refDevice"].get("entity").is_none(),
        "flat does not also inline: {n}"
    );
}

/// 4.5.23.3: a target already in the array is not appended twice — two
/// Relationships onto one Entity add it once.
#[tokio::test(flavor = "multi_thread")]
async fn clause_4_5_23_3_a_target_already_in_the_array_is_not_repeated() {
    allow_private();
    let mut st = AppState::new("me".into());
    // the notification pipeline is wired, not implied: without this the
    // Subscription is stored and nothing ever delivers
    antares_api::wire(&mut st).await;
    seed_device(&st, "urn:ngsi-ld:Device:flat2").await;
    let rx = subscribe_flat(&st).await;
    let n = fire(
        &st,
        json!({"id": "urn:ngsi-ld:Vehicle:flat2", "type": "Vehicle",
               "refDevice": {"type": "Relationship",
                             "object": "urn:ngsi-ld:Device:flat2"},
               "spareDevice": {"type": "Relationship",
                               "object": "urn:ngsi-ld:Device:flat2"}}),
        rx,
    )
    .await;

    assert_eq!(
        ids(&n),
        vec!["urn:ngsi-ld:Vehicle:flat2", "urn:ngsi-ld:Device:flat2"],
        "one entry per URI however many Relationships reach it: {n}"
    );
}

/// Control for the two above: the same fixture with `join` absent (the
/// Table 5.2.12-1 default `"@none"`) delivers the Linking Entity alone.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_2_12_join_absent_delivers_the_linking_entity_alone() {
    allow_private();
    let mut st = AppState::new("me".into());
    // the notification pipeline is wired, not implied: without this the
    // Subscription is stored and nothing ever delivers
    antares_api::wire(&mut st).await;
    seed_device(&st, "urn:ngsi-ld:Device:flat3").await;
    let (uri, rx) = capture_server().await;
    let (status, body) = send(
        &st,
        "subscriptions",
        json!({"type": "Subscription", "entities": [{"type": "Vehicle"}],
        "notification": {"endpoint": {"uri": uri}}}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let n = fire(
        &st,
        json!({"id": "urn:ngsi-ld:Vehicle:flat3", "type": "Vehicle",
               "refDevice": {"type": "Relationship",
                             "object": "urn:ngsi-ld:Device:flat3"}}),
        rx,
    )
    .await;
    assert_eq!(ids(&n), vec!["urn:ngsi-ld:Vehicle:flat3"], "{n}");
}
