// SPDX-License-Identifier: EUPL-1.2
//! 5.9.2.4, on a Context Source Registration's `expiresAt`: "If expiresAt is
//! a date and time in the future, implementations shall delete the
//! Registration when this point in time is reached."
//!
//! Deleted, not hidden. A Subscription is the opposite case — 5.8.6 keeps an
//! expired one retrievable with `status: "expired"` and updatable — and a
//! Registration carries no such member, so once the instant passes the
//! registration is gone for EVERY operation that names its id: 5.9.3.4 and
//! 5.9.4.4 raise ResourceNotFound for one the endpoint "does not know
//! about", and 5.9.2.4 raises AlreadyExists only for one that exists, so the
//! id may be registered again.
//!
//! The sweep is lazy, which is allowed ("clean-up processes will only run
//! periodically … final deletion will always lag the expiresAt timestamp"),
//! but laziness may not change an answer.

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

const ID: &str = "urn:ngsi-ld:ContextSourceRegistration:expiring";

async fn send(st: &AppState, method: &str, path: &str, body: Option<Value>) -> (StatusCode, Value) {
    let mut b = Request::builder().method(method).uri(path);
    let req = match body {
        None => b.body(Body::empty()).expect("request"),
        Some(v) => {
            let s = v.to_string();
            b = b.header("Content-Type", "application/json");
            b.header("Content-Length", s.len())
                .body(Body::from(s))
                .expect("request")
        }
    };
    let res = antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("response");
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .expect("body");
    let doc = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, doc)
}

/// A registration that expires `ms` from now. The value has to be in the
/// future: 5.9.2.4 makes a past `expiresAt` a BadRequestData at creation, so
/// the only honest way to reach the expired state is to wait for it.
fn doc_expiring_in(ms: i64) -> Value {
    // The margin scales with the runner, like every wait below: creation
    // validates `expiresAt` against the clock at the moment it runs, so on a
    // loaded machine a fixed few hundred milliseconds can already be past by
    // then and 5.9.2.4 rejects the document the test needs.
    let at = chrono::Utc::now()
        + chrono::Duration::milliseconds(ms * antares_api::state::slow_factor() as i64);
    json!({
        "id": ID,
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": "Vehicle"}]}],
        "endpoint": "http://127.0.0.1:9",
        "expiresAt": at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
    })
}

async fn wait_past_expiry(ms: u64) {
    tokio::time::sleep(std::time::Duration::from_millis(
        ms * antares_api::state::slow_factor(),
    ))
    .await;
}

/// Create a registration that expires almost at once, and return once the
/// instant has passed. Every test below starts here, so each names ONE
/// operation and fails on its own.
async fn expired(st: &AppState) -> String {
    let path = format!("/ngsi-ld/v1/csourceRegistrations/{ID}");
    let (status, body) = send(
        st,
        "POST",
        "/ngsi-ld/v1/csourceRegistrations",
        Some(doc_expiring_in(400)),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (status, _) = send(st, "GET", &path, None).await;
    assert_eq!(status, StatusCode::OK, "live before the instant passes");
    wait_past_expiry(700).await;
    path
}

/// The read path's own answer, pinned: the three operations below must not
/// be reconciled by loosening this one instead.
#[tokio::test(flavor = "multi_thread")]
async fn retrieving_an_expired_registration_is_resource_not_found() {
    let st = AppState::new("antares-reg-expiry-get".into());
    let path = expired(&st).await;
    let (status, body) = send(&st, "GET", &path, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "5.9.3.4 retrieve: {body}");
}

/// 5.9.3.4: an update raises ResourceNotFound for a registration the
/// endpoint "does not know about", and after the instant it knows about
/// none — so a patch may not quietly resurrect and mutate one.
#[tokio::test(flavor = "multi_thread")]
async fn updating_an_expired_registration_is_resource_not_found() {
    let st = AppState::new("antares-reg-expiry-patch".into());
    let path = expired(&st).await;
    let (status, body) = send(
        &st,
        "PATCH",
        &path,
        Some(json!({"endpoint": "http://127.0.0.1:10"})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

/// 5.9.4.4: "If the NGSI-LD endpoint does not know about the target context
/// source registration … an error of type ResourceNotFound shall be
/// raised." Answering 204 reports a deletion the client never performed.
#[tokio::test(flavor = "multi_thread")]
async fn deleting_an_expired_registration_is_resource_not_found() {
    let st = AppState::new("antares-reg-expiry-delete".into());
    let path = expired(&st).await;
    let (status, body) = send(&st, "DELETE", &path, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

/// 5.9.2.4 raises AlreadyExists for a registration that EXISTS. One whose
/// instant has passed was deleted, so its id is free: refusing it strands
/// the identifier for the lifetime of the deployment.
#[tokio::test(flavor = "multi_thread")]
async fn the_id_of_an_expired_registration_can_be_registered_again() {
    let st = AppState::new("antares-reg-expiry-recreate".into());
    let path = expired(&st).await;
    let (status, body) = send(
        &st,
        "POST",
        "/ngsi-ld/v1/csourceRegistrations",
        Some(doc_expiring_in(60_000)),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    // and the fresh one behaves like any live registration
    let (status, _) = send(&st, "GET", &path, None).await;
    assert_eq!(status, StatusCode::OK, "the new registration is live");
    let (status, _) = send(&st, "DELETE", &path, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "and deletes normally");
}

/// The mirror of the rule: a registration with no `expiresAt` "shall last
/// forever (or until it is deleted from the system)" (5.9.2.4), so nothing
/// above may start treating a live registration as expired.
#[tokio::test(flavor = "multi_thread")]
async fn a_registration_without_expiry_is_never_treated_as_gone() {
    let st = AppState::new("antares-reg-forever".into());
    let id = "urn:ngsi-ld:ContextSourceRegistration:forever";
    let path = format!("/ngsi-ld/v1/csourceRegistrations/{id}");
    let (status, body) = send(
        &st,
        "POST",
        "/ngsi-ld/v1/csourceRegistrations",
        Some(json!({
            "id": id,
            "type": "ContextSourceRegistration",
            "information": [{"entities": [{"type": "Vehicle"}]}],
            "endpoint": "http://127.0.0.1:9",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    wait_past_expiry(700).await;

    let (status, _) = send(&st, "GET", &path, None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = send(
        &st,
        "PATCH",
        &path,
        Some(json!({"endpoint": "http://127.0.0.1:10"})),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (status, _) = send(&st, "DELETE", &path, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}
