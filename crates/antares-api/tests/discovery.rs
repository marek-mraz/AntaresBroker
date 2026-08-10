//! 4.5.10+ discovery representations (/types, /attributes).

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

async fn send(st: &AppState, req: Request<Body>) -> (StatusCode, String) {
    let resp = antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("response");
    let status = resp.status();
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

/// 4.5.10: the entity type list is a JSON-LD object with id (a URI), the
/// fixed type "EntityTypeList" and typeList — and nothing else beyond an
/// optional @context.
#[tokio::test(flavor = "multi_thread")]
async fn entity_type_list_shape() {
    let st = AppState::new("test".into());
    let create = r#"{"id":"urn:ngsi-ld:Disc:1","type":"Building",
        "v":{"type":"Property","value":1}}"#;
    let (status, body) = send(
        &st,
        Request::builder()
            .method("POST")
            .uri("/ngsi-ld/v1/entities")
            .header("Content-Type", "application/json")
            .header("Content-Length", create.len())
            .body(Body::from(create))
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let (status, body) = send(
        &st,
        Request::builder()
            .uri("/ngsi-ld/v1/types")
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let doc: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert!(
        doc["id"].as_str().is_some_and(|s| s.starts_with("urn:")),
        "id must be a URI: {body}"
    );
    assert_eq!(doc["type"], "EntityTypeList");
    let list = doc["typeList"].as_array().expect("typeList array");
    assert!(list.iter().any(|t| t == "Building"), "{body}");
    let extra: Vec<&String> = doc
        .as_object()
        .expect("object")
        .keys()
        .filter(|k| !["id", "type", "typeList", "@context"].contains(&k.as_str()))
        .collect();
    assert!(extra.is_empty(), "unexpected members: {extra:?}");
}
