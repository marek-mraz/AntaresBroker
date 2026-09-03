// SPDX-License-Identifier: EUPL-1.2
//! The registration prefilter, from outside: which Context Sources a
//! distributed read actually contacts.
//!
//! 4.3.6.1 makes the set of Context Sources a read reaches part of the
//! answer's completeness, and 5.12 defines which registrations match. Both
//! registration sources — the store's `matching_registrations` and the
//! `DocMirror` a `bus=nats` deployment installs in front of it — narrow
//! before 5.12 runs, so both owe the same invariant: the narrowing may only
//! ever drop a registration 5.12 would reject. A source dropped there is
//! not an error anyone sees; the read answers 200 with a hole in it.

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

/// A Context Source answering `[]` and recording that it was asked.
struct Mock {
    port: u16,
    hits: Arc<Mutex<usize>>,
}

fn mock_source() -> Mock {
    let reply = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
                 Content-Length: 2\r\nConnection: close\r\n\r\n[]";
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let hits: Arc<Mutex<usize>> = Arc::default();
    let sink = hits.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let mut buf = [0u8; 65536];
            let _ = s.read(&mut buf);
            *sink.lock().expect("lock") += 1;
            let _ = s.write_all(reply.as_bytes());
        }
    });
    Mock { port, hits }
}

async fn send(st: &AppState, req: Request<Body>) -> StatusCode {
    antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("response")
        .status()
}

async fn register(st: &AppState, id: &str, port: u16, information: Value) {
    let body = json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "mode": "inclusive",
        "operations": ["federationOps"],
        "information": information,
        "endpoint": format!("http://127.0.0.1:{port}"),
    })
    .to_string();
    let status = send(
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
    assert_eq!(status, StatusCode::CREATED, "registration create {id}");
}

async fn query(st: &AppState, q: &str) {
    let status = send(
        st,
        Request::builder()
            .method("GET")
            .uri(format!("/ngsi-ld/v1/entities?{q}"))
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "query {q}");
}

/// A broker whose registration reads go through the mirror, hydrated from
/// the store the way `bus=nats` wiring hydrates it.
fn with_mirror(st: &AppState) -> AppState {
    let m = Arc::new(antares_api::mirror::DocMirror::default());
    antares_api::notify::seed_mirror(&*st.store, m.as_ref(), antares_store::Kind::Registration)
        .expect("hydrate");
    let mut out = st.clone();
    out.reg_mirror = Some(m);
    out
}

/// The Vehicle type as the broker stores it, so a registration created
/// through the API and a query naming `Vehicle` meet on the same IRI.
const VEHICLE: &str = "https://uri.etsi.org/ngsi-ld/default-context/Vehicle";

/// Installing the mirror must not change which sources a read reaches.
/// Five registration shapes, four queries, both registration sources: the
/// hit counts have to agree, or one deployment mode federates differently
/// from the other.
#[tokio::test(flavor = "multi_thread")]
async fn the_mirror_and_the_store_reach_the_same_sources() {
    antares_jsonld::allow_private_egress(true);
    let shapes: Vec<(&str, Value)> = vec![
        ("typed", json!([{"entities": [{"type": "Vehicle"}]}])),
        ("other", json!([{"entities": [{"type": "Building"}]}])),
        (
            "ided",
            json!([{"entities": [{"type": "Vehicle", "id": "urn:ngsi-ld:Vehicle:1"}]}]),
        ),
        (
            "pattern",
            json!([{"entities": [{"type": "Vehicle", "idPattern": "urn:ngsi-ld:Vehicle:9.*"}]}]),
        ),
        ("attrsonly", json!([{"propertyNames": ["speed"]}])),
    ];
    let queries = [
        "type=Vehicle",
        "type=Vehicle&id=urn:ngsi-ld:Vehicle:1",
        "type=Vehicle&idPattern=urn:ngsi-ld:Vehicle:9.*",
        "type=Vehicle&id=urn:ngsi-ld:Vehicle:1&idPattern=urn:ngsi-ld:Vehicle:9.*",
    ];

    for q in queries {
        let mut counts = Vec::new();
        for through_mirror in [false, true] {
            let base = AppState::new("antares-narrow".into());
            let mut mocks = Vec::new();
            for (name, info) in &shapes {
                let m = mock_source();
                register(
                    &base,
                    &format!("urn:ngsi-ld:ContextSourceRegistration:{name}"),
                    m.port,
                    info.clone(),
                )
                .await;
                mocks.push((*name, m));
            }
            let st = if through_mirror {
                with_mirror(&base)
            } else {
                base
            };
            query(&st, q).await;
            counts.push(
                mocks
                    .iter()
                    .map(|(n, m)| (*n, *m.hits.lock().expect("lock")))
                    .collect::<Vec<_>>(),
            );
        }
        assert_eq!(
            counts[0], counts[1],
            "query {q}: the mirror reached a different set of sources than the store"
        );
    }
}

