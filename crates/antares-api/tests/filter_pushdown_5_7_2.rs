// SPDX-License-Identifier: EUPL-1.2
//! 5.7.2.4 filter forwarding on distributed queries.
//!
//! The clause splits on the split-entities setting: "If split entities flag
//! is … true or … the deployment allows split entities, the filters (filter
//! conditions specified by the query, geospatial restrictions imposed by the
//! geoquery, Scope query, Attributes) shall be removed before forwarding the
//! request" — and in the non-split case (this deployment's default) the
//! request is forwarded WITH its filters, so a peer holding 50k entities
//! returns its filtered subset instead of everything. 5.7.4.4 mirrors the
//! same rule for temporal queries. The local re-check on the merged result
//! stays in place either way.

use antares_api::AppState;
use axum::body::Body;
use axum::http::Request;
use std::io::{Read, Write};
use std::sync::Arc;
use tower::ServiceExt;

const REMOTE_ID: &str = "urn:ngsi-ld:Vehicle:pushdown";

/// Mock Context Source: replies one entity to every request and keeps the
/// last request it saw — head (request line + headers) and body, so both the
/// query-parameter forward and the 5.2.23 Query body can be asserted.
struct Mock {
    port: u16,
    last_head: Arc<std::sync::Mutex<String>>,
    last_body: Arc<std::sync::Mutex<String>>,
}

fn mock_source(temporal: bool) -> Mock {
    let entity = if temporal {
        serde_json::json!([{
            "id": REMOTE_ID,
            "type": "Vehicle",
            "speed": [{"type": "Property", "value": 90, "observedAt": "2026-08-01T00:00:00Z"}],
        }])
    } else {
        // satisfies every filter the push-down tests forward (q on speed,
        // near-point geoquery, scopeQ=/A) so the local re-check keeps it
        serde_json::json!([{
            "id": REMOTE_ID,
            "type": "Vehicle",
            "scope": "/A",
            "speed": {"type": "Property", "value": 90},
            "location": {
                "type": "GeoProperty",
                "value": {"type": "Point", "coordinates": [8.6, 41.2]},
            },
        }])
    };
    let body = entity.to_string();
    let reply = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let last_head: Arc<std::sync::Mutex<String>> = Arc::default();
    let last_body: Arc<std::sync::Mutex<String>> = Arc::default();
    let head = last_head.clone();
    let body_slot = last_body.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            // read until the declared body has arrived — a POST body may land
            // in a later segment than its headers
            let mut raw = Vec::new();
            let mut buf = [0u8; 8192];
            loop {
                let n = s.read(&mut buf).unwrap_or(0);
                if n == 0 {
                    break;
                }
                raw.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&raw).into_owned();
                let Some((h, b)) = text.split_once("\r\n\r\n") else {
                    continue;
                };
                let want: usize = h
                    .lines()
                    .find_map(|l| {
                        l.strip_prefix("content-length:")
                            .or_else(|| l.strip_prefix("Content-Length:"))
                    })
                    .and_then(|v| v.trim().parse().ok())
                    .unwrap_or(0);
                if b.len() >= want {
                    break;
                }
            }
            let text = String::from_utf8_lossy(&raw).into_owned();
            let (h, b) = text.split_once("\r\n\r\n").unwrap_or((text.as_str(), ""));
            *head.lock().expect("lock") = h.to_owned();
            *body_slot.lock().expect("lock") = b.to_owned();
            let _ = s.write_all(reply.as_bytes());
        }
    });
    Mock {
        port,
        last_head,
        last_body,
    }
}

async fn register(
    st: &AppState,
    port: u16,
    ops: &[&str],
    csi: Option<serde_json::Value>,
    props: Option<&[&str]>,
) {
    let mut info = serde_json::json!({"entities": [{"type": "Vehicle"}]});
    if let Some(names) = props {
        info["propertyNames"] = serde_json::json!(names);
    }
    let mut doc = serde_json::json!({
        "id": format!("urn:ngsi-ld:ContextSourceRegistration:pushdown-{port}"),
        "type": "ContextSourceRegistration",
        "mode": "inclusive",
        "operations": ops,
        "information": [info],
        "endpoint": format!("http://127.0.0.1:{port}"),
    });
    if let Some(c) = csi {
        doc["contextSourceInfo"] = c;
    }
    let body = doc.to_string();
    let req = Request::builder()
        .method("POST")
        .uri("/ngsi-ld/v1/csourceRegistrations")
        .header("Content-Type", "application/json")
        .header("Content-Length", body.len())
        .body(Body::from(body))
        .expect("request");
    let res = antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("response");
    assert_eq!(res.status(), 201, "registration create");
}

