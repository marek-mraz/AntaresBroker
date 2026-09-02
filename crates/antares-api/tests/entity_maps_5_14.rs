// SPDX-License-Identifier: EUPL-1.2
//! 5.14 Context Source Entity Mapping — wire-level tests through the router:
//! Retrieve/Update/Delete EntityMap (5.14.1-5.14.3, resource 6.32), Create
//! EntityMap for Query Entities (5.14.4, 6.34) and for the Temporal
//! Evolution (5.14.5, 6.35), plus the 5.5.14 NGSILD-EntityMap usage flow.

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

fn state() -> AppState {
    let mut st = AppState::new("antares-em".into());
    antares_api::wire(&mut st); // temporal auto-recording
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

/// Create an EntityMap over all Vehicles; returns (map path, map doc).
async fn create_map(st: &AppState) -> (String, Value) {
    let (status, headers, body) = get(&st.clone(), "/ngsi-ld/v1/entityMaps?type=Vehicle").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let loc = headers
        .get("NGSILD-EntityMap")
        .and_then(|v| v.to_str().ok())
        .expect("NGSILD-EntityMap header")
        .to_owned();
    (loc, body)
}

/// 5.14.4.4/.5: the created map lists every matching id under "@none", the
/// 5.2.39 shape is complete, and the NGSILD-EntityMap header names the
/// resource (6.34.3.1).
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_14_4_create_map_for_query() {
    let st = state();
    create_vehicle(&st, "urn:ngsi-ld:Vehicle:m1", 40).await;
    create_vehicle(&st, "urn:ngsi-ld:Vehicle:m2", 90).await;

    let (loc, map) = create_map(&st).await;
    assert!(loc.starts_with("/ngsi-ld/v1/entityMaps/"), "{loc}");
    assert_eq!(map["type"], "EntityMap", "{map}");
    assert!(map["id"].as_str().is_some_and(|i| i.contains(':')), "{map}");
    assert!(map["expiresAt"].is_string(), "{map}");
    let emap = map["entityMap"].as_object().expect("entityMap");
    assert_eq!(emap.len(), 2, "{map}");
    assert_eq!(emap["urn:ngsi-ld:Vehicle:m1"], json!(["@none"]));
    assert_eq!(emap["urn:ngsi-ld:Vehicle:m2"], json!(["@none"]));
    // no registrations involved → linkedMaps stays empty
    assert_eq!(map["linkedMaps"], json!({}), "{map}");

    // filters narrow the map (q applies unless split)
    let (status, _, narrowed) = get(&st, "/ngsi-ld/v1/entityMaps?type=Vehicle&q=speed%3E50").await;
    assert_eq!(status, StatusCode::CREATED);
    let emap = narrowed["entityMap"].as_object().expect("entityMap");
    assert_eq!(emap.len(), 1, "{narrowed}");
    assert!(emap.contains_key("urn:ngsi-ld:Vehicle:m2"));
    assert!(!emap.contains_key("urn:ngsi-ld:Vehicle:m1"), "{narrowed}");
}

/// 5.14.4.4: a query without type/attrs/q/georel/local is "too wide" → 400;
/// an invalid entityMapLifetime is 400.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_14_4_too_wide_and_bad_lifetime() {
    let st = state();
    let (status, _, body) = get(&st, "/ngsi-ld/v1/entityMaps?id=urn:ngsi-ld:Vehicle:m1").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(
        body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData",
        "{body}"
    );
    let (status, _, _) = get(
        &st,
        "/ngsi-ld/v1/entityMaps?type=Vehicle&entityMapLifetime=1hour",
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    // local=true qualifies on its own (5.14.4.4 e)
    let (status, _, _) = get(&st, "/ngsi-ld/v1/entityMaps?local=true").await;
    assert_eq!(status, StatusCode::CREATED);
}

/// 6.34.3.2: the POST form takes a 5.2.23 Query object.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_14_4_post_query_form() {
    let st = state();
    create_vehicle(&st, "urn:ngsi-ld:Vehicle:p1", 10).await;
    let body = json!({"type": "Query", "entities": [{"type": "Vehicle"}]}).to_string();
    let (status, headers, map) = send(
        &st,
        Request::builder()
            .method("POST")
            .uri("/ngsi-ld/v1/entityMaps")
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{map}");
    assert!(headers.get("NGSILD-EntityMap").is_some());
    assert!(
        map["entityMap"]
            .as_object()
            .is_some_and(|o| o.contains_key("urn:ngsi-ld:Vehicle:p1")),
        "{map}"
    );
    // a non-Query body is 400
    let bad = json!({"type": "NotAQuery"}).to_string();
    let (status, _, _) = send(
        &st,
        Request::builder()
            .method("POST")
            .uri("/ngsi-ld/v1/entityMaps")
            .header("Content-Type", "application/json")
            .header("Content-Length", bad.len())
            .body(Body::from(bad))
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// 5.14.1.4: invalid-URI id → 400, unknown id → 404, live id → the 5.2.39
/// document.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_14_1_retrieve() {
    let st = state();
    create_vehicle(&st, "urn:ngsi-ld:Vehicle:r1", 10).await;
    let (loc, map) = create_map(&st).await;

    let (status, _, body) = get(&st, &loc).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], map["id"]);
    assert_eq!(body["type"], "EntityMap");
    assert!(
        body["entityMap"]
            .as_object()
            .is_some_and(|o| o.contains_key("urn:ngsi-ld:Vehicle:r1")),
        "{body}"
    );

    let (status, _, body) = get(&st, "/ngsi-ld/v1/entityMaps/not%20a%20uri").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(
        body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData",
        "{body}"
    );

    let (status, _, body) = get(&st, "/ngsi-ld/v1/entityMaps/urn:ngsi-ld:entitymap:none").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        body["type"], "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound",
        "{body}"
    );
}

