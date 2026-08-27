// SPDX-License-Identifier: EUPL-1.2
//! 5.7.4.3 Table: expandValues / jsonKeys on the temporal query — the same
//! 4.9 EXAMPLE 12 type coercion the entity query applies (attribute values
//! expanded against the @context before executing the query).

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

async fn send(st: &AppState, req: Request<Body>) -> (StatusCode, Value) {
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

async fn get(st: &AppState, uri: &str) -> (StatusCode, Value) {
    send(
        st,
        Request::builder()
            .method("GET")
            .uri(uri)
            .body(Body::empty())
            .expect("request"),
    )
    .await
}

/// 5.7.4.3: q=category==commercial only matches the VocabProperty's
/// expanded URI when expandValues names the attribute — accepted AND
/// applied on GET /temporal/entities; without it the entity must NOT match.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_7_4_3_expand_values_coerces_the_temporal_query() {
    let mut st = AppState::new("me".into());
    antares_api::notify::wire(&mut st); // temporal auto-recording

    let body = json!({"id": "urn:ngsi-ld:Shop:ev1", "type": "Shop",
        "category": {"type": "VocabProperty", "vocab": "commercial"}})
    .to_string();
    let (status, b) = send(
        &st,
        Request::builder()
            .method("POST")
            .uri("/ngsi-ld/v1/entities")
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{b}");

    let window = "timerel=after&timeAt=2000-01-01T00:00:00Z&timeproperty=createdAt";

    // with expandValues: accepted and the coerced term matches
    let (status, body) = get(
        &st,
        &format!("/ngsi-ld/v1/temporal/entities?type=Shop&q=category%3D%3Dcommercial&expandValues=category&{window}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let ids: Vec<&str> = body
        .as_array()
        .expect("array")
        .iter()
        .filter_map(|d| d["id"].as_str())
        .collect();
    assert_eq!(ids, vec!["urn:ngsi-ld:Shop:ev1"], "{body}");

    // without expandValues the literal does not match the expanded vocab
    let (status, body) = get(
        &st,
        &format!("/ngsi-ld/v1/temporal/entities?type=Shop&q=category%3D%3Dcommercial&{window}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body.as_array().map(Vec::len), Some(0), "{body}");
}
