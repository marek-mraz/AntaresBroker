// SPDX-License-Identifier: EUPL-1.2
//! 5.7.2.4 split entities (p.202): "the filters (filter conditions specified
//! by the query, geospatial restrictions imposed by the geoquery, Scope
//! query, Attributes) shall be removed before forwarding the request. These
//! filters then have to be applied after the Entity information from
//! different Context Sources AND LOCAL INFORMATION, if there is any, has
//! been aggregated."
//!
//! The local arm must therefore not filter its part either — a q satisfiable
//! only by the AGGREGATED entity (speed remote, brandName local) has to
//! match. The memory store never pushed filters, so it always behaved; the
//! SQL stores compile q into the WHERE and used to drop the local half
//! before the merge (ETSI matrix: IOP_EXT_QRY_02_03 '[]' on pg/timescale).

use antares_api::AppState;
use axum::body::Body;
use axum::http::Request;
use std::io::{Read, Write};
use tower::ServiceExt;

/// The programmatic egress override, not `ANTARES_EGRESS_ALLOW_PRIVATE`: a
/// sibling test reading the environment while another rewrote it saw the
/// policy missing and refused the loopback forward. An atomic store carries
/// the same switch with no write for a reader to land in the middle of.
fn allow_private() {
    antares_jsonld::allow_private_egress(true);
}

const ENTITY: &str = "urn:ngsi-ld:Vehicle:split572";

fn mock_replying(reply: String) -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let mut buf = [0u8; 8192];
            let _ = s.read(&mut buf);
            let _ = s.write_all(reply.as_bytes());
        }
    });
    port
}

fn remote_half_reply() -> String {
    let body = serde_json::json!([{
        "id": ENTITY,
        "type": "Vehicle",
        "speed": {"type": "Property", "value": 42},
    }])
    .to_string();
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

async fn send(st: &AppState, req: Request<Body>) -> (u16, String) {
    let res = antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("response");
    let status = res.status().as_u16();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .expect("body");
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

async fn post(st: &AppState, tenant: &str, uri: &str, body: String) -> (u16, String) {
    send(
        st,
        Request::builder()
            .method("POST")
            .uri(uri)
            .header("Content-Type", "application/json")
            .header("NGSILD-Tenant", tenant)
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("request"),
    )
    .await
}

async fn get(st: &AppState, tenant: &str, uri: &str) -> (u16, String) {
    send(
        st,
        Request::builder()
            .method("GET")
            .uri(uri)
            .header("NGSILD-Tenant", tenant)
            .body(Body::empty())
            .expect("request"),
    )
    .await
}

/// Seed the scenario: an inclusive queryEntity source holding the speed
/// half, the brandName half local.
async fn seed(st: &AppState, tenant: &str) {
    let port = mock_replying(remote_half_reply());
    let (status, b) = post(
        st,
        tenant,
        "/ngsi-ld/v1/csourceRegistrations",
        serde_json::json!({
            "id": format!("urn:ngsi-ld:ContextSourceRegistration:split-{port}"),
            "type": "ContextSourceRegistration",
            "mode": "inclusive",
            "operations": ["queryEntity"],
            "information": [{"entities": [{"type": "Vehicle"}]}],
            "endpoint": format!("http://127.0.0.1:{port}"),
        })
        .to_string(),
    )
    .await;
    assert_eq!(status, 201, "{b}");
    let (status, b) = post(
        st,
        tenant,
        "/ngsi-ld/v1/entities",
        serde_json::json!({
            "id": ENTITY, "type": "Vehicle",
            "brandName": {"type": "Property", "value": "Mercedes"},
        })
        .to_string(),
    )
    .await;
    assert_eq!(status, 201, "{b}");
}

const SPLIT_Q: &str = "/ngsi-ld/v1/entities?type=Vehicle&splitEntities=true&q=speed%3E20%3BbrandName%3D%3D%22Mercedes%22";
const SPLIT_Q_MISS: &str =
    "/ngsi-ld/v1/entities?type=Vehicle&splitEntities=true&q=speed%3E200%3BbrandName%3D%3D%22Mercedes%22";

async fn assert_aggregate_filter(st: &AppState, tenant: &str) {
    seed(st, tenant).await;

    // satisfiable only by the AGGREGATED entity: speed remote, brand local
    let (status, body) = get(st, tenant, SPLIT_Q).await;
    assert_eq!(status, 200, "{body}");
    assert!(
        body.contains(ENTITY),
        "the aggregated entity must match the split-entities filter: {body}"
    );
    assert!(body.contains("Mercedes"), "local half merged: {body}");
    assert!(body.contains("speed"), "remote half merged: {body}");

    // the aggregate filter still filters: a failing q drops the entity
    let (status, body) = get(st, tenant, SPLIT_Q_MISS).await;
    assert_eq!(status, 200, "{body}");
    assert!(
        !body.contains(ENTITY),
        "aggregate q must still apply: {body}"
    );
}

/// Memory-store control: never pushed filters, must keep working.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_7_2_4_split_aggregate_filter_memory() {
    allow_private();
    let st = AppState::new("antares1".into());
    assert_aggregate_filter(&st, "sagg-mem").await;
}

/// The SQL arm: the compiled q must NOT drop the local half before the
/// merge. Needs a live PostGIS, so it is ignored by default — a run
/// without a database reports it as `ignored`, never as a pass. When it
/// IS selected the missing URL is a hard failure, not a vacuous pass.
#[cfg(feature = "postgres")]
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a live PostGIS in ANTARES_TEST_DATABASE_URL: cargo test -p antares-api --test split_entities_5_7_2 -- --ignored"]
async fn clause_5_7_2_4_split_aggregate_filter_postgres() {
    let url = std::env::var("ANTARES_TEST_DATABASE_URL")
        .expect("ANTARES_TEST_DATABASE_URL must point at a live PostGIS");
    allow_private();
    let pool = antares_sql::store::pg::connect(&url, 5)
        .await
        .expect("connect");
    let store =
        antares_sql::store::any::AnyStore::Pg(antares_sql::store::any::PgBackend::new(pool));
    let st = AppState::with_store("antares1".into(), std::sync::Arc::new(store), "postgres");
    let tenant = format!("sagg{}", &uuid::Uuid::new_v4().simple().to_string()[..12]);
    assert_aggregate_filter(&st, &tenant).await;
}
