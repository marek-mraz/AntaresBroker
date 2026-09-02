// SPDX-License-Identifier: EUPL-1.2
//! The subscription mirror is the only place the matcher reads candidates
//! from, so what happens when it cannot be filled decides whether a tenant
//! is notified at all.
//!
//! `CurrentStateDriver::subscription_tenants` says it of the data path: "A
//! SUBSET is a silent outage: a tenant missing here never fires a periodic
//! notification and never reaches the mirror." A store error absorbed into
//! an empty list is that same subset, reached from the error path — and it
//! is reachable, because the Postgres arm refuses `list` with
//! `TooManyResults` once a tenant holds `MAX_UNDECIDED_ROWS` documents, and
//! any connection failure at startup does the same.
//!
//! `subs_for` documents the intended degradation: the store scan is there
//! "as the never-wired fallback so a missing mirror degrades to
//! correct-but-slow". A half-filled mirror is the one state that is neither
//! correct nor slow, so a seed that cannot complete must leave the mirror
//! uninstalled rather than install what it managed to read.

#![allow(clippy::unwrap_used)] // an unwrap here is the assertion

mod common;

use antares_api::AppState;
use antares_model::TenantId;
use antares_store::{CurrentStateDriver, Kind};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::Double;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;

/// set_var once: a sibling test reading the env while another rewrites it
/// saw the policy missing and refused the loopback forward.
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

async fn expect_notification(rx: &mut tokio::sync::mpsc::Receiver<Value>, why: &str) -> Value {
    tokio::time::timeout(
        std::time::Duration::from_secs(5 * antares_api::state::slow_factor()),
        rx.recv(),
    )
    .await
    .unwrap_or_else(|_| panic!("{why}"))
    .expect("one notification")
}

