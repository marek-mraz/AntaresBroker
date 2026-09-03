// SPDX-License-Identifier: EUPL-1.2
//! 4.5.19 on the QUERY surface (5.7.4): the aggregated temporal
//! representation of many Entities, computed by the store.
//!
//! The single-Entity retrieve aggregates the instances the API already
//! holds; the query pushes the bucket matrix into SQL when nothing after
//! the store call could change the answer, and presents what comes back.
//! That second path is only reachable on a PostgreSQL-backed broker, so
//! this file skips loudly without `ANTARES_TEST_DATABASE_URL` (container
//! recipe in `antares-sql/tests/pg.rs`).
//!
//! What 4.5.19 obliges of the answer, and what is asserted here: `id`,
//! `type` and the Attribute under its term; the Attribute object labelled
//! `"Property"`; one member per requested aggregation method, keyed by the
//! method name; the member value an Array with as many elements as there
//! are periods; each element an Array of exactly three, in the order
//! value, period start, period end. `aggrPeriodDuration=PT0S` "is
//! interpreted as a duration spanning the whole time range specified by
//! the temporal query" (4.5.19.1), so that is one period. The broker's
//! documented departure from the literal element count — only periods
//! holding an instance are emitted, `docs/spec/4/4.5.19.0.md` — cannot
//! show at that duration, where the whole range is the one period.
#![cfg(feature = "postgres")]

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

const WINDOW: &str = "timerel=after&timeAt=2026-03-01T00:00:00Z";

async fn send(st: &AppState, req: Request<Body>) -> (StatusCode, Value, Option<String>) {
    let res = antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("response");
    let status = res.status();
    let count = res
        .headers()
        .get("NGSILD-Results-Count")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .expect("body");
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, body, count)
}

/// A Pg-backed broker and a tenant of its own, so a local re-run never
/// reads the rows of the one before it.
async fn pg_state() -> Option<(AppState, String)> {
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
    let store =
        antares_sql::store::any::AnyStore::Pg(antares_sql::store::any::PgBackend::new(pool));
    let st = AppState::with_store("me".into(), std::sync::Arc::new(store), "postgres");
    let tenant = format!("agg{}", &uuid::Uuid::new_v4().simple().to_string()[..12]);
    Some((st, tenant))
}

/// Three instances of one numeric Property and one of a second, at three
/// instants inside the query window.
async fn seed(st: &AppState, tenant: &str, id: &str, speeds: [i64; 3]) {
    let doc = json!({
        "id": id,
        "type": "Vehicle",
        "speed": [
            {"type": "Property", "value": speeds[0], "observedAt": "2026-03-01T10:00:00Z"},
            {"type": "Property", "value": speeds[1], "observedAt": "2026-03-01T11:00:00Z"},
            {"type": "Property", "value": speeds[2], "observedAt": "2026-03-01T12:00:00Z"}],
        "fuel": [
            {"type": "Property", "value": 40, "observedAt": "2026-03-01T10:00:00Z"}]})
    .to_string();
    let (status, body, _) = send(
        st,
        Request::builder()
            .method("POST")
            .uri("/ngsi-ld/v1/temporal/entities")
            .header("Content-Type", "application/json")
            .header("NGSILD-Tenant", tenant)
            .header("Content-Length", doc.len())
            .body(Body::from(doc))
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

async fn query(st: &AppState, tenant: &str, q: &str) -> (StatusCode, Value, Option<String>) {
    send(
        st,
        Request::builder()
            .method("GET")
            .uri(format!("/ngsi-ld/v1/temporal/entities?{q}"))
            .header("NGSILD-Tenant", tenant)
            .body(Body::empty())
            .expect("request"),
    )
    .await
}

fn entity<'a>(body: &'a Value, id: &str) -> &'a Value {
    body.as_array()
        .expect("the query answers a list")
        .iter()
        .find(|e| e["id"] == id)
        .unwrap_or_else(|| panic!("{id} missing from {body}"))
}

/// Every triple 4.5.19 mandates: exactly three elements, the value first,
/// then the period's start and end, both DateTimes, start before end.
fn check_triples(rows: &Value, periods: usize, what: &str) {
    let rows = rows
        .as_array()
        .unwrap_or_else(|| panic!("{what} must be an Array: {rows}"));
    assert_eq!(rows.len(), periods, "{what}: one element per period");
    for r in rows {
        let t = r
            .as_array()
            .unwrap_or_else(|| panic!("{what}: each period is an Array: {r}"));
        assert_eq!(
            t.len(),
            3,
            "{what}: value, start, end and nothing else: {r}"
        );
        let (s, e) = (
            t[1].as_str().unwrap_or_else(|| panic!("{what} start: {r}")),
            t[2].as_str().unwrap_or_else(|| panic!("{what} end: {r}")),
        );
        assert!(
            s.ends_with('Z') && e.ends_with('Z'),
            "{what}: DateTimes: {r}"
        );
        assert!(s < e, "{what}: the period starts before it ends: {r}");
    }
}

