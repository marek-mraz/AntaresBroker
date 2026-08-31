// SPDX-License-Identifier: EUPL-1.2
//! 5.5.9.3 Pagination with Entity maps — the paged federated fetch.
//!
//! "In the case of queries based on Entity maps, the set of Entities
//! considered for the result is fixed with the initial query … filters shall
//! be rechecked before returning results … Entities not or no longer fitting
//! the query shall be removed from the Entity map during pagination. Pages
//! shall always be filled to the maximum, as long as Entities are available."
//!
//! The point of the paged fetch: a query referencing a map with N candidate
//! ids must NOT fetch (locally or via forwards) all N at once — only the
//! page's chunk. Memory per request is O(limit), not O(map).

use antares_api::AppState;
use axum::body::Body;
use axum::http::Request;
use serde_json::Value;
use std::io::{Read, Write};
use std::sync::Arc;
use tower::ServiceExt;

/// Mock Context Source answering `[]` and recording every request head.
struct Mock {
    port: u16,
    heads: Arc<std::sync::Mutex<Vec<String>>>,
}

fn mock_source() -> Mock {
    let reply = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
                 Content-Length: 2\r\nConnection: close\r\n\r\n[]";
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let heads: Arc<std::sync::Mutex<Vec<String>>> = Arc::default();
    let sink = heads.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let mut buf = [0u8; 65536];
            let n = s.read(&mut buf).unwrap_or(0);
            sink.lock().expect("lock").push(
                String::from_utf8_lossy(&buf[..n])
                    .split("\r\n\r\n")
                    .next()
                    .unwrap_or_default()
                    .to_owned(),
            );
            let _ = s.write_all(reply.as_bytes());
        }
    });
    Mock { port, heads }
}

async fn send(st: &AppState, req: Request<Body>) -> (axum::http::response::Parts, String) {
    let res = antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("response");
    let (parts, body) = res.into_parts();
    let bytes = axum::body::to_bytes(body, usize::MAX).await.expect("body");
    (parts, String::from_utf8_lossy(&bytes).into_owned())
}

async fn create_vehicle(st: &AppState, id: &str) {
    let body = serde_json::json!({
        "id": id,
        "type": "Vehicle",
        // observedAt so the default ANTARES_TEMPORAL_RECORD=observed gate
        // admits the instance and the temporal query has history to answer
        "speed": {"type": "Property", "value": 50, "observedAt": "2026-01-01T00:00:00Z"},
    })
    .to_string();
    let req = Request::builder()
        .method("POST")
        .uri("/ngsi-ld/v1/entities")
        .header("Content-Type", "application/json")
        .header("Content-Length", body.len())
        .body(Body::from(body))
        .expect("request");
    let (parts, b) = send(st, req).await;
    assert_eq!(parts.status, 201, "entity create: {b}");
}

async fn get(st: &AppState, uri: &str, map: Option<&str>) -> (axum::http::response::Parts, String) {
    let mut req = Request::builder().method("GET").uri(uri);
    if let Some(m) = map {
        req = req.header("NGSILD-EntityMap", m);
    }
    send(st, req.body(Body::empty()).expect("request")).await
}

/// Create N entities, build an EntityMap over them, return the map ref.
async fn seed_with_map(st: &AppState, n: usize) -> String {
    for i in 0..n {
        create_vehicle(st, &format!("urn:ngsi-ld:Vehicle:pg-{i:03}")).await;
    }
    let (parts, _) = get(st, "/ngsi-ld/v1/entities?type=Vehicle&entityMap=true", None).await;
    assert_eq!(parts.status, 201, "entityMap=true answers 201");
    parts
        .headers
        .get("NGSILD-EntityMap")
        .and_then(|v| v.to_str().ok())
        .expect("map header")
        .to_owned()
}

fn entity_count(body: &str) -> usize {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v.as_array().map(Vec::len))
        .unwrap_or(0)
}

