// SPDX-License-Identifier: EUPL-1.2
//! What the broker answers when its storage has no connection to give.
//!
//! This is an Antares decision, not a CIM 009 requirement. Table 6.3.2-1
//! maps the eleven API error types of clause 5.5.2 to HTTP status codes and
//! has no entry for an overloaded server. Clause 6.3.2 then continues: "In
//! addition, implementations shall support the standard specific errors of
//! HTTP bindings, such as the following", and lists 405, 413, 411, 415 and
//! 406 — an open list of conditions that belong to the binding rather than
//! to the information model. A connection pool that ran out of time is one
//! of those: the operation was never attempted, nothing about the request
//! was wrong, and the same request succeeds once the queue drains.
//!
//! So the answer is 503 with `Retry-After` (IETF RFC 7231 clause 6.6.4 and
//! clause 7.1.3) and no payload body, which is how clause 6.3.4 already has
//! the broker answer the binding's own conditions (411 and 415 are "just a
//! status code (without any payload body)"). No `https://uri.etsi.org/`
//! error type is claimed for a condition the specification does not name.
#![allow(clippy::unwrap_used)] // an unwrap here is the assertion

mod common;

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::Double;
use http_body_util::BodyExt;
use serde_json::json;
use std::sync::Arc;
use tower::ServiceExt;

/// A state whose reads all hit the pool wall, over the real store.
fn overloaded_state() -> AppState {
    let mut st = AppState::new("me".into());
    st.store = Arc::new(Double::overloaded(st.store.clone()));
    st
}

/// 6.3.4 makes `Content-Length` a precondition of a write, so the helper
/// sends the body as bytes and states its length — a request without it is
/// answered 411 before it ever reaches the store.
async fn send(
    st: &AppState,
    method: &str,
    uri: &str,
    body: &str,
) -> (StatusCode, Vec<u8>, Option<String>) {
    let resp = antares_api::router(st.clone())
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("Content-Type", "application/json")
                .header("Content-Length", body.len())
                .body(Body::from(body.to_owned()))
                .expect("request"),
        )
        .await
        .expect("response");
    let status = resp.status();
    let retry = resp
        .headers()
        .get(axum::http::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    (status, bytes.to_vec(), retry)
}

/// The whole point: a client can tell "come back" apart from "your request
/// was wrong" and from "the broker is broken", and is told when to come back.
#[tokio::test]
async fn an_exhausted_pool_answers_503_with_retry_after() {
    let st = overloaded_state();
    let (code, body, retry) = send(&st, "GET", "/ngsi-ld/v1/entities?type=Vehicle", "").await;
    assert_eq!(
        code,
        StatusCode::SERVICE_UNAVAILABLE,
        "overload is not a 500"
    );
    let secs: u64 = retry
        .expect("503 without Retry-After leaves the client guessing")
        .parse()
        .expect("Retry-After is delta-seconds");
    assert!(
        secs > 5,
        "a retry inside the acquire timeout ({secs} s) walks into the same wall"
    );
    assert!(body.is_empty(), "the binding's own errors carry no body");
}

/// Retrieve is the other read shape, and it must not answer 404: the broker
/// never learned whether the entity exists.
#[tokio::test]
async fn a_retrieve_under_overload_is_not_a_not_found() {
    let st = overloaded_state();
    let (code, _, retry) = send(&st, "GET", "/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:1", "").await;
    assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE);
    assert!(retry.is_some());
}

/// A store failure that is NOT the pool wall keeps its Table 6.3.2-1 answer.
/// The 503 is reached by one named condition, not by every internal error.
#[tokio::test]
async fn an_ordinary_store_failure_is_still_internal_error_500() {
    let mut st = AppState::new("me".into());
    st.store = Arc::new(Double::refusing_doc(
        st.store.clone(),
        antares_store::Kind::Entity,
        "urn:ngsi-ld:Vehicle:1",
    ));
    let (code, body, retry) =
        send(&st, "GET", "/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:1", "").await;
    assert_eq!(code, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(retry.is_none(), "a fault is not a retry invitation");
    let pd: serde_json::Value = serde_json::from_slice(&body).expect("problem details");
    assert_eq!(
        pd["type"], "https://uri.etsi.org/ngsi-ld/errors/InternalError",
        "Table 6.3.2-1 still owns the errors it names"
    );
}

/// A batch that hits the wall before it writes anything answers 503 for the
/// whole array, not a 207 of identical entries. Nothing was created, so the
/// client can retry the batch whole; `ngsi_of` keeps the per-item collapse
/// for a wall reached after the array started landing.
#[tokio::test]
async fn a_batch_that_wrote_nothing_answers_503_for_the_whole_array() {
    let st = overloaded_state();
    let payload = json!([{"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle"}]).to_string();
    let (code, body, retry) =
        send(&st, "POST", "/ngsi-ld/v1/entityOperations/create", &payload).await;
    assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE);
    assert!(retry.is_some(), "the client is told when to come back");
    assert!(
        body.is_empty(),
        "a 503 that listed successes would invite a duplicating retry: {}",
        String::from_utf8_lossy(&body)
    );
}
