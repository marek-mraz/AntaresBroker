// SPDX-License-Identifier: EUPL-1.2
//! What a policy decision does to a notification (ADR-0020).
//!
//! 5.8.6 makes delivery broker-initiated: the notification goes from the
//! broker to the subscriber's endpoint, and no request is in flight to read
//! a subject off. So the subject is the subscriber's, taken from the
//! creating request and kept with the subscription — a broker-internal
//! member beside the `@context` the same clause already keeps there.
//!
//! 5.11.7 spells out the bookkeeping 5.8.6 shares: a notification that
//! "shall be sent" increments `notification.timesSent` and stamps
//! `lastNotification`. A notification the engine drops is not sent, so
//! neither moves — the same reading the broker already applies to a
//! cooldown (5.2.15) and an open circuit, and Table 5.2.14.2-1 defines
//! `timesSent` as the "number of times that the notification has been
//! sent".
//!
//! 5.8.3 and 5.8.4 serve the 5.2.12 Subscription data type, which has no
//! member for any of this, and 5.8.1.4 forwards a reduced copy to a Context
//! Source. Neither may carry the subject: that is the "assert what must NOT
//! be there" half of every test below.
#![cfg(feature = "test-kit")]
#![allow(clippy::unwrap_used)] // an unwrap here is the assertion

use antares_api::policy::{
    Decision, DecisionFuture, Filter, NotifyDecision, Operation, PolicyEngine, Subject,
};
use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

/// The header this file's deployment carries into the subject. Every test
/// sets the same value, so the once-read static is the same whichever test
/// touches it first.
fn subject_header() -> &'static str {
    std::env::set_var("ANTARES_POLICY_SUBJECT_HEADERS", "X-Subject");
    "X-Subject"
}

/// The subject headers of each notification the engine was asked about.
type Asked = Arc<Mutex<Vec<Vec<(String, String)>>>>;

/// An engine that answers every notification the same way and records the
/// subject headers it was asked about. Writes are allowed, so a test seeds
/// through the same router the notification comes out of.
struct Notifier {
    answer: NotifyDecision,
    seen: Asked,
}

impl Notifier {
    fn new(answer: NotifyDecision) -> (Arc<Self>, Asked) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        (
            Arc::new(Self {
                answer,
                seen: Arc::clone(&seen),
            }),
            seen,
        )
    }
}

impl PolicyEngine for Notifier {
    fn name(&self) -> &str {
        "notifier"
    }

    fn decide<'a>(&'a self, _s: &'a Subject, _op: &'a Operation<'a>) -> DecisionFuture<'a> {
        Box::pin(std::future::ready(Decision::Allow))
    }

    fn pre_notify(&self, s: &Subject, _sub: &Value, _n: &mut Value) -> NotifyDecision {
        if let Ok(mut v) = self.seen.lock() {
            v.push(s.headers.clone());
        }
        self.answer.clone()
    }
}

fn state(answer: NotifyDecision) -> (AppState, Asked) {
    // before the first request: the header list is read once per process
    subject_header();
    antares_jsonld::allow_private_egress(true);
    let (engine, seen) = Notifier::new(answer);
    let mut st = AppState::new("me".into()).with_policy(engine);
    antares_api::wire(&mut st);
    (st, seen)
}

/// A capture server yielding the BODY of each notification POST.
async fn capture() -> (String, tokio::sync::mpsc::Receiver<Value>) {
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
                let _ = tx
                    .send(serde_json::from_slice(&body).unwrap_or(Value::Null))
                    .await;
                StatusCode::OK
            }
        }),
    );
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    (format!("http://{addr}/notify"), rx)
}

async fn call(
    st: &AppState,
    method: &str,
    path: &str,
    subject: Option<&str>,
    doc: Option<Value>,
) -> (StatusCode, Value) {
    let mut b = Request::builder()
        .method(method)
        .uri(format!("/ngsi-ld/v1/{path}"));
    if let Some(s) = subject {
        b = b.header(subject_header(), s);
    }
    let req = match doc {
        Some(v) => {
            let payload = v.to_string();
            b = b
                .header("Content-Type", "application/json")
                .header("Content-Length", payload.len());
            b.body(Body::from(payload)).expect("req")
        }
        None => b.body(Body::empty()).expect("req"),
    };
    let resp = antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("resp");
    let status = resp.status();
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    let doc = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, doc)
}

