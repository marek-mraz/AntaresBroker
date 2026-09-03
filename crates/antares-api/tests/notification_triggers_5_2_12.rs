// SPDX-License-Identifier: EUPL-1.2
//! Table 5.2.12-1 `notificationTrigger`: "The notification triggers listed
//! indicate what kind of changes shall trigger a notification. If not
//! present, the default is the combination `"attributeCreated"` and
//! `"attributeUpdated"`."
//!
//! Read literally, that default says something a subscriber does not expect:
//! creating an Entity that carries no Attributes is neither an Attribute
//! created nor an Attribute updated, so a Subscription that did not ask for
//! `entityCreated` hears nothing at all. The Entity is legal — 5.6.1 requires
//! only `id` and `type` — so this is a real shape arriving at a real
//! Subscription, and the silence is the clause working, not a delivery lost.
//! The same Subscription declaring `entityCreated` must be told.

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

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

/// Records every notification body it is given.
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

async fn subscribe(st: &AppState, id: &str, uri: &str, trigger: Option<&str>) {
    let mut doc = json!({
        "id": id,
        "type": "Subscription",
        "entities": [{"type": "Trigger"}],
        "notification": {"endpoint": {"uri": uri}},
    });
    if let Some(t) = trigger {
        doc["notificationTrigger"] = json!([t]);
    }
    let (status, body) = call(st, "POST", "subscriptions", &doc.to_string()).await;
    assert_eq!(status, StatusCode::CREATED, "subscribe {id}: {body}");
}

/// The positive half decides when the negative half may be read: once the
/// declared-trigger sink has been served, the fan-out for that change has
/// run. A delivery to the other endpoint is still a separate request on its
/// own task, so the silence is only worth asserting after waiting for it.
#[tokio::test(flavor = "multi_thread")]
async fn an_attribute_less_create_reaches_only_the_subscription_that_asked_for_it() {
    antares_jsonld::allow_private_egress(true);
    let mut st = AppState::new("test-triggers".into());
    antares_api::wire(&mut st).await;

    let (uri_default, mut rx_default) = sink().await;
    let (uri_created, mut rx_created) = sink().await;
    subscribe(
        &st,
        "urn:ngsi-ld:Subscription:default-triggers",
        &uri_default,
        None,
    )
    .await;
    subscribe(
        &st,
        "urn:ngsi-ld:Subscription:entity-created",
        &uri_created,
        Some("entityCreated"),
    )
    .await;

    // 5.6.1: `id` and `type` are the whole Entity. No Attribute is created
    // here, so the default combination matches nothing.
    let (status, body) = call(
        &st,
        "POST",
        "entities",
        &json!({"id": "urn:ngsi-ld:Trigger:bare", "type": "Trigger"}).to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let wait = std::time::Duration::from_secs(5 * antares_api::state::slow_factor());
    let n = tokio::time::timeout(wait, rx_created.recv())
        .await
        .expect("the entityCreated subscription must be told")
        .expect("one notification");
    assert_eq!(n["data"][0]["id"], "urn:ngsi-ld:Trigger:bare", "{n}");

    let quiet = tokio::time::timeout(wait, rx_default.recv()).await;
    assert!(
        quiet.is_err(),
        "the default combination is attributeCreated + attributeUpdated, and \
         this create made no Attribute: {:?}",
        quiet.ok().flatten()
    );

    // …and the silence was the trigger set, not a Subscription that never
    // worked: give the same Entity an Attribute and the same endpoint is
    // served. Without this the negative half above would also pass for a
    // subscription the broker had quietly dropped.
    let (status, body) = call(
        &st,
        "POST",
        "entities/urn:ngsi-ld:Trigger:bare/attrs",
        &json!({"speed": {"type": "Property", "value": 1}}).to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let n = tokio::time::timeout(wait, rx_default.recv())
        .await
        .expect("attributeCreated is in the default combination")
        .expect("one notification");
    assert_eq!(n["data"][0]["speed"]["value"], 1, "{n}");
}
