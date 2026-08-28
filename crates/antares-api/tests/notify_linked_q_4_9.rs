// SPDX-License-Identifier: EUPL-1.2
//! 4.9 linked-entity subqueries (`attr{path}`, EXAMPLE 13/14) inside a
//! Subscription's q: the notification matcher shall resolve the linked
//! Entity through the local store — a matching linked term fires the
//! notification, a non-matching one must NOT.

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

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

/// 4.9: `isConnectedTo{batteryLevel}<0.5` — the Relationship hop resolves
/// through the store; the low-battery Device makes the Vehicle match, the
/// healthy Device must NOT.
#[tokio::test(flavor = "multi_thread")]
async fn clause_4_9_linked_q_resolves_through_the_store_in_notifications() {
    std::env::set_var("ANTARES_EGRESS_ALLOW_PRIVATE", "true");
    let mut st = AppState::new("me".into());
    antares_api::notify::wire(&mut st);

    let (status, body) = send(
        &st,
        "entities",
        json!({"id": "urn:ngsi-ld:Device:low", "type": "Device",
               "batteryLevel": {"type": "Property", "value": 0.3}}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (status, _) = send(
        &st,
        "entities",
        json!({"id": "urn:ngsi-ld:Device:full", "type": "Device",
               "batteryLevel": {"type": "Property", "value": 0.95}}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (uri, mut rx) = capture_server().await;
    let (status, body) = send(
        &st,
        "subscriptions",
        json!({"type": "Subscription", "entities": [{"type": "Vehicle"}],
               "q": "isConnectedTo{batteryLevel}<0.5",
               "notification": {"endpoint": {"uri": uri}}}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    // the healthy device's vehicle must NOT notify
    let (status, _) = send(
        &st,
        "entities",
        json!({"id": "urn:ngsi-ld:Vehicle:ok", "type": "Vehicle",
               "isConnectedTo": {"type": "Relationship", "object": "urn:ngsi-ld:Device:full"}}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // the low-battery device's vehicle fires
    let (status, _) = send(
        &st,
        "entities",
        json!({"id": "urn:ngsi-ld:Vehicle:low", "type": "Vehicle",
               "isConnectedTo": {"type": "Relationship", "object": "urn:ngsi-ld:Device:low"}}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let n = tokio::time::timeout(
        std::time::Duration::from_secs(5 * antares_api::state::slow_factor()),
        rx.recv(),
    )
    .await
    .expect("linked-q notification within 5s")
    .expect("one notification");
    let ids: Vec<&str> = n["data"]
        .as_array()
        .expect("data")
        .iter()
        .filter_map(|d| d["id"].as_str())
        .collect();
    assert_eq!(ids, vec!["urn:ngsi-ld:Vehicle:low"], "{n}");
    // no second notification for the healthy vehicle
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(800), rx.recv())
            .await
            .is_err(),
        "the non-matching vehicle must not notify"
    );
}
