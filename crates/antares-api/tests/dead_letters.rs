// SPDX-License-Identifier: EUPL-1.2
//! Dead letters on the admin surface: notifications the delivery policy
//! gave up on are listed, replayed once through the same binding, or
//! deleted — always for ONE tenant named by `?tenant=`, never reachable
//! under the NGSI-LD API root.

use antares_api::AppState;
use antares_model::TenantId;
use antares_store::Kind;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

async fn send(st: &AppState, method: &str, path: &str) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(method)
        .uri(path)
        // what every HTTP/1.1 client sends for a bodyless request: the
        // bounds wall in front of this surface answers a POST without a
        // length with 6.3.4's bare 411.
        .header(axum::http::header::CONTENT_LENGTH, "0")
        .body(Body::empty())
        .expect("request");
    let res = antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("response");
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .expect("body");
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, body)
}

fn state() -> AppState {
    antares_jsonld::allow_private_egress(true);
    AppState::new("antares-dead-letters".into())
}

type Seen = Arc<Mutex<Vec<(Vec<(String, String)>, Value)>>>;

/// A receiver answering `status`, recording headers and body.
async fn receiver(status: StatusCode) -> (String, Arc<AtomicUsize>, Seen) {
    let hits: Arc<AtomicUsize> = Arc::default();
    let seen: Seen = Arc::default();
    let (h, s) = (hits.clone(), seen.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let app = axum::Router::new().route(
        "/notify",
        axum::routing::post(
            move |headers: axum::http::HeaderMap, body: axum::body::Bytes| {
                let (h, s) = (h.clone(), s.clone());
                async move {
                    h.fetch_add(1, Ordering::SeqCst);
                    let hs = headers
                        .iter()
                        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
                        .collect();
                    s.lock()
                        .expect("lock")
                        .push((hs, serde_json::from_slice(&body).unwrap_or(Value::Null)));
                    status
                }
            },
        ),
    );
    tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });
    (format!("http://{addr}/notify"), hits, seen)
}

fn letter(id: &str, sub: &str, uri: &str, last_at: &str) -> Value {
    json!({
        "id": id, "type": "DeadLetter", "subscriptionId": sub, "uri": uri,
        "binding": "http", "timeoutMs": 2000, "attempts": 2,
        "firstError": "HTTP 503", "lastError": "HTTP 503",
        "firstAt": "2026-01-01T00:00:00Z", "lastAt": last_at,
        "headers": [["Content-Type", "application/json"], ["NGSILD-Tenant", "acme"]],
        "payload": {"id": "urn:ngsi-ld:Notification:1", "type": "Notification",
                    "subscriptionId": sub, "data": [{"id": "urn:ngsi-ld:Room:1", "type": "Room"}]},
    })
}

fn seed(st: &AppState, tenant: &str, id: &str, sub: &str, uri: &str, last_at: &str) {
    let t = TenantId::new(tenant).expect("tenant");
    assert!(st
        .store
        .create(&t, Kind::DeadLetter, id, letter(id, sub, uri, last_at))
        .expect("seed"));
}

fn stored(st: &AppState, tenant: &str, id: &str) -> Option<Value> {
    let t = TenantId::new(tenant).expect("tenant");
    st.store.get(&t, Kind::DeadLetter, id).expect("get")
}

