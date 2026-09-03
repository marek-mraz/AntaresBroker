// SPDX-License-Identifier: EUPL-1.2
//! Table 5.2.12-1 `jsonldContext`: "The dereferenceable URI of the JSON-LD
//! `@context` to be used when sending a notification resulting from the
//! subscription. If not provided, the `@context` used for the subscription
//! shall be used as a default."
//!
//! The default is the load-bearing half. A Context Subscriber creates its
//! Subscription under its own vocabulary and expects the Notification back in
//! that vocabulary; falling back to the core context instead would hand it
//! terms it never used, for Attributes it selected by name. The Subscription
//! here binds `velocity` to the IRI the stored Entity's `speed` expands to,
//! so the two vocabularies disagree on exactly one term and the Notification
//! says which one was applied.

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

/// The one term that separates the two vocabularies: `speed` of the default
/// context, named `velocity` by the Subscription's own `@context`.
const SPEED_IRI: &str = "https://uri.etsi.org/ngsi-ld/default-context/speed";

async fn call(st: &AppState, path: &str, ctype: &str, body: &str) -> (StatusCode, String) {
    let req = Request::builder()
        .method("POST")
        .uri(format!("/ngsi-ld/v1/{path}"))
        .header("Content-Type", ctype)
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

async fn sink() -> (String, tokio::sync::mpsc::Receiver<Value>) {
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
                let _ = tx
                    .send(serde_json::from_slice(&body).unwrap_or(Value::Null))
                    .await;
                StatusCode::OK
            }
        }),
    );
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    (format!("http://{addr}/notify"), rx)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_notification_falls_back_to_the_context_the_subscription_was_made_with() {
    antares_jsonld::allow_private_egress(true);
    let mut st = AppState::new("test-notify-context".into());
    antares_api::wire(&mut st).await;

    let (uri, mut rx) = sink().await;
    // No `jsonldContext` member: the fallback is the whole point. `accept`
    // ld+json is what puts the @context in the body to read (5.3.1).
    let sub = json!({
        "id": "urn:ngsi-ld:Subscription:own-vocabulary",
        "type": "Subscription",
        "entities": [{"type": "Vehicle"}],
        "notification": {"endpoint": {"uri": uri, "accept": "application/ld+json"}},
        // ONE inline @context, deliberately: an array of more than one entry
        // is hosted as an ImplicitlyCreated @context and written back into
        // `jsonldContext` (5.13.1), which is the member being provided — the
        // opposite of the case this test is for. A single entry leaves
        // `jsonldContext` absent, so the default is the only thing that can
        // put the Subscription's vocabulary in the Notification.
        "@context": {"velocity": SPEED_IRI}
    });
    let (status, body) = call(
        &st,
        "subscriptions",
        "application/ld+json",
        &sub.to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    // The Entity is written in the DEFAULT vocabulary, so `speed` expands to
    // the IRI the Subscription calls `velocity`. Nothing about the write
    // knows the Subscription exists.
    let (status, body) = call(
        &st,
        "entities",
        "application/json",
        &json!({"id": "urn:ngsi-ld:Vehicle:ctx1", "type": "Vehicle",
                "speed": {"type": "Property", "value": 42}})
        .to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let n = tokio::time::timeout(
        std::time::Duration::from_secs(5 * antares_api::state::slow_factor()),
        rx.recv(),
    )
    .await
    .expect("notification within the window")
    .expect("one notification");

    let entity = &n["data"][0];
    assert_eq!(entity["id"], "urn:ngsi-ld:Vehicle:ctx1", "{n}");
    assert!(
        entity.get("@context").is_some(),
        "ld+json carries the @context in the entity (5.3.1): {n}"
    );
    assert_eq!(
        entity["velocity"]["value"], 42,
        "the Notification must speak the Subscription's own vocabulary, not \
         the core context it fell back to: {n}"
    );
    assert!(
        entity.get("speed").is_none(),
        "the core-context term must not appear beside the one that replaced \
         it: {n}"
    );
}