/// THE paged-fetch property: with a 30-id map and limit=5, no single fetch —
/// local or forwarded — may name the whole map. Every forwarded request's id
/// list stays within one chunk (limit.max(20)), so memory is O(page).
#[tokio::test(flavor = "multi_thread")]
async fn map_paging_fetches_only_the_pages_chunk() {
    std::env::set_var("ANTARES_EGRESS_ALLOW_PRIVATE", "true");
    let st = AppState::new("antares1".into());
    let m = mock_source();
    let body = serde_json::json!({
        "id": format!("urn:ngsi-ld:ContextSourceRegistration:mappage-{}", m.port),
        "type": "ContextSourceRegistration",
        "mode": "inclusive",
        "operations": ["queryEntity"],
        "information": [{"entities": [{"type": "Vehicle"}]}],
        "endpoint": format!("http://127.0.0.1:{}", m.port),
    })
    .to_string();
    let req = Request::builder()
        .method("POST")
        .uri("/ngsi-ld/v1/csourceRegistrations")
        .header("Content-Type", "application/json")
        .header("Content-Length", body.len())
        .body(Body::from(body))
        .expect("request");
    let (parts, b) = send(&st, req).await;
    assert_eq!(parts.status, 201, "registration create: {b}");

    let map_ref = seed_with_map(&st, 30).await;
    m.heads.lock().expect("lock").clear();

    let (parts, body) = get(
        &st,
        "/ngsi-ld/v1/entities?type=Vehicle&limit=5",
        Some(&map_ref),
    )
    .await;
    assert_eq!(parts.status, 200);
    assert_eq!(entity_count(&body), 5, "page filled to the maximum: {body}");

    let heads = m.heads.lock().expect("lock").clone();
    assert!(!heads.is_empty(), "the map page fetch must still federate");
    for head in &heads {
        let ids = head
            .split("id=")
            .nth(1)
            .map(|rest| {
                let list = rest.split(&[' ', '&'][..]).next().unwrap_or_default();
                list.matches("Vehicle%3Apg-").count() + list.matches("Vehicle:pg-").count()
            })
            .unwrap_or(0);
        assert!(
            ids <= 20,
            "a forwarded fetch named {ids} ids — the whole map instead of one chunk: {head}"
        );
    }
}

/// Pages are filled to the maximum and the next/prev links walk the map.
#[tokio::test(flavor = "multi_thread")]
async fn map_paging_fills_pages_and_links() {
    let st = AppState::new("antares1".into());
    let map_ref = seed_with_map(&st, 5).await;

    let (parts, body) = get(
        &st,
        "/ngsi-ld/v1/entities?type=Vehicle&limit=2",
        Some(&map_ref),
    )
    .await;
    assert_eq!(parts.status, 200);
    assert_eq!(entity_count(&body), 2);
    let links: Vec<String> = parts
        .headers
        .get_all(axum::http::header::LINK)
        .iter()
        .filter_map(|v| v.to_str().ok().map(str::to_owned))
        .collect();
    assert!(
        links
            .iter()
            .any(|l| l.contains("rel=\"next\"") && l.contains("offset=2")),
        "next link with offset=2 expected: {links:?}"
    );

    let (parts, body) = get(
        &st,
        "/ngsi-ld/v1/entities?type=Vehicle&limit=2&offset=4",
        Some(&map_ref),
    )
    .await;
    assert_eq!(parts.status, 200);
    assert_eq!(
        entity_count(&body),
        1,
        "last page has the remainder: {body}"
    );
    let links: Vec<String> = parts
        .headers
        .get_all(axum::http::header::LINK)
        .iter()
        .filter_map(|v| v.to_str().ok().map(str::to_owned))
        .collect();
    assert!(
        !links.iter().any(|l| l.contains("rel=\"next\"")),
        "no next past the last page: {links:?}"
    );
    assert!(
        links
            .iter()
            .any(|l| l.contains("rel=\"prev\"") && l.contains("offset=2")),
        "prev link with offset=2 expected: {links:?}"
    );
}

