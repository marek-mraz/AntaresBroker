// SPDX-License-Identifier: EUPL-1.2
//! 4.21 NGSI-LD Attribute Projection Language applied to notification
//! `pick`/`omit`, which Table 5.2.14.1-1 defines as "a valid attribute
//! projection language string as per clause 4.21".
//!
//! The grammar is
//!
//! ```text
//! ProjectionTerm   = AttrName *1(LinkedEntityTerm) *(orOp ProjectionTerm)
//! LinkedEntityTerm = %x7B ProjectionTerm %x7D          ; {ProjectionTerm}
//! orOp             = %x7C / %x2C                       ; |  ,
//! ```
//!
//! so a term may carry ONE optional LinkedEntityTerm that constrains the
//! Attributes taken from an Entity reached by Linked Entity Retrieval (join).
//!
//! Regression guarded here: the notification path once built its projection
//! nodes by hand instead of calling the 4.21 parser, so `refDevice{model}`
//! became a literal Attribute name matching nothing and the Relationship was
//! dropped from the payload entirely. The query and temporal paths always
//! parsed it, so the divergence was invisible from the outside.
//!
//! Both directions are asserted on purpose: a nested term MUST constrain the
//! Linked Entity, and a bare term MUST NOT — the ABNF makes nesting the
//! mechanism for reaching inside, so "always project the Linked Entity" would
//! be just as wrong as never projecting it.

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

/// The programmatic egress override, not `ANTARES_EGRESS_ALLOW_PRIVATE`: a
/// sibling test reading the environment while another rewrote it saw the
/// policy missing and refused the loopback forward. An atomic store carries
/// the same switch with no write for a reader to land in the middle of.
fn allow_private() {
    antares_jsonld::allow_private_egress(true);
}

