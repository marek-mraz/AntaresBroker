// SPDX-License-Identifier: EUPL-1.2
//! Table 5.2.12-1 gives a Subscription an `expandValues` member: "Values of
//! the identified attributes should be expanded against the supplied
//! @context using JSON-LD type coercion prior to executing the query" — the
//! same pair 4.9 gives an entity query, on the condition that decides a
//! notification. A Subscription that names the Attribute must therefore
//! match the Entity whose stored value is the expanded term, and one that
//! does not name it must not.

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

const DC: &str = "https://uri.etsi.org/ngsi-ld/default-context";

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

async fn subscribe(st: &AppState, id: &str, uri: &str, expand: Option<&str>) {
    let mut doc = json!({
        "id": id,
        "type": "Subscription",
        "entities": [{"type": "Coach"}],
        "q": "category==\"Camping\"",
        "notification": {"endpoint": {"uri": uri}},
    });
    if let Some(list) = expand {
        doc["expandValues"] = json!(list);
    }
    let (status, body) = call(st, "POST", "subscriptions", &doc.to_string()).await;
    assert_eq!(status, StatusCode::CREATED, "subscribe {id}: {body}");
}

/// The positive half decides when the negative half may be read: once the
/// coercing subscription has been served, the fan-out for that change has
/// run, and only then is the other endpoint's silence worth asserting.
#[tokio::test(flavor = "multi_thread")]
async fn a_subscription_that_names_the_attribute_compares_the_coerced_value() {
    antares_jsonld::allow_private_egress(true);
    let mut st = AppState::new("test-expandvalues".into());
    antares_api::wire(&mut st);

    let (uri_coerced, mut rx_coerced) = sink().await;
    let (uri_literal, mut rx_literal) = sink().await;
    subscribe(
        &st,
        "urn:ngsi-ld:Subscription:coerced",
        &uri_coerced,
        Some("category"),
    )
    .await;
    subscribe(&st, "urn:ngsi-ld:Subscription:literal", &uri_literal, None).await;

    // The stored value is the term expanded against the supplied @context,
    // which is what a client that sent `"category": "Camping"` under a
    // context binding the term would have written.
    let (status, body) = call(
        &st,
        "POST",
        "entities",
        &json!({
            "id": "urn:ngsi-ld:Coach:1",
            "type": "Coach",
            "category": {"type": "Property", "value": format!("{DC}/Camping")}
        })
        .to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let wait = std::time::Duration::from_secs(5 * antares_api::state::slow_factor());
    let n = tokio::time::timeout(wait, rx_coerced.recv())
        .await
        .expect("expandValues names category, so the coerced term matches")
        .expect("one notification");
    assert_eq!(n["data"][0]["id"], "urn:ngsi-ld:Coach:1", "{n}");

    let quiet = tokio::time::timeout(wait, rx_literal.recv()).await;
    assert!(
        quiet.is_err(),
        "without expandValues the condition compares the literal: {:?}",
        quiet.ok().flatten()
    );

    // …and that silence was the missing list, not a Subscription the broker
    // had dropped: the same endpoint is served by the value it does compare.
    let (status, body) = call(
        &st,
        "POST",
        "entities",
        &json!({
            "id": "urn:ngsi-ld:Coach:2",
            "type": "Coach",
            "category": {"type": "Property", "value": "Camping"}
        })
        .to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let n = tokio::time::timeout(wait, rx_literal.recv())
        .await
        .expect("the literal value is what that Subscription compares")
        .expect("one notification");
    assert_eq!(n["data"][0]["id"], "urn:ngsi-ld:Coach:2", "{n}");
}
