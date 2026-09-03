// SPDX-License-Identifier: EUPL-1.2
//! What a Context Source is allowed to put into this broker's EntityMap.
//!
//! 5.14.4.4: "For each matching Context Source Registration, the request is
//! forwarded for remote querying by matching endpoints. The result of each
//! remote query is an EntityMap. The mapping between the Context Source
//! Registration and the EntityMap Id is added to the linkedMaps element of
//! the local EntityMap and for the Entity ids included in the returned
//! Entity Maps a mapping to the Context Source Registration is added to the
//! entityMap element of the local EntityMap. The local EntityMap is stored
//! and made accessible based on its identifier."
//!
//! What travels is therefore a peer's payload, and Table 5.2.39-2 says what
//! it may contain: `entityMap` is "a set of key-value pairs whose keys shall
//! be strings representing Entity ids", `linkedMaps` values "shall represent
//! the associated EntityMap id", and Table 5.2.39-1 restricts an EntityMap
//! id to a valid URI. The merged document is stored under this broker's own
//! id and served from `/entityMaps/{id}` as this broker's answer, and each
//! of its keys is what a later paged read fetches (5.5.9.3) and interpolates
//! into a forwarded path — so a key the peer invented is a key this broker
//! then vouches for.

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use std::io::{Read, Write};
use tower::ServiceExt;

/// A Context Source answering every request with one canned EntityMap.
fn mock_source(map: &Value) -> u16 {
    let payload = map.to_string();
    let reply = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{payload}",
        payload.len()
    );
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let mut buf = [0u8; 65536];
            let _ = s.read(&mut buf);
            let _ = s.write_all(reply.as_bytes());
        }
    });
    port
}

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

/// A registration pointing at `port` that offers the operation 5.14.4
/// forwards (4.20 `createEntityMapQueryEntity`).
async fn register(st: &AppState, port: u16) -> String {
    let reg_id = format!("urn:ngsi-ld:ContextSourceRegistration:emshape-{port}");
    let body = json!({
        "id": reg_id,
        "type": "ContextSourceRegistration",
        "mode": "inclusive",
        "operations": ["createEntityMapQueryEntity"],
        "information": [{"entities": [{"type": "Vehicle"}]}],
        "endpoint": format!("http://127.0.0.1:{port}"),
    })
    .to_string();
    let (status, b) = send(
        st,
        Request::builder()
            .method("POST")
            .uri("/ngsi-ld/v1/csourceRegistrations")
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "registration create: {b}");
    reg_id
}

/// GET /entityMaps over the registered type — the 5.14.4 fan-out.
async fn create_map(st: &AppState) -> Value {
    let (status, body) = send(
        st,
        Request::builder()
            .method("GET")
            .uri("/ngsi-ld/v1/entityMaps?type=Vehicle")
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "create map: {body}");
    body
}

fn keys(map: &Value) -> Vec<String> {
    map.get("entityMap")
        .and_then(Value::as_object)
        .expect("entityMap object")
        .keys()
        .cloned()
        .collect()
}

/// Table 5.2.39-2: the keys "shall be strings representing Entity ids". A
/// peer that answers something else does not get to name a key of the map
/// this broker stores — the good ids in the same response still merge, so
/// the check is per key, not per peer.
#[tokio::test(flavor = "multi_thread")]
async fn a_peer_key_that_is_not_an_entity_id_never_enters_the_local_map() {
    antares_jsonld::allow_private_egress(true);
    let st = AppState::new("antares-emshape".into());
    let port = mock_source(&json!({
        "id": "urn:ngsi-ld:entitymap:peer-1",
        "type": "EntityMap",
        "entityMap": {
            "urn:ngsi-ld:Vehicle:good": ["@none"],
            "not a uri": ["@none"],
            "": ["@none"],
            "urn:ngsi-ld:Vehicle:../../csourceRegistrations": ["@none"],
        },
        "linkedMaps": {},
    }));
    let reg_id = register(&st, port).await;
    let map = create_map(&st).await;

    assert_eq!(
        map["entityMap"]["urn:ngsi-ld:Vehicle:good"],
        json!([reg_id]),
        "the peer's valid id must still merge: {map}"
    );
    let got = keys(&map);
    assert_eq!(
        got,
        vec!["urn:ngsi-ld:Vehicle:good".to_owned()],
        "a key that is not an Entity id entered the stored map: {map}"
    );
}