/// One subscription over Vehicle, notifying `uri`, created by `who`.
async fn subscribe(st: &AppState, uri: &str, who: Option<&str>) -> String {
    let (code, body) = call(
        st,
        "POST",
        "subscriptions",
        who,
        Some(
            json!({"type": "Subscription", "entities": [{"type": "Vehicle"}],
                    "notification": {"endpoint": {"uri": uri}}}),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{body}");
    "urn:ngsi-ld:Subscription:policy".to_owned()
}

/// A Vehicle whose creation fires the subscription.
async fn fire(st: &AppState, id: &str) {
    let (code, body) = call(
        st,
        "POST",
        "entities",
        None,
        Some(json!({"id": id, "type": "Vehicle",
                    "speed": {"type": "Property", "value": 10},
                    "colour": {"type": "Property", "value": "red"}})),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{body}");
}

/// The one subscription the tests created, as the store holds it.
async fn stored_sub(st: &AppState) -> Value {
    let (code, list) = call(st, "GET", "subscriptions?options=sysAttrs", None, None).await;
    assert_eq!(code, StatusCode::OK, "{list}");
    list.as_array()
        .and_then(|a| a.first())
        .cloned()
        .unwrap_or(Value::Null)
}

const WAIT: std::time::Duration = std::time::Duration::from_secs(10);

/// 5.11.7 increments `timesSent` for a notification that "shall be sent",
/// and Table 5.2.14.2-1 defines it as the number of times the notification
/// HAS BEEN sent. A dropped one never was: no POST, no stamp, no count —
/// and no `lastFailure` either, because it is not a failed delivery.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dropped_notification_is_no_attempt_at_all() {
    let (st, _) = state(NotifyDecision::Drop);
    let (uri, mut rx) = capture().await;
    subscribe(&st, &uri, Some("alice")).await;
    fire(&st, "urn:ngsi-ld:Vehicle:dropped").await;

    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(1500), rx.recv())
            .await
            .is_err(),
        "a dropped notification was delivered anyway"
    );
    let n = stored_sub(&st).await;
    let n = &n["notification"];
    assert!(
        n.get("timesSent").is_none(),
        "5.11.7 counts a notification that was sent: {n}"
    );
    assert!(
        n.get("lastNotification").is_none(),
        "lastNotification is the instant one was sent: {n}"
    );
    assert!(
        n.get("lastFailure").is_none(),
        "a drop is not a failed delivery: {n}"
    );
    assert_ne!(n.get("status").and_then(Value::as_str), Some("failed"));
}

/// A delivered notification still moves the bookkeeping, so the assertion
/// above is about the drop and not about a broker that stopped counting.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_delivered_notification_still_counts() {
    let (st, _) = state(NotifyDecision::Deliver);
    let (uri, mut rx) = capture().await;
    subscribe(&st, &uri, Some("alice")).await;
    fire(&st, "urn:ngsi-ld:Vehicle:sent").await;

    let body = tokio::time::timeout(WAIT, rx.recv())
        .await
        .expect("a notification arrived")
        .expect("body");
    assert_eq!(body["data"][0]["id"], "urn:ngsi-ld:Vehicle:sent");
    let n = stored_sub(&st).await;
    assert_eq!(n["notification"]["timesSent"], json!(1), "{n}");
}

/// The projection is applied to the entities of `data`, in place: the
/// member the engine named is gone and everything else the subscription
/// asked for is still there.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_filtered_notification_loses_only_what_the_engine_named() {
    let (st, _) = state(NotifyDecision::Filter(Filter {
        omit: vec!["colour".into()],
        ..Filter::default()
    }));
    let (uri, mut rx) = capture().await;
    subscribe(&st, &uri, Some("alice")).await;
    fire(&st, "urn:ngsi-ld:Vehicle:filtered").await;

    let body = tokio::time::timeout(WAIT, rx.recv())
        .await
        .expect("a notification arrived")
        .expect("body");
    let e = &body["data"][0];
    assert_eq!(e["id"], "urn:ngsi-ld:Vehicle:filtered");
    assert!(e.get("speed").is_some(), "more was removed than named: {e}");
    assert!(e.get("colour").is_none(), "omitted member delivered: {e}");
}

/// ADR-0020 asks an engine to write its rules against IRIs, and by the time
/// the seam sees a notification the document is compacted. A rule written
/// the way the ADR asks has to remove the member it names rather than
/// silently match nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_projection_named_as_an_iri_removes_the_member_it_names() {
    let (st, _) = state(NotifyDecision::Filter(Filter {
        omit: vec!["https://uri.etsi.org/ngsi-ld/default-context/colour".into()],
        ..Filter::default()
    }));
    let (uri, mut rx) = capture().await;
    subscribe(&st, &uri, Some("alice")).await;
    fire(&st, "urn:ngsi-ld:Vehicle:iri").await;

    let body = tokio::time::timeout(WAIT, rx.recv())
        .await
        .expect("a notification arrived")
        .expect("body");
    let e = &body["data"][0];
    assert!(e.get("speed").is_some(), "{e}");
    assert!(
        e.get("colour").is_none(),
        "an IRI-named omit matched nothing: {e}"
    );
}