async fn send(st: &AppState, path: &str, doc: Value) -> (StatusCode, String) {
    let body = doc.to_string();
    let req = Request::builder()
        .method("POST")
        .uri(format!("/ngsi-ld/v1/{path}"))
        .header("Content-Type", "application/json")
        .header("Content-Length", body.len())
        .body(Body::from(body))
        .expect("req");
    let resp = antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("resp");
    let status = resp.status();
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

async fn capture_server() -> (String, tokio::sync::mpsc::Receiver<Value>) {
    let (tx, rx) = tokio::sync::mpsc::channel::<Value>(4);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let app = axum::Router::new().route(
        "/notify",
        axum::routing::post(move |body: axum::body::Bytes| {
            let tx = tx.clone();
            async move {
                let v: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
                let _ = tx.send(v).await;
                StatusCode::OK
            }
        }),
    );
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    (format!("http://{addr}/notify"), rx)
}

/// A Device carrying one Attribute a subscriber asked for (`model`) and one it
/// did not (`apiKey`), so "was the Linked Entity constrained?" is answerable
/// from the payload alone.
async fn seed_device(st: &AppState) {
    let (status, body) = send(
        st,
        "entities",
        json!({"id": "urn:ngsi-ld:Device:d1", "type": "Device",
               "model": {"type": "Property", "value": "X100"},
               "apiKey": {"type": "Property", "value": "SECRET-CANARY"}}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

async fn subscribe(st: &AppState, pick: Value) -> tokio::sync::mpsc::Receiver<Value> {
    let (uri, rx) = capture_server().await;
    let (status, body) = send(
        st,
        "subscriptions",
        json!({"type": "Subscription", "entities": [{"type": "Vehicle"}],
        "notification": {
            "endpoint": {"uri": uri},
            "pick": pick,
            "join": "inline",
            "joinLevel": 1
        }}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    rx
}

async fn fire_and_collect(st: &AppState, mut rx: tokio::sync::mpsc::Receiver<Value>) -> Value {
    let (status, body) = send(
        st,
        "entities",
        json!({"id": "urn:ngsi-ld:Vehicle:v1", "type": "Vehicle",
               "speed": {"type": "Property", "value": 42},
               "refDevice": {"type": "Relationship", "object": "urn:ngsi-ld:Device:d1"}}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    tokio::time::timeout(
        std::time::Duration::from_secs(5 * antares_api::state::slow_factor()),
        rx.recv(),
    )
    .await
    .expect("notification within 5s")
    .expect("one notification")
}

fn linked_entity(n: &Value) -> Value {
    n["data"][0]["refDevice"]["entity"].clone()
}

/// A LinkedEntityTerm constrains the joined Entity: `refDevice{model}` delivers
/// `model` and nothing else from the Device.
#[tokio::test(flavor = "multi_thread")]
async fn clause_4_21_nested_term_constrains_the_linked_entity() {
    allow_private();
    let mut st = AppState::new("me".into());
    antares_api::notify::wire(&mut st);
    seed_device(&st).await;

    let rx = subscribe(&st, json!(["id", "type", "speed", "refDevice{model}"])).await;
    let n = fire_and_collect(&st, rx).await;

    let linked = linked_entity(&n);
    assert!(
        !linked.is_null(),
        "refDevice must survive: a nested term is a valid ProjectionTerm, not a \
         literal Attribute name that matches nothing — {n}"
    );
    assert!(
        linked.get("model").is_some(),
        "the Attribute named inside {{…}} must be delivered — {n}"
    );
    assert!(
        linked.get("apiKey").is_none(),
        "an Attribute NOT named inside {{…}} must not be delivered — {n}"
    );
    assert!(
        !serde_json::to_string(&n)
            .expect("json")
            .contains("SECRET-CANARY"),
        "no unrequested Linked Entity value may appear anywhere in the payload — {n}"
    );
}

/// The other direction, so the fix cannot be "over-corrected": a BARE
/// Attribute name has no LinkedEntityTerm, so per the ABNF it selects the
/// Relationship whole. Projecting it anyway would break 4.21.
#[tokio::test(flavor = "multi_thread")]
async fn clause_4_21_bare_term_selects_the_whole_linked_entity() {
    allow_private();
    let mut st = AppState::new("me".into());
    antares_api::notify::wire(&mut st);
    seed_device(&st).await;

    let rx = subscribe(&st, json!(["id", "type", "speed", "refDevice"])).await;
    let n = fire_and_collect(&st, rx).await;

    let linked = linked_entity(&n);
    assert!(
        linked.get("model").is_some() && linked.get("apiKey").is_some(),
        "a bare term carries no LinkedEntityTerm, so the joined Entity is not \
         constrained; a PEP wanting a bound must write refDevice{{…}} or set \
         join to @none — {n}"
    );
}

/// 4.21: "either a comma or a pipe character can be used as alternative
/// representations of the or operator" — including inside a LinkedEntityTerm,
/// and including when the whole thing arrives as one array element.
#[tokio::test(flavor = "multi_thread")]
async fn clause_4_21_pipe_and_comma_are_equivalent_in_notifications() {
    allow_private();
    let mut st = AppState::new("me".into());
    antares_api::notify::wire(&mut st);
    seed_device(&st).await;

    // one array element carrying a disjunction, and a pipe inside the braces
    let rx = subscribe(&st, json!(["id,type|speed", "refDevice{model|apiKey}"])).await;
    let n = fire_and_collect(&st, rx).await;

    assert_eq!(
        n["data"][0]["speed"]["value"],
        json!(42),
        "pipe orOp at top level — {n}"
    );
    let linked = linked_entity(&n);
    assert!(
        linked.get("model").is_some() && linked.get("apiKey").is_some(),
        "pipe orOp inside a LinkedEntityTerm selects both — {n}"
    );
}

/// `omit` takes the same language: with children it removes only the named
/// Attribute from the Linked Entity, leaving the rest.
#[tokio::test(flavor = "multi_thread")]
async fn clause_4_21_omit_with_children_constrains_the_linked_entity() {
    allow_private();
    let mut st = AppState::new("me".into());
    antares_api::notify::wire(&mut st);
    seed_device(&st).await;

    let (uri, mut rx) = capture_server().await;
    let (status, body) = send(
        &st,
        "subscriptions",
        json!({"type": "Subscription", "entities": [{"type": "Vehicle"}],
               "notification": {"endpoint": {"uri": uri},
                                "omit": ["refDevice{apiKey}"],
                                "join": "inline", "joinLevel": 1}}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let (status, _) = send(
        &st,
        "entities",
        json!({"id": "urn:ngsi-ld:Vehicle:v1", "type": "Vehicle",
               "speed": {"type": "Property", "value": 42},
               "refDevice": {"type": "Relationship", "object": "urn:ngsi-ld:Device:d1"}}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let n = tokio::time::timeout(
        std::time::Duration::from_secs(5 * antares_api::state::slow_factor()),
        rx.recv(),
    )
    .await
    .expect("notification within 5s")
    .expect("one notification");

    let linked = linked_entity(&n);
    assert!(
        linked.get("apiKey").is_none(),
        "the Attribute named inside {{…}} must be omitted from the Linked Entity — {n}"
    );
    assert!(
        linked.get("model").is_some(),
        "omit with children removes only what it names, not the whole Linked Entity — {n}"
    );
}
