// SPDX-License-Identifier: EUPL-1.2
//! The Attribute name in the URL path is input, and 5.6.4.4 rules it the
//! same way it rules the Entity id: "If the target Attribute name is not
//! valid or it is not present, then an error of type BadRequestData shall be
//! raised", after "Apply term expansion as mandated by clause 5.5.7, so that
//! the fully qualified name (URI) associated to the target Attribute is
//! properly obtained". A name that expands to something that is not a URI
//! has no fully qualified name, so it is not valid — and 5.6.19.4, 5.6.5.4,
//! 5.6.13.4 and 5.6.14.4 repeat the sentence for the other operations that
//! address an Attribute by path.
//!
//! A request @context is client data, so a term can be pointed at any string
//! at all. Without the check the expanded name lands on a member of the
//! stored document that is not an Attribute — the Entity `type`, its
//! `scope` — and the operation edits the Entity's own structure instead of
//! an Attribute of it.

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

const ID: &str = "urn:ngsi-ld:Vehicle:ap1";

async fn send(st: &AppState, req: Request<Body>) -> (StatusCode, Value) {
    let res = antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("response");
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .expect("body");
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, body)
}

/// POST a client-owned @context document and return the URL it is served
/// at. 5.13.2.4 answers with `Location`, and the stored `URL` the broker
/// resolves it by comes back from the details view.
async fn hosted_context(st: &AppState, term: &str, target: &str) -> String {
    let doc = json!({"@context": {term: target}}).to_string();
    let res = antares_api::router(st.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/ngsi-ld/v1/jsonldContexts")
                .header("Content-Type", "application/ld+json")
                .header("Content-Length", doc.len())
                .body(Body::from(doc))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(res.status(), StatusCode::CREATED, "jsonldContexts");
    let loc = res
        .headers()
        .get("Location")
        .and_then(|v| v.to_str().ok())
        .expect("the created context names its location")
        .to_owned();
    let (status, meta) = send(
        st,
        Request::builder()
            .method("GET")
            .uri(format!("{loc}?details=true"))
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{meta}");
    meta["URL"]
        .as_str()
        .expect("the stored @context URL")
        .to_owned()
}

/// A Link header naming a hosted @context document.
fn link(url: &str) -> String {
    format!("<{url}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"")
}

/// One Entity with a type, a scope and one ordinary Attribute, plus its
/// Temporal Evolution, so every operation that takes an Attribute in the
/// path has a target that exists.
async fn seeded() -> AppState {
    let mut st = AppState::new("me".into());
    antares_api::notify::wire(&mut st);
    for (uri, body) in [
        (
            "/ngsi-ld/v1/entities",
            json!({
                "id": ID, "type": "Vehicle", "scope": "/road/a1",
                "speed": {"type": "Property", "value": 10}
            }),
        ),
        (
            "/ngsi-ld/v1/temporal/entities",
            json!({
                "id": ID, "type": "Vehicle",
                "speed": [{"type": "Property", "value": 10,
                           "observedAt": "2026-01-01T09:00:00Z"}]
            }),
        ),
    ] {
        let body = body.to_string();
        let (status, b) = send(
            &st,
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("Content-Type", "application/json")
                .header("Content-Length", body.len())
                .body(Body::from(body))
                .expect("request"),
        )
        .await;
        assert!(status.is_success(), "seed {uri}: {status} {b}");
    }
    st
}

/// The Entity as it reads back through 5.6.7 Retrieve Entity.
async fn entity(st: &AppState) -> Value {
    let (status, body) = send(
        st,
        Request::builder()
            .method("GET")
            .uri(format!("/ngsi-ld/v1/entities/{ID}"))
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

fn is_bad_request(status: StatusCode, body: &Value, what: &str) {
    assert_eq!(status, StatusCode::BAD_REQUEST, "{what}: {body}");
    assert_eq!(
        body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData",
        "{what}: {body}"
    );
    // 6.3.2 ProblemDetails and nothing else — the rejected name must not come
    // back with the stored member it was pointed at.
    for k in ["tenant", "entityMap", "regs", "value"] {
        assert!(body.get(k).is_none(), "{what}: {k} leaked: {body}");
    }
}

/// A term the request @context maps onto a bare `type`. 5.5.7 expansion
/// yields `type`, which is not a URI, so 5.6.4.4 has no fully qualified name
/// to work with and the request is BadRequestData — not a 500, and not an
/// edit of the Entity's type member.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_6_4_path_attribute_mapped_onto_type_is_bad_request() {
    let st = seeded().await;
    let body = json!({"@context": {"foo": "type"}, "type": "Property", "value": 5}).to_string();
    let (status, b) = send(
        &st,
        Request::builder()
            .method("PATCH")
            .uri(format!("/ngsi-ld/v1/entities/{ID}/attrs/foo"))
            .header("Content-Type", "application/ld+json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("request"),
    )
    .await;
    is_bad_request(status, &b, "5.6.4 onto type");
    let e = entity(&st).await;
    assert_eq!(e["type"], "Vehicle", "the Entity type was edited: {e}");
}

/// The same term against 5.6.5 Delete Attribute. The Entity type is a
/// mandatory member (5.2.4); deleting it through an Attribute endpoint
/// leaves a document no type-scoped query or subscription can ever match.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_6_5_deleting_a_path_attribute_mapped_onto_type_is_bad_request() {
    let st = seeded().await;
    // 5.6.5 carries no body, so its @context comes from the Link header —
    // and a jsonldContexts document is the client's own to write.
    let url = hosted_context(&st, "bar", "type").await;
    let (status, b) = send(
        &st,
        Request::builder()
            .method("DELETE")
            .uri(format!("/ngsi-ld/v1/entities/{ID}/attrs/bar"))
            .header("Link", link(&url))
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    is_bad_request(status, &b, "5.6.5 onto type");
    let e = entity(&st).await;
    assert_eq!(e["type"], "Vehicle", "the Entity type was deleted: {e}");
}

/// 5.6.19 Replace Attribute reads the same path name. It already refused the
/// mapped term further down the pipeline; the clause puts the refusal at the
/// name, so the error is BadRequestData whichever guard fires.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_6_19_path_attribute_mapped_onto_a_reserved_member_is_bad_request() {
    let st = seeded().await;
    for target in ["type", "scope", "id", "value"] {
        let body = json!({"@context": {"baz": target}, "type": "Property", "value": 5}).to_string();
        let (status, b) = send(
            &st,
            Request::builder()
                .method("PUT")
                .uri(format!("/ngsi-ld/v1/entities/{ID}/attrs/baz"))
                .header("Content-Type", "application/ld+json")
                .header("Content-Length", body.len())
                .body(Body::from(body))
                .expect("request"),
        )
        .await;
        is_bad_request(status, &b, &format!("5.6.19 onto {target}"));
    }
    let e = entity(&st).await;
    assert_eq!(e["type"], "Vehicle", "{e}");
    assert_eq!(e["scope"], "/road/a1", "{e}");
    assert_eq!(e["id"], ID, "{e}");
}

/// 5.6.13.4 repeats the sentence for the Temporal Evolution, whose document
/// carries the same `type` member. Its own clause note already says the name
/// is interpolated into a forwarded request path, so it is guarded input
/// twice over.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_6_13_temporal_path_attribute_mapped_onto_type_is_bad_request() {
    let st = seeded().await;
    let url = hosted_context(&st, "qux", "type").await;
    let (status, b) = send(
        &st,
        Request::builder()
            .method("DELETE")
            .uri(format!("/ngsi-ld/v1/temporal/entities/{ID}/attrs/qux"))
            .header("Link", link(&url))
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    is_bad_request(status, &b, "5.6.13 onto type");

    let (status, t) = send(
        &st,
        Request::builder()
            .method("GET")
            .uri(format!(
                "/ngsi-ld/v1/temporal/entities/{ID}?timerel=after&timeAt=2000-01-01T00:00:00Z"
            ))
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{t}");
    assert_eq!(t["type"], "Vehicle", "the temporal type was deleted: {t}");
}

/// `GET /entities/{id}/attrs/{attr}` is the 2.0 #14 pre-adoption and reads
/// the same path name. A name that expands onto `type` would answer with the
/// Entity's type member dressed as an Attribute, which is the stored
/// representation leaking into a response.
#[tokio::test(flavor = "multi_thread")]
async fn preadoption_retrieving_a_path_attribute_mapped_onto_type_is_bad_request() {
    let st = seeded().await;
    let url = hosted_context(&st, "zap", "type").await;
    for path in ["", "/value"] {
        let (status, b) = send(
            &st,
            Request::builder()
                .method("GET")
                .uri(format!("/ngsi-ld/v1/entities/{ID}/attrs/zap{path}"))
                .header("Link", link(&url))
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        is_bad_request(status, &b, &format!("#14 onto type{path}"));
    }
}

/// The check is on the shape of the expanded name, not on a deny list of
/// member names: an Attribute whose term expands to a real URI keeps working
/// through every one of these endpoints, and so does a path that already
/// carries the fully qualified name.
#[tokio::test(flavor = "multi_thread")]
async fn a_term_that_expands_to_a_uri_still_addresses_its_attribute() {
    let st = seeded().await;
    let (status, b) = send(
        &st,
        Request::builder()
            .method("GET")
            .uri(format!("/ngsi-ld/v1/entities/{ID}/attrs/speed"))
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{b}");
    assert_eq!(b["value"], 10, "{b}");

    let body = json!({"type": "Property", "value": 42}).to_string();
    let (status, b) = send(
        &st,
        Request::builder()
            .method("PATCH")
            .uri(format!("/ngsi-ld/v1/entities/{ID}/attrs/speed"))
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{b}");
    assert_eq!(entity(&st).await["speed"]["value"], 42);

    // and a term the request @context points at a URI of the client's own
    // choosing is an Attribute name like any other
    let ctx = json!({"velocity": "https://example.invalid/v/speed"});
    let body = json!({"@context": ctx, "velocity": {"type": "Property", "value": 7}}).to_string();
    let (status, b) = send(
        &st,
        Request::builder()
            .method("POST")
            .uri(format!("/ngsi-ld/v1/entities/{ID}/attrs"))
            .header("Content-Type", "application/ld+json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("request"),
    )
    .await;
    assert!(status.is_success(), "append a mapped term: {status} {b}");
    let body = json!({"@context": ctx, "type": "Property", "value": 8}).to_string();
    let (status, b) = send(
        &st,
        Request::builder()
            .method("PATCH")
            .uri(format!("/ngsi-ld/v1/entities/{ID}/attrs/velocity"))
            .header("Content-Type", "application/ld+json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "patch a mapped term: {b}");
}
