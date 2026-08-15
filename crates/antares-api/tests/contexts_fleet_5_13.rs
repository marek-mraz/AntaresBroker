//! 5.13 behind a load balancer (fleet run 2026-08-15, 7 red TPs): two api
//! instances share ONE store, and the Cached-@context bookkeeping must be
//! shared with it — numberOfHits/lastUsage visible from either instance
//! (5.13.3.5), and a delete through one instance honoured by the other
//! (5.13.5.4): the peer's warm in-memory copy must not keep serving a
//! deleted @context nor block its re-creation on next use.

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;

/// Tiny @context server: serves one JSON-LD context document on any GET and
/// counts fetches (the negative half: a warm cache must NOT refetch).
fn mock_ctx_server() -> (String, Arc<std::sync::atomic::AtomicUsize>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let fetches = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let n = fetches.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            n.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let body = r#"{"@context":{"fleetTemp":"http://example.org/fleetTemp"}}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/ld+json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            use std::io::{Read, Write};
            let mut buf = [0u8; 2048];
            let _ = s.read(&mut buf);
            let _ = s.write_all(resp.as_bytes());
        }
    });
    (format!("http://127.0.0.1:{port}/fleet-ctx.jsonld"), fetches)
}

async fn get(st: &AppState, uri: &str) -> (StatusCode, Value) {
    let resp = antares_api::router(st.clone())
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(uri)
                .body(Body::empty())
                .expect("req"),
        )
        .await
        .expect("resp");
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, body)
}

async fn delete(st: &AppState, uri: &str) -> StatusCode {
    antares_api::router(st.clone())
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(uri)
                .body(Body::empty())
                .expect("req"),
        )
        .await
        .expect("resp")
        .status()
}

#[tokio::test(flavor = "multi_thread")]
async fn cached_context_bookkeeping_is_shared_across_instances() {
    std::env::set_var("ANTARES_EGRESS_ALLOW_PRIVATE", "true");
    let store = Arc::new(antares_sql::store::any::AnyStore::Mem(Default::default()));
    let a = AppState::with_store("podA".into(), store.clone(), antares_sql::StoreMode::Memory);
    let b = AppState::with_store("podB".into(), store.clone(), antares_sql::StoreMode::Memory);
    let (url, fetches) = mock_ctx_server();
    let local_id = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, url.as_bytes()).to_string();
    let details = format!("/ngsi-ld/v1/jsonldContexts/{local_id}?details=true");

    // instance A uses the external @context (fetch + Cached write-through)
    a.loader.resolve(&json!(url)).await.expect("resolve on A");
    assert_eq!(fetches.load(std::sync::atomic::Ordering::SeqCst), 1);

    // instance B must see the Cached entry WITH the hit count (5.13.3.5) —
    // per-instance counters read 0 here, the fleet split-brain
    let (sc, body) = get(&b, &details).await;
    assert_eq!(sc, StatusCode::OK, "B must see A's Cached entry: {body}");
    assert_eq!(body["kind"], "Cached", "{body}");
    assert_eq!(
        body["numberOfHits"], 1,
        "hits must be shared, not per-instance: {body}"
    );

    // a second use on A: B's view counts it. The previous details GET
    // itself bumped after rendering (the 053_06/053_08 serve-counts-as-hit
    // contract), so the ledger reads 1 (A) + 1 (B's GET) + 1 (A) = 3.
    a.loader
        .resolve(&json!(url))
        .await
        .expect("re-resolve on A");
    let (_, body) = get(&b, &details).await;
    assert_eq!(body["numberOfHits"], 3, "second use invisible to B: {body}");
    // and the warm cache did NOT refetch (negative: hit-counting must not
    // turn every use into a network round-trip)
    assert_eq!(fetches.load(std::sync::atomic::Ordering::SeqCst), 1);

    // delete WITHOUT reload through B: gone for everyone (5.13.5.4) …
    let sc = delete(&b, &format!("/ngsi-ld/v1/jsonldContexts/{local_id}")).await;
    assert_eq!(sc, StatusCode::NO_CONTENT);
    let (sc, _) = get(&b, &details).await;
    assert_eq!(sc, StatusCode::NOT_FOUND, "deleted entry must 404");

    // … including instance A, whose loader memory is still warm: the next
    // use must honour the delete — refetch and re-create the Cached row —
    // not serve the deleted copy forever out of local cache
    a.loader
        .resolve(&json!(url))
        .await
        .expect("use after delete on A");
    assert_eq!(
        fetches.load(std::sync::atomic::Ordering::SeqCst),
        2,
        "A must refetch after B's delete (stale local copy served instead)"
    );
    let (sc, body) = get(&b, &details).await;
    assert_eq!(
        sc,
        StatusCode::OK,
        "re-use must re-create the Cached entry: {body}"
    );
    assert_eq!(
        body["numberOfHits"], 1,
        "fresh entry restarts counting: {body}"
    );
}
