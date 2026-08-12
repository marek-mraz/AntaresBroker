//! EntityMap USAGE on the consumption operations (not the 5.14 CRUD):
//! Retrieve Entity 5.7.1.4, Retrieve Temporal Evolution 5.7.3.4 and Query
//! Temporal Evolution 5.7.4.4 — "if a flag to return an EntityMap was
//! present … a new EntityMap shall be created"; a supplied NGSILD-EntityMap
//! location is retrieved and, if live, is the ONLY source used to determine
//! which Context Source Registrations match; unknown/expired → recreate.

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

fn state() -> AppState {
    let mut st = AppState::new("antares-em-usage".into());
    antares_api::notify::wire(&mut st); // temporal auto-recording
    st
}

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

async fn get(st: &AppState, uri: &str) -> (StatusCode, axum::http::HeaderMap, Value) {
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

async fn get_with_map(
    st: &AppState,
    uri: &str,
    map_ref: &str,
) -> (StatusCode, axum::http::HeaderMap, Value) {
    send(
        st,
        Request::builder()
            .method("GET")
            .uri(uri)
            .header("NGSILD-EntityMap", map_ref)
            .body(Body::empty())
            .expect("request"),
    )
    .await
}

async fn create_vehicle(st: &AppState, id: &str, speed: i64) {
    let body = json!({
        "id": id,
        "type": "Vehicle",
        "speed": {"type": "Property", "value": speed},
    })
    .to_string();
    let (status, _, _) = send(
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
    assert_eq!(status, StatusCode::CREATED);
}

fn map_header(headers: &axum::http::HeaderMap) -> Option<String> {
    headers
        .get("NGSILD-EntityMap")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

/// 5.7.1.4: "If a flag to return an EntityMap was present in the request,
/// and no EntityMap currently exists, then a new EntityMap shall be
/// created" — GET /entities/{id}?entityMap=true answers with the
/// NGSILD-EntityMap location; the map holds the entity under "@none".
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_7_1_4_retrieve_entitymap_true_creates_map() {
    let st = state();
    create_vehicle(&st, "urn:ngsi-ld:Vehicle:r1", 30).await;

    let (status, headers, body) = get(
        &st,
        "/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:r1?entityMap=true",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let loc = map_header(&headers).expect("NGSILD-EntityMap header on the retrieve");
    // the retrieve body stays an Entity — the map is a separate resource
    assert!(body.get("entityMap").is_none(), "{body}");
    assert_eq!(body["id"], "urn:ngsi-ld:Vehicle:r1");

    let (status, _, map) = get(&st, &loc).await;
    assert_eq!(status, StatusCode::OK, "{map}");
    assert_eq!(map["type"], "EntityMap", "{map}");
    assert_eq!(map["entityMap"]["urn:ngsi-ld:Vehicle:r1"], json!(["@none"]));
}

/// 5.7.1.4: "If the resource cannot be found, or the data has expired, a
/// new EntityMap shall be created" — an unknown NGSILD-EntityMap reference
/// yields a fresh map, not an error.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_7_1_4_unknown_map_reference_recreates() {
    let st = state();
    create_vehicle(&st, "urn:ngsi-ld:Vehicle:r2", 30).await;

    let (status, headers, body) = get_with_map(
        &st,
        "/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:r2",
        "urn:ngsi-ld:entitymap:does-not-exist",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let loc = map_header(&headers).expect("a NEW map is created and returned");
    assert!(
        !loc.contains("does-not-exist"),
        "must not echo the dead reference: {loc}"
    );
    assert_eq!(body["id"], "urn:ngsi-ld:Vehicle:r2");
}

/// 5.7.1.4: "If the data has not expired, only the retrieved Entity Map
/// shall be used to determine which Context Source Registrations match the
/// Entity ID" — a live map whose entry is local-only ("@none") gates a
/// matching registration OUT of the retrieve.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_7_1_4_live_map_gates_registrations() {
    std::env::set_var("ANTARES_EGRESS_ALLOW_PRIVATE", "true");
    let st = state();
    create_vehicle(&st, "urn:ngsi-ld:Vehicle:r3", 30).await;

    // canned Context Source serving one extra attribute for r3
    let remote = json!({
        "id": "urn:ngsi-ld:Vehicle:r3",
        "type": "Vehicle",
        "remoteAttr": {"type": "Property", "value": 7},
    })
    .to_string();
    let reply = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{remote}",
        remote.len()
    );
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            use std::io::{Read, Write};
            let mut buf = [0u8; 8192];
            let _ = s.read(&mut buf);
            let _ = s.write_all(reply.as_bytes());
        }
    });
    let reg = json!({
        "id": "urn:ngsi-ld:ContextSourceRegistration:usage-fed",
        "type": "ContextSourceRegistration",
        "operations": ["retrieveEntity"],
        "information": [{"entities": [{"type": "Vehicle"}]}],
        "endpoint": format!("http://127.0.0.1:{port}"),
    })
    .to_string();
    let (status, _, body) = send(
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
    assert_eq!(status, StatusCode::CREATED, "{body}");

    // sanity: without a map the registration IS consulted and merged
    let (status, _, merged) = get(&st, "/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:r3").await;
    assert_eq!(status, StatusCode::OK, "{merged}");
    assert!(merged.get("remoteAttr").is_some(), "{merged}");

    // a LOCAL-scope map: the r3 entry lists only "@none"
    let (status, headers, body) = get(
        &st,
        "/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:r3?entityMap=true&local=true",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let loc = map_header(&headers).expect("map location");

    // with the live map in use, the registration must NOT be consulted
    let (status, headers, gated) =
        get_with_map(&st, "/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:r3", &loc).await;
    assert_eq!(status, StatusCode::OK, "{gated}");
    assert!(
        gated.get("remoteAttr").is_none(),
        "map gates the registration out: {gated}"
    );
    assert_eq!(map_header(&headers).as_deref(), Some(loc.as_str()));
}

/// 5.7.3.4: the temporal retrieve accepts the EntityMap flag and creates a
/// map holding the entity ("@none" for locally held data).
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_7_3_4_temporal_retrieve_entitymap_true_creates_map() {
    let st = state();
    create_vehicle(&st, "urn:ngsi-ld:Vehicle:t1", 30).await;

    let (status, headers, body) = get(
        &st,
        "/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:Vehicle:t1?timerel=after&timeAt=2000-01-01T00:00:00Z&timeproperty=createdAt&entityMap=true",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let loc = map_header(&headers).expect("NGSILD-EntityMap header on the temporal retrieve");
    let (status, _, map) = get(&st, &loc).await;
    assert_eq!(status, StatusCode::OK, "{map}");
    assert_eq!(map["entityMap"]["urn:ngsi-ld:Vehicle:t1"], json!(["@none"]));
}

/// 5.7.4.4: a live map fixes the temporal query's result set — Entities
/// outside the map must NOT be returned, and the map location is echoed.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_7_4_4_live_map_fixes_the_result_set() {
    let st = state();
    create_vehicle(&st, "urn:ngsi-ld:Vehicle:q1", 30).await;
    create_vehicle(&st, "urn:ngsi-ld:Vehicle:q2", 60).await;

    // a temporal map narrowed to q1
    let (status, headers, body) = get(
        &st,
        "/ngsi-ld/v1/temporal/entities?type=Vehicle&id=urn:ngsi-ld:Vehicle:q1&timerel=after&timeAt=2000-01-01T00:00:00Z&timeproperty=createdAt&entityMap=true",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let loc = map_header(&headers).expect("map location");

    // same query WITHOUT the id narrowing, but with the map in use
    let (status, headers, body) = get_with_map(
        &st,
        "/ngsi-ld/v1/temporal/entities?type=Vehicle&timerel=after&timeAt=2000-01-01T00:00:00Z&timeproperty=createdAt",
        &loc,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let arr = body.as_array().expect("array");
    let ids: Vec<&str> = arr
        .iter()
        .filter_map(|d| d.get("id").and_then(Value::as_str))
        .collect();
    assert_eq!(ids, vec!["urn:ngsi-ld:Vehicle:q1"], "{body}");
    assert!(
        !ids.contains(&"urn:ngsi-ld:Vehicle:q2"),
        "q2 is outside the map: {body}"
    );
    assert_eq!(map_header(&headers).as_deref(), Some(loc.as_str()));
}

/// 5.7.4.4: "If the resource cannot be found, or the data has expired, a
/// new EntityMap shall be created" — the temporal query recreates (201).
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_7_4_4_unknown_map_recreates_201() {
    let st = state();
    create_vehicle(&st, "urn:ngsi-ld:Vehicle:q3", 30).await;

    let (status, headers, body) = get_with_map(
        &st,
        "/ngsi-ld/v1/temporal/entities?type=Vehicle&timerel=after&timeAt=2000-01-01T00:00:00Z&timeproperty=createdAt",
        "urn:ngsi-ld:entitymap:gone",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let loc = map_header(&headers).expect("fresh map");
    assert!(!loc.contains(":gone"), "{loc}");
}
