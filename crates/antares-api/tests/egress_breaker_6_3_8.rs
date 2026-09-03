// SPDX-License-Identifier: EUPL-1.2
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

/// Accepts, waits `delay_ms`, then answers 200 — alive, but slower than an
/// impatient subscriber's deadline. Records every delivered Vehicle id so a
/// test can ask which TENANT's notification arrived.
fn mock_slow(
    delay_ms: u64,
) -> (
    u16,
    Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let ids: Arc<std::sync::Mutex<std::collections::HashSet<String>>> = Arc::default();
    let seen = ids.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let seen = seen.clone();
            std::thread::spawn(move || {
                let body = read_request(&mut s);
                std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                for hit in body.match_indices("urn:ngsi-ld:Vehicle:") {
                    let rest = &body[hit.0..];
                    let end = rest.find('"').unwrap_or(rest.len());
                    if let Ok(mut g) = seen.lock() {
                        g.insert(rest[..end].to_owned());
                    }
                }
                let _ = s.write_all(
                    b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
                );
                let _ = s.flush();
            });
        }
    });
    (port, ids)
}

async fn post_as(st: &AppState, tenant: &str, uri: &str, body: String) -> u16 {
    send(
        st,
        Request::builder()
            .method("POST")
            .uri(uri)
            .header("Content-Type", "application/json")
            .header("NGSILD-Tenant", tenant)
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("request"),
    )
    .await
}

async fn subscribe_as(st: &AppState, tenant: &str, id: &str, port: u16, timeout_ms: u32) {
    let status = post_as(
        st,
        tenant,
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
    assert_eq!(status, 201, "subscribe as {tenant}");
}

async fn create_vehicle_as(st: &AppState, tenant: &str, n: usize) {
    let status = post_as(
        st,
        tenant,
        "/ngsi-ld/v1/entities",
        serde_json::json!({
            "id": format!("urn:ngsi-ld:Vehicle:brk{n}"),
            "type": "Vehicle",
            "speed": {"type": "Property", "value": n},
        })
        .to_string(),
    )
    .await;
    assert_eq!(status, 201, "create as {tenant}");
}

async fn wait_id(
    ids: &std::sync::Mutex<std::collections::HashSet<String>>,
    want: &str,
    ms: u64,
) -> bool {
    for _ in 0..(ms / 100) {
        if ids.lock().is_ok_and(|g| g.contains(want)) {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    false
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

async fn state() -> AppState {
    antares_jsonld::allow_private_egress(true);
    let mut st = AppState::new("antares1".into());
    antares_api::wire(&mut st).await;
    st
}

/// An endpoint that RESPONDS (with an error) is alive — every matching
/// change must still be attempted, past any trip threshold.
#[tokio::test(flavor = "multi_thread")]
async fn responding_endpoint_errors_never_suppress_later_sends() {
    let st = state().await;
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
    let st = state().await;
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

/// 4.14: "the NGSI-LD API operations for managing, retrieving and subscribing
/// to entity information, but also any context source related operations only
/// apply to the information of the specified `Tenant` in isolation and never
/// have any effect on the information of other `Tenants`."
///
/// Tenants share destinations — one consumer host, one MQTT broker — and the
/// breaker is keyed by `scheme://host:port` alone. So a tenant whose own
/// `endpoint.timeout` is too short for that host (the 6.3.8 floor is 100 ms)
/// trips the breaker for every other tenant pointing at it, and the victim
/// sees no evidence at all: a suppressed delivery deliberately does not move
/// `timesSent`, `lastNotification` or `status`.
///
/// The sibling map one struct field over was given exactly this treatment —
/// `reg_key` scopes the 5.2.34 cooldown "PER TENANT (5.5.10)" — and the
/// breaker beside it was not.
#[tokio::test(flavor = "multi_thread")]
async fn one_tenants_timeouts_never_suppress_another_tenants_notifications() {
    let st = state().await;
    // Alive, and answers well inside tenant-b's deadline — but not inside
    // tenant-a's, which sits at the 6.3.8 floor. The endpoint deadline the
    // broker arms is `endpoint.timeout` STRETCHED by `slow_factor` under a
    // sanitizer, so the mock's delay has to move with it: left at 600 ms it
    // lands INSIDE tenant-a's stretched 1 s, every delivery succeeds, and the
    // premise this test rests on — that tenant A trips its own breaker —
    // silently stops holding.
    let (port, ids) = mock_slow(600 * antares_api::state::slow_factor());
    subscribe_as(&st, "brk-tenant-a", "impatient", port, 100).await;

    // Tenant A earns its own breaker: each delivery must finish timing out
    // before the next, so the failures count as consecutive.
    for n in 200..205 {
        create_vehicle_as(&st, "brk-tenant-a", n).await;
        tokio::time::sleep(std::time::Duration::from_millis(
            900 * antares_api::state::slow_factor(),
        ))
        .await;
    }
    // The premise, established the way a client would see it: tenant A's own
    // next notification is now suppressed. Without this the test could pass
    // on a breaker that never tripped.
    create_vehicle_as(&st, "brk-tenant-a", 205).await;
    assert!(
        !wait_id(
            &ids,
            "urn:ngsi-ld:Vehicle:brk205",
            2_000 * antares_api::state::slow_factor()
        )
        .await,
        "tenant A did not trip its own breaker, so the rest proves nothing"
    );

    // Tenant B is patient, and its subscription is active. Its notification
    // has to be attempted.
    subscribe_as(&st, "brk-tenant-b", "patient", port, 5_000).await;
    create_vehicle_as(&st, "brk-tenant-b", 900).await;
    assert!(
        wait_id(
            &ids,
            "urn:ngsi-ld:Vehicle:brk900",
            10_000 * antares_api::state::slow_factor()
        )
        .await,
        "4.14: one tenant's failing endpoint must never suppress another \
         tenant's notification to the same host:port"
    );
}
