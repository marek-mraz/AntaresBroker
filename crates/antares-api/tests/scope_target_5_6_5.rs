// SPDX-License-Identifier: EUPL-1.2
//! 5.6.5.4 Delete Attribute: "Apply term expansion as mandated by clause
//! 5.5.7 so that the fully qualified name (URI) associated to the target
//! Attribute is properly obtained" and then "If the target Attribute is
//! scope, remove the scope Attribute from the target Entity." The target is
//! what the name expands to, so the reserved member is addressable by either
//! spelling — the Partial Update (5.6.4.4) and Replace Attribute (5.6.19.4)
//! paths already refuse both spellings, and Delete has to remove both.

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

async fn call(st: &AppState, method: &str, path: &str, body: &str) -> (StatusCode, String) {
    let req = Request::builder()
        .method(method)
        .uri(format!("/ngsi-ld/v1/{path}"))
        .header("Content-Type", "application/json")
        .header("Content-Length", body.len())
        .body(Body::from(body.to_owned()))
        .expect("req");
    let resp = antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("resp");
    let status = resp.status();
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

async fn seeded(id: &str) -> AppState {
    let st = AppState::new("test".into());
    let body = format!(
        r#"{{"id":"{id}","type":"Scoped","scope":"/a/b",
             "v":{{"type":"Property","value":1}}}}"#
    );
    let (status, resp) = call(&st, "POST", "entities", &body).await;
    assert_eq!(status, StatusCode::CREATED, "{resp}");
    st
}

async fn scope_of(st: &AppState, id: &str) -> Value {
    let (status, body) = call(st, "GET", &format!("entities/{id}"), "").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let doc: Value = serde_json::from_str(&body).expect("entity");
    doc["scope"].clone()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_short_name_removes_the_scope() {
    let id = "urn:ngsi-ld:Scoped:1";
    let st = seeded(id).await;
    let (status, body) = call(&st, "DELETE", &format!("entities/{id}/attrs/scope"), "").await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    assert_eq!(scope_of(&st, id).await, Value::Null, "the scope survived");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_expanded_name_removes_the_same_scope() {
    let id = "urn:ngsi-ld:Scoped:2";
    let st = seeded(id).await;
    let (status, body) = call(
        &st,
        "DELETE",
        &format!("entities/{id}/attrs/https%3A%2F%2Furi.etsi.org%2Fngsi-ld%2Fscope"),
        "",
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NO_CONTENT,
        "the fully qualified name of the reserved member is not the reserved member: {body}"
    );
    assert_eq!(scope_of(&st, id).await, Value::Null, "the scope survived");
}