/// `pick` reduces the entity to what it names, and 5.2.4 keeps `id` and
/// `type` because a document without them is not an Entity.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pick_leaves_the_entity_frame_and_nothing_it_did_not_name() {
    let (st, _) = state(NotifyDecision::Filter(Filter {
        pick: vec!["speed".into()],
        ..Filter::default()
    }));
    let (uri, mut rx) = capture().await;
    subscribe(&st, &uri, Some("alice")).await;
    fire(&st, "urn:ngsi-ld:Vehicle:picked").await;

    let body = tokio::time::timeout(WAIT, rx.recv())
        .await
        .expect("a notification arrived")
        .expect("body");
    let e = &body["data"][0];
    assert!(e.get("id").is_some() && e.get("type").is_some(), "{e}");
    assert!(e.get("speed").is_some(), "{e}");
    assert!(e.get("colour").is_none(), "{e}");
}

/// A narrowing the notification path cannot apply is not one it may claim
/// to have applied: the entities were chosen by the subscription's own
/// conditions long before the seam sees them, so a `q` on this answer is
/// refused the way a panicking engine is — the notification is not sent.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_notification_filter_that_carries_a_query_is_dropped() {
    let (st, _) = state(NotifyDecision::Filter(Filter {
        q: Some(antares_ql::parse_q("speed<50").expect("q")),
        ..Filter::default()
    }));
    let (uri, mut rx) = capture().await;
    subscribe(&st, &uri, Some("alice")).await;
    // the entity MATCHES the condition, so a broker that silently ignored
    // the q would deliver and the test would pass for the wrong reason
    fire(&st, "urn:ngsi-ld:Vehicle:qfilter").await;

    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(1500), rx.recv())
            .await
            .is_err(),
        "a narrowing the broker cannot apply was reported as applied"
    );
    let n = stored_sub(&st).await;
    assert!(
        n["notification"].get("timesSent").is_none(),
        "a refused notification was counted as sent: {n}"
    );
}

/// The engine is asked about the SUBSCRIBER, not about whoever's write
/// happened to trigger the notification: 5.8.6 delivery is broker-initiated
/// and the creating request is the only place a subject can come from.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_notification_subject_is_the_subscribers() {
    let (st, seen) = state(NotifyDecision::Deliver);
    let (uri, mut rx) = capture().await;
    subscribe(&st, &uri, Some("alice")).await;
    // the write is nobody's in particular; the notification is still alice's
    fire(&st, "urn:ngsi-ld:Vehicle:subject").await;
    tokio::time::timeout(WAIT, rx.recv())
        .await
        .expect("a notification arrived");

    let asked = seen.lock().expect("lock").clone();
    assert_eq!(asked.len(), 1, "one notification, one decision: {asked:?}");
    assert_eq!(
        asked[0],
        vec![("x-subject".to_owned(), "alice".to_owned())],
        "the engine was asked about the wrong subject"
    );
}

/// The stored subject is the broker's own record: 5.2.12 defines no member
/// for it, 5.8.3/5.8.4 serve that data type, and a client can neither seed
/// it at creation nor replace it by patch.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_stored_subject_is_in_no_representation_and_no_client_can_set_it() {
    let (st, seen) = state(NotifyDecision::Deliver);
    let (uri, mut rx) = capture().await;
    let member = "__subject";

    // a create that tries to seed the member, from a subject of its own
    let (code, body) = call(
        &st,
        "POST",
        "subscriptions",
        Some("alice"),
        Some(
            json!({"id": "urn:ngsi-ld:Subscription:policy", "type": "Subscription",
                    "entities": [{"type": "Vehicle"}],
                    "__subject": [["x-subject", "mallory"]],
                    "notification": {"endpoint": {"uri": uri}}}),
        ),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{body}");

    // a patch that tries to replace it
    let (code, body) = call(
        &st,
        "PATCH",
        "subscriptions/urn:ngsi-ld:Subscription:policy",
        Some("mallory"),
        Some(json!({"__subject": [["x-subject", "mallory"]]})),
    )
    .await;
    assert_eq!(code, StatusCode::NO_CONTENT, "{body}");

    for path in [
        "subscriptions",
        "subscriptions?options=sysAttrs",
        "subscriptions/urn:ngsi-ld:Subscription:policy",
        "subscriptions/urn:ngsi-ld:Subscription:policy?options=sysAttrs",
    ] {
        let (code, doc) = call(&st, "GET", path, None, None).await;
        assert_eq!(code, StatusCode::OK, "{path}: {doc}");
        let served = doc.to_string();
        assert!(
            !served.contains(member),
            "{path} served the internal subject member: {served}"
        );
        assert!(
            !served.contains("alice") && !served.contains("mallory"),
            "{path} served the subject's value: {served}"
        );
    }

    // and the one the engine is asked about is still the creator's
    fire(&st, "urn:ngsi-ld:Vehicle:proof").await;
    tokio::time::timeout(WAIT, rx.recv())
        .await
        .expect("a notification arrived");
    assert_eq!(
        seen.lock()
            .expect("lock")
            .first()
            .cloned()
            .unwrap_or_default(),
        vec![("x-subject".to_owned(), "alice".to_owned())],
        "a client body rewrote the stored subject"
    );
}