#[tokio::test(flavor = "multi_thread")]
async fn list_is_per_tenant_newest_first_filtered_and_bounded() {
    let st = state();
    seed(
        &st,
        "acme",
        "urn:ngsi-ld:DeadLetter:1",
        "urn:s:1",
        "http://127.0.0.1:9/n",
        "2026-01-01T00:00:01Z",
    );
    seed(
        &st,
        "acme",
        "urn:ngsi-ld:DeadLetter:2",
        "urn:s:2",
        "http://127.0.0.1:9/n",
        "2026-01-01T00:00:03Z",
    );
    seed(
        &st,
        "acme",
        "urn:ngsi-ld:DeadLetter:3",
        "urn:s:1",
        "http://127.0.0.1:9/n",
        "2026-01-01T00:00:02Z",
    );
    seed(
        &st,
        "other",
        "urn:ngsi-ld:DeadLetter:9",
        "urn:s:1",
        "http://127.0.0.1:9/n",
        "2026-01-01T00:00:09Z",
    );

    let (s, b) = send(&st, "GET", "/q/dead-letters?tenant=acme").await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let ids: Vec<&str> = b
        .as_array()
        .expect("array")
        .iter()
        .map(|l| l["id"].as_str().unwrap_or(""))
        .collect();
    assert_eq!(
        ids,
        [
            "urn:ngsi-ld:DeadLetter:2",
            "urn:ngsi-ld:DeadLetter:3",
            "urn:ngsi-ld:DeadLetter:1"
        ]
    );
    assert!(
        !b.to_string().contains("DeadLetter:9"),
        "another tenant's letter leaked: {b}"
    );

    let (_, b) = send(
        &st,
        "GET",
        "/q/dead-letters?tenant=acme&subscription=urn:s:1",
    )
    .await;
    assert_eq!(b.as_array().expect("array").len(), 2);
    let (_, b) = send(&st, "GET", "/q/dead-letters?tenant=acme&limit=1").await;
    assert_eq!(b.as_array().expect("array").len(), 1);
    assert_eq!(b[0]["id"], "urn:ngsi-ld:DeadLetter:2");
    let (_, b) = send(&st, "GET", "/q/dead-letters").await;
    assert_eq!(b, json!([]), "the default tenant holds none");
    let (_, b) = send(&st, "GET", "/q/dead-letters?tenant=never-seen").await;
    assert_eq!(b, json!([]));
}

