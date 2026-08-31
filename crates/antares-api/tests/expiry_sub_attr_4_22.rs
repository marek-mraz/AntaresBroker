// SPDX-License-Identifier: EUPL-1.2
//! 4.22 Transient Storage of Entities and Attributes: "expiresAt is defined
//! as the system temporal Property at which a certain Entity, Property or
//! Relationship shall become invalid and may be automatically removed from
//! the Context Broker." A sub-Attribute is a Property or a Relationship, and
//! the clause draws no line at depth 1: a sub-Attribute past its stamp has
//! become invalid and must not be served, while the Attribute carrying it and
//! its live siblings stay. Physical removal may lag ("clean-up processes will
//! only run periodically"), the read boundary may not.

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

/// The Attribute survives its expired sub-Attribute; the live sibling and the
/// Attribute's own value are untouched. `value` carries JSON that spells
/// `expiresAt` itself — user data, not an Attribute, so nothing may remove it.
#[tokio::test(flavor = "multi_thread")]
async fn an_expired_sub_attribute_is_not_served() {
    let st = AppState::new("test".into());
    let id = "urn:ngsi-ld:Vehicle:422-sub";
    let body = format!(
        r#"{{"id":"{id}","type":"Vehicle",
             "speed":{{"type":"Property","value":[{{"expiresAt":"2020-01-01T00:00:00Z"}}],
               "gone":{{"type":"Property","value":1,"expiresAt":"2020-01-01T00:00:00Z"}},
               "live":{{"type":"Property","value":2,"expiresAt":"2100-01-01T00:00:00Z"}},
               "plain":{{"type":"Property","value":3}}}}}}"#
    );
    let (status, resp) = call(&st, "POST", "entities", &body).await;
    assert_eq!(status, StatusCode::CREATED, "{resp}");

    let (status, resp) = call(&st, "GET", &format!("entities/{id}?options=sysAttrs"), "").await;
    assert_eq!(status, StatusCode::OK, "{resp}");
    let doc: Value = serde_json::from_str(&resp).expect("json");
    let speed = &doc["speed"];
    assert!(
        speed.get("gone").is_none(),
        "expired sub-Attribute served: {resp}"
    );
    assert_eq!(speed["live"]["value"], 2, "live sub-Attribute lost: {resp}");
    assert_eq!(speed["plain"]["value"], 3, "sibling lost: {resp}");
    assert_eq!(
        speed["value"],
        serde_json::json!([{"expiresAt": "2020-01-01T00:00:00Z"}]),
        "user JSON is not an Attribute: {resp}"
    );
}

/// An Attribute whose only sub-Attribute expired keeps serving itself: the
/// expiry of a sub-Attribute is not the expiry of its parent (4.22 applies
/// per Property, and the parent carries no stamp).
#[tokio::test(flavor = "multi_thread")]
async fn the_parent_attribute_outlives_its_only_sub_attribute() {
    let st = AppState::new("test".into());
    let id = "urn:ngsi-ld:Vehicle:422-sub-only";
    let body = format!(
        r#"{{"id":"{id}","type":"Vehicle",
             "speed":{{"type":"Property","value":10,
               "gone":{{"type":"Property","value":1,"expiresAt":"2020-01-01T00:00:00Z"}}}}}}"#
    );
    let (status, resp) = call(&st, "POST", "entities", &body).await;
    assert_eq!(status, StatusCode::CREATED, "{resp}");

    let (status, resp) = call(&st, "GET", &format!("entities/{id}"), "").await;
    assert_eq!(status, StatusCode::OK, "{resp}");
    let doc: Value = serde_json::from_str(&resp).expect("json");
    assert_eq!(doc["speed"]["value"], 10, "{resp}");
    assert!(doc["speed"].get("gone").is_none(), "{resp}");
}
