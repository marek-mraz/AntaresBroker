// SPDX-License-Identifier: EUPL-1.2
//! 5.13 behind a load balancer: two api
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

/// set_var once: a sibling test reading the env while another rewrites it
/// saw the policy missing and refused the loopback forward (TSan flake).
fn allow_private() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| std::env::set_var("ANTARES_EGRESS_ALLOW_PRIVATE", "true"));
}

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

async fn get_with_header(st: &AppState, uri: &str, name: &str, value: &str) -> (StatusCode, Value) {
    let resp = antares_api::router(st.clone())
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(uri)
                .header(name, value)
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

async fn post_ld(st: &AppState, uri: &str, body: Value) -> StatusCode {
    antares_api::router(st.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("Content-Type", "application/ld+json")
                .header("Content-Length", body.to_string().len())
                .body(Body::from(body.to_string()))
                .expect("req"),
        )
        .await
        .expect("resp")
        .status()
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
    allow_private();
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

/// 5.13.5.4 (matrix 12: 053_06 404 / 051_03 KeyError 'kind'): a delete
/// through B while A's FETCHED doc cache is still warm but its merged cache
/// is cold (any put_local/evict clears merged globally). A's next counted
/// use resolves cold, the fetch is served from the warm doc — the
/// write-through never runs — and the row must STILL be re-created: a use
/// after a delete re-caches the @context, it does not lose it.
#[tokio::test(flavor = "multi_thread")]
async fn deleted_row_recreated_when_peer_doc_cache_is_warm() {
    allow_private();
    let store = Arc::new(antares_sql::store::any::AnyStore::Mem(Default::default()));
    let a = AppState::with_store("podA".into(), store.clone(), antares_sql::StoreMode::Memory);
    let b = AppState::with_store("podB".into(), store.clone(), antares_sql::StoreMode::Memory);
    let (url, fetches) = mock_ctx_server();
    let local_id = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, url.as_bytes()).to_string();
    let details = format!("/ngsi-ld/v1/jsonldContexts/{local_id}?details=true");

    a.loader.resolve(&json!(url)).await.expect("resolve on A");
    let sc = delete(&b, &format!("/ngsi-ld/v1/jsonldContexts/{local_id}")).await;
    assert_eq!(sc, StatusCode::NO_CONTENT);

    // knock A's merged cache cold while its fetched doc stays warm
    a.loader
        .put_local("http://unrelated.example/ctx".into(), json!({}))
        .await;

    a.loader
        .resolve(&json!(url))
        .await
        .expect("use after delete on A");
    let (sc, body) = get(&b, &details).await;
    assert_eq!(
        sc,
        StatusCode::OK,
        "counted use must re-create the deleted Cached row: {body}"
    );
    assert_eq!(body["kind"], "Cached", "{body}");
    assert_eq!(body["numberOfHits"], 1, "{body}");
    assert_eq!(
        fetches.load(std::sync::atomic::Ordering::SeqCst),
        2,
        "re-creation must refetch, not resurrect the pre-delete copy"
    );
}

/// 5.13.4.4 (matrix 12: 053_08 numberOfHits 3 != 2): a broker resolving an
/// ImplicitlyCreated @context by HTTP (fleet peer behind the LB) marks the
/// fetch as internal — the serving instance must NOT add a serve-hit on top
/// of the resolving instance's own bump. Client serves still count.
#[tokio::test(flavor = "multi_thread")]
async fn internal_context_fetch_does_not_count_as_serve_hit() {
    let store = Arc::new(antares_sql::store::any::AnyStore::Mem(Default::default()));
    let a = AppState::with_store("podA".into(), store, antares_sql::StoreMode::Memory);
    let sc = post_ld(
        &a,
        "/ngsi-ld/v1/subscriptions",
        json!({
            "id": "urn:ngsi-ld:Subscription:fleet-implicit",
            "type": "Subscription",
            "entities": [{"type": "Vehicle"}],
            "notification": {"endpoint": {"uri": "http://example.org/sink"}},
            "@context": [
                {"speed": "http://example.org/speed"},
                "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld"
            ]
        }),
    )
    .await;
    assert_eq!(sc, StatusCode::CREATED);
    let (sc, list) = get(
        &a,
        "/ngsi-ld/v1/jsonldContexts?details=true&kind=ImplicitlyCreated",
    )
    .await;
    assert_eq!(sc, StatusCode::OK);
    let lid = list[0]["localId"].as_str().expect("implicit id").to_owned();

    // the fleet peer's loader fetch (internal marker): serves, never counts
    let (sc, body) = get_with_header(
        &a,
        &format!("/ngsi-ld/v1/jsonldContexts/{lid}"),
        "X-Antares-Ctx-Fetch",
        "1",
    )
    .await;
    assert_eq!(sc, StatusCode::OK);
    assert!(body.get("@context").is_some(), "must serve content: {body}");
    let (_, body) = get(
        &a,
        &format!("/ngsi-ld/v1/jsonldContexts/{lid}?details=true"),
    )
    .await;
    assert_eq!(
        body["numberOfHits"], 0,
        "internal fetch must not count as a serve hit: {body}"
    );

    // a CLIENT serve does count (053_08 arithmetic): the details GET above
    // bumped after rendering (→1), this plain GET bumps again (→2)
    let (sc, _) = get(&a, &format!("/ngsi-ld/v1/jsonldContexts/{lid}")).await;
    assert_eq!(sc, StatusCode::OK);
    let (_, body) = get(
        &a,
        &format!("/ngsi-ld/v1/jsonldContexts/{lid}?details=true"),
    )
    .await;
    assert_eq!(body["numberOfHits"], 2, "client serves must count: {body}");
}
