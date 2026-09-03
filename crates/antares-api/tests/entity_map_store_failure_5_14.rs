// SPDX-License-Identifier: EUPL-1.2
//! EntityMaps whose storage refuses to answer.
//!
//! 5.5.14 draws the line this file tests. For a request that *uses* a map it
//! is explicit: "If an EntityMap has expired, or cannot be accessed, no
//! inference can be made as to which entities are held within the Context
//! Sources and a new one shall be created" — unreadable is one of the ways a
//! map cannot be accessed, so a consumption request recovers by creating a
//! new map and must not fail.
//!
//! Nothing extends that recovery to the EntityMaps resource itself (5.14) or
//! to storing a map. There the store's refusal is Table 6.3.2-1's
//! InternalError, "there has been an error during the operation execution":
//! answering 404 claims the map is not known when the broker does not know
//! that, and answering 201 hands back the id of a map that was never stored,
//! which 5.5.14's "An EntityMap fixes the Entities to be considered for
//! subsequent requests" then cannot fix anything with.
#![allow(clippy::unwrap_used)] // an unwrap here is the assertion

mod common;

use antares_api::AppState;
use antares_model::TenantId;
use antares_store::Kind;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::Double;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;

const MAP_ID: &str = "urn:ngsi-ld:entitymap:seeded";

async fn send(st: &AppState, method: &str, uri: &str) -> (StatusCode, Vec<u8>, Option<String>) {
    let resp = antares_api::router(st.clone())
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    let status = resp.status();
    let map = resp
        .headers()
        .get("NGSILD-EntityMap")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    (status, bytes.to_vec(), map)
}

async fn create_entity(st: &AppState, id: &str) {
    let payload = json!({"id": id, "type": "Vehicle",
                         "speed": {"type": "Property", "value": 10}})
    .to_string();
    let status = antares_api::router(st.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/ngsi-ld/v1/entities")
                .header("Content-Type", "application/json")
                .header("Content-Length", payload.len())
                .body(Body::from(payload))
                .expect("request"),
        )
        .await
        .expect("response")
        .status();
    assert_eq!(status, StatusCode::CREATED);
}

/// One Entity and one live EntityMap naming it, both in the store, before the
/// double is put in front of that map's row.
async fn state_with_a_seeded_map() -> AppState {
    let st = AppState::new("me".into());
    create_entity(&st, "urn:ngsi-ld:Vehicle:one").await;
    let expires = (chrono::Utc::now() + chrono::Duration::hours(1))
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    st.store
        .create(
            &TenantId::default(),
            Kind::EntityMap,
            MAP_ID,
            json!({"id": MAP_ID, "type": "EntityMap", "expiresAt": expires,
                   "entityMap": {"urn:ngsi-ld:Vehicle:one": ["@none"]},
                   "linkedMaps": {}}),
        )
        .await
        .expect("seed the map");
    st
}

/// 5.14.1.4 answers 404 for an id "not known to the system". A read the store
/// refused says nothing about whether the id is known, so it is not that 404:
/// a client that takes it at face value drops a map that is still there and
/// still fixing its candidate set.
#[tokio::test]
async fn an_unreadable_map_is_not_reported_as_missing() {
    let mut st = state_with_a_seeded_map().await;
    st.store = Arc::new(Double::refusing_doc(
        st.store.clone(),
        Kind::EntityMap,
        MAP_ID,
    ));
    let (status, body, _) = send(&st, "GET", &format!("/ngsi-ld/v1/entityMaps/{MAP_ID}")).await;
    let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    assert_eq!(
        status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "a refused read is not an absent map: {body}"
    );
    assert_eq!(
        body["type"],
        json!("https://uri.etsi.org/ngsi-ld/errors/InternalError"),
        "{body}"
    );
}

