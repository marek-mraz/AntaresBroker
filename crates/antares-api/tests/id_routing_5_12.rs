//! 5.12 / 4.3.6.1 — id & idPattern routing decisions at the query fan-out.
//!
//! 5.12 (pp. 241-242): a query-side id pattern matches an EntityInfo only
//! via "The specified id pattern matches the id in the EntityInfo" or "Both
//! a specified id pattern and an idPattern in the Entity Info are present";
//! with an exact-id EntityInfo and a non-matching query pattern NO condition
//! holds, so the registration is not matched and the source is never dialed.
//!
//! 4.3.6.1 (p. 40): "It is the responsibility of the Context Broker to
//! respect the registration parameters when issuing distributed requests …
//! This applies for any kind of context data a Context Broker can exchange
//! such as Entity IDs, entity types, attribute names … Ultimately, all
//! constraints specified in the registration shall be respected." — the
//! forwarded id list is narrowed to the ids the registration can match.

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tower::ServiceExt;

const ID_BB: &str = "urn:ngsi-ld:Vehicle:sk_banskabystrica:odpady:0042";
const ID_PRESOV: &str = "urn:ngsi-ld:Vehicle:sk_presov:odpady:0001";
// ^urn:ngsi-ld:Vehicle:sk_banskabystrica:.*$ / ^…sk_presov:.*$, percent-encoded
const PAT_BB_ENC: &str = "%5Eurn%3Angsi-ld%3AVehicle%3Ask_banskabystrica%3A.%2A%24";
const PAT_PRESOV_ENC: &str = "%5Eurn%3Angsi-ld%3AVehicle%3Ask_presov%3A.%2A%24";

struct Mock {
    port: u16,
    hits: Arc<AtomicUsize>,
    last_head: Arc<std::sync::Mutex<String>>,
}

fn mock_replying(reply: &'static str) -> Mock {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let reply = reply.replacen("\r\n", "\r\nConnection: close\r\n", 1);
    let hits: Arc<AtomicUsize> = Arc::default();
    let last_head: Arc<std::sync::Mutex<String>> = Arc::default();
    let (seen, head) = (hits.clone(), last_head.clone());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            seen.fetch_add(1, Ordering::SeqCst);
            let mut buf = [0u8; 8192];
            let n = s.read(&mut buf).unwrap_or(0);
            *head.lock().expect("lock") = String::from_utf8_lossy(&buf[..n])
                .split("\r\n\r\n")
                .next()
                .unwrap_or_default()
                .to_owned();
            let _ = s.write_all(reply.as_bytes());
        }
    });
    Mock {
        port,
        hits,
        last_head,
    }
}

fn state() -> AppState {
    std::env::set_var("ANTARES_EGRESS_ALLOW_PRIVATE", "true");
    AppState::new("antares1".into())
}

async fn send(st: &AppState, req: Request<Body>) -> axum::http::Response<Body> {
    antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("response")
}

async fn register(st: &AppState, information: serde_json::Value, port: u16) {
    let doc = serde_json::json!({
        "id": "urn:ngsi-ld:ContextSourceRegistration:idr",
        "type": "ContextSourceRegistration",
        "information": information,
        "endpoint": format!("http://127.0.0.1:{port}"),
    });
    let body = doc.to_string();
    let req = Request::builder()
        .method("POST")
        .uri("/ngsi-ld/v1/csourceRegistrations")
        .header("Content-Type", "application/json")
        .header("Content-Length", body.len())
        .body(Body::from(body))
        .expect("request");
    assert_eq!(send(st, req).await.status(), StatusCode::CREATED);
}

fn query(uri: String) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .expect("request")
}

const EMPTY_ARR: &str =
    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n[]";

/// 5.12: exact-id EntityInfo vs a foreign query idPattern — no match
/// condition holds, the source records ZERO requests; the same pattern
/// aimed at the registered id IS forwarded (self-proving control).
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_12_query_pattern_vs_exact_entityinfo_id_not_forwarded() {
    let st = state();
    let m = mock_replying(EMPTY_ARR);
    let info = serde_json::json!([{"entities": [{"type": "Vehicle", "id": ID_BB}]}]);
    register(&st, info, m.port).await;

    let res = send(
        &st,
        query(format!(
            "/ngsi-ld/v1/entities?type=Vehicle&idPattern={PAT_PRESOV_ENC}"
        )),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        m.hits.load(Ordering::SeqCst),
        0,
        "a foreign query pattern vs an exact EntityInfo id must not forward (5.12)"
    );

    // control: a pattern that matches the registered id forwards (cond 4)
    let res = send(
        &st,
        query(format!(
            "/ngsi-ld/v1/entities?type=Vehicle&idPattern={PAT_BB_ENC}"
        )),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        m.hits.load(Ordering::SeqCst),
        1,
        "a pattern matching the EntityInfo id forwards (5.12 condition 4)"
    );
}

