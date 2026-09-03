// SPDX-License-Identifier: EUPL-1.2
//! 4.9: an Attribute named in both `expandValues` and `jsonKeys`. The clause
//! states no precedence between the two lists, so the broker settles it: the
//! `jsonKeys` entry describes the value itself ("uninterpretable as JSON-LD"),
//! the `expandValues` entry only asks for a comparison, and coercing a value
//! the client has declared unreadable builds a term the stored value can
//! never carry. `jsonKeys` therefore subtracts from `expandValues` on the
//! entity query (6.4.3.2) and on the temporal query (5.7.4.3) alike.

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

async fn get_ids(st: &AppState, uri: &str) -> Vec<String> {
    let (status, body) = send(
        st,
        Request::builder()
            .method("GET")
            .uri(uri)
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body.as_array()
        .expect("array")
        .iter()
        .filter_map(|d| d["id"].as_str().map(str::to_owned))
        .collect()
}

async fn seed(st: &AppState, id: &str) {
    let body = json!({"id": id, "type": "Shop",
        "category": {"type": "VocabProperty", "vocab": "commercial"}})
    .to_string();
    let (status, b) = send(
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

/// 6.4.3.2 on GET /entities: `expandValues=category` makes the literal match
/// the VocabProperty's expanded URI (4.9 EXAMPLE 12); naming the same
/// Attribute in `jsonKeys` takes it back out of the expansion, and naming a
/// different one leaves the expansion alone.
#[tokio::test(flavor = "multi_thread")]
async fn json_keys_subtract_from_expand_values_on_the_entity_query() {
    let st = AppState::new("me".into());
    seed(&st, "urn:ngsi-ld:Shop:jk1").await;
    let q = "/ngsi-ld/v1/entities?type=Shop&q=category%3D%3Dcommercial";

    assert_eq!(
        get_ids(&st, &format!("{q}&expandValues=category")).await,
        vec!["urn:ngsi-ld:Shop:jk1".to_owned()],
        "expandValues alone coerces the term"
    );
    assert!(
        get_ids(&st, &format!("{q}&expandValues=category&jsonKeys=category"))
            .await
            .is_empty(),
        "an Attribute in both lists is not expanded"
    );
    assert_eq!(
        get_ids(&st, &format!("{q}&expandValues=category&jsonKeys=other")).await,
        vec!["urn:ngsi-ld:Shop:jk1".to_owned()],
        "jsonKeys only removes the Attributes it names"
    );
}

/// 5.7.4.3 carries the same pair on GET /temporal/entities, and settles the
/// overlap the same way.
#[tokio::test(flavor = "multi_thread")]
async fn json_keys_subtract_from_expand_values_on_the_temporal_query() {
    let mut st = AppState::new("me".into());
    antares_api::wire(&mut st).await; // temporal auto-recording
    seed(&st, "urn:ngsi-ld:Shop:jk2").await;
    let q = "/ngsi-ld/v1/temporal/entities?type=Shop&q=category%3D%3Dcommercial\
             &timerel=after&timeAt=2000-01-01T00:00:00Z&timeproperty=createdAt";

    assert_eq!(
        get_ids(&st, &format!("{q}&expandValues=category")).await,
        vec!["urn:ngsi-ld:Shop:jk2".to_owned()],
        "expandValues alone coerces the term"
    );
    assert!(
        get_ids(&st, &format!("{q}&expandValues=category&jsonKeys=category"))
            .await
            .is_empty(),
        "an Attribute in both lists is not expanded"
    );
}
