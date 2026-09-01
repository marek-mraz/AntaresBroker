// SPDX-License-Identifier: EUPL-1.2
//! Security regression suite: known context-broker CVE-class findings,
//! kept failing-forever as tests.
//!
//! * Caches keyed by client-supplied URLs must have a SIZE cap,
//!   not just a TTL — asserted by overfilling the @context cache.
//! * Deleting a subscription must remove ALL its state — no
//!   orphaned bookkeeping keeps notifying (Scorpio's callback-UUID leak).
//! * The SIZE verdict comes before any parse — an oversized
//!   body of invalid JSON answers 413, never a parse error.
//! * Cross-tenant probes run per-commit in tenant_isolation.rs, a
//!   stricter cadence than a per-release e2e.

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

/// Size before parse: 5 MB of garbage that is NOT JSON must bounce off the size
/// wall (bare 413) — a 400 here would mean something tried to parse it.
#[tokio::test(flavor = "multi_thread")]
async fn oversized_body_is_rejected_before_any_parse() {
    let st = AppState::new("test".into());
    let garbage = "x".repeat(5 * 1024 * 1024); // over MAX_BODY_BYTES, invalid JSON
    let req = Request::builder()
        .method("POST")
        .uri("/ngsi-ld/v1/entities")
        .header("Content-Type", "application/json")
        // 6.3.4: absent Content-Length is its own bare 411 — this test is
        // about the SIZE wall, so declare the (oversized) length honestly
        .header("Content-Length", garbage.len())
        .body(Body::from(garbage))
        .expect("request");
    assert_eq!(send(&st, req).await, StatusCode::PAYLOAD_TOO_LARGE);
}

/// The parsed-@context cache is capped — 400 distinct
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
        "client-keyed cache must be size-capped: {fetched} entries"
    );
}

/// One batch may not multiply the per-resolution @context fetch cap by its
/// item count. The loader stops ONE resolution at `MAX_CONTEXT_FETCHES`
/// fetched documents; a batch resolves once per item, so a body inside the
/// 4 MiB limit that names a different @context URL per item would buy a
/// fetch per item at a host the client chooses — egress amplification, and
/// a port scan of whatever the deployment lets the broker reach. The
/// listener below counts connections, so the assertion is on what left the
/// process, not on what the response says.
#[tokio::test(flavor = "multi_thread")]
async fn one_batch_cannot_multiply_the_context_fetch_cap() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let hits = Arc::new(AtomicUsize::new(0));
    let seen = hits.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut sock) = stream else { break };
            seen.fetch_add(1, Ordering::SeqCst);
            let mut buf = [0u8; 2048];
            let _ = sock.read(&mut buf);
            let body = r#"{"@context":{"t":"https://example.org/t"}}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/ld+json\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes());
        }
    });

    const ITEMS: usize = 200;
    let items: Vec<serde_json::Value> = (0..ITEMS)
        .map(|i| {
            serde_json::json!({
                "id": format!("urn:ngsi-ld:Batch:{i}"),
                "type": "t",
                "@context": format!("http://127.0.0.1:{}/c{i}.jsonld", addr.port()),
            })
        })
        .collect();
    let body = serde_json::Value::Array(items).to_string();
    let st = AppState::new("test".into());
    let req = Request::builder()
        .method("POST")
        .uri("/ngsi-ld/v1/entityOperations/create")
        .header("Content-Type", "application/ld+json")
        .header("Content-Length", body.len())
        .body(Body::from(body))
        .expect("request");
    let status = send(&st, req).await;
    assert!(
        status.is_success() || status == StatusCode::MULTI_STATUS,
        "the batch answers per item, it does not fail as a whole: {status}"
    );

    let fetched = hits.load(Ordering::SeqCst);
    assert!(
        fetched <= antares_api::bounds::MAX_CONTEXT_FETCHES,
        "one request fetched {fetched} @context documents, cap is {}",
        antares_api::bounds::MAX_CONTEXT_FETCHES
    );
    assert!(
        fetched < ITEMS,
        "every item fetched its own @context — the cap did not apply"
    );
}

