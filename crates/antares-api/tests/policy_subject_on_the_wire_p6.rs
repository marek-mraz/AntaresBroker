// SPDX-License-Identifier: EUPL-1.2
//! The subject never leaves this process (ADR-0020).
//!
//! The seam is given identity headers so it can ask an engine who is
//! asking. Everything else the broker does with a request is outbound: it
//! forwards to Context Sources (4.3.6), it copies subscriptions to them
//! (5.8.1.4), and it puts changes on a bus. None of those may carry the
//! subject, and one of them could: 4.3.6.5 lets a registration name a
//! header to copy from the triggering request —
//!
//!   "contextSourceInfo ⇒ extra headers; the special value
//!    urn:ngsi-ld:request copies the header from the triggering request"
//!
//! — and a registration is client-supplied. Without a rule, whoever can
//! register a Context Source can read the identity of every request that
//! fans out to it.
//!
//! What DOES carry the subject, deliberately, is the broker's own
//! subscription mirror: 5.8.6 delivery is broker-initiated, so the pod that
//! sends the notification has to decide it under the subscriber. A test
//! below asserts that too, because losing it there would be a silent policy
//! bypass rather than a visible failure.
#![cfg(feature = "test-kit")]
#![allow(clippy::unwrap_used)] // an unwrap here is the assertion

use antares_api::AppState;
use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

fn subject_header() -> &'static str {
    std::env::set_var("ANTARES_POLICY_SUBJECT_HEADERS", "X-Subject");
    "X-Subject"
}

fn state() -> AppState {
    subject_header();
    antares_jsonld::allow_private_egress(true);
    AppState::new("me".into())
}

/// Everything one peer was sent, request by request, as raw bytes.
type Wire = Arc<Mutex<Vec<String>>>;

/// A peer that answers `reply` and records what it was asked.
fn peer(reply_body: &str) -> (u16, Wire) {
    let reply = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{reply_body}",
        reply_body.len()
    );
    let seen: Wire = Arc::default();
    let recorder = Arc::clone(&seen);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let mut buf = [0u8; 16384];
            let n = s.read(&mut buf).unwrap_or(0);
            if let Ok(mut v) = recorder.lock() {
                v.push(String::from_utf8_lossy(&buf[..n]).into_owned());
            }
            let _ = s.write_all(reply.as_bytes());
        }
    });
    (port, seen)
}

async fn call(
    st: &AppState,
    method: &str,
    path: &str,
    who: Option<&str>,
    doc: Option<Value>,
) -> (StatusCode, Value) {
    let mut b = Request::builder()
        .method(method)
        .uri(format!("/ngsi-ld/v1/{path}"));
    if let Some(s) = who {
        b = b.header(subject_header(), s);
    }
    let req = match doc {
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

/// A Context Source over Vehicles whose `contextSourceInfo` asks for the
/// two headers by the 4.3.6.5 copy-from-the-request value.
async fn register(st: &AppState, port: u16, csi: Value) {
    let (code, body) = call(
        st,
        "POST",
        "csourceRegistrations",
        None,
        Some(json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:p6",
            "type": "ContextSourceRegistration",
            "mode": "inclusive",
            "operations": ["queryEntity"],
            "information": [{"entities": [{"type": "Vehicle"}]}],
            "contextSourceInfo": csi,
            "endpoint": format!("http://127.0.0.1:{port}"),
        })),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{body}");
}

/// 4.3.6.5's copy mechanism must not reach the subject. The same request
/// carries an ordinary header the registration also asks for, so the test
/// separates "the guard works" from "the copy mechanism is broken".
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_registration_cannot_ask_for_the_subject_header() {
    let st = state();
    let (port, seen) = peer("[]");
    register(
        &st,
        port,
        json!([{"key": "X-Subject", "value": "urn:ngsi-ld:request"},
               {"key": "X-Trace", "value": "urn:ngsi-ld:request"}]),
    )
    .await;

    let (code, body) = call(&st, "GET", "entities?type=Vehicle", Some("alice"), None).await;
    assert_eq!(code, StatusCode::OK, "{body}");

    let asked = seen.lock().expect("lock").clone();
    assert!(!asked.is_empty(), "the peer was never reached");
    for req in &asked {
        let lower = req.to_ascii_lowercase();
        assert!(
            !lower.contains("x-subject"),
            "the subject header reached a Context Source: {req}"
        );
        assert!(
            !req.contains("alice"),
            "the subject's value reached a Context Source: {req}"
        );
    }
}

/// The guard is narrow: a registration that asks for an ordinary header
/// still gets it, so 4.3.6.5 is not broken in the name of the seam.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_ordinary_header_is_still_copied_from_the_request() {
    let st = state();
    let (port, seen) = peer("[]");
    register(
        &st,
        port,
        json!([{"key": "X-Trace", "value": "urn:ngsi-ld:request"}]),
    )
    .await;

    let req = Request::builder()
        .method("GET")
        .uri("/ngsi-ld/v1/entities?type=Vehicle")
        .header(subject_header(), "alice")
        .header("X-Trace", "abc123")
        .body(Body::empty())
        .expect("req");
    let resp = antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("resp");
    assert_eq!(resp.status(), StatusCode::OK);

    let asked = seen.lock().expect("lock").clone();
    assert!(!asked.is_empty(), "the peer was never reached");
    assert!(
        asked.iter().any(|r| r.contains("abc123")),
        "4.3.6.5 stopped copying the header it was asked to copy: {asked:?}"
    );
    assert!(
        asked.iter().all(|r| !r.contains("alice")),
        "the subject travelled beside it: {asked:?}"
    );
}

