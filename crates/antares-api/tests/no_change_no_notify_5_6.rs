// SPDX-License-Identifier: EUPL-1.2
//! 5.6.4, 5.6.5 and 5.6.19 all address ONE Attribute of one Entity, and all
//! three answer `ResourceNotFound` when the Entity does not carry it (Table
//! 6.3.2-1). A request answered with 404 changed nothing, so it is not a
//! change: 5.8.3 sends a notification when "an Entity ... is created, or its
//! Attributes are updated", and 4.8 dates `modifiedAt` at the moment the
//! Entity "was last modified". A subscriber that hears from an Entity nobody
//! touched cannot tell a real update from a miss.

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

fn allow_private() {
    antares_jsonld::allow_private_egress(true);
}

async fn call(st: &AppState, method: &str, path: &str, body: &str) -> (StatusCode, String) {
    let req = Request::builder()
        .method(method)
        .uri(format!("/ngsi-ld/v1/{path}"))
        .header("Content-Type", "application/json")
        .header("Content-Length", body.len())
        .body(Body::from(body.to_owned()))
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
    let (tx, rx) = tokio::sync::mpsc::channel::<Value>(8);
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

/// The first notification a subscriber receives after the three misses. The
/// assertion is ordering, not silence: the only write that changed anything
/// is the append of `b`, so if a 404 emitted a change it arrives first and
/// carries an Entity without `b`.
#[tokio::test(flavor = "multi_thread")]
async fn a_miss_on_one_attribute_notifies_nobody() {
    allow_private();
    let mut st = AppState::new("test".into());
    antares_api::notify::wire(&mut st);

    let (status, body) = call(
        &st,
        "POST",
        "entities",
        &json!({"id": "urn:ngsi-ld:Miss:1", "type": "Miss",
                "a": {"type": "Property", "value": 1}})
        .to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let (uri, mut rx) = capture_server().await;
    let (status, body) = call(
        &st,
        "POST",
        "subscriptions",
        &json!({"type": "Subscription", "entities": [{"type": "Miss"}],
                "notification": {"endpoint": {"uri": uri}}})
        .to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    // 5.6.4 partial update, 5.6.19 replace, 5.6.5 delete — each on an
    // Attribute the Entity does not have
    for (method, body) in [
        ("PATCH", r#"{"type":"Property","value":2}"#),
        ("PUT", r#"{"type":"Property","value":2}"#),
        ("DELETE", ""),
    ] {
        let (status, resp) = call(
            &st,
            method,
            "entities/urn:ngsi-ld:Miss:1/attrs/nosuch",
            body,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method}: {resp}");
    }

    let (status, body) = call(
        &st,
        "POST",
        "entities/urn:ngsi-ld:Miss:1/attrs",
        r#"{"b":{"type":"Property","value":9}}"#,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    let n = tokio::time::timeout(
        std::time::Duration::from_secs(5 * antares_api::state::slow_factor()),
        rx.recv(),
    )
    .await
    .expect("notification within 5s")
    .expect("one notification");
    assert_eq!(
        n["data"][0]["b"]["value"], 9,
        "the first notification is not the append — a miss was published as a change: {n}"
    );
}
