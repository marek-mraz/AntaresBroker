// SPDX-License-Identifier: EUPL-1.2
//! 6.3.14 gives `NGSILD-Tenant` one value: the Tenant the operation runs
//! against. It is not a list-type field, so repeated field lines cannot be
//! joined (RFC 9110 clause 5.3) and a request carrying two names none. The
//! outer wall reads it before any handler does — to refuse the tenant
//! namespace the broker mints for itself, and to answer NonexistentTenant —
//! and both of those decisions have to be about the value the operation will
//! use, or the wall guards one request and the handler answers another.

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::Value;
use tower::ServiceExt;

async fn get(st: &AppState, path: &str, tenants: &[&str]) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method("GET")
        .uri(format!("/ngsi-ld/v1/{path}"));
    for t in tenants {
        req = req.header("NGSILD-Tenant", *t);
    }
    let res = antares_api::router(st.clone())
        .oneshot(req.body(Body::empty()).expect("request"))
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

fn kind(body: &Value) -> &str {
    body["title"].as_str().unwrap_or("")
}

#[tokio::test(flavor = "multi_thread")]
async fn two_tenant_headers_are_bad_request_data_not_a_missing_tenant() {
    let st = AppState::new("me".into());
    let (status, body) = get(&st, "entities/urn:ngsi-ld:V:1", &["alpha", "alpha"]).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(kind(&body), "BadRequestData", "{body}");
}

/// The reserved-namespace guard has to see the second value too: a request
/// the wall waves through on its first name is a request the wall did not
/// check.
#[tokio::test(flavor = "multi_thread")]
async fn a_reserved_tenant_hidden_behind_a_second_header_is_still_refused() {
    let st = AppState::new("me".into());
    let (status, body) = get(&st, "entities/urn:ngsi-ld:V:1", &["alpha", "snap-index"]).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(kind(&body), "BadRequestData", "{body}");
}

/// One header still answers 5.5.10's NonexistentTenant, and 6.3.14's echo
/// still rides that error response.
#[tokio::test(flavor = "multi_thread")]
async fn one_header_naming_an_unknown_tenant_is_still_nonexistent_tenant() {
    let st = AppState::new("me".into());
    let (status, body) = get(&st, "entities/urn:ngsi-ld:V:1", &["ghost"]).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(kind(&body), "NonexistentTenant", "{body}");
}