/// 5.14.3.4 Delete EntityMap the same way round: 404 tells the client the map
/// is gone and to stop asking, while the row the delete could not touch keeps
/// its place against the per-tenant ceiling.
#[tokio::test]
async fn an_unreachable_map_is_not_reported_as_deleted() {
    let mut st = state_with_a_seeded_map().await;
    st.store = Arc::new(Double::refusing_doc(
        st.store.clone(),
        Kind::EntityMap,
        MAP_ID,
    ));
    let (status, body, _) = send(&st, "DELETE", &format!("/ngsi-ld/v1/entityMaps/{MAP_ID}")).await;
    let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    assert_eq!(
        status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "a refused delete is not a deleted map: {body}"
    );
}

/// 5.14.2.4 Update EntityMap: the partial update reads the map before it
/// writes the new expiry, and a refused read is not "no such map" either.
#[tokio::test]
async fn an_unreadable_map_is_not_missing_for_an_update() {
    let mut st = state_with_a_seeded_map().await;
    st.store = Arc::new(Double::refusing_doc(
        st.store.clone(),
        Kind::EntityMap,
        MAP_ID,
    ));
    let payload = json!({"expiresAt": "2099-01-01T00:00:00.000Z"}).to_string();
    let resp = antares_api::router(st.clone())
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/ngsi-ld/v1/entityMaps/{MAP_ID}"))
                .header("Content-Type", "application/json")
                .header("Content-Length", payload.len())
                .body(Body::from(payload))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

/// The bound: a per-tenant ceiling on stored maps that is only enforced if
/// the broker can count what it already holds. When that count is refused,
/// the map must not be created anyway — an answer of 201 there is a client
/// holding an id for a map outside every ceiling the broker believes in.
#[tokio::test]
async fn a_map_the_broker_cannot_bound_is_not_created() {
    let mut st = AppState::new("me".into());
    create_entity(&st, "urn:ngsi-ld:Vehicle:one").await;
    st.store = Arc::new(Double::flaky_list(st.store.clone(), usize::MAX));
    let (status, body, _) =
        send(&st, "GET", "/ngsi-ld/v1/entityMaps?type=Vehicle&local=true").await;
    let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    assert_ne!(
        status,
        StatusCode::CREATED,
        "a map that could not be bounded was answered as created: {body}"
    );
    // Which refusal it is belongs to the store — the ceiling this double
    // stands for is a TooManyResults, a lost connection an InternalError.
    // What this pins is that the failure reaches the client at all.
    assert!(
        status.is_client_error() || status.is_server_error(),
        "{body}"
    );
    assert!(
        body["type"]
            .as_str()
            .is_some_and(|t| t.starts_with("https://uri.etsi.org/ngsi-ld/errors/")),
        "the refusal is reported as an NGSI-LD ProblemDetails: {body}"
    );
}

/// The other side of the same line, and the reason the refusal is not simply
/// propagated everywhere: 5.5.14 says a map that "cannot be accessed" is
/// replaced, not raised. A query naming an unreadable map answers from a NEW
/// map — same status as any query, a different map id in the header.
#[tokio::test]
async fn a_query_naming_an_unreadable_map_gets_a_new_one() {
    let mut st = state_with_a_seeded_map().await;
    st.store = Arc::new(Double::refusing_doc(
        st.store.clone(),
        Kind::EntityMap,
        MAP_ID,
    ));
    let resp = antares_api::router(st.clone())
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/ngsi-ld/v1/entities?type=Vehicle")
                .header("NGSILD-EntityMap", MAP_ID)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    let status = resp.status();
    let map = resp
        .headers()
        .get("NGSILD-EntityMap")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    assert_eq!(
        status,
        StatusCode::CREATED,
        "6.4.3.2: the query that creates the replacement answers 201: {body}"
    );
    let map = map.expect("a replacement EntityMap is created and reported");
    assert!(
        !map.ends_with(MAP_ID),
        "the unreadable map was reported as the one that answered: {map}"
    );
    assert_eq!(
        body.as_array().map(Vec::len),
        Some(1),
        "the query still answers from the Entities themselves: {body}"
    );
}