fn state() -> AppState {
    std::env::set_var("ANTARES_EGRESS_ALLOW_PRIVATE", "true");
    AppState::new("antares1".into())
}

async fn get(st: &AppState, uri: &str) -> String {
    let req = Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .expect("request");
    let res = antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("response");
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .expect("body");
    String::from_utf8_lossy(&bytes).into_owned()
}

/// 5.7.2.4 non-split: q, the geoquery and scopeQ travel with the forward.
#[tokio::test(flavor = "multi_thread")]
async fn non_split_query_forwards_filters() {
    let st = state();
    let m = mock_source(false);
    register(&st, m.port, &["queryEntity"], None, None).await;

    let body = get(
        &st,
        "/ngsi-ld/v1/entities?type=Vehicle&q=speed%3E20&scopeQ=%2FA&\
         georel=near%3BmaxDistance%3D%3D2000&geometry=Point&coordinates=%5B8.6,41.2%5D",
    )
    .await;
    assert!(body.contains(REMOTE_ID), "remote data still flows: {body}");

    let head = m.last_head.lock().expect("lock").clone();
    for needle in [
        "q=speed",
        "scopeQ=",
        "georel=near",
        "geometry=Point",
        "coordinates=",
    ] {
        assert!(
            head.contains(needle),
            "forwarded request must carry {needle}: {head}"
        );
    }
}

/// A source that supports only queryBatch (5.6.7-shaped POST) must be asked
/// the SAME question as one supporting queryEntity: the 5.2.23 Query body
/// carries the client's type, attrs, q, scopeQ and geoQ. Forwarding a body
/// built from the registration's own EntityInfo instead returns entities the
/// client never asked for and hides the ones it did.
#[tokio::test(flavor = "multi_thread")]
async fn query_batch_forwards_the_clients_query_not_the_registrations() {
    let st = state();
    let m = mock_source(false);
    register(&st, m.port, &["queryBatch"], None, None).await;

    let body = get(
        &st,
        "/ngsi-ld/v1/entities?type=Vehicle&attrs=speed&q=speed%3E20&scopeQ=%2FA&\
         georel=near%3BmaxDistance%3D%3D2000&geometry=Point&coordinates=%5B8.6,41.2%5D",
    )
    .await;
    assert!(body.contains(REMOTE_ID), "remote data still flows: {body}");

    let head = m.last_head.lock().expect("lock").clone();
    assert!(
        head.starts_with("POST /ngsi-ld/v1/entityOperations/query"),
        "the batch source is asked over the query resource: {head}"
    );
    let sent: serde_json::Value =
        serde_json::from_str(&m.last_body.lock().expect("lock").clone()).expect("Query body");
    assert_eq!(sent["type"], "Query");
    assert_eq!(sent["entities"][0]["type"], "Vehicle");
    assert_eq!(sent["attrs"][0], "speed");
    assert_eq!(sent["q"], "speed>20");
    assert_eq!(sent["scopeQ"], "/A");
    assert_eq!(sent["geoQ"]["georel"], "near;maxDistance==2000");
    assert_eq!(sent["geoQ"]["geometry"], "Point");
    assert_eq!(sent["geoQ"]["coordinates"], serde_json::json!([8.6, 41.2]));
}

/// 5.7.2.4 split: "the filters … shall be removed before forwarding".
#[tokio::test(flavor = "multi_thread")]
async fn split_query_strips_filters_before_forwarding() {
    let st = state();
    let m = mock_source(false);
    register(&st, m.port, &["queryEntity"], None, None).await;

    let _ = get(
        &st,
        "/ngsi-ld/v1/entities?type=Vehicle&splitEntities=true&q=speed%3E20&scopeQ=%2FA",
    )
    .await;
    let head = m.last_head.lock().expect("lock").clone();
    assert!(head.contains("type="), "the forward still happened: {head}");
    for needle in ["q=", "scopeQ="] {
        assert!(
            !head.contains(needle),
            "split entities: {needle} must be stripped from the forward: {head}"
        );
    }
}