/// 5.5.9.3: an Entity deleted after map creation no longer fits the query —
/// paging over it prunes it from the map and the page still fills to the
/// maximum from the remaining candidates.
#[tokio::test(flavor = "multi_thread")]
async fn map_paging_prunes_stale_entities_and_fills_pages() {
    let st = AppState::new("antares1".into());
    let map_ref = seed_with_map(&st, 5).await;
    let victim = "urn:ngsi-ld:Vehicle:pg-001";

    let req = Request::builder()
        .method("DELETE")
        .uri(format!("/ngsi-ld/v1/entities/{victim}"))
        .body(Body::empty())
        .expect("request");
    let (parts, _) = send(&st, req).await;
    assert_eq!(parts.status, 204);

    let (parts, body) = get(
        &st,
        "/ngsi-ld/v1/entities?type=Vehicle&limit=3",
        Some(&map_ref),
    )
    .await;
    assert_eq!(parts.status, 200);
    assert_eq!(
        entity_count(&body),
        3,
        "page fills past the stale entry: {body}"
    );
    assert!(!body.contains(victim), "deleted entity must not be served");

    // the visited stale entry is gone from the map itself
    let map_id = map_ref.rsplit('/').next().expect("id");
    let (parts, map_body) = get(&st, &format!("/ngsi-ld/v1/entityMaps/{map_id}"), None).await;
    assert_eq!(parts.status, 200);
    assert!(
        !map_body.contains(victim),
        "pruned from the map during pagination: {map_body}"
    );
}

/// count=true over a map walks every candidate: the count is the matching
/// total, not the page size.
#[tokio::test(flavor = "multi_thread")]
async fn map_paging_count_is_the_matching_total() {
    let st = AppState::new("antares1".into());
    let map_ref = seed_with_map(&st, 7).await;

    let (parts, body) = get(
        &st,
        "/ngsi-ld/v1/entities?type=Vehicle&limit=2&count=true",
        Some(&map_ref),
    )
    .await;
    assert_eq!(parts.status, 200);
    assert_eq!(entity_count(&body), 2);
    let count = parts
        .headers
        .get("NGSILD-Results-Count")
        .and_then(|v| v.to_str().ok());
    assert_eq!(count, Some("7"), "count walks the whole map");
}

/// Read the map document back through the API and return its candidate ids.
async fn map_ids(st: &AppState, map_ref: &str) -> Vec<String> {
    let (parts, body) = get(st, map_ref, None).await;
    assert_eq!(parts.status, 200, "retrieve EntityMap: {body}");
    serde_json::from_str::<Value>(&body)
        .ok()
        .and_then(|v| v["entityMap"].as_object().cloned())
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default()
}

fn ids_of(body: &str) -> Vec<String> {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|e| e["id"].as_str().map(str::to_owned))
        .collect()
}

/// 5.5.9.3: "the set of Entities considered for the result is fixed with the
/// initial query creating the Entity map" — the map is the CANDIDATE set and
/// the request's own filters narrow it. A request that names `id=` therefore
/// asks for the intersection: the answer holds only the named Entity, and the
/// Entities it did not ask about stay in the map for the next page. Judging
/// them against a filter they were never in scope for and deleting them
/// ("Entities not or no longer fitting the query shall be removed") destroys
/// the cursor for every later request that references the same map.
#[tokio::test(flavor = "multi_thread")]
async fn a_narrowed_request_neither_widens_the_answer_nor_empties_the_map() {
    // wire() installs the temporal auto-record hook, so the temporal half of
    // this test has history to answer from
    let mut st = AppState::new("antares-map-narrow".into());
    antares_api::notify::wire(&mut st);
    let map_ref = seed_with_map(&st, 4).await;
    let before = map_ids(&st, &map_ref).await;
    assert_eq!(before.len(), 4, "{before:?}");
    let one = "urn:ngsi-ld:Vehicle:pg-001";

    for path in [
        "/ngsi-ld/v1/entities?type=Vehicle",
        "/ngsi-ld/v1/temporal/entities?type=Vehicle&timerel=after&timeAt=1970-01-01T00:00:00Z",
    ] {
        let (parts, body) = get(&st, &format!("{path}&id={one}"), Some(&map_ref)).await;
        assert_eq!(parts.status, 200, "{path}: {body}");
        assert_eq!(
            ids_of(&body),
            vec![one.to_owned()],
            "{path} must answer the intersection of the map and the request's id="
        );
        let mut after = map_ids(&st, &map_ref).await;
        after.sort();
        assert_eq!(
            after, before,
            "{path} pruned Entities the request never asked about"
        );
    }
}
