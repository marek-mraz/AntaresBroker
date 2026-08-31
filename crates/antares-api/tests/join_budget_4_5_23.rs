// SPDX-License-Identifier: EUPL-1.2
//! 4.5.23.1: "When retrieving Linked Entities, it is necessary to limit
//! retrieval to avoid cascades of an excessive length, duplicates or loops."
//! `joinLevel` limits the depth of the cascade; the broker's own lookup
//! ceiling limits its width. A query answers a whole page, so the ceiling has
//! to be spent across the page — minting a fresh allowance for every Entity
//! in the answer multiplies it by the page size, and the request the ceiling
//! exists to bound is exactly a page of densely linked Entities.

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

/// Targets per root. Two roots at this width demand more lookups than one
/// request's ceiling (1000) allows, and fewer than one root's own would.
const WIDTH: usize = 600;

async fn send(st: &AppState, req: Request<Body>) -> (StatusCode, axum::http::HeaderMap, Value) {
    let res = antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("response");
    let status = res.status();
    let headers = res.headers().clone();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .expect("body");
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, headers, body)
}

async fn create(st: &AppState, doc: Value) {
    let body = doc.to_string();
    let (status, _, b) = send(
        st,
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
}

fn warned(headers: &axum::http::HeaderMap) -> bool {
    headers
        .get_all("NGSILD-Warning")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .any(|v| v.contains("linked entity retrieval was truncated"))
}

/// Two Linking Entities on one page, each naming `WIDTH` Linked Entities:
/// together they ask for more lookups than the request may buy, so the answer
/// is truncated and says so (6.3.17). With an allowance per Entity instead,
/// neither root reaches its own ceiling and the request quietly performs
/// twice the work the ceiling names.
#[tokio::test(flavor = "multi_thread")]
async fn the_lookup_ceiling_bounds_the_request_not_each_entity_on_the_page() {
    let st = AppState::new("antares-join-budget".into());
    for i in 0..WIDTH {
        create(
            &st,
            json!({"id": format!("urn:ngsi-ld:T:{i}"), "type": "T"}),
        )
        .await;
    }
    for root in ["a", "b"] {
        let mut doc = serde_json::Map::new();
        doc.insert("id".into(), json!(format!("urn:ngsi-ld:Root:{root}")));
        doc.insert("type".into(), json!("Root"));
        for i in 0..WIDTH {
            doc.insert(
                format!("r{i}"),
                json!({"type": "Relationship", "object": format!("urn:ngsi-ld:T:{i}")}),
            );
        }
        create(&st, Value::Object(doc)).await;
    }

    // one root alone stays inside the allowance: no truncation
    let (status, headers, body) = send(
        &st,
        Request::builder()
            .method("GET")
            .uri("/ngsi-ld/v1/entities/urn:ngsi-ld:Root:a?join=inline&joinLevel=1")
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        !warned(&headers),
        "one Entity fits the allowance: {headers:?}"
    );

    // the page of both does not
    let (status, headers, body) = send(
        &st,
        Request::builder()
            .method("GET")
            .uri("/ngsi-ld/v1/entities?type=Root&join=inline&joinLevel=1")
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body.as_array().map(Vec::len), Some(2), "{body}");
    assert!(
        warned(&headers),
        "a page that asks for more lookups than the request may buy must say the \
         retrieval was truncated: {headers:?}"
    );
}
