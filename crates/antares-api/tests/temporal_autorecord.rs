//! 5.6.4 + auto-recording: every Partial Attribute Update appends a new
//! attribute instance to the temporal evolution — the regression behind the
//! playground's flat history charts (create recorded, PATCHes silently not).

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

#[tokio::test(flavor = "multi_thread")]
async fn partial_update_appends_temporal_instances() {
    let mut st = AppState::new("test".into());
    antares_api::notify::wire(&mut st);

    let (status, body) = send(
        &st,
        Request::builder()
            .method("POST")
            .uri("/ngsi-ld/v1/entities")
            .header("Content-Type", "application/json")
            .body(Body::from(
                r#"{"id":"urn:ngsi-ld:Rec:1","type":"Rec",
                    "v":{"type":"Property","value":1,"observedAt":"2026-08-08T14:00:00Z"}}"#,
            ))
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    for (i, ts) in [(2, "2026-08-08T14:00:10Z"), (3, "2026-08-08T14:00:20Z")] {
        let (status, body) = send(
            &st,
            Request::builder()
                .method("PATCH")
                .uri("/ngsi-ld/v1/entities/urn:ngsi-ld:Rec:1/attrs/v")
                .header("Content-Type", "application/json")
                .body(Body::from(format!(
                    r#"{{"type":"Property","value":{i},"observedAt":"{ts}"}}"#
                )))
                .expect("request"),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    }

    let (status, body) = send(
        &st,
        Request::builder()
            .uri("/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:Rec:1")
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let doc: serde_json::Value = serde_json::from_str(&body).expect("json");
    let instances = doc["v"].as_array().expect("v instance array");
    let values: Vec<i64> = instances
        .iter()
        .filter_map(|i| i["value"].as_i64())
        .collect();
    assert_eq!(
        values.len(),
        3,
        "each PATCH must append an instance, got {body}"
    );
    assert!(
        [1, 2, 3].iter().all(|v| values.contains(v)),
        "expected values 1..3, got {values:?}"
    );
}
