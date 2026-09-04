// SPDX-License-Identifier: EUPL-1.2
//! 5.5.12: "For each member of the Fragment, whose value is an NGSI-LD Null,
//! contained by the target, the target member is removed." `scope` is such a
//! member, and 4.18 allows the sentinel there for exactly this reason —
//! "urn:ngsi-ld:null" shall only appear for deleted scopes. The merge stored
//! the sentinel as the scope instead of removing the member, so the Entity
//! came back scoped to a scope no 4.18 grammar accepts.

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::Value;
use tower::ServiceExt;

const ID: &str = "urn:ngsi-ld:Scoped:m1";

async fn call(st: &AppState, method: &str, path: &str, body: &str) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(method)
        .uri(format!("/ngsi-ld/v1/{path}"))
        .header("Content-Type", "application/json")
        .header("Content-Length", body.len())
        .body(Body::from(body.to_owned()))
        .expect("request");
    let res = antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("response");
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .expect("body");
    let parsed = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, parsed)
}

async fn seeded(scope: &str) -> AppState {
    let st = AppState::new("test".into());
    let scope_member = if scope.is_empty() {
        String::new()
    } else {
        format!(r#""scope":"{scope}","#)
    };
    let body = format!(
        r#"{{"id":"{ID}","type":"Scoped",{scope_member}"v":{{"type":"Property","value":1}}}}"#
    );
    let (status, resp) = call(&st, "POST", "entities", &body).await;
    assert_eq!(status, StatusCode::CREATED, "seed: {resp}");
    st
}

async fn stored(st: &AppState) -> Value {
    let (status, doc) = call(st, "GET", &format!("entities/{ID}"), "").await;
    assert_eq!(status, StatusCode::OK, "{doc}");
    doc
}

#[tokio::test(flavor = "multi_thread")]
async fn a_null_scope_in_a_merge_removes_the_scope() {
    let st = seeded("/a/b").await;
    let (status, body) = call(
        &st,
        "PATCH",
        &format!("entities/{ID}"),
        r#"{"scope":"urn:ngsi-ld:null"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let doc = stored(&st).await;
    assert_eq!(
        doc["scope"],
        Value::Null,
        "the scope survived the merge: {doc}"
    );
    assert_eq!(
        doc["v"]["value"], 1,
        "the merge took the rest with it: {doc}"
    );
}

/// "contained by the target" — with nothing to remove the member is not
/// created, least of all holding the sentinel.
#[tokio::test(flavor = "multi_thread")]
async fn a_null_scope_on_an_unscoped_entity_creates_nothing() {
    let st = seeded("").await;
    let (status, body) = call(
        &st,
        "PATCH",
        &format!("entities/{ID}"),
        r#"{"scope":"urn:ngsi-ld:null"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let doc = stored(&st).await;
    assert_eq!(
        doc["scope"],
        Value::Null,
        "a scope appeared from a deletion: {doc}"
    );
}

/// The ordinary merge is untouched: a scope value replaces the stored one.
#[tokio::test(flavor = "multi_thread")]
async fn a_scope_value_in_a_merge_replaces_the_scope() {
    let st = seeded("/a/b").await;
    let (status, body) = call(
        &st,
        "PATCH",
        &format!("entities/{ID}"),
        r#"{"scope":"/x/y"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    assert_eq!(stored(&st).await["scope"], "/x/y");
}
