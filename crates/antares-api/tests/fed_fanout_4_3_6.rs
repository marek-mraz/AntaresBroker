//! Concurrent distributed fan-out (4.3.6.1). The clause distributes one
//! query "to all registered Context Sources" with no mandate on request
//! order; result priority is fixed by the 4.5.5 merge (aux after non-aux),
//! not by which source answered first. So the broker may — and does — issue
//! the forwards concurrently: total latency is the slowest peer, not the sum
//! of peers. Merge semantics are asserted unchanged.

use antares_api::AppState;
use axum::body::Body;
use axum::http::Request;
use std::io::{Read, Write};
use tower::ServiceExt;

/// Mock Context Source that sleeps `delay` before answering with one entity.
fn slow_source(entity_id: &str, delay: std::time::Duration) -> u16 {
    let body = serde_json::json!([{
        "id": entity_id,
        "type": "Vehicle",
        "speed": {"type": "Property", "value": 42},
    }])
    .to_string();
    let reply = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let reply = reply.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 8192];
                let _ = s.read(&mut buf);
                std::thread::sleep(delay);
                let _ = s.write_all(reply.as_bytes());
            });
        }
    });
    port
}

async fn register(st: &AppState, port: u16, mode: &str) {
    let body = serde_json::json!({
        "id": format!("urn:ngsi-ld:ContextSourceRegistration:fanout-{port}"),
        "type": "ContextSourceRegistration",
        "mode": mode,
        "operations": ["queryEntity"],
        "information": [{"entities": [{"type": "Vehicle"}]}],
        "endpoint": format!("http://127.0.0.1:{port}"),
    })
    .to_string();
    let req = Request::builder()
        .method("POST")
        .uri("/ngsi-ld/v1/csourceRegistrations")
        .header("Content-Type", "application/json")
        .header("Content-Length", body.len())
        .body(Body::from(body))
        .expect("request");
    let res = antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("response");
    assert_eq!(res.status(), 201, "registration create");
}

/// 4.3.6.1: three half-second sources answered concurrently — the query
/// completes in roughly one delay, not three, and every source's entity is
/// merged. The elapsed ceiling (1400 ms) fails the sequential shape
/// (>= 1500 ms) with margin on both sides.
#[tokio::test(flavor = "multi_thread")]
async fn fanout_is_concurrent_and_complete() {
    std::env::set_var("ANTARES_EGRESS_ALLOW_PRIVATE", "true");
    let st = AppState::new("antares1".into());
    let delay = std::time::Duration::from_millis(500);
    let ids = [
        "urn:ngsi-ld:Vehicle:fanout-a",
        "urn:ngsi-ld:Vehicle:fanout-b",
        "urn:ngsi-ld:Vehicle:fanout-c",
    ];
    for id in ids {
        let port = slow_source(id, delay);
        register(&st, port, "inclusive").await;
    }

    let started = std::time::Instant::now();
    let req = Request::builder()
        .method("GET")
        .uri("/ngsi-ld/v1/entities?type=Vehicle")
        .body(Body::empty())
        .expect("request");
    let res = antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("response");
    let elapsed = started.elapsed();

    assert_eq!(res.status(), 200);
    let body = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .expect("body");
    let text = String::from_utf8_lossy(&body);
    for id in ids {
        assert!(text.contains(id), "entity from every source merged: {id}");
    }
    // negative: one entity per source, no duplicates from the merge
    for id in ids {
        assert_eq!(text.matches(id).count(), 1, "no duplicate for {id}");
    }
    assert!(
        elapsed < std::time::Duration::from_millis(1400),
        "three 500 ms sources must fan out concurrently, took {elapsed:?}"
    );
}