/// 4.5.19: the answer carries id and type, the Attribute is labelled
/// "Property", and each requested method is an Array of [value, start,
/// end] triples with one element per period — PT0S being one period over
/// the query's whole range (4.5.19.1).
#[tokio::test(flavor = "multi_thread")]
async fn clause_4_5_19_the_query_answers_one_triple_per_period_per_method() {
    let Some((st, tenant)) = pg_state().await else {
        return;
    };
    seed(&st, &tenant, "urn:ngsi-ld:V:agg-a", [10, 20, 30]).await;
    seed(&st, &tenant, "urn:ngsi-ld:V:agg-b", [1, 2, 3]).await;

    let (status, body, _) = query(
        &st,
        &tenant,
        &format!(
            "type=Vehicle&options=aggregatedValues\
             &aggrMethods=totalCount,avg,min,max&aggrPeriodDuration=PT0S&limit=10&{WINDOW}"
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let a = entity(&body, "urn:ngsi-ld:V:agg-a");
    assert_eq!(a["type"], "Vehicle", "4.5.1 members survive: {a}");
    let speed = &a["speed"];
    assert_eq!(speed["type"], "Property", "4.5.19 labels the object: {a}");
    for m in ["totalCount", "avg", "min", "max"] {
        check_triples(&speed[m], 1, m);
    }
    assert_eq!(speed["totalCount"][0][0], 3, "three instances: {a}");
    assert_eq!(speed["avg"][0][0].as_f64(), Some(20.0), "{a}");
    assert_eq!(speed["min"][0][0].as_f64(), Some(10.0), "{a}");
    assert_eq!(speed["max"][0][0].as_f64(), Some(30.0), "{a}");
    // the second entity is aggregated over its own instances, not the union
    let b = entity(&body, "urn:ngsi-ld:V:agg-b");
    assert_eq!(b["speed"]["avg"][0][0].as_f64(), Some(2.0), "{b}");
    // a method that was not asked for is not invented
    assert!(
        a["speed"].get("sum").is_none() && a["speed"].get("stddev").is_none(),
        "only the requested methods: {a}"
    );
}

/// 5.7.4.4 with 4.5.19: `attrs` selects which Attributes are aggregated,
/// and an Entity keeps its 4.5.1 members while the Attributes it did not
/// name are absent.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_7_4_4_attrs_narrows_the_aggregated_answer() {
    let Some((st, tenant)) = pg_state().await else {
        return;
    };
    seed(&st, &tenant, "urn:ngsi-ld:V:agg-attrs", [4, 6, 8]).await;

    let (status, body, _) = query(
        &st,
        &tenant,
        &format!(
            "type=Vehicle&attrs=speed&options=aggregatedValues\
             &aggrMethods=avg&aggrPeriodDuration=PT0S&limit=10&{WINDOW}"
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let e = entity(&body, "urn:ngsi-ld:V:agg-attrs");
    assert_eq!(e["speed"]["avg"][0][0].as_f64(), Some(6.0), "{e}");
    assert!(e.get("fuel").is_none(), "attrs left fuel out: {e}");
    assert_eq!(e["id"], "urn:ngsi-ld:V:agg-attrs", "{e}");
    assert_eq!(e["type"], "Vehicle", "{e}");
}

/// 5.5.9 on the aggregated answer: `limit` pages the Entities, not the
/// periods, and the count is the whole match set rather than the page.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_5_9_the_aggregated_query_pages_entities_and_counts_all() {
    let Some((st, tenant)) = pg_state().await else {
        return;
    };
    for (i, id) in ["urn:ngsi-ld:V:agg-p1", "urn:ngsi-ld:V:agg-p2"]
        .iter()
        .enumerate()
    {
        seed(&st, &tenant, id, [i as i64 + 1; 3]).await;
    }

    let (status, body, count) = query(
        &st,
        &tenant,
        &format!(
            "type=Vehicle&options=aggregatedValues&aggrMethods=avg\
             &aggrPeriodDuration=PT0S&limit=1&count=true&{WINDOW}"
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body.as_array().map(Vec::len),
        Some(1),
        "limit=1 returns one Entity: {body}"
    );
    assert_eq!(count.as_deref(), Some("2"), "the count is the match set");
    check_triples(&body[0]["speed"]["avg"], 1, "avg");
}
