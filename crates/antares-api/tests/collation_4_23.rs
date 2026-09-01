// SPDX-License-Identifier: EUPL-1.2
//! 4.23 Entity ordering through query parameters.
//!
//! 4.23.1 / 4.23.3 EXAMPLES 6/7: the collation parameter — orderBy string
//! comparison under an ICU collation (RFC 6067 tag) instead of codepoint
//! order; 5.2.43 maps the OrderingParams `collation` member onto it.
//!
//! 4.23.3 EXAMPLES 8/9/10: distance ordering, whose reference geometry is
//! `orderFrom` (+ `orderGeometry`, default Point). The clause writes those
//! examples as QUERY PARAMETERS, and Table 6.4.3.2-1 makes `orderFrom`
//! mandatory when orderBy asks for distance, so the GET form is the one the
//! spec specifies and the one asserted here.

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

/// Seed a Vehicle whose `location` GeoProperty is a Point, for the distance
/// ordering below.
async fn seed_at(st: &AppState, suffix: &str, lon: f64, lat: f64) {
    let body = json!({
        "id": format!("urn:ngsi-ld:Vehicle:dist{suffix}"),
        "type": "Vehicle",
        "location": {
            "type": "GeoProperty",
            "value": {"type": "Point", "coordinates": [lon, lat]},
        },
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

fn ids(body: &Value) -> Vec<String> {
    body.as_array()
        .expect("array")
        .iter()
        .filter_map(|d| d["id"].as_str().map(str::to_owned))
        .collect()
}

/// 4.23.3 EXAMPLE 8/9, written by the clause as query parameters:
/// `?orderBy=location;dist-asc&orderFrom=[8,40]` ranks Entities in ascending
/// distance from the reference Point, and `dist-desc` in descending distance.
/// Both directions are asserted against the SAME seed, so a broker that
/// allow-listed `orderFrom` without reading it cannot pass: it would return
/// one order for both.
#[tokio::test(flavor = "multi_thread")]
async fn clause_4_23_3_dist_ordering_reads_order_from_on_a_get() {
    let st = AppState::new("dist".into());
    seed_at(&st, "near", 8.01, 40.01).await;
    seed_at(&st, "far", 10.0, 45.0).await;

    let (status, body) = get(
        &st,
        "/ngsi-ld/v1/entities?type=Vehicle&orderBy=location;dist-asc&orderFrom=%5B8,40%5D",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        ids(&body),
        [
            "urn:ngsi-ld:Vehicle:distnear",
            "urn:ngsi-ld:Vehicle:distfar"
        ],
        "dist-asc ranks the nearer Entity first: {body}"
    );

    let (status, body) = get(
        &st,
        "/ngsi-ld/v1/entities?type=Vehicle&orderBy=location;dist-desc&orderFrom=%5B8,40%5D",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        ids(&body),
        [
            "urn:ngsi-ld:Vehicle:distfar",
            "urn:ngsi-ld:Vehicle:distnear"
        ],
        "dist-desc ranks the farther Entity first: {body}"
    );
}

/// Table 6.4.3.2-1 `orderFrom`: "It shall be one if orderBy uses order by
/// distance". Without it there is no reference geometry, so the request is
/// refused rather than answered in some arbitrary order.
#[tokio::test(flavor = "multi_thread")]
async fn clause_4_23_3_dist_ordering_without_order_from_is_refused() {
    let st = AppState::new("distnoref".into());
    seed_at(&st, "solo", 8.0, 40.0).await;
    let (status, body) = get(
        &st,
        "/ngsi-ld/v1/entities?type=Vehicle&orderBy=location;dist-asc",
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "distance ordering has no reference geometry without orderFrom: {body}"
    );
}

/// 4.23.3 EXAMPLE 10: the reference geometry is not always a Point —
/// `orderFrom=[[8,40],[9,42],[9,45],[8,40]]&orderGeometry=LineString` ranks
/// by distance from a LineString. Read as the default Point, that coordinate
/// list is not a Point at all, so a broker ignoring `orderGeometry` cannot
/// answer this the same way.
#[tokio::test(flavor = "multi_thread")]
async fn clause_4_23_3_order_geometry_selects_the_reference_geometry() {
    let st = AppState::new("distline".into());
    // on the segment from [8,40] to [9,42]
    seed_at(&st, "online", 8.5, 41.0).await;
    seed_at(&st, "offline", 20.0, 60.0).await;

    let (status, body) = get(
        &st,
        concat!(
            "/ngsi-ld/v1/entities?type=Vehicle&orderBy=location;dist-asc",
            "&orderFrom=%5B%5B8,40%5D,%5B9,42%5D,%5B9,45%5D,%5B8,40%5D%5D",
            "&orderGeometry=LineString"
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        ids(&body),
        [
            "urn:ngsi-ld:Vehicle:distonline",
            "urn:ngsi-ld:Vehicle:distoffline"
        ],
        "a LineString reference ranks the Entity on the line first: {body}"
    );
}
