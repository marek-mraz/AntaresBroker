// SPDX-License-Identifier: EUPL-1.2
//! The `type` selector on Delete Entity (5.6.6.4) and Replace Entity
//! (5.6.18.4) is a term, and 5.5.7 term expansion is what turns a term into
//! the fully qualified name the stored document carries. The @context that
//! expansion uses is the request's own — 6.3.5 gives an `application/json`
//! request its @context in the Link header — so the same word selects
//! different types for different clients.
//!
//! Getting this wrong on Delete is destructive in one direction and a lie in
//! the other: an Entity the client's selector excluded is deleted anyway, or
//! an Entity it named is reported "not known".

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

const ID: &str = "urn:ngsi-ld:E:type-sel";

async fn send(st: &AppState, req: Request<Body>) -> (StatusCode, axum::http::HeaderMap, Value) {
    let res = antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("response");
    let status = res.status();
    let headers = res.headers().clone();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .expect("body");
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, headers, body)
}

fn req(method: &str, uri: &str, link: Option<&str>, body: Option<Value>) -> Request<Body> {
    let mut b = Request::builder().method(method).uri(uri);
    if let Some(l) = link {
        b = b.header("Link", l);
    }
    match body {
        None => b.body(Body::empty()).expect("request"),
        Some(v) => {
            let s = v.to_string();
            b.header("Content-Type", "application/json")
                .header("Content-Length", s.len())
                .body(Body::from(s))
                .expect("request")
        }
    }
}

/// A Hosted @context (5.13.2) mapping `Sensor` somewhere other than the core
/// default context, returned as a ready-made Link header value.
async fn hosted_link(st: &AppState) -> String {
    let (status, headers, _) = send(
        st,
        req(
            "POST",
            "/ngsi-ld/v1/jsonldContexts",
            None,
            Some(json!({"@context": {"Sensor": "https://example.org/Sensor"}})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let loc = headers
        .get("Location")
        .and_then(|l| l.to_str().ok())
        .expect("Location")
        .to_owned();
    let (_, _, meta) = send(st, req("GET", &format!("{loc}?details=true"), None, None)).await;
    let url = meta["URL"].as_str().expect("stored @context URL");
    format!("<{url}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"")
}

async fn create(st: &AppState, link: Option<&str>) {
    let (status, _, body) = send(
        st,
        req(
            "POST",
            "/ngsi-ld/v1/entities",
            link,
            Some(json!({"id": ID, "type": "Sensor"})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

async fn exists(st: &AppState) -> bool {
    let (status, _, _) = send(
        st,
        req("GET", &format!("/ngsi-ld/v1/entities/{ID}"), None, None),
    )
    .await;
    status == StatusCode::OK
}

/// 5.6.6.4 identifies the target "by its id (URI), and where specified type".
/// The Entity was created under the core default context, so a client whose
/// own @context points `Sensor` elsewhere has named a type this Entity does
/// not carry: the delete must not touch it.
#[tokio::test(flavor = "multi_thread")]
async fn delete_does_not_take_the_type_selector_from_the_wrong_context() {
    let st = AppState::new("antares-type-sel".into());
    let link = hosted_link(&st).await;
    create(&st, None).await;

    let (status, _, body) = send(
        &st,
        req(
            "DELETE",
            &format!("/ngsi-ld/v1/entities/{ID}?type=Sensor"),
            Some(&link),
            None,
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "the selector named example.org/Sensor, the Entity is a default-context Sensor: {body}"
    );
    assert!(exists(&st).await, "the Entity must still be there");

    // the other direction: the same request against an Entity that really is
    // an example.org Sensor deletes it
    let (status, _, body) = send(
        &st,
        req("DELETE", &format!("/ngsi-ld/v1/entities/{ID}"), None, None),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    create(&st, Some(&link)).await;
    let (status, _, body) = send(
        &st,
        req(
            "DELETE",
            &format!("/ngsi-ld/v1/entities/{ID}?type=Sensor"),
            Some(&link),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    assert!(!exists(&st).await, "the Entity must be gone");
}

/// 5.6.18.4 narrows the same way, and a replace that ignores the selector
/// overwrites an Entity the client excluded.
#[tokio::test(flavor = "multi_thread")]
async fn replace_does_not_take_the_type_selector_from_the_wrong_context() {
    let st = AppState::new("antares-type-sel-r".into());
    let link = hosted_link(&st).await;
    create(&st, None).await;

    let (status, _, body) = send(
        &st,
        req(
            "PUT",
            &format!("/ngsi-ld/v1/entities/{ID}?type=Sensor"),
            Some(&link),
            Some(json!({"id": ID, "type": "Sensor", "v": {"type": "Property", "value": 1}})),
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "the selector named a type this Entity does not carry: {body}"
    );
    let (_, _, stored) = send(
        &st,
        req("GET", &format!("/ngsi-ld/v1/entities/{ID}"), None, None),
    )
    .await;
    assert!(
        stored.get("v").is_none(),
        "the excluded Entity must not have been replaced: {stored}"
    );
}
