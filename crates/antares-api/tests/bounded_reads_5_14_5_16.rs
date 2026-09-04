// SPDX-License-Identifier: EUPL-1.2
//! Reads that walk a tenant do it a page at a time.
//!
//! 5.16.1.4 tells a Snapshot fill to retrieve "all pages" of its queries and
//! 5.14.4.4 tells an EntityMap to record the ids a query matched. Neither
//! says how much of the tenant may be in memory while that happens, and both
//! sit in front of the 100 000 000-Entity target: a fill that asks the store
//! for the whole match set at once, or a candidate scan that does, is bounded
//! by the tenant rather than by the broker.
//!
//! What is asserted here is the request the store receives. A backend that
//! cannot cut a page answers whole and is no worse off than before; one that
//! can is never asked to do more.
#![allow(clippy::unwrap_used)] // an unwrap here is the assertion

mod common;

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::Double;
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;

async fn send(
    st: &AppState,
    method: &str,
    path: &str,
    body: Option<String>,
) -> (StatusCode, axum::http::HeaderMap, Value) {
    let b = Request::builder().method(method).uri(path);
    let req = match body {
        Some(body) => b
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body)),
        None => b.body(Body::empty()),
    }
    .expect("request");
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

async fn create_vehicle(st: &AppState, id: &str) {
    let body = json!({"id": id, "type": "Vehicle",
        "speed": {"type": "Property", "value": 80}})
    .to_string();
    let (status, _, b) = send(st, "POST", "/ngsi-ld/v1/entities", Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "{b}");
}

/// Three Entities and a state whose store counts what it is asked for. The
/// counter starts after the seeding writes, so only the read under test is
/// in it.
async fn seeded() -> (AppState, Arc<Double>) {
    let mut st = AppState::new("antares-bounds".into());
    antares_api::wire(&mut st).await;
    for id in ["urn:ngsi-ld:Vehicle:b1", "urn:ngsi-ld:Vehicle:b2"] {
        create_vehicle(&st, id).await;
    }
    let spy = Arc::new(Double::passthrough(st.store.clone()));
    st.store = spy.clone();
    (st, spy)
}

