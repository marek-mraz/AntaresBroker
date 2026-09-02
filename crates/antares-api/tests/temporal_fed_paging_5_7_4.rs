// SPDX-License-Identifier: EUPL-1.2
//! 5.7.4.4 + 5.5.9: pagination applies AFTER the federated temporal union
//! is merged — the SQL page pushdown must be disabled when registrations
//! match, or page 1 returns local-page + all-remote rows (matrix-9
//! IOP_EXT_TMP_03_04: pg/timescale returned 3 for limit=2; memory/file
//! never pre-page, which is why only SQL cells failed).
//!
//! Needs a live PostGIS (container recipe in antares-sql/tests/pg.rs), so
//! it is ignored by default: a run without a database reports it as
//! `ignored` instead of passing vacuously.
#![cfg(feature = "postgres")]

use antares_api::state::AppState;
use antares_sql::store::any::{AnyStore, PgBackend};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::sync::Arc;
use tower::ServiceExt;

/// One-shot HTTP mock: replies `reply` to every request on its own thread.
fn mock_replying(reply: String) -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let mut buf = [0u8; 16384];
            let _ = s.read(&mut buf);
            let _ = s.write_all(reply.as_bytes());
        }
    });
    port
}

async fn send(st: &AppState, req: Request<Body>) -> axum::http::Response<Body> {
    antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("response")
}

async fn body_json(res: axum::http::Response<Body>) -> Value {
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .expect("body");
    serde_json::from_slice(&bytes).expect("json body")
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a live PostGIS in ANTARES_TEST_DATABASE_URL: cargo test -p antares-api --test temporal_fed_paging_5_7_4 -- --ignored"]
async fn clause_5_7_4_pages_partition_the_federated_union_on_pg() {
    let url = std::env::var("ANTARES_TEST_DATABASE_URL")
        .expect("ANTARES_TEST_DATABASE_URL must point at a live PostGIS");
    antares_jsonld::allow_private_egress(true);
    let pool = antares_sql::store::pg::connect(&url, 5)
        .await
        .expect("connect");
    antares_sql::store::pg::ensure_tenant(&pool, &antares_model::TenantId::default())
        .await
        .expect("tenant row");
    let st = AppState::with_store(
        "fedpage.example".into(),
        Arc::new(AnyStore::Pg(PgBackend::new(pool))),
        "postgres",
    );

    // unique type per run — the DB outlives the test process
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .subsec_nanos();
    let etype = format!("FedPage{}x{nanos}", std::process::id());
    let eid = |tail: &str| format!("urn:ngsi-ld:FedPage:{etype}-{tail}");

    // local temporal entity "-a"
    let doc = json!({
        "id": eid("a"), "type": etype,
        "speed": [{"type": "Property", "value": 1, "observedAt": "2026-05-01T00:00:00Z"}]
    })
    .to_string();
    let req = Request::builder()
        .method("POST")
        .uri("/ngsi-ld/v1/temporal/entities")
        .header("Content-Type", "application/json")
        .header("Content-Length", doc.len())
        .body(Body::from(doc))
        .expect("request");
    let res = send(&st, req).await;
    assert!(
        res.status() == StatusCode::CREATED || res.status() == StatusCode::NO_CONTENT,
        "local temporal upsert: {}",
        res.status()
    );

    // remote broker serving "-b" and "-c"
    let remote = json!([
        {"id": eid("b"), "type": etype,
         "speed": [{"type": "Property", "value": 1, "observedAt": "2026-05-01T00:00:00Z"}]},
        {"id": eid("c"), "type": etype,
         "speed": [{"type": "Property", "value": 1, "observedAt": "2026-05-01T00:00:00Z"}]}
    ])
    .to_string();
    let port = mock_replying(format!(
        "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{remote}",
        remote.len()
    ));
    let reg = json!({
        "id": format!("urn:ngsi-ld:ContextSourceRegistration:fedpage-{etype}"),
        "type": "ContextSourceRegistration",
        "operations": ["queryTemporal"],
        "information": [{"entities": [{"type": etype}]}],
        "endpoint": format!("http://127.0.0.1:{port}"),
    })
    .to_string();
    let req = Request::builder()
        .method("POST")
        .uri("/ngsi-ld/v1/csourceRegistrations")
        .header("Content-Type", "application/json")
        .header("Content-Length", reg.len())
        .body(Body::from(reg))
        .expect("request");
    assert_eq!(send(&st, req).await.status(), StatusCode::CREATED);

    let page = |q: String| {
        Request::builder()
            .method("GET")
            .uri(format!(
                "/ngsi-ld/v1/temporal/entities?type={etype}&timerel=after&timeAt=2020-01-01T00:00:00Z&{q}"
            ))
            .body(Body::empty())
            .expect("request")
    };
    let res = send(&st, page("limit=2".into())).await;
    assert_eq!(res.status(), StatusCode::OK);
    let p1 = body_json(res).await.as_array().cloned().expect("array");
    assert_eq!(p1.len(), 2, "page 1 holds exactly limit entities: {p1:?}");

    let res = send(&st, page("limit=2&offset=2".into())).await;
    assert_eq!(res.status(), StatusCode::OK);
    let p2 = body_json(res).await.as_array().cloned().expect("array");
    assert_eq!(p2.len(), 1, "page 2 holds the remainder: {p2:?}");

    // the two pages partition the union — no repeats, nothing lost
    let mut ids: Vec<String> = p1
        .iter()
        .chain(p2.iter())
        .filter_map(|e| e.get("id").and_then(Value::as_str).map(str::to_owned))
        .collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids, vec![eid("a"), eid("b"), eid("c")], "3 distinct ids");
}
