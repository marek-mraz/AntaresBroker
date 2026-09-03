// SPDX-License-Identifier: EUPL-1.2
//! 5.6.3.4 Append Attributes with overwrite denied: "If overwrite is not
//! allowed the existing default Attribute in the target Entity shall be left
//! untouched." An append that touches nothing did not modify the Entity, so
//! nothing about it may move -- 4.8 defines `modifiedAt` as the time at which
//! the Entity "was last modified in an NGSI-LD system" -- and no new instance
//! may enter its temporal evolution.

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

async fn send(st: &AppState, method: &str, uri: &str, body: &str) -> (StatusCode, String) {
    let req = Request::builder()
        .method(method)
        .uri(uri)
        .header("Content-Type", "application/json")
        .header("Content-Length", body.len())
        .body(Body::from(body.to_owned()))
        .expect("request");
    let resp = antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("response");
    let status = resp.status();
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

async fn entity(st: &AppState, id: &str) -> Value {
    let (status, body) = send(
        st,
        "GET",
        &format!("/ngsi-ld/v1/entities/{id}?options=sysAttrs"),
        "",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    serde_json::from_str(&body).expect("entity")
}

const CREATE: &str = r#"{"id":"urn:ngsi-ld:NoOv:1","type":"NoOv",
    "v":{"type":"Property","value":1}}"#;

async fn seeded() -> AppState {
    let mut st = AppState::new("test".into());
    antares_api::wire(&mut st).await;
    let (status, body) = send(&st, "POST", "/ngsi-ld/v1/entities", CREATE).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    st
}

#[tokio::test(flavor = "multi_thread")]
async fn a_denied_overwrite_leaves_the_whole_entity_untouched() {
    let st = seeded().await;
    let before = entity(&st, "urn:ngsi-ld:NoOv:1").await;

    let (status, body) = send(
        &st,
        "POST",
        "/ngsi-ld/v1/entities/urn:ngsi-ld:NoOv:1/attrs?options=noOverwrite",
        r#"{"v":{"type":"Property","value":2}}"#,
    )
    .await;
    assert_eq!(status, StatusCode::MULTI_STATUS, "{body}");
    let report: Value = serde_json::from_str(&body).expect("UpdateResult");
    assert_eq!(
        report["notUpdated"][0]["attributeName"], "https://uri.etsi.org/ngsi-ld/default-context/v",
        "the existing attribute is reported, not applied: {body}"
    );

    assert_eq!(
        entity(&st, "urn:ngsi-ld:NoOv:1").await,
        before,
        "a denied overwrite wrote to the Entity"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_denied_overwrite_records_no_temporal_instance() {
    let st = seeded().await;
    let (status, body) = send(
        &st,
        "POST",
        "/ngsi-ld/v1/entities/urn:ngsi-ld:NoOv:1/attrs?options=noOverwrite",
        r#"{"v":{"type":"Property","value":2}}"#,
    )
    .await;
    assert_eq!(status, StatusCode::MULTI_STATUS, "{body}");

    let (status, body) = send(
        &st,
        "GET",
        "/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:NoOv:1",
        "",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let hist: Value = serde_json::from_str(&body).expect("temporal entity");
    let instances = hist["v"].as_array().expect("v history").len();
    assert_eq!(
        instances, 1,
        "the denied append entered the history: {body}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_entity_type_the_target_already_has_touches_nothing() {
    let st = seeded().await;
    let before = entity(&st, "urn:ngsi-ld:NoOv:1").await;

    let (status, body) = send(
        &st,
        "POST",
        "/ngsi-ld/v1/entities/urn:ngsi-ld:NoOv:1/attrs",
        r#"{"type":"NoOv"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    assert_eq!(
        entity(&st, "urn:ngsi-ld:NoOv:1").await,
        before,
        "an Entity Type already in the list wrote to the Entity"
    );
}
