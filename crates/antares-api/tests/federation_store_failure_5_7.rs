// SPDX-License-Identifier: EUPL-1.2
//! A distributed operation whose registrations cannot be read.
//!
//! 5.7 executes an operation across the Context Sources whose registrations
//! match it; which ones those are is decided by reading the registration
//! store. 6.3.17 gives the broker a vocabulary for a source that misbehaves —
//! `NGSILD-Warning` 199/299, `207 Multi-Status` for unsafe methods — and none
//! of it covers the broker failing to read its OWN registrations: nothing was
//! forwarded, so nothing can be reported per registration. That is the
//! InternalError of Table 6.3.2-1, "there has been an error during the
//! operation execution": the broker cannot tell whether the answer it holds
//! is the whole answer, so it must not present it as one.
#![allow(clippy::unwrap_used)] // an unwrap here is the assertion

mod common;

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::Double;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;

async fn get(st: &AppState, uri: &str) -> (StatusCode, Value) {
    let resp = antares_api::router(st.clone())
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(uri)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    let status = resp.status();
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn post(st: &AppState, path: &str, body: Value) -> StatusCode {
    let payload = body.to_string();
    antares_api::router(st.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/ngsi-ld/v1/{path}"))
                .header("Content-Type", "application/json")
                .header("Content-Length", payload.len())
                .body(Body::from(payload))
                .expect("request"),
        )
        .await
        .expect("response")
        .status()
}

/// One local Entity and one registration covering the same Entity Type, both
/// created through the API, then a store whose registration read is refused.
async fn state_with_unreadable_registrations() -> AppState {
    let mut st = AppState::new("me".into());
    assert_eq!(
        post(
            &st,
            "entities",
            json!({"id": "urn:ngsi-ld:Vehicle:local", "type": "Vehicle",
                   "speed": {"type": "Property", "value": 10}}),
        )
        .await,
        StatusCode::CREATED
    );
    assert_eq!(
        post(
            &st,
            "csourceRegistrations",
            json!({"type": "ContextSourceRegistration",
                   "endpoint": "http://127.0.0.1:9",
                   "information": [{"entities": [{"type": "Vehicle"}]}]}),
        )
        .await,
        StatusCode::CREATED
    );
    st.store = Arc::new(Double::refusing_registrations(st.store.clone()));
    st
}

/// A query the broker would have distributed answers 500, not a local-only
/// 200: the client cannot distinguish "this Entity Type has one instance"
/// from "the Context Source holding the other nine was never asked".
#[tokio::test]
async fn a_query_whose_registrations_cannot_be_read_is_not_answered_locally() {
    let st = state_with_unreadable_registrations().await;
    let (status, body) = get(&st, "/ngsi-ld/v1/entities?type=Vehicle").await;
    assert_eq!(
        status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "a partial answer must not be served as a complete one: {body}"
    );
    assert_eq!(
        body["type"],
        json!("https://uri.etsi.org/ngsi-ld/errors/InternalError"),
        "{body}"
    );
}

/// The same for a retrieval by id: a `redirect` or `exclusive` registration
/// holds the Attributes this broker does not, so the local document is a
/// fragment of the Entity, never the Entity.
#[tokio::test]
async fn a_retrieval_whose_registrations_cannot_be_read_is_not_answered_locally() {
    let st = state_with_unreadable_registrations().await;
    let (status, body) = get(&st, "/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:local").await;
    assert_eq!(
        status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "a fragment must not be served as the Entity: {body}"
    );
}
