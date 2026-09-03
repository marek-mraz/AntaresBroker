// SPDX-License-Identifier: EUPL-1.2
//! A subscription that crosses two organizations, each behind its own proxy.
//!
//! 5.8.1.4 splits a Subscription in two: the broker keeps the subscriber's
//! own Subscription and sends every matching Context Source a reduced copy
//! whose notification endpoint is the broker itself, then remaps what comes
//! back onto the original `subscriptionId` and delivers it. Between two
//! organizations neither leg is a direct connection — the copy leaves
//! through the consumer's egress proxy and arrives through the provider's
//! ingress proxy, and the notification comes back the other way through the
//! provider's egress and the consumer's ingress.
//!
//! Four hops, two brokers and one delivery: the subscriber's endpoint is
//! never told to any of them, and what it receives carries the id it
//! created, not the one the provider knows.
#![cfg(feature = "test-kit")]
#![allow(clippy::unwrap_used)] // an unwrap here is the assertion

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

mod common;
use common::net::{proxy, sink, verbatim, Wire};

const OWN_SUB: &str = "urn:ngsi-ld:Subscription:cross-org";

async fn send(st: &AppState, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let mut b = Request::builder().method(method).uri(uri);
    let req = match body {
        Some(v) => {
            let payload = v.to_string();
            b = b
                .header("Content-Type", "application/json")
                .header("Content-Length", payload.len());
            b.body(Body::from(payload)).expect("req")
        }
        None => b.body(Body::empty()).expect("req"),
    };
    let resp = antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("resp");
    let status = resp.status();
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    let doc = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, doc)
}

/// Poll until `f` holds, or fail naming what was waited for. The wait is
/// scaled the way every delivery test in this workspace scales it.
async fn wait_for<F: Fn() -> bool>(what: &str, f: F) {
    let deadline = 200 * antares_api::state::slow_factor();
    for _ in 0..deadline {
        if f() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for {what}");
}

/// The bodies of the requests a fixture recorded, parsed.
fn bodies(seen: &Wire) -> Vec<Value> {
    seen.lock()
        .expect("lock")
        .iter()
        .filter_map(|r| r.split_once("\r\n\r\n").map(|(_, b)| b.to_owned()))
        .filter_map(|b| serde_json::from_str(b.trim_end_matches('\0')).ok())
        .collect()
}

/// The whole two-organization deployment, wired the way it is deployed:
/// the consumer's socket is bound before its state exists, because the
/// public URL it advertises to the provider is the return chain's entrance
/// and the state reads that URL once, when it is built.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_subscription_crosses_two_organizations_through_four_proxy_hops() {
    antares_jsonld::allow_private_egress(true);

    // The provider organization: its broker, and the ingress proxy in front
    // of it that the consumer organization's egress proxy dials.
    let mut provider = AppState::new("provider".into());
    antares_api::wire(&mut provider);
    let provider_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let provider_port = provider_listener.local_addr().expect("addr").port();
    let provider_router = antares_api::router(provider.clone());
    tokio::spawn(async move {
        axum::serve(provider_listener, provider_router)
            .await
            .expect("serve");
    });
    let (provider_ingress, provider_ingress_seen) = proxy(provider_port, verbatim);
    let (consumer_egress, _) = proxy(provider_ingress, verbatim);

    // The consumer organization: the return chain first, so its entrance is
    // known before the state that advertises it is built.
    let consumer_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let consumer_port = consumer_listener.local_addr().expect("addr").port();
    let (consumer_ingress, consumer_ingress_seen) = proxy(consumer_port, verbatim);
    let (provider_egress, _) = proxy(consumer_ingress, verbatim);
    std::env::set_var(
        "ANTARES_PUBLIC_URL",
        format!("http://127.0.0.1:{provider_egress}"),
    );
    let mut consumer = AppState::new("consumer".into());
    antares_api::wire(&mut consumer);
    let consumer_router = antares_api::router(consumer.clone());
    tokio::spawn(async move {
        axum::serve(consumer_listener, consumer_router)
            .await
            .expect("serve");
    });

    // The subscriber, inside the consumer organization and known to nobody
    // outside it.
    let (subscriber, delivered) = sink();

    let (code, body) = send(
        &consumer,
        "POST",
        "/ngsi-ld/v1/csourceRegistrations",
        Some(json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:cross-org",
            "type": "ContextSourceRegistration",
            "information": [{"entities": [{"type": "Vehicle"}]}],
            "operations": ["federationOps"],
            "endpoint": format!("http://127.0.0.1:{consumer_egress}"),
        })),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{body}");

    let (code, body) = send(
        &consumer,
        "POST",
        "/ngsi-ld/v1/subscriptions",
        Some(json!({
            "id": OWN_SUB,
            "type": "Subscription",
            "entities": [{"type": "Vehicle"}],
            "notification": {"endpoint": {"uri": format!("http://127.0.0.1:{subscriber}/n")}},
        })),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{body}");

    // 5.8.1.4: the reduced copy reaches the provider through both hops.
    wait_for("the forwarded subscription copy", || {
        provider_ingress_seen
            .lock()
            .expect("lock")
            .iter()
            .any(|r| r.starts_with("POST /ngsi-ld/v1/subscriptions"))
    })
    .await;
    let copy = bodies(&provider_ingress_seen)
        .into_iter()
        .find(|b| b["type"] == "Subscription")
        .expect("the copy is a Subscription");
    let copy_endpoint = copy["notification"]["endpoint"]["uri"]
        .as_str()
        .expect("endpoint")
        .to_owned();
    assert_eq!(
        copy_endpoint,
        format!("http://127.0.0.1:{provider_egress}/ex/v1/remote-notify"),
        "the copy must name the consumer's advertised entrance: {copy}"
    );
    assert!(
        !copy.to_string().contains(&subscriber.to_string()),
        "the copy told the provider where the subscriber is: {copy}"
    );
    assert_ne!(
        copy["id"].as_str(),
        Some(OWN_SUB),
        "the copy must not carry the subscriber's own Subscription id: {copy}"
    );

    // A write inside the provider organization fires the copy, the
    // notification comes back through the return chain, and the consumer
    // delivers it to the subscriber under the id the subscriber created.
    let (code, body) = send(
        &provider,
        "POST",
        "/ngsi-ld/v1/entities",
        Some(json!({
            "id": "urn:ngsi-ld:Vehicle:cross-org-1", "type": "Vehicle",
            "speed": {"type": "Property", "value": 80},
        })),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{body}");

    wait_for("the notification the subscriber receives", || {
        !delivered.lock().expect("lock").is_empty()
    })
    .await;
    let note = bodies(&delivered)
        .into_iter()
        .find(|b| b["type"] == "Notification")
        .expect("a Notification body");
    assert_eq!(
        note["subscriptionId"], OWN_SUB,
        "the subscriber was told the provider's id for its own Subscription: {note}"
    );
    assert_eq!(
        note["data"][0]["id"], "urn:ngsi-ld:Vehicle:cross-org-1",
        "{note}"
    );
    assert!(
        consumer_ingress_seen
            .lock()
            .expect("lock")
            .iter()
            .any(|r| r.contains("/ex/v1/remote-notify")),
        "the notification did not come back through the consumer's proxy"
    );
}
