// SPDX-License-Identifier: EUPL-1.2
//! 6.3.8 notification headers: the MIME type of the POST comes from
//! `endpoint.accept` and the Link header from the served @context, while
//! every `receiverInfo` pair becomes one custom header. A pair keyed like a
//! header the binding sets itself must not put a second value of it on the
//! wire — the client appends what it is handed, so the receiver would have
//! to choose between two, and for `NGSILD-Tenant` (6.3.22) that choice is
//! which tenant it believes the data came from.

use antares_api::AppState;
use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

async fn send(st: &AppState, method: &str, path: &str, tenant: &str, doc: Value) -> StatusCode {
    let body = doc.to_string();
    let req = Request::builder()
        .method(method)
        .uri(format!("/ngsi-ld/v1/{path}"))
        .header("Content-Type", "application/json")
        .header("Content-Length", body.len())
        .header("NGSILD-Tenant", tenant)
        .body(Body::from(body))
        .expect("req");
    let resp = antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("resp");
    let status = resp.status();
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    assert!(
        status.is_success(),
        "{method} {path}: {status} {}",
        String::from_utf8_lossy(&bytes)
    );
    status
}

/// Capture server yielding the HEADERS of the first notification POST.
async fn capture_headers() -> (String, tokio::sync::mpsc::Receiver<HeaderMap>) {
    let (tx, rx) = tokio::sync::mpsc::channel::<HeaderMap>(4);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let app = axum::Router::new().route(
        "/notify",
        axum::routing::post(move |headers: HeaderMap| {
            let tx = tx.clone();
            async move {
                let _ = tx.send(headers).await;
                StatusCode::OK
            }
        }),
    );
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    (format!("http://{addr}/notify"), rx)
}

#[tokio::test]
async fn a_receiver_info_pair_never_doubles_a_header_the_binding_owns() {
    antares_jsonld::allow_private_egress(true);
    let mut st = AppState::new("me".into());
    antares_api::wire(&mut st);
    let (uri, mut rx) = capture_headers().await;

    // The subscriber names, in receiverInfo, every header the binding sets
    // for itself — including the tenant marker of a tenant it is not in.
    send(
        &st,
        "POST",
        "subscriptions",
        "acme",
        json!({"type": "Subscription", "entities": [{"type": "Vehicle"}],
               "notification": {"endpoint": {"uri": uri, "receiverInfo": [
                   {"key": "Content-Type", "value": "text/plain"},
                   {"key": "Link", "value": "<https://elsewhere/ctx>"},
                   {"key": "ngsild-tenant", "value": "victim"},
                   {"key": "X-Kept", "value": "yes"}]}}}),
    )
    .await;
    send(
        &st,
        "POST",
        "entities",
        "acme",
        json!({"id": "urn:ngsi-ld:Vehicle:hdr1", "type": "Vehicle",
               "speed": {"type": "Property", "value": 1}}),
    )
    .await;

    let h = tokio::time::timeout(std::time::Duration::from_secs(10), rx.recv())
        .await
        .expect("a notification arrived")
        .expect("headers");
    let all = |n: &str| h.get_all(n).iter().count();

    assert_eq!(all("content-type"), 1, "6.3.8: one MIME type per POST");
    assert_eq!(h["content-type"], "application/json");
    assert_eq!(all("link"), 1, "6.3.8: one @context reference");
    assert!(h["link"]
        .to_str()
        .expect("ascii")
        .contains("json-ld#context"));
    assert_eq!(all("ngsild-tenant"), 1, "6.3.22: one tenant, the broker's");
    assert_eq!(h["ngsild-tenant"], "acme");
    // an ordinary pair is still a custom header (6.3.8)
    assert_eq!(h["x-kept"], "yes");
}
