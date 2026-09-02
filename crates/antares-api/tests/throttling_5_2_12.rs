// SPDX-License-Identifier: EUPL-1.2
//! 5.2.12 `throttling`: "Minimal period of time in seconds which shall
//! elapse between two consecutive notifications" — the matcher
//! reads subscriptions from the SubMirror, but the bookkeeping writeback
//! only went through the store, so the mirror copy never carried
//! `notification.lastNotification` and throttling suppressed nothing.

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

async fn send(st: &AppState, method: &str, path: &str, doc: Value) -> StatusCode {
    let body = doc.to_string();
    let req = Request::builder()
        .method(method)
        .uri(format!("/ngsi-ld/v1/{path}"))
        .header("Content-Type", "application/json")
        .header("Content-Length", body.len())
        .body(Body::from(body))
        .expect("req");
    antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("resp")
        .status()
}

/// Counting capture server: every POST increments the counter.
async fn counting_server() -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    let count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let c = count.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let app = axum::Router::new().route(
        "/notify",
        axum::routing::post(move || {
            let c = c.clone();
            async move {
                c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                StatusCode::OK
            }
        }),
    );
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    (format!("http://{addr}/notify"), count)
}

#[tokio::test(flavor = "multi_thread")]
async fn clause_5_2_12_throttling_suppresses_consecutive_notifications() {
    antares_jsonld::allow_private_egress(true);
    let mut st = AppState::new("antares-throttle".into());
    antares_api::notify::wire(&mut st);
    let (uri, count) = counting_server().await;

    let sub = json!({
        "id": "urn:ngsi-ld:Subscription:throttle",
        "type": "Subscription",
        "entities": [{"type": "Vehicle"}],
        "throttling": 30,
        "notification": {"endpoint": {"uri": uri}},
    });
    assert_eq!(
        send(&st, "POST", "subscriptions", sub).await,
        StatusCode::CREATED
    );

    let e = json!({"id": "urn:ngsi-ld:Vehicle:thr", "type": "Vehicle",
        "speed": {"type": "Property", "value": 1}});
    assert_eq!(send(&st, "POST", "entities", e).await, StatusCode::CREATED);

    // the first notification is due — wait for it
    for _ in 0..50 {
        if count.load(std::sync::atomic::Ordering::SeqCst) == 1 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(
        count.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "the first matching change notifies"
    );

    // three distinct updates inside the 30 s throttling window
    for v in [2, 3, 4] {
        let frag = json!({"speed": {"type": "Property", "value": v}});
        assert_eq!(
            send(&st, "PATCH", "entities/urn:ngsi-ld:Vehicle:thr/attrs", frag).await,
            StatusCode::NO_CONTENT
        );
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    }
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    assert_eq!(
        count.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "updates inside the throttling window must NOT notify (5.2.12)"
    );
}
