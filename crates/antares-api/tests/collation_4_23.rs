// SPDX-License-Identifier: EUPL-1.2
//! 4.23.1 / 4.23.3 EXAMPLES 6/7: the collation parameter — orderBy string
//! comparison under an ICU collation (RFC 6067 tag) instead of codepoint
//! order; 5.2.43 maps the OrderingParams `collation` member onto it.

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

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

async fn get(st: &AppState, uri: &str) -> (StatusCode, Value) {
    send(
        st,
        Request::builder()
            .method("GET")
            .uri(uri)
            .body(Body::empty())
            .expect("request"),
    )
    .await
}

async fn seed(st: &AppState, suffix: &str, name: &str) {
    let body = json!({
        "id": format!("urn:ngsi-ld:Vehicle:coll{suffix}"),
        "type": "Vehicle",
        "name": {"type": "Property", "value": name},
    })
    .to_string();
    let (status, b) = send(
        st,
        Request::builder()
            .method("POST")
            .uri("/ngsi-ld/v1/entities")
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{b}");
}

fn names(body: &Value) -> Vec<String> {
    body.as_array()
        .expect("array")
        .iter()
        .filter_map(|d| d["name"]["value"].as_str().map(str::to_owned))
        .collect()
}

/// 4.23.3 EXAMPLE 6/7: under the root collation "á" sorts with "a" (before
/// "b"); under codepoint order it sorts after "b". Both orders asserted so
/// the collation path is proven distinct from the default.
#[tokio::test(flavor = "multi_thread")]
async fn clause_4_23_3_collation_orders_accented_strings() {
    let st = AppState::new("me".into());
    seed(&st, "1", "b").await;
    seed(&st, "2", "A").await;
    seed(&st, "3", "á").await;

    // codepoint default: "A" < "b" < "á"
    let (status, body) = get(&st, "/ngsi-ld/v1/entities?type=Vehicle&orderBy=name").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(names(&body), vec!["A", "b", "á"], "{body}");

    // root collation: "A" < "á" < "b"
    let (status, body) = get(
        &st,
        "/ngsi-ld/v1/entities?type=Vehicle&orderBy=name&collation=und",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(names(&body), vec!["A", "á", "b"], "{body}");
}

/// 4.23.3 EXAMPLE 6: -u-ks- strength — level1 (primary) makes case
/// differences tie so "abc" ranks before "ABD"; codepoint order puts
/// "ABD" first.
#[tokio::test(flavor = "multi_thread")]
async fn clause_4_23_3_strength_keyword_is_honoured() {
    let st = AppState::new("me".into());
    seed(&st, "k1", "abc").await;
    seed(&st, "k2", "ABD").await;

    let (status, body) = get(&st, "/ngsi-ld/v1/entities?type=Vehicle&orderBy=name").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(names(&body), vec!["ABD", "abc"], "codepoint: {body}");

    let (status, body) = get(
        &st,
        "/ngsi-ld/v1/entities?type=Vehicle&orderBy=name&collation=und-u-ks-level1",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(names(&body), vec!["abc", "ABD"], "primary strength: {body}");
}

/// An unparseable collation tag is BadRequestData, not a silent codepoint
/// fallback.
#[tokio::test(flavor = "multi_thread")]
async fn clause_4_23_3_invalid_collation_is_400() {
    let st = AppState::new("me".into());
    seed(&st, "x", "a").await;
    let (status, body) = get(
        &st,
        "/ngsi-ld/v1/entities?type=Vehicle&orderBy=name&collation=!!nope",
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(
        body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData",
        "{body}"
    );
}

/// 5.2.43 Table 5.2.43-1: the OrderingParams collation member flattens onto
/// the collation parameter for POST /entityOperations/query.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_2_43_post_query_collation_member() {
    let st = AppState::new("me".into());
    seed(&st, "p1", "b").await;
    seed(&st, "p2", "á").await;

    let body = json!({
        "type": "Query",
        "entities": [{"type": "Vehicle"}],
        "ordering": {"orderBy": ["name"], "collation": "und"},
    })
    .to_string();
    let (status, body) = send(
        &st,
        Request::builder()
            .method("POST")
            .uri("/ngsi-ld/v1/entityOperations/query")
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(names(&body), vec!["á", "b"], "{body}");
}

/// 5.7.2.4 and 5.7.4.4: "If a preferred collation setting is present and it
/// does not conform to a valid ICU collation (see IETF RFC 6067 [36]) then an
/// error of type BadRequestData shall be raised." The sentence is about the
/// parameter being present, not about an `orderBy` happening to consume it.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_7_4_4_an_invalid_collation_is_refused_without_an_order_by() {
    let st = AppState::new("me".into());
    seed(&st, "c1", "a").await;
    for uri in [
        "/ngsi-ld/v1/entities?type=Vehicle&collation=!!nope",
        "/ngsi-ld/v1/temporal/entities?type=Vehicle&timerel=after\
         &timeAt=2000-01-01T00:00:00Z&collation=!!nope",
    ] {
        let (status, body) = get(&st, uri).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}: {body}");
        assert_eq!(
            body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData",
            "{uri}: {body}"
        );
    }
}

/// 5.7.4.4: "If the ordering parameter is present and the execution of the
/// operation is not limited to the local scope (see clause 5.5.13) then an
/// error of type BadRequestData shall be raised." 4.23.1 gives the reason —
/// sort ordering is never applied to distributed operations — and the
/// current-state query already refuses it; the temporal one has to agree.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_7_4_4_temporal_ordering_requires_local_scope() {
    let st = AppState::new("me".into());
    seed(&st, "o1", "a").await;
    let reg = json!({
        "id": "urn:ngsi-ld:ContextSourceRegistration:order",
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": "Vehicle"}]}],
        // TEST-NET-3: never contacted, the request is refused before any forward
        "endpoint": "http://203.0.113.7:9999",
    })
    .to_string();
    let (status, b) = send(
        &st,
        Request::builder()
            .method("POST")
            .uri("/ngsi-ld/v1/csourceRegistrations")
            .header("Content-Type", "application/json")
            .header("Content-Length", reg.len())
            .body(Body::from(reg))
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{b}");

    let window = "type=Vehicle&timerel=after&timeAt=2000-01-01T00:00:00Z";
    let (status, body) = get(
        &st,
        &format!("/ngsi-ld/v1/temporal/entities?{window}&orderBy=id"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(
        body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData",
        "{body}"
    );

    // limited to the local scope, the same ordering is served (5.5.13)
    let (status, body) = get(
        &st,
        &format!("/ngsi-ld/v1/temporal/entities?{window}&orderBy=id&local=true"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}