/// After DELETE, a subscription's state is GONE — later matching
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

    // egress: the receiver is loopback, which is denied by default —
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
                .header("Content-Length", body.len())
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
        "a deleted subscription kept notifying — orphaned state"
    );
}

/// HTTP Parameter Pollution across the gateway seam. Implementations
/// disagree on which occurrence of a repeated query parameter wins — first,
/// last, or the values joined — so a policy layer in front of the broker can
/// authorize one value while the broker acts on another. CIM 009 delegates
/// authorization to that layer (no clause gives the broker an authz model),
/// which makes the disagreement the whole exposure. The broker refuses the
/// ambiguity instead of picking a side, the way 6.3.14 already refuses a
/// repeated `NGSILD-Tenant`.
#[tokio::test(flavor = "multi_thread")]
async fn a_repeated_query_parameter_is_refused_not_silently_resolved() {
    let st = AppState::new("test".into());
    for uri in [
        // the selector a gateway would police
        "/ngsi-ld/v1/entities?type=Vehicle&type=Secret",
        // and the filter behind it
        "/ngsi-ld/v1/entities?type=Vehicle&q=speed%3E1&q=speed%3C1",
        // an empty first occurrence must not hide the repeat
        "/ngsi-ld/v1/entities?type=&type=Secret",
    ] {
        let req = Request::builder()
            .method("GET")
            .uri(uri)
            .body(Body::empty())
            .expect("request");
        assert_eq!(
            send(&st, req).await,
            StatusCode::BAD_REQUEST,
            "a repeated query parameter must not resolve silently: {uri}"
        );
    }
}

/// The repeated-parameter guard must refuse the ambiguity WITHOUT refusing
/// the spec's own way of naming several types. 4.17 Entity Type Selection
/// Language passes a disjunction inside ONE parameter (`,` and `|` are OR,
/// `(a;b)` is AND), so a conformant client never repeats a parameter and
/// nothing it sends is affected.
#[tokio::test(flavor = "multi_thread")]
async fn the_repeated_parameter_guard_leaves_conformant_queries_alone() {
    let st = AppState::new("test".into());
    for uri in [
        // 4.17: two types are one parameter, not two
        "/ngsi-ld/v1/entities?type=Vehicle,Building",
        "/ngsi-ld/v1/entities?type=Vehicle%7CBuilding",
        // distinct parameters are not a repeat
        "/ngsi-ld/v1/entities?type=Vehicle&attrs=speed",
        // 6.3.20 allows a parameter to appear once with an empty value
        "/ngsi-ld/v1/entities?type=Vehicle&attrs=",
    ] {
        let req = Request::builder()
            .method("GET")
            .uri(uri)
            .body(Body::empty())
            .expect("request");
        assert_eq!(
            send(&st, req).await,
            StatusCode::OK,
            "the guard must not touch a conformant query: {uri}"
        );
    }
}

/// The repeat is counted on the DECODED key, so percent-encoding cannot hide
/// it. `%74ype` is `type`: a guard comparing raw strings would let the second
/// occurrence through and hand the policy layer in front exactly the
/// disagreement the guard exists to remove.
#[tokio::test(flavor = "multi_thread")]
async fn a_percent_encoded_key_cannot_smuggle_a_repeat_past_the_guard() {
    let st = AppState::new("test".into());
    for uri in [
        // %74 is 't'
        "/ngsi-ld/v1/entities?type=Vehicle&%74ype=Secret",
        // the same the other way round: the encoded one first
        "/ngsi-ld/v1/entities?%74ype=Vehicle&type=Secret",
        // a valueless first occurrence is still an occurrence
        "/ngsi-ld/v1/entities?type&type=Secret",
        // `+` decodes to a space, so both of these name the key `a b`
        "/ngsi-ld/v1/entities?a+b=1&a%20b=2",
    ] {
        let req = Request::builder()
            .method("GET")
            .uri(uri)
            .body(Body::empty())
            .expect("request");
        assert_eq!(
            send(&st, req).await,
            StatusCode::BAD_REQUEST,
            "an encoded repeat must be refused like a plain one: {uri}"
        );
    }
}