/// 5.16.1.4: the fill retrieves every page of its query, one page at a time.
/// Buffering the match set to count it against the snapshot's budget is the
/// same read the budget exists to refuse.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_16_1_the_fill_reads_its_query_a_page_at_a_time() {
    let (st, spy) = seeded().await;
    let snap = json!({"type": "Snapshot",
        "snapshotQueries": [{"type": "Query", "entities": [{"type": "Vehicle"}]}]})
    .to_string();
    let (status, headers, body) = send(&st, "POST", "/ngsi-ld/v1/snapshots", Some(snap)).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let loc = headers
        .get("Location")
        .and_then(|v| v.to_str().ok())
        .expect("Location header")
        .to_owned();

    let mut ready = Value::Null;
    for _ in 0..100 * antares_api::state::slow_factor() {
        let (status, _, body) = send(&st, "GET", &loc, None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        if body["snapshotStatus"] != "preparing" {
            ready = body;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(ready["snapshotStatus"], "success", "{ready}");
    assert_eq!(
        spy.unpaged_queries(),
        0,
        "the fill asked the store for a whole match set"
    );
}

/// 5.14.4.4: the candidate scan behind an EntityMap is bounded the same way.
/// `idPattern` is applied after the store answers (5.2.33), which is a reason
/// to keep the caller's regex — not a reason to read the tenant.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_14_4_the_candidate_scan_is_read_a_page_at_a_time() {
    let (st, spy) = seeded().await;
    let (status, _, body) = send(
        &st,
        "GET",
        "/ngsi-ld/v1/entityMaps?type=Vehicle&idPattern=%5Eurn:ngsi-ld:Vehicle:",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(
        body["entityMap"].as_object().map(|m| m.len()),
        Some(2),
        "{body}"
    );
    assert_eq!(
        spy.unpaged_queries(),
        0,
        "the candidate scan asked the store for the whole tenant"
    );
}

/// The walks above against a store that really pages: the memory arm answers
/// every query with the whole match set, so it can only prove what was asked
/// for, never what a chunked walk does with the answer. Skips without
/// ANTARES_TEST_DATABASE_URL (the container recipe is in antares-sql/tests/pg.rs).
#[cfg(feature = "postgres")]
mod paging_store {
    use super::*;
    use antares_model::TenantId;
    use antares_store::Kind;

    /// A Postgres-backed state with an empty tenant and a page size small
    /// enough that a handful of Entities spans several chunks. The tenant is
    /// the caller's own: these tests run in parallel against one database, and
    /// a shared tenant would have each of them seeding and cleaning the other's
    /// rows.
    async fn pg_state(chunk: usize, tenant_name: &str) -> Option<(AppState, TenantId)> {
        let url = match std::env::var("ANTARES_TEST_DATABASE_URL") {
            Ok(u) => u,
            Err(_) => {
                eprintln!("SKIP: ANTARES_TEST_DATABASE_URL not set");
                return None;
            }
        };
        let pool = antares_sql::store::pg::connect(&url, 5)
            .await
            .expect("connect");
        let tenant = TenantId::new(tenant_name).expect("tenant");
        antares_sql::store::pg::ensure_tenant(&pool, &tenant)
            .await
            .expect("tenant row");
        let mut st = AppState::with_store(
            "antares-bounds".into(),
            Arc::new(antares_sql::store::any::AnyStore::Pg(
                antares_sql::store::any::PgBackend::new(pool),
            )),
            "postgres",
        );
        antares_api::wire(&mut st).await;
        for doc in st.store.list(&tenant, Kind::Entity).await.expect("list") {
            if let Some(id) = doc["id"].as_str() {
                st.store
                    .delete(&tenant, Kind::Entity, id)
                    .await
                    .expect("clean");
            }
        }
        st.max_limit = chunk;
        Some((st, tenant))
    }

    async fn seed(st: &AppState, tenant: &TenantId, n: usize) {
        for i in 1..=n {
            st.store
                .create(
                    tenant,
                    Kind::Entity,
                    &format!("urn:ngsi-ld:Vehicle:w{i:02}"),
                    json!({"id": format!("urn:ngsi-ld:Vehicle:w{i:02}"),
                           "type": ["https://uri.etsi.org/ngsi-ld/default-context/Vehicle"]}),
                )
                .await
                .expect("seed");
        }
    }

    async fn send_t(
        st: &AppState,
        tenant: &TenantId,
        method: &str,
        path: &str,
        body: Option<String>,
    ) -> (StatusCode, axum::http::HeaderMap, Value) {
        let b = Request::builder()
            .method(method)
            .uri(path)
            .header("NGSILD-Tenant", tenant.as_str());
        let req = match body {
            Some(body) => b
                .header("Content-Type", "application/json")
                .header("Content-Length", body.len())
                .body(Body::from(body)),
            None => b.body(Body::empty()),
        }
        .expect("request");
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

    /// 5.16.1.4: "all pages are to be retrieved completely". A walk that
    /// stops at the first page copies a fraction of the match set into the
    /// snapshot and reports the query a success over it.
    #[tokio::test(flavor = "multi_thread")]
    async fn clause_5_16_1_the_fill_copies_every_page() {
        let Some((mut st, tenant)) = pg_state(3, "boundedfill").await else {
            return;
        };
        seed(&st, &tenant, 9).await;
        let snap = json!({"type": "Snapshot",
            "snapshotQueries": [{"type": "Query", "entities": [{"type": "Vehicle"}]}]})
        .to_string();
        let (status, headers, body) =
            send_t(&st, &tenant, "POST", "/ngsi-ld/v1/snapshots", Some(snap)).await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        let loc = headers
            .get("Location")
            .and_then(|v| v.to_str().ok())
            .expect("Location header")
            .to_owned();
        let mut ready = Value::Null;
        for _ in 0..200 * antares_api::state::slow_factor() {
            let (status, _, body) = send_t(&st, &tenant, "GET", &loc, None).await;
            assert_eq!(status, StatusCode::OK, "{body}");
            if body["snapshotStatus"] != "preparing" {
                ready = body;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        assert_eq!(ready["snapshotStatus"], "success", "{ready}");
        let sid = ready["id"].as_str().expect("id").to_owned();

        // read the frozen copy in one page, not three
        st.max_limit = 1000;
        let req = Request::builder()
            .method("GET")
            .uri("/ngsi-ld/v1/entities?type=Vehicle&limit=1000")
            .header("NGSILD-Tenant", tenant.as_str())
            .header("NGSILD-Snapshot", &sid)
            .body(Body::empty())
            .expect("request");
        let res = antares_api::router(st.clone())
            .oneshot(req)
            .await
            .expect("response");
        assert_eq!(res.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .expect("body");
        let list: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        assert_eq!(
            list.as_array().map(Vec::len),
            Some(9),
            "the fill stopped at a page boundary: {list}"
        );

        for doc in st.store.list(&tenant, Kind::Entity).await.expect("list") {
            if let Some(id) = doc["id"].as_str() {
                st.store
                    .delete(&tenant, Kind::Entity, id)
                    .await
                    .expect("clean");
            }
        }
    }

    /// 5.14.4.4: the candidate ids are the query's matches, and an idPattern
    /// is applied after the store answers — so a chunk can come back with
    /// nothing in it while every match is still waiting behind it.
    #[tokio::test(flavor = "multi_thread")]
    async fn clause_5_14_4_the_candidate_walk_reaches_the_last_chunk() {
        let Some((st, tenant)) = pg_state(3, "boundedmap").await else {
            return;
        };
        seed(&st, &tenant, 9).await;
        let (status, _, body) = send_t(
            &st,
            &tenant,
            "GET",
            "/ngsi-ld/v1/entityMaps?type=Vehicle&idPattern=w0%5B89%5D$",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        assert_eq!(
            body["entityMap"].as_object().map(|m| m.len()),
            Some(2),
            "the walk gave up before the chunk holding the matches: {body}"
        );
        for doc in st.store.list(&tenant, Kind::Entity).await.expect("list") {
            if let Some(id) = doc["id"].as_str() {
                st.store
                    .delete(&tenant, Kind::Entity, id)
                    .await
                    .expect("clean");
            }
        }
    }
}