#[tokio::test(flavor = "multi_thread")]
async fn bad_tenant_and_bad_limit_are_400() {
    let st = state();
    for path in [
        "/q/dead-letters?tenant=snap-abc",
        "/q/dead-letters?tenant=distsub-index",
        "/q/dead-letters?tenant=bad%20name",
        "/q/dead-letters?tenant=..%2F..",
        "/q/dead-letters?limit=0",
        "/q/dead-letters?limit=-1",
        "/q/dead-letters?limit=ten",
    ] {
        let (s, b) = send(&st, "GET", path).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{path}: {b}");
        assert!(
            b["type"]
                .as_str()
                .is_some_and(|t| t.ends_with("BadRequestData")),
            "{b}"
        );
    }
    let (s, _) = send(&st, "DELETE", "/q/dead-letters/urn:x?tenant=snap-abc").await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = send(&st, "POST", "/q/dead-letters/urn:x/replay?tenant=snap-abc").await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_endpoint_userinfo_is_redacted_in_the_listing_only() {
    let st = state();
    seed(
        &st,
        "acme",
        "urn:ngsi-ld:DeadLetter:1",
        "urn:s:1",
        "http://bob:s3cret@127.0.0.1:9/n",
        "2026-01-01T00:00:01Z",
    );
    let (_, b) = send(&st, "GET", "/q/dead-letters?tenant=acme").await;
    assert!(!b.to_string().contains("s3cret"), "{b}");
    assert!(
        b[0]["uri"]
            .as_str()
            .is_some_and(|u| u.contains("127.0.0.1:9/n")),
        "{b}"
    );
    // the stored letter keeps the URI a replay needs
    assert_eq!(
        stored(&st, "acme", "urn:ngsi-ld:DeadLetter:1").expect("kept")["uri"],
        "http://bob:s3cret@127.0.0.1:9/n"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn replay_delivers_the_same_request_and_deletes_the_letter() {
    let st = state();
    let (uri, hits, seen) = receiver(StatusCode::OK).await;
    seed(
        &st,
        "acme",
        "urn:ngsi-ld:DeadLetter:1",
        "urn:s:1",
        &uri,
        "2026-01-01T00:00:01Z",
    );
    let (s, b) = send(
        &st,
        "POST",
        "/q/dead-letters/urn:ngsi-ld:DeadLetter:1/replay?tenant=acme",
    )
    .await;
    assert_eq!(s, StatusCode::NO_CONTENT, "{b}");
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    let (headers, body) = seen.lock().expect("lock")[0].clone();
    assert!(
        headers.contains(&("content-type".into(), "application/json".into())),
        "{headers:?}"
    );
    assert!(
        headers.contains(&("ngsild-tenant".into(), "acme".into())),
        "{headers:?}"
    );
    assert_eq!(body["type"], "Notification");
    assert_eq!(body["data"][0]["id"], "urn:ngsi-ld:Room:1");
    assert!(
        stored(&st, "acme", "urn:ngsi-ld:DeadLetter:1").is_none(),
        "delivered letter is gone"
    );
    let (s, _) = send(
        &st,
        "POST",
        "/q/dead-letters/urn:ngsi-ld:DeadLetter:1/replay?tenant=acme",
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND, "a second replay finds nothing");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_replay_keeps_the_letter_and_extends_its_history() {
    let st = state();
    let (uri, hits, _) = receiver(StatusCode::SERVICE_UNAVAILABLE).await;
    seed(
        &st,
        "acme",
        "urn:ngsi-ld:DeadLetter:1",
        "urn:s:1",
        &uri,
        "2026-01-01T00:00:01Z",
    );
    let (s, b) = send(
        &st,
        "POST",
        "/q/dead-letters/urn:ngsi-ld:DeadLetter:1/replay?tenant=acme",
    )
    .await;
    assert_eq!(s, StatusCode::BAD_GATEWAY, "{b}");
    assert_eq!(b["detail"], "HTTP 503");
    assert_eq!(
        hits.load(Ordering::SeqCst),
        1,
        "replay is ONE attempt, never a retry loop"
    );
    let l = stored(&st, "acme", "urn:ngsi-ld:DeadLetter:1").expect("kept");
    assert_eq!(l["attempts"], 3);
    assert_eq!(l["lastError"], "HTTP 503");
    assert_ne!(l["lastAt"], "2026-01-01T00:00:01Z", "lastAt moved");
    assert_eq!(l["firstAt"], "2026-01-01T00:00:00Z", "firstAt never moves");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_replay_from_another_tenant_cannot_reach_the_letter() {
    let st = state();
    let (uri, hits, _) = receiver(StatusCode::OK).await;
    seed(
        &st,
        "acme",
        "urn:ngsi-ld:DeadLetter:1",
        "urn:s:1",
        &uri,
        "2026-01-01T00:00:01Z",
    );
    for path in [
        "/q/dead-letters/urn:ngsi-ld:DeadLetter:1/replay?tenant=other",
        "/q/dead-letters/urn:ngsi-ld:DeadLetter:1/replay",
        "/q/dead-letters/urn:ngsi-ld:DeadLetter:1?tenant=other",
    ] {
        let method = if path.contains("replay") {
            "POST"
        } else {
            "DELETE"
        };
        let (s, _) = send(&st, method, path).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "{path}");
    }
    assert_eq!(
        hits.load(Ordering::SeqCst),
        0,
        "nothing was sent on behalf of the wrong tenant"
    );
    assert!(
        stored(&st, "acme", "urn:ngsi-ld:DeadLetter:1").is_some(),
        "letter untouched"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn delete_removes_one_letter_once() {
    let st = state();
    seed(
        &st,
        "acme",
        "urn:ngsi-ld:DeadLetter:1",
        "urn:s:1",
        "http://127.0.0.1:9/n",
        "2026-01-01T00:00:01Z",
    );
    seed(
        &st,
        "acme",
        "urn:ngsi-ld:DeadLetter:2",
        "urn:s:1",
        "http://127.0.0.1:9/n",
        "2026-01-01T00:00:02Z",
    );
    let (s, _) = send(
        &st,
        "DELETE",
        "/q/dead-letters/urn:ngsi-ld:DeadLetter:1?tenant=acme",
    )
    .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s, _) = send(
        &st,
        "DELETE",
        "/q/dead-letters/urn:ngsi-ld:DeadLetter:1?tenant=acme",
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert!(
        stored(&st, "acme", "urn:ngsi-ld:DeadLetter:2").is_some(),
        "the sibling stays"
    );
}

/// The letter's endpoint URI decides the binding, exactly as it does for a
/// fresh send: a scheme no sink serves, or a letter that cannot be read back
/// into a notification, answers 502 and puts nothing on the wire.
#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_binding_or_unreadable_letter_is_a_502_not_a_send() {
    let st = state();
    let (uri, hits, _) = receiver(StatusCode::OK).await;
    let t = TenantId::new("acme").expect("tenant");
    let mut l = letter(
        "urn:ngsi-ld:DeadLetter:1",
        "urn:s:1",
        "smtp://mail.example.org/queue",
        "2026-01-01T00:00:01Z",
    );
    l["binding"] = json!("smtp");
    st.store
        .create(&t, Kind::DeadLetter, "urn:ngsi-ld:DeadLetter:1", l)
        .expect("seed");
    let mut l = letter(
        "urn:ngsi-ld:DeadLetter:2",
        "urn:s:1",
        &uri,
        "2026-01-01T00:00:01Z",
    );
    l["headers"] = json!("not a list");
    st.store
        .create(&t, Kind::DeadLetter, "urn:ngsi-ld:DeadLetter:2", l)
        .expect("seed");
    for id in ["urn:ngsi-ld:DeadLetter:1", "urn:ngsi-ld:DeadLetter:2"] {
        let (s, b) = send(
            &st,
            "POST",
            &format!("/q/dead-letters/{id}/replay?tenant=acme"),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_GATEWAY, "{b}");
        assert!(stored(&st, "acme", id).is_some());
    }
    assert_eq!(hits.load(Ordering::SeqCst), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn replay_respects_the_egress_policy_of_the_moment() {
    // The refusing policy is set on the state itself: the env var is
    // process-global and the other tests in this binary read it too.
    let mut st = state();
    st.egress = Arc::new(antares_api::egress::Egress::new(
        antares_jsonld::EgressPolicy {
            allow_private: false,
        },
    ));
    let (uri, hits, _) = receiver(StatusCode::OK).await;
    seed(
        &st,
        "acme",
        "urn:ngsi-ld:DeadLetter:1",
        "urn:s:1",
        &uri,
        "2026-01-01T00:00:01Z",
    );
    let (s, b) = send(
        &st,
        "POST",
        "/q/dead-letters/urn:ngsi-ld:DeadLetter:1/replay?tenant=acme",
    )
    .await;
    assert_eq!(s, StatusCode::BAD_GATEWAY, "{b}");
    assert_eq!(
        hits.load(Ordering::SeqCst),
        0,
        "a private-range endpoint is refused, not tried"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn dead_letter_routes_are_not_under_the_api_root_and_health_counts_them() {
    let st = state();
    for (m, p) in [
        ("GET", "/ngsi-ld/v1/q/dead-letters"),
        ("GET", "/ngsi-ld/v1/dead-letters"),
        ("POST", "/ngsi-ld/v1/dead-letters/x/replay"),
    ] {
        let (s, _) = send(&st, m, p).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "{p}");
    }
    let (s, b) = send(&st, "GET", "/q/health").await;
    assert_eq!(s, StatusCode::OK);
    assert!(b["deadLetters"].is_u64(), "{b}");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_listing_blanks_every_credential_a_letter_carries() {
    let st = state();
    let t = TenantId::new("acme").expect("tenant");
    let id = "urn:ngsi-ld:DeadLetter:9";
    let mut l = letter(
        id,
        "urn:s:9",
        "http://127.0.0.1:9/n",
        "2026-01-01T00:00:01Z",
    );
    // 5.2.22: a KeyValuePair's example content is "the HTTP Authentication
    // Header", so receiverInfo is where a subscriber's endpoint credential
    // lives. notifierInfo is opaque to every sink but the endpoint's own, so
    // the broker cannot tell which of its keys names a secret.
    l["receiverInfo"] = json!([["Authorization", "Bearer s3cret-token"]]);
    l["notifierInfo"] = json!([["MQTT-QoS", "1"], ["MQTT-Password", "hunter2"]]);
    // a letter written before the bindings moved behind the registry carries
    // the same values already rendered into headers
    l["headers"] = json!([
        ["Content-Type", "application/json"],
        ["Authorization", "Bearer legacy-token"]
    ]);
    assert!(st.store.create(&t, Kind::DeadLetter, id, l).expect("seed"));

    let (_, b) = send(&st, "GET", "/q/dead-letters?tenant=acme").await;
    let shown = b.to_string();
    for secret in ["s3cret-token", "hunter2", "legacy-token"] {
        assert!(!shown.contains(secret), "{secret} leaked: {shown}");
    }
    // the keys stay: an operator has to see which parameters were set
    for key in ["Authorization", "MQTT-QoS", "Content-Type"] {
        assert!(shown.contains(key), "{key} lost: {shown}");
    }
    // the stored letter keeps what a replay has to send
    let kept = stored(&st, "acme", id).expect("kept");
    assert_eq!(kept["receiverInfo"][0][1], "Bearer s3cret-token");
    assert_eq!(kept["notifierInfo"][1][1], "hunter2");
    assert_eq!(kept["headers"][1][1], "Bearer legacy-token");
}

/// A DeadLetter row that is not a JSON object reaches the listing the same
/// way every other row does — the store hands back whatever it holds, and a
/// row written by an older binary, a migration or an operator with psql is
/// not this process's own `json!` literal. Redaction has to survive it: the
/// listing is the route that exists so credentials do NOT leave, and a panic
/// there takes the connection with it.
#[tokio::test]
async fn a_stored_letter_of_the_wrong_shape_does_not_take_the_listing_down() {
    let st = state();
    let t = TenantId::new("acme").expect("tenant");
    for (id, doc) in [
        ("urn:ngsi-ld:DeadLetter:string", Value::String("x".into())),
        ("urn:ngsi-ld:DeadLetter:array", Value::Array(vec![])),
        ("urn:ngsi-ld:DeadLetter:number", Value::from(7)),
        ("urn:ngsi-ld:DeadLetter:null", Value::Null),
    ] {
        assert!(st
            .store
            .create(&t, Kind::DeadLetter, id, doc)
            .expect("seed"));
    }
    seed(
        &st,
        "acme",
        "urn:ngsi-ld:DeadLetter:ok",
        "urn:s:1",
        "http://user:secret@127.0.0.1:9/n",
        "2026-01-01T00:00:01Z",
    );

    let (s, b) = send(&st, "GET", "/q/dead-letters?tenant=acme").await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let listed = b.as_array().expect("array");
    assert_eq!(listed.len(), 5, "every row is listed: {b}");
    assert!(
        !b.to_string().contains("secret"),
        "the userinfo of the well-formed row is still redacted: {b}"
    );

    // and the replay bookkeeping writes through the same shape
    let (s, _) = send(
        &st,
        "POST",
        "/q/dead-letters/urn:ngsi-ld:DeadLetter:string/replay?tenant=acme",
    )
    .await;
    assert_eq!(s, StatusCode::BAD_GATEWAY, "an unreadable letter is a 502");
}