/// A restart whose mirror seed cannot read the store still notifies: the
/// mirror is left uninstalled and matching takes the store-scan fallback,
/// rather than the process running for its whole life against a mirror that
/// silently holds none of the tenant's subscriptions.
#[tokio::test(flavor = "multi_thread")]
async fn a_mirror_seed_that_cannot_read_the_store_does_not_silence_the_tenant() {
    allow_private();

    let mut first = AppState::new("me".into());
    antares_api::wire(&mut first);

    let (uri, mut rx) = capture_server().await;
    let (status, body) = send(
        &first,
        "subscriptions",
        json!({"type": "Subscription", "entities": [{"type": "Vehicle"}],
               "notification": {"endpoint": {"uri": uri}}}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    // The subscription works before the restart, so a later silence is the
    // seed and nothing else.
    let (status, body) = send(
        &first,
        "entities",
        json!({"id": "urn:ngsi-ld:Vehicle:seed-1", "type": "Vehicle",
               "speed": {"type": "Property", "value": 1}}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    expect_notification(&mut rx, "the subscription never fired before the restart").await;

    // The restart: a fresh state over the same documents, whose first `list`
    // is refused the way the Postgres arm refuses one past its ceiling.
    let mut restarted = AppState::new("me".into());
    restarted.store = Arc::new(Double::flaky_list(first.store.clone(), 1));
    antares_api::wire(&mut restarted);

    let (status, body) = send(
        &restarted,
        "entities",
        json!({"id": "urn:ngsi-ld:Vehicle:seed-2", "type": "Vehicle",
               "speed": {"type": "Property", "value": 2}}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let n = expect_notification(
        &mut rx,
        "the tenant was silenced by a seed that could not read the store",
    )
    .await;
    assert_eq!(
        n["data"][0]["id"].as_str(),
        Some("urn:ngsi-ld:Vehicle:seed-2"),
        "{n}"
    );
}

/// The seed that CAN read the store still installs the mirror — the
/// fallback is for the failure, not a replacement for the index.
#[tokio::test(flavor = "multi_thread")]
async fn a_seed_that_reads_the_store_installs_the_mirror() {
    allow_private();

    let mut first = AppState::new("me".into());
    antares_api::wire(&mut first);

    let (uri, _rx) = capture_server().await;
    let (status, body) = send(
        &first,
        "subscriptions",
        json!({"type": "Subscription", "entities": [{"type": "Vehicle"}],
               "notification": {"endpoint": {"uri": uri}}}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let mut restarted = AppState::new("me".into());
    restarted.store = Arc::new(Double::flaky_list(first.store.clone(), 0));
    antares_api::wire(&mut restarted);

    assert!(
        restarted.sub_mirror.is_some(),
        "a complete seed must install the mirror it filled"
    );
}

/// 5.5.6 licenses TooManyResults for "a **query operation** … producing so
/// many results that can potentially exhaust client or server resources".
/// The mirror seed is not a query operation, and it must see every
/// subscription of every tenant: `subscription_tenants` states the rule —
/// "A SUBSET is a silent outage: a tenant missing here never fires a
/// periodic notification and never reaches the mirror."
///
/// Borrowing the client-query row ceiling for it turned one tenant's stored
/// volume into every OTHER tenant's outage. The document list refuses at
/// `MAX_UNDECIDED_ROWS` (10 000, a tenth of the 100 000-per-broker target),
/// the seed aborted its whole loop on that error, and the mirror was left
/// uninstalled process-wide — so on the next restart, every tenant fell back
/// to a full store scan per change, and the tenant over the ceiling fired
/// nothing at all.
#[tokio::test(flavor = "multi_thread")]
async fn a_tenant_over_the_list_ceiling_does_not_take_the_mirror_down_for_everyone() {
    allow_private();

    let mut first = AppState::new("me".into());
    antares_api::wire(&mut first);

    let (uri, mut rx) = capture_server().await;
    let (status, body) = send(
        &first,
        "subscriptions",
        json!({"type": "Subscription", "entities": [{"type": "Vehicle"}],
               "notification": {"endpoint": {"uri": uri}}}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    // A store that refuses EVERY `list`, the way the Postgres arm refuses one
    // past its ceiling, while the paged read it leaves uncapped still works.
    let mut restarted = AppState::new("me".into());
    restarted.store = Arc::new(Double::flaky_list(first.store.clone(), usize::MAX));
    antares_api::wire(&mut restarted);

    assert!(
        restarted.sub_mirror.is_some(),
        "the seed must read what it cannot be refused: a client-query ceiling \
         is not a reason to leave every tenant unmatched"
    );

    let (status, body) = send(
        &restarted,
        "entities",
        json!({"id": "urn:ngsi-ld:Vehicle:ceiling-1", "type": "Vehicle",
               "speed": {"type": "Property", "value": 3}}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let n = expect_notification(
        &mut rx,
        "a tenant past the document ceiling never fired again",
    )
    .await;
    assert_eq!(
        n["data"][0]["id"].as_str(),
        Some("urn:ngsi-ld:Vehicle:ceiling-1"),
        "{n}"
    );
}

/// The same walk fills the registration mirror, on the same terms.
///
/// `bus=nats` wires two mirrors and re-fills one of them after a consumer
/// gap. Those three sites carried a second copy of this walk that read
/// through the ceiling `list` carries for client queries AND turned every
/// error into an empty list, so a tenant past the ceiling hydrated as zero
/// documents with nothing logged. The nats path cannot degrade the way the
/// local one does: `subs_for` and `reg_docs` fall back to the store only
/// when the mirror is ABSENT, so a mirror installed and SHORT is served as
/// the truth — that tenant's subscriptions never fire and its registrations
/// never forward, for the life of the process.
///
/// One function now fills every mirror in both bus modes, which is what
/// keeps the rule from being fixed in one copy and left broken in the rest.
/// This pins the half the local-bus test cannot reach: a different `Kind`
/// and a different `Mirror` implementation.
#[tokio::test(flavor = "multi_thread")]
async fn the_same_seed_fills_a_registration_mirror_past_a_refused_list() {
    let st = AppState::new("me".into());
    let tenant = TenantId::new("me").expect("tenant");
    let reg = json!({
        "id": "urn:ngsi-ld:ContextSourceRegistration:seeded",
        "type": "ContextSourceRegistration",
        "endpoint": "http://cs.invalid/ngsi-ld/v1",
        "information": [{"entities": [{"type": "Vehicle"}]}],
    });
    st.store
        .create(
            &tenant,
            Kind::Registration,
            "urn:ngsi-ld:ContextSourceRegistration:seeded",
            reg.clone(),
        )
        .expect("seed a registration");

    // Refuses every `list`, the way the Postgres arm refuses one past its
    // ceiling. The paged read it leaves uncapped still answers.
    let store = Double::flaky_list(st.store.clone(), usize::MAX);
    assert!(
        store.list(&tenant, Kind::Registration).is_err(),
        "the double must actually refuse the read this is about"
    );

    let mirror = antares_api::mirror::DocMirror::default();
    antares_api::notify::seed_mirror(&store, &mirror, Kind::Registration)
        .expect("a refused `list` is not a reason to serve an empty registration mirror");

    let docs = mirror.docs("me");
    assert_eq!(
        docs.len(),
        1,
        "the registration never reached the mirror, so this tenant forwards to no Context Source: {docs:?}"
    );
    assert_eq!(
        docs[0]["id"], reg["id"],
        "the mirror holds something other than the stored registration: {docs:?}"
    );

    // A mirror for a tenant that holds nothing is empty, not an error: the
    // domain may be a superset of the tenants holding this kind.
    assert!(
        antares_api::mirror::DocMirror::default()
            .docs("nobody")
            .is_empty(),
        "an unseeded tenant must read empty rather than borrow another tenant's rows"
    );
}