/// The prefilter hole: a query carrying `id` AND `idPattern` narrowed on
/// the id list alone, so a registration whose own entity id matches only
/// the query's PATTERN was dropped before 5.12 ever judged it — and
/// `entity_info_matches` accepts exactly that case.
///
/// The `mirror=false` arm runs over the memory store, whose
/// `matching_registrations` narrows nothing (the trait allows it: "a
/// backend may narrow; returning the full tenant list is always correct"),
/// so it passes either way here. The same query against Postgres, whose
/// `csource_index` WHERE has the same shape the mirror's buckets do, is
/// what the ETSI Postgres cells exercise.
#[tokio::test(flavor = "multi_thread")]
async fn an_id_list_beside_an_id_pattern_never_hides_a_source() {
    antares_jsonld::allow_private_egress(true);
    for through_mirror in [false, true] {
        let base = AppState::new("antares-narrow".into());
        let m = mock_source();
        register(
            &base,
            "urn:ngsi-ld:ContextSourceRegistration:byid",
            m.port,
            json!([{"entities": [{"type": "Vehicle", "id": "urn:ngsi-ld:Vehicle:99"}]}]),
        )
        .await;
        let st = if through_mirror {
            with_mirror(&base)
        } else {
            base
        };
        query(
            &st,
            "type=Vehicle&id=urn:ngsi-ld:Vehicle:1&idPattern=urn:ngsi-ld:Vehicle:9.*",
        )
        .await;
        assert!(
            *m.hits.lock().expect("lock") > 0,
            "mirror={through_mirror}: the source whose id matches the query's idPattern \
             was never asked"
        );
    }
}

/// The narrowing is not vacuous: a source declaring an unrelated type is
/// not contacted at all, through either registration source.
#[tokio::test(flavor = "multi_thread")]
async fn an_unrelated_type_is_never_contacted() {
    antares_jsonld::allow_private_egress(true);
    for through_mirror in [false, true] {
        let base = AppState::new("antares-narrow".into());
        let m = mock_source();
        register(
            &base,
            "urn:ngsi-ld:ContextSourceRegistration:building",
            m.port,
            json!([{"entities": [{"type": "Building"}]}]),
        )
        .await;
        let st = if through_mirror {
            with_mirror(&base)
        } else {
            base
        };
        query(&st, "type=Vehicle").await;
        assert_eq!(
            *m.hits.lock().expect("lock"),
            0,
            "mirror={through_mirror}: a Building source answered a Vehicle query"
        );
    }
}

/// The mirror keys on the IRI the document carries, which is what 5.12
/// compares — a query naming the term `Vehicle` has to reach a registration
/// stored under the expanded type.
#[tokio::test(flavor = "multi_thread")]
async fn a_term_in_the_query_meets_the_iri_in_the_registration() {
    antares_jsonld::allow_private_egress(true);
    let base = AppState::new("antares-narrow".into());
    let m = mock_source();
    register(
        &base,
        "urn:ngsi-ld:ContextSourceRegistration:iri",
        m.port,
        json!([{"entities": [{"type": VEHICLE}]}]),
    )
    .await;
    let st = with_mirror(&base);
    query(&st, "type=Vehicle").await;
    assert!(
        *m.hits.lock().expect("lock") > 0,
        "the term did not meet the IRI the registration declares"
    );
}
