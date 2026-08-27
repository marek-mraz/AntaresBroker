// SPDX-License-Identifier: EUPL-1.2
//! 5.3.1 Notification data type — the GeoJSON reading of the `data` member:
//! endpoint.accept application/geo+json ⇒ data is a FeatureCollection
//! (5.2.30); receiverInfo Prefer body=json ⇒ that FeatureCollection carries
//! no @context.

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

async fn send(st: &AppState, method: &str, path: &str, doc: Value) -> (StatusCode, String) {
    let body = doc.to_string();
    let req = Request::builder()
        .method(method)
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

/// One-shot capture server: returns (uri, receiver) where the receiver
/// yields the JSON body of the first POST.
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

async fn notified_body(receiver_info: Option<Value>) -> Value {
    std::env::set_var("ANTARES_EGRESS_ALLOW_PRIVATE", "true");
    let mut st = AppState::new("me".into());
    antares_api::notify::wire(&mut st);
    let (uri, mut rx) = capture_server().await;
    let mut ep = json!({"uri": uri, "accept": "application/geo+json"});
    if let Some(ri) = receiver_info {
        ep["receiverInfo"] = ri;
    }
    let (status, body) = send(
        &st,
        "POST",
        "subscriptions",
        json!({"type": "Subscription", "entities": [{"type": "Vehicle"}],
               "notification": {"endpoint": ep}}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (status, body) = send(
        &st,
        "POST",
        "entities",
        json!({"id": "urn:ngsi-ld:Vehicle:geo1", "type": "Vehicle",
               "location": {"type": "GeoProperty",
                   "value": {"type": "Point", "coordinates": [1.0, 2.0]}}}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
        .await
        .expect("notification within 5s")
        .expect("one notification")
}

/// Table 5.3.1-1: geo+json accept ⇒ data is a FeatureCollection; the
/// Notification wrapper keeps id/type/subscriptionId/notifiedAt.
#[tokio::test(flavor = "multi_thread")]
async fn geojson_accept_delivers_a_feature_collection() {
    let n = notified_body(None).await;
    assert_eq!(n["type"], "Notification", "{n}");
    assert!(n["id"].as_str().is_some_and(|s| s.starts_with("urn:")));
    assert!(n["subscriptionId"].as_str().is_some());
    assert!(n["notifiedAt"].as_str().is_some());
    let data = &n["data"];
    assert_eq!(data["type"], "FeatureCollection", "{n}");
    let features = data["features"].as_array().expect("features");
    assert_eq!(features.len(), 1);
    assert_eq!(features[0]["type"], "Feature");
    assert_eq!(features[0]["id"], "urn:ngsi-ld:Vehicle:geo1");
    assert!(
        data.get("@context").is_some(),
        "without Prefer body=json the FeatureCollection carries @context: {n}"
    );
    assert!(
        !data.is_array(),
        "data must not be an Entity[] under geo+json"
    );
}

/// Table 5.3.1-1: "if the notification.endpoint.receiverInfo contains the
/// key Prefer and it is set to the value body=json, then the
/// FeatureCollection will not contain an @context field".
#[tokio::test(flavor = "multi_thread")]
async fn prefer_body_json_strips_the_context() {
    let n = notified_body(Some(json!([{"key": "Prefer", "value": "body=json"}]))).await;
    assert_eq!(n["data"]["type"], "FeatureCollection", "{n}");
    assert!(
        n["data"].get("@context").is_none(),
        "Prefer body=json must strip @context from the FeatureCollection: {n}"
    );
}