/// A registered jsonldContext recompacts only attrs/type/geoproperty on the
/// forward — q/scopeQ terms cannot be recompacted, so push-down is skipped
/// for that registration (filtering at the remote against wrong terms would
/// silently drop data the local re-check can never resurrect).
#[tokio::test(flavor = "multi_thread")]
async fn jsonld_context_registration_skips_filter_pushdown() {
    let st = state();
    let m = mock_source(false);
    register(
        &st,
        m.port,
        &["queryEntity"],
        Some(serde_json::json!([{
            "key": "jsonldContext",
            "value": "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld"
        }])),
        None,
    )
    .await;

    let _ = get(&st, "/ngsi-ld/v1/entities?type=Vehicle&q=speed%3E20").await;
    let head = m.last_head.lock().expect("lock").clone();
    assert!(head.contains("type="), "the forward still happened: {head}");
    assert!(
        !head.contains("q="),
        "q push-down must be skipped for a jsonldContext registration: {head}"
    );
}

/// 5.7.4.4 mirrors 5.7.2.4 for temporal queries: with split entities the
/// value/geo/scope filters are stripped from the forward (the temporal path
/// used to forward them unconditionally).
#[tokio::test(flavor = "multi_thread")]
async fn split_temporal_query_strips_filters_before_forwarding() {
    let st = state();
    let m = mock_source(true);
    register(&st, m.port, &["queryTemporal"], None, None).await;

    let _ = get(
        &st,
        "/ngsi-ld/v1/temporal/entities?type=Vehicle&timerel=after&\
         timeAt=2026-01-01T00:00:00Z&splitEntities=true&q=speed%3E20&scopeQ=%2FA",
    )
    .await;
    let head = m.last_head.lock().expect("lock").clone();
    assert!(
        head.contains("timerel=after"),
        "the temporal window still travels with the forward: {head}"
    );
    for needle in ["q=", "scopeQ="] {
        assert!(
            !head.contains(needle),
            "split entities: {needle} must be stripped from the temporal forward: {head}"
        );
    }
}

/// Non-split temporal keeps forwarding the filters (existing behaviour).
#[tokio::test(flavor = "multi_thread")]
async fn non_split_temporal_query_forwards_filters() {
    let st = state();
    let m = mock_source(true);
    register(&st, m.port, &["queryTemporal"], None, None).await;

    let _ = get(
        &st,
        "/ngsi-ld/v1/temporal/entities?type=Vehicle&timerel=after&\
         timeAt=2026-01-01T00:00:00Z&q=speed%3E20",
    )
    .await;
    let head = m.last_head.lock().expect("lock").clone();
    assert!(
        head.contains("q=speed"),
        "non-split temporal forward carries q: {head}"
    );
}

/// 5.2.9: a registration scoped by `propertyNames` narrows the forwarded read
/// to that scope, and the names travel as TERMS. The peer expands `attrs`
/// against the @context it is handed (5.5.7), so an already-expanded IRI asks
/// a question the peer's own context need not answer, and the registration's
/// whole scope drops out of the answer.
#[tokio::test(flavor = "multi_thread")]
async fn registration_scope_forwards_attrs_as_terms() {
    let st = state();
    let m = mock_source(true);
    register(&st, m.port, &["queryTemporal"], None, Some(&["speed"])).await;

    let _ = get(
        &st,
        "/ngsi-ld/v1/temporal/entities?type=Vehicle&timerel=after&timeAt=2026-01-01T00:00:00Z",
    )
    .await;
    let head = m.last_head.lock().expect("lock").clone();
    assert!(
        head.contains("attrs=speed"),
        "the registration scope must be forwarded as a term: {head}"
    );
    assert!(
        !head.contains("default-context"),
        "the scope must not travel as an expanded IRI: {head}"
    );
}