/// 5.14.2.4: the fragment's expiresAt is applied; output-only members
/// (5.2.39: entityMap, linkedMaps) are ignored even when provided.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_14_2_update() {
    let st = state();
    create_vehicle(&st, "urn:ngsi-ld:Vehicle:u1", 10).await;
    let (loc, _) = create_map(&st).await;

    let frag = json!({
        "expiresAt": "2099-01-01T00:00:00Z",
        "entityMap": {"urn:ngsi-ld:Vehicle:injected": ["@none"]},
        "linkedMaps": {"urn:reg": "urn:map"},
    })
    .to_string();
    let (status, _, body) = send(
        &st,
        Request::builder()
            .method("PATCH")
            .uri(&loc)
            .header("Content-Type", "application/json")
            .header("Content-Length", frag.len())
            .body(Body::from(frag))
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    let (_, _, body) = get(&st, &loc).await;
    // Table 6.4.3.2-1: "the actual expiresAt time of the EntityMap shall be
    // set by the Context Broker or Context Source, possibly overriding the
    // requested duration" — a year-2099 request is clamped to the ceiling.
    let stored = body["expiresAt"].as_str().expect("expiresAt");
    assert!(
        stored < "2099-01-01T00:00:00Z",
        "a requested lifetime beyond the broker ceiling must be overridden: {body}"
    );
    assert!(
        stored > "2026-08-17T00:00:00Z",
        "the clamp must still leave the map usable: {body}"
    );
    // the output-only members were NOT overwritten by the client
    assert!(
        !body["entityMap"]
            .as_object()
            .is_some_and(|o| o.contains_key("urn:ngsi-ld:Vehicle:injected")),
        "{body}"
    );
    assert_eq!(body["linkedMaps"], json!({}), "{body}");

    // a non-DateTime expiresAt is 400 (4.6.3)
    let bad = json!({"expiresAt": "tomorrow"}).to_string();
    let (status, _, _) = send(
        &st,
        Request::builder()
            .method("PATCH")
            .uri(&loc)
            .header("Content-Type", "application/json")
            .header("Content-Length", bad.len())
            .body(Body::from(bad))
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // unknown id → 404
    let frag = json!({"expiresAt": "2099-01-01T00:00:00Z"}).to_string();
    let (status, _, _) = send(
        &st,
        Request::builder()
            .method("PATCH")
            .uri("/ngsi-ld/v1/entityMaps/urn:ngsi-ld:entitymap:none")
            .header("Content-Type", "application/json")
            .header("Content-Length", frag.len())
            .body(Body::from(frag))
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// 5.14.3.4: delete removes the map (204/404), invalid id → 400.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_14_3_delete() {
    let st = state();
    create_vehicle(&st, "urn:ngsi-ld:Vehicle:d1", 10).await;
    let (loc, _) = create_map(&st).await;

    let del = |uri: String| {
        let st = st.clone();
        async move {
            send(
                &st,
                Request::builder()
                    .method("DELETE")
                    .uri(uri)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
        }
    };
    let (status, _, body) = del(loc.clone()).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(body, Value::Null);
    let (status, _, _) = get(&st, &loc).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, _) = del(loc).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, _) = del("/ngsi-ld/v1/entityMaps/not%20a%20uri".into()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// 5.14.3.4: "If the NGSI-LD endpoint does not know about a matching
/// EntityMap for the EntityMap ID, then an error of type ResourceNotFound
/// shall be raised." An expired map is one the endpoint does not know about —
/// 5.5.14 puts it beyond access and the retrieve answers 404 for it — so the
/// delete of the same id at the same instant answers 404 too. A 204 there
/// tells the client it deleted a map the broker had already stopped serving.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_14_3_delete_of_an_expired_map_is_not_found() {
    let st = state();
    create_vehicle(&st, "urn:ngsi-ld:Vehicle:d2", 10).await;
    let (loc, _) = create_map(&st).await;

    // 5.14.2.4 sets the expiry; nothing requires it to be in the future, and
    // a past instant is the shortest way to an expired map over the wire.
    let frag = json!({"expiresAt": "1970-01-01T00:00:00Z"}).to_string();
    let (status, _, _) = send(
        &st,
        Request::builder()
            .method("PATCH")
            .uri(&loc)
            .header("Content-Type", "application/json")
            .header("Content-Length", frag.len().to_string())
            .body(Body::from(frag))
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // the delete comes FIRST: a retrieve prunes the row on touch, so asking
    // for it beforehand would hide the answer this test is about.
    let (status, _, _) = send(
        &st,
        Request::builder()
            .method("DELETE")
            .uri(&loc)
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "the delete agrees with the retrieve about what the broker knows"
    );
    let (status, _, _) = get(&st, &loc).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "an expired map is not served"
    );
}

/// 6.4.3.2: entityMap=true on the entity query → 201 + NGSILD-EntityMap
/// header, and the entity payload is still the query result.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_14_4_entity_map_true_on_query() {
    let st = state();
    create_vehicle(&st, "urn:ngsi-ld:Vehicle:q1", 10).await;
    let (status, headers, body) =
        get(&st, "/ngsi-ld/v1/entities?type=Vehicle&entityMap=true").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let loc = headers
        .get("NGSILD-EntityMap")
        .and_then(|v| v.to_str().ok())
        .expect("NGSILD-EntityMap header")
        .to_owned();
    let arr = body.as_array().expect("entity list");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["id"], "urn:ngsi-ld:Vehicle:q1");
    // the created map is retrievable and lists the entity
    let (status, _, map) = get(&st, &loc).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        map["entityMap"]
            .as_object()
            .is_some_and(|o| o.contains_key("urn:ngsi-ld:Vehicle:q1")),
        "{map}"
    );
    // without entityMap=true the query keeps its 200 and carries no header
    let (status, headers, _) = get(&st, "/ngsi-ld/v1/entities?type=Vehicle").await;
    assert_eq!(status, StatusCode::OK);
    assert!(headers.get("NGSILD-EntityMap").is_none());
}