/// The peer's own "held locally" marker is about the PEER. Merged as a key
/// it would claim this broker holds an Entity called `@none`, and there is
/// no Entity id it could stand for on this side.
#[tokio::test(flavor = "multi_thread")]
async fn a_peer_s_at_none_marker_is_not_an_entity_of_this_broker() {
    antares_jsonld::allow_private_egress(true);
    let st = AppState::new("antares-emshape".into());
    let port = mock_source(&json!({
        "id": "urn:ngsi-ld:entitymap:peer-2",
        "type": "EntityMap",
        "entityMap": {"@none": ["urn:ngsi-ld:ContextSourceRegistration:deeper"]},
        "linkedMaps": {},
    }));
    register(&st, port).await;
    let map = create_map(&st).await;
    assert!(
        !keys(&map).iter().any(|k| k == "@none"),
        "the peer's local-hold marker became a key of this broker's map: {map}"
    );
}

/// Table 5.2.39-1 restricts an EntityMap id to a valid URI, and 5.14.1.4
/// raises BadRequestData for one that is not. A peer's id travels back out
/// as the `NGSILD-EntityMap` header of every later forwarded page, so an id
/// this broker would refuse from a client is not one it stores from a peer.
#[tokio::test(flavor = "multi_thread")]
async fn a_peer_map_id_that_is_not_a_uri_never_enters_linked_maps() {
    antares_jsonld::allow_private_egress(true);
    let st = AppState::new("antares-emshape".into());
    let port = mock_source(&json!({
        "id": "not a uri",
        "type": "EntityMap",
        "entityMap": {"urn:ngsi-ld:Vehicle:good": ["@none"]},
        "linkedMaps": {},
    }));
    let reg_id = register(&st, port).await;
    let map = create_map(&st).await;

    assert_eq!(
        map["entityMap"]["urn:ngsi-ld:Vehicle:good"],
        json!([reg_id]),
        "the entity ids still merge without a usable map id: {map}"
    );
    assert_eq!(
        map["linkedMaps"],
        json!({}),
        "an EntityMap id this broker would refuse from a client was stored: {map}"
    );
}

/// The local half of the map is truncated to the broker's `max_limit`
/// (`entities.rs`, 5.14.4.4), so one Context Source does not get to be
/// larger than the broker itself: the same ceiling bounds what a peer
/// contributes. Without it the only bound is the 16 MiB forwarded-response
/// ceiling — hundreds of thousands of keys per peer, per registration,
/// stored for the map's whole lifetime.
#[tokio::test(flavor = "multi_thread")]
async fn one_peer_contributes_at_most_the_brokers_own_ceiling() {
    antares_jsonld::allow_private_egress(true);
    let st = AppState::new("antares-emshape".into());
    let ceiling = st.max_limit;
    let mut entries = serde_json::Map::new();
    for i in 0..ceiling + 50 {
        entries.insert(
            format!("urn:ngsi-ld:Vehicle:flood-{i:06}"),
            json!(["@none"]),
        );
    }
    let port = mock_source(&json!({
        "id": "urn:ngsi-ld:entitymap:peer-3",
        "type": "EntityMap",
        "entityMap": Value::Object(entries),
        "linkedMaps": {},
    }));
    register(&st, port).await;
    let map = create_map(&st).await;
    assert_eq!(
        keys(&map).len(),
        ceiling,
        "one peer wrote past the broker's own map ceiling of {ceiling}"
    );
}