/// 5.8.1.4 forwards a reduced copy of the Subscription to a Context Source.
/// The copy is a document, so the subject would travel as a member rather
/// than as a header — and the broker's own delivery bookkeeping already
/// taught this path that its private record stays home.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_forwarded_subscription_copy_carries_no_subject() {
    // the forwarded copy's notification endpoint is this broker's own URL
    std::env::set_var("ANTARES_PUBLIC_URL", "http://127.0.0.1:9999");
    let mut st = state();
    antares_api::wire(&mut st);
    let (port, seen) = peer("{}");
    let (code, body) = call(
        &st,
        "POST",
        "csourceRegistrations",
        None,
        Some(json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:p6sub",
            "type": "ContextSourceRegistration",
            "mode": "inclusive",
            "operations": ["federationOps"],
            "information": [{"entities": [{"type": "Vehicle"}]}],
            "endpoint": format!("http://127.0.0.1:{port}"),
        })),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{body}");

    let (code, body) = call(
        &st,
        "POST",
        "subscriptions",
        Some("alice"),
        Some(
            json!({"id": "urn:ngsi-ld:Subscription:p6", "type": "Subscription",
                    "entities": [{"type": "Vehicle"}],
                    "notification": {"endpoint": {"uri": "http://127.0.0.1:1/notify"}}}),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{body}");

    // the copy is forwarded off the request path
    for _ in 0..50 {
        if !seen.lock().expect("lock").is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let asked = seen.lock().expect("lock").clone();
    assert!(!asked.is_empty(), "no subscription copy was forwarded");
    for req in &asked {
        assert!(
            !req.contains("__"),
            "a broker-internal member reached a Context Source: {req}"
        );
        assert!(
            !req.to_ascii_lowercase().contains("alice"),
            "the subject reached a Context Source: {req}"
        );
    }
}

/// The change queue is the entity write path: what a matcher pod, a
/// temporal recorder or a bus consumer reads. An Entity carries no subject
/// and none is added on the way out.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_change_event_carries_no_subject() {
    let seen: Arc<Mutex<Vec<Value>>> = Arc::default();
    let recorder = Arc::clone(&seen);
    let mut st = state();
    // after `wire`, which installs the pipeline's own flush: this test is
    // about what the queue carries, so it stands in for that consumer
    antares_api::wire(&mut st);
    st.change_flush = Some(Arc::new(move |batch| {
        if let Ok(mut v) = recorder.lock() {
            for (tenant, before, after) in batch {
                v.push(json!({"tenant": tenant, "before": before, "after": after}));
            }
        }
    }));

    let (code, body) = call(
        &st,
        "POST",
        "entities",
        Some("alice"),
        Some(json!({"id": "urn:ngsi-ld:Vehicle:p6", "type": "Vehicle",
                    "speed": {"type": "Property", "value": 10}})),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{body}");

    for _ in 0..50 {
        if !seen.lock().expect("lock").is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let changes = seen.lock().expect("lock").clone();
    assert!(!changes.is_empty(), "the write emitted no change");
    for c in &changes {
        let s = c.to_string();
        assert!(
            !s.contains("alice") && !s.to_ascii_lowercase().contains("x-subject"),
            "a change event carried the subject: {s}"
        );
    }
}

/// And the one place it does travel, on purpose. The subscription mirror is
/// how a notifier pod learns about a subscription an api pod stored, and
/// 5.8.6 delivery is broker-initiated: a pod that mirrored the subscription
/// without its subject would decide every notification under nobody, which
/// is a policy bypass that fails silently. The mirror is the broker's own
/// state — the same value the store row already holds — not a peer.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_broker_s_own_subscription_mirror_keeps_the_subject() {
    let seen: Arc<Mutex<Vec<Value>>> = Arc::default();
    let recorder = Arc::clone(&seen);
    let mut st = state();
    st.sub_sync = Some(Arc::new(move |tenant, _kind, id, doc| {
        if let Ok(mut v) = recorder.lock() {
            v.push(json!({"tenant": tenant.as_str(), "id": id, "doc": doc}));
        }
    }));

    let (code, body) = call(
        &st,
        "POST",
        "subscriptions",
        Some("alice"),
        Some(
            json!({"id": "urn:ngsi-ld:Subscription:p6mirror", "type": "Subscription",
                    "entities": [{"type": "Vehicle"}],
                    "notification": {"endpoint": {"uri": "http://127.0.0.1:1/notify"}}}),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{body}");

    let mirrored = seen.lock().expect("lock").clone();
    assert_eq!(mirrored.len(), 1, "{mirrored:?}");
    assert_eq!(
        mirrored[0]["doc"]["__subject"],
        json!([["x-subject", "alice"]]),
        "the mirror lost the subject a notification has to be decided under"
    );
}

/// A HeaderMap is what the seam reads a subject from; nothing here should
/// have taught the broker to write one back out.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_response_echoes_the_subject_back() {
    let st = state();
    let (code, body) = call(
        &st,
        "POST",
        "entities",
        Some("alice"),
        Some(json!({"id": "urn:ngsi-ld:Vehicle:p6echo", "type": "Vehicle"})),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{body}");

    let req = Request::builder()
        .method("GET")
        .uri("/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:p6echo")
        .header(subject_header(), "alice")
        .body(Body::empty())
        .expect("req");
    let resp = antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("resp");
    assert_eq!(resp.status(), StatusCode::OK);
    let headers: &HeaderMap = resp.headers();
    assert!(
        headers.get("X-Subject").is_none(),
        "the response echoed the subject header back: {headers:?}"
    );
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    assert!(
        !String::from_utf8_lossy(&bytes).contains("alice"),
        "the served entity carried the subject"
    );
}
