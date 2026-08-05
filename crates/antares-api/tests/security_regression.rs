//! I7 — the §16.6 security regression suite: the §14 Scorpio findings with
//! security character, kept failing-forever as tests.
//!
//! * R4-class: caches keyed by client-supplied URLs must have a SIZE cap,
//!   not just a TTL — asserted by overfilling the @context cache.
//! * L6-class: deleting a subscription must remove ALL its state — no
//!   orphaned bookkeeping keeps notifying (Scorpio's callback-UUID leak).
//! * WS-44-class: the SIZE verdict comes before any parse — an oversized
//!   body of invalid JSON answers 413, never a parse error.
//! * Cross-tenant probes run per-commit in tenant_isolation.rs (§16.1), a
//!   stricter cadence than the per-release e2e the task names.

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tower::ServiceExt;

async fn send(st: &AppState, req: Request<Body>) -> StatusCode {
    antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("response")
        .status()
}

/// WS-44 order: 5 MB of garbage that is NOT JSON must bounce off the size
/// wall (bare 413) — a 400 here would mean something tried to parse it.
#[tokio::test(flavor = "multi_thread")]
async fn oversized_body_is_rejected_before_any_parse() {
    let st = AppState::new("test".into());
    let garbage = "x".repeat(5 * 1024 * 1024); // over MAX_BODY_BYTES, invalid JSON
    let req = Request::builder()
        .method("POST")
        .uri("/ngsi-ld/v1/entities")
        .header("Content-Type", "application/json")
        .body(Body::from(garbage))
        .expect("request");
    assert_eq!(send(&st, req).await, StatusCode::PAYLOAD_TOO_LARGE);
}

/// R4-class: the parsed-@context cache is capped — 400 distinct
/// client-supplied URLs cannot grow it past its max size.
#[tokio::test(flavor = "multi_thread")]
async fn context_cache_size_cap_holds_under_client_keyed_load() {
    let loader = antares_jsonld::Loader::new();
    for i in 0..400 {
        loader
            .put_local(
                format!("https://attacker.example/ctx/{i}"),
                serde_json::json!({"term": format!("https://x/{i}")}),
            )
            .await;
    }
    let stats = loader.cache_stats();
    let fetched = stats["fetched"].as_u64().expect("count");
    assert!(
        fetched <= 256,
        "client-keyed cache must be size-capped (R4): {fetched} entries"
    );
}

/// L6-class: after DELETE, a subscription's state is GONE — later matching
/// changes produce no delivery attempt. (Scorpio orphaned callback UUIDs
/// forever; the wrong-typed-key NPE meant the delete path never cleaned up.)
#[tokio::test(flavor = "multi_thread")]
async fn deleted_subscription_stops_notifying() {
    // a real receiver so a delivery, if it happened, would be observable
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let hits: Arc<AtomicUsize> = Arc::default();
    let sink = hits.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            sink.fetch_add(1, Ordering::SeqCst);
            let mut buf = [0u8; 4096];
            let _ = s.read(&mut buf);
            let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
        }
    });

    // egress: the receiver is loopback, which is denied by default (§16.4) —
    // allow it for this process the way the ETSI stacks do
    std::env::set_var("ANTARES_EGRESS_ALLOW_PRIVATE", "true");
    let mut st = AppState::new("test".into());
    antares_api::notify::wire(&mut st);
    let st = st;

    let post = |path: &'static str, body: String| {
        let st = st.clone();
        async move {
            let req = Request::builder()
                .method("POST")
                .uri(path)
                .header("Content-Type", "application/json")
                .body(Body::from(body))
                .expect("request");
            send(&st, req).await
        }
    };
    let sub = format!(
        r#"{{"id":"urn:ngsi-ld:Subscription:i7","type":"Subscription",
            "entities":[{{"type":"SecReg"}}],
            "notification":{{"endpoint":{{"uri":"http://127.0.0.1:{port}/notify"}}}}}}"#
    );
    assert_eq!(
        post("/ngsi-ld/v1/subscriptions", sub).await,
        StatusCode::CREATED
    );
    assert_eq!(
        post(
            "/ngsi-ld/v1/entities",
            // an attribute, or the default (attribute-level) triggers never fire
            r#"{"id":"urn:ngsi-ld:SecReg:1","type":"SecReg",
                    "temperature":{"type":"Property","value":1}}"#
                .into()
        )
        .await,
        StatusCode::CREATED
    );
    // sanity: the egress gate must allow the receiver, or the test is void
    st.egress
        .check_url(&format!("http://127.0.0.1:{port}/notify"))
        .await
        .expect("egress must allow the loopback receiver (ANTARES_EGRESS_ALLOW_PRIVATE)");

    // the live subscription must deliver — proves the receiver observes
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while hits.load(Ordering::SeqCst) == 0 {
        assert!(
            std::time::Instant::now() < deadline,
            "live subscription never delivered — receiver broken, test void"
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let before = hits.load(Ordering::SeqCst);

    // DELETE, then a matching change: no further delivery may happen
    let req = Request::builder()
        .method("DELETE")
        .uri("/ngsi-ld/v1/subscriptions/urn:ngsi-ld:Subscription:i7")
        .body(Body::empty())
        .expect("request");
    assert_eq!(send(&st, req).await, StatusCode::NO_CONTENT);
    assert_eq!(
        post(
            "/ngsi-ld/v1/entities",
            r#"{"id":"urn:ngsi-ld:SecReg:2","type":"SecReg",
                    "temperature":{"type":"Property","value":2}}"#
                .into()
        )
        .await,
        StatusCode::CREATED
    );
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    assert_eq!(
        hits.load(Ordering::SeqCst),
        before,
        "a deleted subscription kept notifying — orphaned state (L6)"
    );
}