/// 4.3.6.1: the forwarded query's id list carries ONLY the ids the
/// registration's idPattern can match — never the full client list.
#[tokio::test(flavor = "multi_thread")]
async fn clause_4_3_6_1_forwarded_id_list_narrowed_to_registration() {
    let st = state();
    let m = mock_replying(EMPTY_ARR);
    let info = serde_json::json!(
        [{"entities": [{"type": "Vehicle",
            "idPattern": "^urn:ngsi-ld:Vehicle:sk_banskabystrica:.*$"}]}]
    );
    register(&st, info, m.port).await;

    let res = send(
        &st,
        query(format!(
            "/ngsi-ld/v1/entities?type=Vehicle&id={ID_PRESOV},{ID_BB}"
        )),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        m.hits.load(Ordering::SeqCst),
        1,
        "the matching CSR is dialed"
    );
    let head = m
        .last_head
        .lock()
        .expect("lock")
        .replace("%3A", ":")
        .replace("%2C", ",");
    assert!(
        head.contains(ID_BB),
        "the matching id is on the forwarded request: {head}"
    );
    assert!(
        !head.contains(ID_PRESOV),
        "an id the registration cannot match must NOT be forwarded (4.3.6.1): {head}"
    );
}

/// Over-narrowing guard: a type-only EntityInfo imposes no id restriction —
/// the client's full id list is forwarded unchanged.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_12_type_only_entityinfo_keeps_the_full_id_list() {
    let st = state();
    let m = mock_replying(EMPTY_ARR);
    let info = serde_json::json!([{"entities": [{"type": "Vehicle"}]}]);
    register(&st, info, m.port).await;

    let res = send(
        &st,
        query(format!(
            "/ngsi-ld/v1/entities?type=Vehicle&id={ID_PRESOV},{ID_BB}"
        )),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(m.hits.load(Ordering::SeqCst), 1);
    let head = m
        .last_head
        .lock()
        .expect("lock")
        .replace("%3A", ":")
        .replace("%2C", ",");
    assert!(head.contains(ID_BB), "both ids forwarded: {head}");
    assert!(head.contains(ID_PRESOV), "both ids forwarded: {head}");
}

/// 5.12 attribute conditions on the query fan-out: a RegistrationInfo
/// listing only fillLevel is NOT dialed for ?attrs= of another attribute;
/// an entities-only RegistrationInfo (empty combination) matches any attrs.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_12_attrs_scope_gates_the_query_fanout() {
    let st = state();
    let m = mock_replying(EMPTY_ARR);
    let info = serde_json::json!(
        [{"entities": [{"type": "Vehicle"}], "propertyNames": ["fillLevel"]}]
    );
    register(&st, info, m.port).await;

    let res = send(
        &st,
        query("/ngsi-ld/v1/entities?type=Vehicle&attrs=speed".into()),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        m.hits.load(Ordering::SeqCst),
        0,
        "an attribute-scope mismatch must not forward (5.12)"
    );

    let res = send(
        &st,
        query("/ngsi-ld/v1/entities?type=Vehicle&attrs=fillLevel".into()),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        m.hits.load(Ordering::SeqCst),
        1,
        "the registered attribute IS forwarded (5.12)"
    );

    // entities-only registration: "the combination … is empty" ⇒ match
    let st2 = state();
    let m2 = mock_replying(EMPTY_ARR);
    let info = serde_json::json!([{"entities": [{"type": "Vehicle"}]}]);
    register(&st2, info, m2.port).await;
    let res = send(
        &st2,
        query("/ngsi-ld/v1/entities?type=Vehicle&attrs=whatever".into()),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        m2.hits.load(Ordering::SeqCst),
        1,
        "an entities-only RegistrationInfo matches any attrs (5.12)"
    );
}

/// 5.12 datasetId condition (should-level, implemented): disjoint datasetId
/// sets do not match; only one side specifying is a match.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_12_dataset_id_common_value_gates_matching() {
    let st = state();
    let m = mock_replying(EMPTY_ARR);
    let doc = serde_json::json!({
        "id": "urn:ngsi-ld:ContextSourceRegistration:idr-ds",
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": "Vehicle"}]}],
        "datasetId": ["urn:ngsi-ld:Dataset:b"],
        "endpoint": format!("http://127.0.0.1:{}", m.port),
    });
    let body = doc.to_string();
    let req = Request::builder()
        .method("POST")
        .uri("/ngsi-ld/v1/csourceRegistrations")
        .header("Content-Type", "application/json")
        .header("Content-Length", body.len())
        .body(Body::from(body))
        .expect("request");
    assert_eq!(send(&st, req).await.status(), StatusCode::CREATED);

    let res = send(
        &st,
        query("/ngsi-ld/v1/entities?type=Vehicle&datasetId=urn:ngsi-ld:Dataset:a".into()),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        m.hits.load(Ordering::SeqCst),
        0,
        "disjoint datasetId sets must not match (5.12)"
    );

    // only the registration specifies a datasetId ⇒ match
    let res = send(&st, query("/ngsi-ld/v1/entities?type=Vehicle".into())).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        m.hits.load(Ordering::SeqCst),
        1,
        "one-sided datasetId is a match (5.12)"
    );

    // a common value ⇒ match
    let res = send(
        &st,
        query("/ngsi-ld/v1/entities?type=Vehicle&datasetId=urn:ngsi-ld:Dataset:b,urn:ngsi-ld:Dataset:c".into()),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        m.hits.load(Ordering::SeqCst),
        2,
        "a common datasetId value is a match (5.12)"
    );
}
