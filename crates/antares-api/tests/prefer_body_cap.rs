// SPDX-License-Identifier: EUPL-1.2
//! 6.3.6 `Prefer: ngsi-ld=` amendment layer: honouring the preference is
//! optional (RFC 7240 section 2: a server "MAY ignore the preference"), so
//! the layer must never buffer more than the advertised body cap
//! (bounds::MAX_BODY_BYTES) — an oversized response passes through
//! byte-identical, with no Preference-Applied header.

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

fn app(payload: String) -> axum::Router {
    axum::Router::new()
        .route(
            "/doc",
            axum::routing::get(move || {
                let payload = payload.clone();
                async move { ([(header::CONTENT_TYPE, "application/json")], payload) }
            }),
        )
        .layer(axum::middleware::from_fn(
            antares_api::conformance::prefer_version_layer,
        ))
}

async fn get_with_prefer(app: axum::Router) -> (StatusCode, Option<String>, String) {
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/doc")
                .header("Prefer", "ngsi-ld=1.6")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    let status = resp.status();
    let applied = resp
        .headers()
        .get("Preference-Applied")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    (
        status,
        applied,
        String::from_utf8_lossy(&bytes).into_owned(),
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn oversized_response_passes_through_unamended() {
    // one JSON array comfortably above MAX_BODY_BYTES (4 MiB)
    let n = antares_api::bounds::MAX_BODY_BYTES / 10 + 1;
    let payload = format!("[{}]", vec!["123456789"; n].join(","));
    assert!(payload.len() > antares_api::bounds::MAX_BODY_BYTES);

    let (status, applied, body) = get_with_prefer(app(payload.clone())).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        applied.is_none(),
        "an unamended oversized body must NOT claim Preference-Applied"
    );
    assert_eq!(body, payload, "body must pass through byte-identical");
}

#[tokio::test(flavor = "multi_thread")]
async fn small_response_still_gets_preference_applied() {
    let payload = r#"{"id":"urn:x","type":"T"}"#.to_owned();
    let (status, applied, body) = get_with_prefer(app(payload)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(applied.as_deref(), Some("ngsi-ld=1.6"));
}
