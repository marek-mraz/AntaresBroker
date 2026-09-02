// SPDX-License-Identifier: EUPL-1.2
//! The CSourceNotification body is bounded the way every other body is.
//!
//! 5.11.2.4 sends a Context Source Notification "with all matching Context
//! Source Registrations" — a set this broker is built to hold 100 000+ of
//! per tenant. Nothing in the clause makes that one HTTP request: 6.3.4
//! bounds a request body, and the entity Notification path already answers an
//! over-cap delivery with several whole-item notifications rather than one
//! unbounded POST. The registration half has to do the same, or one
//! subscription a client is free to create turns the whole registration set
//! into a single in-memory body.
#![allow(clippy::unwrap_used)] // an unwrap here is the assertion

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

/// Recording endpoint: one entry per received POST body.
fn recording_mock() -> (u16, Arc<Mutex<Vec<String>>>) {
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let sink = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let sink = sink.clone();
            std::thread::spawn(move || {
                use std::io::{Read, Write};
                let mut buf = Vec::new();
                let mut chunk = [0u8; 8192];
                while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    match s.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(n) => buf.extend_from_slice(&chunk[..n]),
                        Err(_) => break,
                    }
                }
                let end = buf
                    .windows(4)
                    .position(|w| w == b"\r\n\r\n")
                    .map(|p| p + 4)
                    .unwrap_or(buf.len());
                let head = String::from_utf8_lossy(&buf[..end]).to_string();
                let want: usize = head
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .and_then(|v| v.trim().parse().ok())
                    })
                    .unwrap_or(0);
                while buf.len() < end + want {
                    match s.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(n) => buf.extend_from_slice(&chunk[..n]),
                        Err(_) => break,
                    }
                }
                sink.lock()
                    .expect("seen")
                    .push(String::from_utf8_lossy(&buf[end..]).to_string());
                // This handler serves ONE request and drops the socket, so the
                // reply has to say so: an HTTP/1.1 response without it leaves
                // the connection persistent by default (RFC 9112 clause 9.3),
                // the client returns it to its pool, and the next
                // notification is written into a socket already being closed.
                let _ = s.write_all(
                    b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
                );
            });
        }
    });
    (port, seen)
}

async fn send(st: &AppState, path: &str, body: Value) -> StatusCode {
    let payload = body.to_string();
    antares_api::router(st.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/ngsi-ld/v1/{path}"))
                .header("Content-Type", "application/json")
                .header("Content-Length", payload.len())
                .body(Body::from(payload))
                .expect("request"),
        )
        .await
        .expect("response")
        .status()
}

/// One test per process: the cap is read once, from the environment.
#[tokio::test(flavor = "multi_thread")]
async fn the_initial_csource_notification_is_cut_into_bounded_bodies() {
    std::env::set_var("ANTARES_MAX_BODY_BYTES", "4096");
    antares_jsonld::allow_private_egress(true);
    let cap = 4096usize;
    let mut st = AppState::new("me".into());
    antares_api::notify::wire(&mut st);
    let (port, seen) = recording_mock();

    // Six registrations, each carrying a Context Source Property big enough
    // that the set cannot travel as one bounded body.
    for i in 0..6 {
        let status = send(
            &st,
            "csourceRegistrations",
            json!({
                "id": format!("urn:ngsi-ld:ContextSourceRegistration:bulk-{i}"),
                "type": "ContextSourceRegistration",
                "information": [{"entities": [{"type": "Vehicle"}]}],
                "endpoint": "http://127.0.0.1:9",
                "note": {"type": "Property", "value": "x".repeat(900)},
            }),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "registration {i}");
    }

    let status = send(
        &st,
        "csourceSubscriptions",
        json!({
            "id": "urn:ngsi-ld:Subscription:bulk-watch",
            "type": "Subscription",
            "entities": [{"type": "Vehicle"}],
            "notification": {"endpoint": {"uri": format!("http://127.0.0.1:{port}/notify")}},
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // The initial notification is dispatched off the request path, and the
    // set is too large for one bounded body, so more than one POST is on the
    // way. Waiting for the FIRST body and then a fixed grace drops whatever
    // has not arrived within it: wait for the registrations themselves, on a
    // deadline that scales with the runner.
    let deadline = std::time::Duration::from_secs(10 * antares_api::state::slow_factor());
    let started = std::time::Instant::now();
    let count = |bodies: &[String]| -> usize {
        bodies
            .iter()
            .map(|b| {
                let v: Value = serde_json::from_str(b).expect("json");
                v["data"].as_array().map(Vec::len).unwrap_or(0)
            })
            .sum()
    };
    let (bodies, delivered) = loop {
        let bodies = seen.lock().expect("seen").clone();
        let delivered = count(&bodies);
        if delivered >= 6 || started.elapsed() >= deadline {
            break (bodies, delivered);
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    };
    assert!(!bodies.is_empty(), "no CSourceNotification arrived");
    assert_eq!(delivered, 6, "every registration is notified: {bodies:?}");
    for b in &bodies {
        assert!(
            b.len() <= cap,
            "a notification body of {} bytes is over the {cap}-byte bound",
            b.len()
        );
    }
}
