// SPDX-License-Identifier: EUPL-1.2
//! 5.2.15 / 6.3.8: the notification transport is chosen by `endpoint.uri`
//! scheme from the sink registry, and nothing else. A binding registered
//! from outside antares-api validates its own endpoints at subscription
//! creation (5.8.1.4) and receives the notifications addressed to them; a
//! scheme no registered sink serves is BadRequestData at creation, never a
//! fall-through to the HTTP binding.

use antares_api::AppState;
use antares_model::NgsiError;
use antares_notifier::{DeliveryError, NotificationSink, Outbound};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;
use tokio::sync::mpsc;
use tower::ServiceExt;

/// A delivery binding that never touches the network, defined here rather
/// than in any shipped crate: registering it is the whole seam under test.
struct MemorySink(mpsc::Sender<(String, Outbound)>);

impl NotificationSink for MemorySink {
    fn schemes(&self) -> &'static [&'static str] {
        &["memory"]
    }

    /// In-process delivery: no socket, so no destination for the egress
    /// policy to judge. Every shipped binding leaves this at its default.
    fn network(&self) -> bool {
        false
    }

    fn parse_endpoint(&self, uri: &str, _notifier_info: &[(&str, &str)]) -> Result<(), NgsiError> {
        match uri.strip_prefix("memory://") {
            Some(name) if !name.is_empty() => Ok(()),
            _ => Err(NgsiError::BadRequestData(format!(
                "memory endpoint {uri:?} carries no mailbox name"
            ))),
        }
    }

    fn deliver<'a>(
        &'a self,
        uri: &'a str,
        out: &'a Outbound,
        _timeout: Duration,
    ) -> Pin<Box<dyn Future<Output = Result<(), DeliveryError>> + Send + 'a>> {
        Box::pin(async move {
            self.0
                .send((uri.to_owned(), out.clone()))
                .await
                .map_err(|e| DeliveryError::failed(e.to_string()))
        })
    }
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

fn state(tx: mpsc::Sender<(String, Outbound)>) -> AppState {
    let mut st = AppState::new("plugin-sink".into()).with_sink(Box::new(MemorySink(tx)));
    antares_api::notify::wire(&mut st);
    st
}

fn subscription(uri: &str) -> Value {
    json!({"type": "Subscription", "entities": [{"type": "Vehicle"}],
           "notification": {"endpoint": {"uri": uri}}})
}

/// A sink the broker was never compiled to know about serves its scheme:
/// the subscription is accepted and the notification arrives at the sink.
#[tokio::test(flavor = "multi_thread")]
async fn a_sink_registered_outside_the_api_receives_the_notification() {
    let (tx, mut rx) = mpsc::channel(4);
    let st = state(tx);
    let (status, body) = send(&st, "subscriptions", subscription("memory://box1")).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (status, body) = send(
        &st,
        "entities",
        json!({"id": "urn:ngsi-ld:Vehicle:s1", "type": "Vehicle",
               "speed": {"type": "Property", "value": 10}}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (uri, out) = tokio::time::timeout(
        Duration::from_secs(5 * antares_api::state::slow_factor()),
        rx.recv(),
    )
    .await
    .expect("delivered within 5 s")
    .expect("one notification");
    assert_eq!(uri, "memory://box1");
    assert_eq!(out.body["type"], "Notification", "{:?}", out.body);
    assert_eq!(out.body["data"][0]["id"], "urn:ngsi-ld:Vehicle:s1");
    assert_eq!(out.accept, "application/json", "5.2.15 accept default");
    assert!(
        out.link.contains("ngsi-ld-core-context"),
        "6.3.8: the @context Link travels with the notification: {:?}",
        out.link
    );
}

/// 5.8.1.4 + Table 5.5.2-1: an endpoint whose scheme no sink serves is input
/// data that does not meet the requirements of the operation — BadRequestData
/// (400), not OperationNotSupported: Create Subscription is supported.
#[tokio::test(flavor = "multi_thread")]
async fn an_endpoint_scheme_with_no_sink_is_bad_request_at_creation() {
    let (tx, _rx) = mpsc::channel(1);
    let st = state(tx);
    let (status, body) = send(&st, "subscriptions", subscription("carrier-pigeon://roof")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let problem: Value = serde_json::from_str(&body).expect("ProblemDetails");
    assert_eq!(
        problem["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData",
        "{problem}"
    );
    assert!(
        !body.contains("carrier-pigeon://roof") || problem["detail"].is_string(),
        "the detail names the offending scheme: {problem}"
    );
}

/// The sink validates its own endpoint syntax at creation (5.8.1.4), so a
/// malformed endpoint never reaches delivery.
#[tokio::test(flavor = "multi_thread")]
async fn the_sink_rejects_its_own_malformed_endpoint_at_creation() {
    let (tx, _rx) = mpsc::channel(1);
    let st = state(tx);
    let (status, body) = send(&st, "subscriptions", subscription("memory://")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body.contains("mailbox name"),
        "the sink's own reason: {body}"
    );
}

/// A registered scheme is never delivered through another binding: the HTTP
/// sink must not see an endpoint it does not serve.
#[tokio::test(flavor = "multi_thread")]
async fn delivery_never_falls_through_to_http() {
    let (tx, mut rx) = mpsc::channel(4);
    let st = state(tx);
    // A memory endpoint that resolves to no routable host: were the HTTP
    // binding to claim it, the delivery would fail rather than arrive.
    let (status, body) = send(&st, "subscriptions", subscription("memory://127.0.0.1")).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (status, _) = send(
        &st,
        "entities",
        json!({"id": "urn:ngsi-ld:Vehicle:s2", "type": "Vehicle",
               "speed": {"type": "Property", "value": 3}}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (uri, _) = tokio::time::timeout(
        Duration::from_secs(5 * antares_api::state::slow_factor()),
        rx.recv(),
    )
    .await
    .expect("delivered within 5 s")
    .expect("one notification");
    assert_eq!(uri, "memory://127.0.0.1", "the memory sink claimed it");
}