/// 5.5.14: a query referencing a live EntityMap is fixed to the map's
/// Entities, the header echoes the map, and local entries that stopped
/// matching are pruned by the creator; an unknown map leads to a new one.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_5_14_entity_map_usage() {
    let st = state();
    create_vehicle(&st, "urn:ngsi-ld:Vehicle:s1", 10).await;
    create_vehicle(&st, "urn:ngsi-ld:Vehicle:s2", 20).await;
    let (loc, map) = create_map(&st).await;
    assert_eq!(map["entityMap"].as_object().expect("obj").len(), 2);

    // delete one entity: the map still lists it until the next processing
    let (status, _, _) = send(
        &st,
        Request::builder()
            .method("DELETE")
            .uri("/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:s2")
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // query via the map: only s1 comes back, header echoes the map
    let (status, headers, body) = send(
        &st,
        Request::builder()
            .method("GET")
            .uri("/ngsi-ld/v1/entities?type=Vehicle")
            .header("NGSILD-EntityMap", &loc)
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        headers
            .get("NGSILD-EntityMap")
            .and_then(|v| v.to_str().ok()),
        Some(loc.as_str())
    );
    let arr = body.as_array().expect("entity list");
    assert_eq!(arr.len(), 1, "{body}");
    assert_eq!(arr[0]["id"], "urn:ngsi-ld:Vehicle:s1");

    // 5.5.14 pruning: the creator removed the no-longer-matching entry
    let (_, _, map) = get(&st, &loc).await;
    let emap = map["entityMap"].as_object().expect("obj");
    assert!(!emap.contains_key("urn:ngsi-ld:Vehicle:s2"), "{map}");
    assert!(emap.contains_key("urn:ngsi-ld:Vehicle:s1"), "{map}");

    // unknown/expired map: a NEW one is created (201 + fresh header)
    let (status, headers, _) = send(
        &st,
        Request::builder()
            .method("GET")
            .uri("/ngsi-ld/v1/entities?type=Vehicle")
            .header(
                "NGSILD-EntityMap",
                "/ngsi-ld/v1/entityMaps/urn:ngsi-ld:entitymap:gone",
            )
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let fresh = headers
        .get("NGSILD-EntityMap")
        .and_then(|v| v.to_str().ok())
        .expect("fresh map header");
    assert!(!fresh.ends_with("urn:ngsi-ld:entitymap:gone"), "{fresh}");
}

/// 5.14.5.4: the temporal EntityMap requires a temporal query (400 without);
/// with one, the S1-S4 candidates land in the map (201 + header).
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_14_5_temporal_map() {
    let st = state();
    // creating an entity auto-records its temporal evolution
    create_vehicle(&st, "urn:ngsi-ld:Vehicle:t1", 10).await;

    let (status, _, body) = get(&st, "/ngsi-ld/v1/temporal/entityMaps?type=Vehicle").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(
        body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData",
        "{body}"
    );

    let (status, headers, map) = get(
        &st,
        "/ngsi-ld/v1/temporal/entityMaps?type=Vehicle&timerel=before&timeAt=2999-01-01T00:00:00Z&timeproperty=createdAt",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{map}");
    assert!(headers.get("NGSILD-EntityMap").is_some());
    assert_eq!(map["type"], "EntityMap");
    assert!(
        map["entityMap"]
            .as_object()
            .is_some_and(|o| o.contains_key("urn:ngsi-ld:Vehicle:t1")),
        "{map}"
    );
    // an entity with no instances in the window is NOT a candidate
    let (status, _, map) = get(
        &st,
        "/ngsi-ld/v1/temporal/entityMaps?type=Vehicle&timerel=before&timeAt=2000-01-01T00:00:00Z&timeproperty=createdAt",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(map["entityMap"], json!({}), "{map}");
}

/// 5.14.4.4 (distributed): a matching registration supporting
/// createEntityMapQueryEntity is forwarded to; the ids of its returned
/// EntityMap join the local map attributed to the registration, and
/// linkedMaps records the remote map id.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_14_4_federated_map_merge() {
    antares_jsonld::allow_private_egress(true);
    let st = state();
    create_vehicle(&st, "urn:ngsi-ld:Vehicle:localf", 10).await;

    // canned Context Source answering with its own EntityMap
    let remote_map = serde_json::json!({
        "id": "urn:ngsi-ld:entitymap:remote-1",
        "type": "EntityMap",
        "expiresAt": "2999-01-01T00:00:00Z",
        "entityMap": {"urn:ngsi-ld:Vehicle:remotef": ["@none"]},
        "linkedMaps": {},
    })
    .to_string();
    let reply = format!(
        "HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{remote_map}",
        remote_map.len()
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
    let reg = serde_json::json!({
        "id": "urn:ngsi-ld:ContextSourceRegistration:em-fed",
        "type": "ContextSourceRegistration",
        "operations": ["createEntityMapQueryEntity"],
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

    let (status, _, map) = get(&st, "/ngsi-ld/v1/entityMaps?type=Vehicle").await;
    assert_eq!(status, StatusCode::CREATED, "{map}");
    let emap = map["entityMap"].as_object().expect("entityMap");
    assert_eq!(
        emap["urn:ngsi-ld:Vehicle:localf"],
        json!(["@none"]),
        "{map}"
    );
    assert_eq!(
        emap["urn:ngsi-ld:Vehicle:remotef"],
        json!(["urn:ngsi-ld:ContextSourceRegistration:em-fed"]),
        "{map}"
    );
    // a remote entity is not "@none"
    assert_ne!(emap["urn:ngsi-ld:Vehicle:remotef"], json!(["@none"]));
    assert_eq!(
        map["linkedMaps"]["urn:ngsi-ld:ContextSourceRegistration:em-fed"],
        "urn:ngsi-ld:entitymap:remote-1",
        "{map}"
    );
}

/// 5.5.14 makes a map usable only while a READABLE expiry is still in the
/// future, and every route reads through that one condition. A row the store
/// hands back in a shape no map can have — a plugin store, a corrupted file,
/// an older writer — therefore reads as absent rather than as a map with
/// missing parts, and no route indexes it as an object on the way. This
/// pins the whole class at the route, so a future reader that judged
/// "expired" instead of "live" would surface here as a panic or a 204.
#[tokio::test(flavor = "multi_thread")]
async fn a_stored_map_of_the_wrong_shape_reads_as_absent() {
    use antares_model::TenantId;
    use antares_store::Kind;

    for (n, stored) in [
        json!("not an object"),
        json!([1, 2, 3]),
        json!(7),
        json!(true),
        Value::Null,
    ]
    .into_iter()
    .enumerate()
    {
        let st = state();
        let id = format!("urn:ngsi-ld:entitymap:shape-{n}");
        st.store
            .create(&TenantId::default(), Kind::EntityMap, &id, stored.clone())
            .expect("seed the row");

        let body = json!({"expiresAt": "2099-01-01T00:00:00.000Z"}).to_string();
        let (status, _, resp) = send(
            &st,
            Request::builder()
                .method("PATCH")
                .uri(format!("/ngsi-ld/v1/entityMaps/{id}"))
                .header("Content-Type", "application/json")
                .header("Content-Length", body.len())
                .body(Body::from(body))
                .expect("request"),
        )
        .await;
        assert!(
            status.is_client_error() || status.is_server_error(),
            "{stored}: an unusable row was reported as updated: {status} {resp}"
        );
        assert!(
            resp["type"]
                .as_str()
                .is_some_and(|t| t.starts_with("https://uri.etsi.org/ngsi-ld/errors/")),
            "{stored}: the refusal is an NGSI-LD ProblemDetails: {resp}"
        );
    }
}

/// The other side: a well-formed map takes the update, and 5.5.14's lifetime
/// ceiling binds the new expiry — a client asking for one far in the future
/// gets the ceiling, not a refusal and not the value it asked for.
#[tokio::test(flavor = "multi_thread")]
async fn a_well_formed_map_takes_the_update_under_the_lifetime_ceiling() {
    use antares_model::TenantId;
    use antares_store::Kind;

    let st = state();
    let id = "urn:ngsi-ld:entitymap:shape-ok";
    st.store
        .create(
            &TenantId::default(),
            Kind::EntityMap,
            id,
            json!({"id": id, "type": "EntityMap",
                   "expiresAt": "2098-01-01T00:00:00.000Z",
                   "entityMap": {}, "linkedMaps": {}}),
        )
        .expect("seed the row");

    let body = json!({"expiresAt": "2099-01-01T00:00:00.000Z"}).to_string();
    let (status, _, resp) = send(
        &st,
        Request::builder()
            .method("PATCH")
            .uri(format!("/ngsi-ld/v1/entityMaps/{id}"))
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{resp}");

    let (status, _, got) = get(&st, &format!("/ngsi-ld/v1/entityMaps/{id}")).await;
    assert_eq!(status, StatusCode::OK, "{got}");
    let stamp = got["expiresAt"].as_str().expect("an expiry");
    assert_ne!(
        stamp, "2098-01-01T00:00:00.000Z",
        "the update did not reach the row: {got}"
    );
    assert!(
        stamp < "2099-01-01T00:00:00.000Z",
        "5.5.14: the lifetime ceiling binds every writer, so a far-future \
         expiry comes back clamped: {got}"
    );
}
