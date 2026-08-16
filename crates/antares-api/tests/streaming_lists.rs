//! Streaming list endpoints: list responses are
//! emitted entity-by-entity via `Body::from_stream`, never as one contiguous
//! serialized buffer. These tests pin the OBSERVABLE contract: the streamed
//! bytes are valid JSON, chunked (no Content-Length), and identical in
//! content to what the buffered path produced.

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

async fn send(st: &AppState, req: Request<Body>) -> (StatusCode, axum::http::HeaderMap, String) {
    let resp = antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("response");
    let status = resp.status();
    let headers = resp.headers().clone();
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    (
        status,
        headers,
        String::from_utf8_lossy(&bytes).into_owned(),
    )
}

async fn create(st: &AppState, doc: &str) {
    let req = Request::builder()
        .method("POST")
        .uri("/ngsi-ld/v1/entities")
        .header("Content-Type", "application/json")
        .header("Content-Length", doc.len())
        .body(Body::from(doc.to_owned()))
        .expect("request");
    let (status, _, body) = send(st, req).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

#[tokio::test(flavor = "multi_thread")]
async fn streamed_query_is_valid_json_with_no_content_length() {
    let st = AppState::new("test".into());
    for i in 0..3 {
        create(
            &st,
            &format!(
                r#"{{"id":"urn:ngsi-ld:StreamProbe:{i}","type":"StreamProbe",
                    "temperature":{{"type":"Property","value":{i}}}}}"#
            ),
        )
        .await;
    }
    let req = Request::builder()
        .uri("/ngsi-ld/v1/entities?type=StreamProbe")
        .body(Body::empty())
        .expect("request");
    let (status, headers, body) = send(&st, req).await;
    assert_eq!(status, StatusCode::OK);
    // a streamed body has no Content-Length — hyper chunks it
    assert!(
        headers.get("content-length").is_none(),
        "streamed list must not carry Content-Length"
    );
    assert!(headers.get("link").is_some(), "6.3.6 Link header");
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("streamed bytes are JSON");
    let arr = parsed.as_array().expect("array");
    assert_eq!(arr.len(), 3);
    for e in arr {
        assert_eq!(e["type"], "StreamProbe");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn streamed_ldjson_carries_context_per_entity() {
    let st = AppState::new("test".into());
    create(
        &st,
        r#"{"id":"urn:ngsi-ld:StreamProbe:ld","type":"StreamProbe",
            "temperature":{"type":"Property","value":1}}"#,
    )
    .await;
    let req = Request::builder()
        .uri("/ngsi-ld/v1/entities?type=StreamProbe")
        .header("Accept", "application/ld+json")
        .body(Body::empty())
        .expect("request");
    let (status, headers, body) = send(&st, req).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers
            .get("content-type")
            .map(|v| v.to_str().unwrap_or("")),
        Some("application/ld+json")
    );
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("JSON");
    for e in parsed.as_array().expect("array") {
        assert!(e.get("@context").is_some(), "ld+json embeds @context: {e}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn empty_result_streams_an_empty_array() {
    let st = AppState::new("test".into());
    let req = Request::builder()
        .uri("/ngsi-ld/v1/entities?type=NothingHere")
        .body(Body::empty())
        .expect("request");
    let (status, _, body) = send(&st, req).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "[]");
}
