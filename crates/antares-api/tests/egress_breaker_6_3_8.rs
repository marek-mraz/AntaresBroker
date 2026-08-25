//! 6.3.8 / 5.8.6: notifications and forwards SHALL be attempted — the
//! per-destination breaker exists for one failure shape only (an UNRESPONSIVE
//! peer must not spend the full deadline on every request). An endpoint that
//! ANSWERS — even with 404/500 — is alive, costs only its own response
//! time, and must never suppress later sends to the same host:port (the
//! ETSI matrix failure shape: earlier tests' failing subscriptions on the
//! suite's one fixed mock port silently starved later tests' expected
//! traffic for a 30 s cooldown).

use antares_api::AppState;
use axum::body::Body;
use axum::http::Request;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tower::ServiceExt;

/// Mock replying `reply` to every request; the counter is the number of
/// DISTINCT Vehicle ids delivered so far — changes queued behind a slow
/// endpoint travel grouped in one notification (5.8.6), so a delivered
/// entity, not a POST, is the unit an attempt is measured in.
fn mock_counting(reply: &'static str) -> (u16, Arc<AtomicUsize>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let hits: Arc<AtomicUsize> = Arc::default();
    let seen = hits.clone();
    let reply = reply.replacen("\r\n", "\r\nConnection: close\r\n", 1);
    std::thread::spawn(move || {
        let mut ids: std::collections::HashSet<String> = Default::default();
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let body = read_request(&mut s);
            for hit in body.match_indices("urn:ngsi-ld:Vehicle:") {
                let rest = &body[hit.0..];
                let end = rest.find('"').unwrap_or(rest.len());
                ids.insert(rest[..end].to_owned());
            }
            seen.store(ids.len(), Ordering::SeqCst);
            let _ = s.write_all(reply.as_bytes());
        }
    });
    (port, hits)
}

/// Headers + the Content-Length'd body of one request, as text.
fn read_request(s: &mut std::net::TcpStream) -> String {
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 8192];
    while let Ok(n) = s.read(&mut chunk) {
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        let text = String::from_utf8_lossy(&buf);
        let Some(head_end) = text.find("\r\n\r\n") else {
            continue;
        };
        let len = text[..head_end]
            .lines()
            .find_map(|l| {
                l.strip_prefix("Content-Length: ")
                    .or_else(|| l.strip_prefix("content-length: "))
            })
            .and_then(|v| v.trim().parse::<usize>().ok())
            .unwrap_or(0);
        if buf.len() >= head_end + 4 + len {
            break;
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

/// Accepts, reads, never answers — the deadline-eater.
fn mock_stalling() -> (u16, Arc<AtomicUsize>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let hits: Arc<AtomicUsize> = Arc::default();
    let seen = hits.clone();
    std::thread::spawn(move || {
        let mut held = Vec::new();
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            seen.fetch_add(1, Ordering::SeqCst);
            let mut buf = [0u8; 8192];
            let _ = s.read(&mut buf);
            held.push(s);
        }
    });
    (port, hits)
}

async fn send(st: &AppState, req: Request<Body>) -> u16 {
    antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("response")
        .status()
        .as_u16()
}

async fn post(st: &AppState, uri: &str, body: String) -> u16 {
    send(
        st,
        Request::builder()
            .method("POST")
            .uri(uri)
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("request"),
    )
    .await
}

async fn subscribe(st: &AppState, id: &str, port: u16, timeout_ms: u32) {
    let status = post(
        st,
        "/ngsi-ld/v1/subscriptions",
        serde_json::json!({
            "id": format!("urn:ngsi-ld:Subscription:{id}"),
            "type": "Subscription",
            "entities": [{"type": "Vehicle"}],
            "notification": {"endpoint": {
                "uri": format!("http://127.0.0.1:{port}/notify"),
                "timeout": timeout_ms,
            }},
        })
        .to_string(),
    )
    .await;
    assert_eq!(status, 201);
}

async fn create_vehicle(st: &AppState, n: usize) {
    let status = post(
        st,
        "/ngsi-ld/v1/entities",
        serde_json::json!({
            "id": format!("urn:ngsi-ld:Vehicle:brk{n}"),
            "type": "Vehicle",
            "speed": {"type": "Property", "value": n},
        })
        .to_string(),
    )
    .await;
    assert_eq!(status, 201);
}

async fn wait_hits(hits: &AtomicUsize, want: usize, ms: u64) -> usize {
    for _ in 0..(ms / 100) {
        if hits.load(Ordering::SeqCst) >= want {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    hits.load(Ordering::SeqCst)
}

fn state() -> AppState {
    std::env::set_var("ANTARES_EGRESS_ALLOW_PRIVATE", "true");
    let mut st = AppState::new("antares1".into());
    antares_api::notify::wire(&mut st);
    st
}

/// An endpoint that RESPONDS (with an error) is alive — every matching
/// change must still be attempted, past any trip threshold.
#[tokio::test(flavor = "multi_thread")]
async fn responding_endpoint_errors_never_suppress_later_sends() {
    let st = state();
    let (port, hits) = mock_counting("HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
    subscribe(&st, "alive404", port, 2000).await;

    // five failed deliveries — the old breaker's trip threshold
    for n in 0..5 {
        create_vehicle(&st, n).await;
    }
    assert_eq!(wait_hits(&hits, 5, 10_000).await, 5, "first five attempted");

    // the endpoint keeps answering: further sends must still be ATTEMPTED
    for n in 5..8 {
        create_vehicle(&st, n).await;
    }
    let got = wait_hits(&hits, 8, 10_000).await;
    assert_eq!(
        got, 8,
        "an alive (responding) endpoint must never be breaker-suppressed"
    );
}

/// The guard stays: a deadline-eating endpoint IS breaker-suppressed
/// after enough consecutive timeouts.
#[tokio::test(flavor = "multi_thread")]
async fn stalling_endpoint_still_trips_the_breaker() {
    let st = state();
    let (port, hits) = mock_stalling();
    subscribe(&st, "staller", port, 300).await;

    for n in 100..105 {
        create_vehicle(&st, n).await;
        // sequential: each delivery must FINISH (time out) before the next
        // so the failures count as consecutive
        wait_hits(&hits, n - 99, 5_000).await;
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    }
    let before = hits.load(Ordering::SeqCst);
    assert!(before >= 5, "five stalled deliveries reached the socket");

    create_vehicle(&st, 999).await;
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    assert_eq!(
        hits.load(Ordering::SeqCst),
        before,
        "a timing-out destination must stay short-circuited within the cooldown"
    );
}
