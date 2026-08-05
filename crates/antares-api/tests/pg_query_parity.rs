//! C11c — store-mode parity for the compiled query path (tasks.md C10/C11/
//! C11b/C11c). ONE fixture set, run through BOTH engines:
//!
//! * the in-memory evaluator (`geo::GeoQuery::matches`, `scope_matches`,
//!   `qeval::eval_q`) — the H7 arbiter, and what `memory`/`file` modes use;
//! * the compiled SQL (`antares-sql/src/compile/*`) executed by PostGIS.
//!
//! The invariant under test is the one the whole pushdown rests on: **SQL may
//! only narrow.** Every entity the evaluator accepts must come back from SQL;
//! extra rows are fine because the caller re-filters. A violation here is a
//! compliance bug — two store modes answering the same query differently —
//! which is exactly what C11c exists to catch.
//!
//! Skips loudly without ANTARES_TEST_DATABASE_URL (see antares-sql/tests/pg.rs
//! for the container recipe).

use antares_jsonld::Loader;
use antares_model::TenantId;
use antares_sql::store::any::{AnyStore, PgBackend};
use antares_sql::store::pg_entity::EntityFilter;
use serde_json::{json, Value};

macro_rules! require_db {
    () => {
        match std::env::var("ANTARES_TEST_DATABASE_URL") {
            Ok(url) => url,
            Err(_) => {
                eprintln!("SKIP: ANTARES_TEST_DATABASE_URL not set");
                return;
            }
        }
    };
}

const NS: &str = "https://uri.etsi.org/ngsi-ld/default-context/";
const LOC: &str = "https://uri.etsi.org/ngsi-ld/location";

fn geo_entity(id: &str, geometry: Value, scopes: &[&str]) -> Value {
    let mut doc = json!({
        "id": id,
        "type": [format!("{NS}Place")],
        "createdAt": "2026-08-04T09:00:00Z",
        "modifiedAt": "2026-08-04T09:00:00Z",
        LOC: [{"type": "GeoProperty", "value": geometry}]
    });
    if !scopes.is_empty() {
        doc["scope"] = json!(scopes);
    }
    doc
}

fn ids(rows: &[Value]) -> Vec<String> {
    let mut v: Vec<String> = rows
        .iter()
        .map(|r| r["id"].as_str().unwrap_or_default().to_owned())
        .collect();
    v.sort();
    v
}

/// The fixtures: points, a line, a polygon with a hole, a MultiPolygon, an
/// entity whose location is multi-instance (deliberately not extractable), and
/// one with no location at all.
fn fixtures() -> Vec<(&'static str, Value)> {
    vec![
        (
            "urn:p:origin",
            geo_entity(
                "urn:p:origin",
                json!({"type": "Point", "coordinates": [0.0, 0.0]}),
                &["/A/B"],
            ),
        ),
        (
            "urn:p:near",
            geo_entity(
                "urn:p:near",
                json!({"type": "Point", "coordinates": [0.001, 0.001]}),
                &["/A/B/C"],
            ),
        ),
        (
            "urn:p:far",
            geo_entity(
                "urn:p:far",
                json!({"type": "Point", "coordinates": [10.0, 10.0]}),
                &["/A"],
            ),
        ),
        (
            "urn:p:inhole",
            geo_entity(
                "urn:p:inhole",
                json!({"type": "Point", "coordinates": [4.5, 4.5]}),
                &["/X/Y"],
            ),
        ),
        (
            "urn:p:line",
            geo_entity(
                "urn:p:line",
                json!({"type": "LineString", "coordinates": [[-1.0, 1.0], [11.0, 1.0]]}),
                &[],
            ),
        ),
        (
            "urn:p:poly",
            geo_entity(
                "urn:p:poly",
                json!({"type": "Polygon", "coordinates": [[[1.0, 1.0], [3.0, 1.0], [3.0, 3.0], [1.0, 3.0], [1.0, 1.0]]]}),
                &["/A//B"],
            ),
        ),
        ("urn:p:nogeo", {
            let mut d = geo_entity(
                "urn:p:nogeo",
                json!({"type": "Point", "coordinates": [0.0, 0.0]}),
                &[],
            );
            d.as_object_mut().expect("obj").remove(LOC);
            d
        }),
        ("urn:p:multi", {
            // two location instances: no single geometry to extract, so the
            // column stays NULL and the row must still reach the evaluator
            let mut d = geo_entity(
                "urn:p:multi",
                json!({"type": "Point", "coordinates": [0.0, 0.0]}),
                &[],
            );
            d[LOC] = json!([
                {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [40.0, 40.0]}},
                {"type": "GeoProperty", "datasetId": "urn:d:2",
                 "value": {"type": "Point", "coordinates": [0.0, 0.0]}}
            ]);
            d
        }),
    ]
}

#[tokio::test(flavor = "multi_thread")]
async fn compiled_sql_never_drops_a_row_the_evaluator_keeps() {
    let url = require_db!();
    let pool = antares_sql::pg::connect(&url, 5).await.expect("connect");
    let t = TenantId::new("parity").expect("tenant");
    antares_sql::pg::ensure_tenant(&pool, &t)
        .await
        .expect("tenant row");
    let store = AnyStore::Pg(PgBackend::new(pool));
    let ctx = Loader::new().core();

    let docs = fixtures();
    let all_ids = {
        let mut v: Vec<String> = docs.iter().map(|(id, _)| (*id).to_owned()).collect();
        v.sort();
        v
    };
    for (id, doc) in &docs {
        let _ = store.delete(&t, antares_sql::store::Kind::Entity, id);
        assert!(
            store
                .create(&t, antares_sql::store::Kind::Entity, id, doc.clone())
                .expect("seed"),
            "seed {id}"
        );
    }

    // geoqueries, as a client would send them
    let geo_cases: Vec<Vec<(&str, &str)>> = vec![
        vec![
            ("georel", "near;maxDistance==500"),
            ("geometry", "Point"),
            ("coordinates", "[0,0]"),
        ],
        vec![
            ("georel", "near;minDistance==500000"),
            ("geometry", "Point"),
            ("coordinates", "[0,0]"),
        ],
        vec![
            ("georel", "within"),
            ("geometry", "Polygon"),
            (
                "coordinates",
                "[[[-1,-1],[6,-1],[6,6],[-1,6],[-1,-1]],[[4,4],[5,4],[5,5],[4,5],[4,4]]]",
            ),
        ],
        vec![
            ("georel", "intersects"),
            ("geometry", "LineString"),
            ("coordinates", "[[2,0],[2,4]]"),
        ],
        vec![
            ("georel", "contains"),
            ("geometry", "Point"),
            ("coordinates", "[2,2]"),
        ],
        vec![
            ("georel", "disjoint"),
            ("geometry", "Polygon"),
            ("coordinates", "[[[-1,-1],[1,-1],[1,1],[-1,1],[-1,-1]]]"),
        ],
        vec![
            ("georel", "equals"),
            ("geometry", "Point"),
            ("coordinates", "[0,0]"),
        ],
        // a non-default geoproperty has no extracted column: must not narrow
        vec![
            ("georel", "within"),
            ("geometry", "Polygon"),
            ("coordinates", "[[[-1,-1],[6,-1],[6,6],[-1,6],[-1,-1]]]"),
            ("geoproperty", "observationSpace"),
        ],
    ];

    for case in &geo_cases {
        let params: std::collections::HashMap<String, String> = case
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        let gq = antares_api::geo::GeoQuery::from_params(&params)
            .expect("valid geoquery")
            .expect("present");

        // the arbiter: what memory/file mode would answer
        let mut expected: Vec<String> = docs
            .iter()
            .filter(|(_, d)| gq.matches(d, &ctx))
            .map(|(id, _)| (*id).to_owned())
            .collect();
        expected.sort();

        let spec = gq.to_sql_spec(&ctx);
        let outcome = store
            .query_entities(
                &t,
                &EntityFilter {
                    geo: spec.as_ref(),
                    ..Default::default()
                },
            )
            .expect("sql query");
        // C11: geo carries a metric residual (near geography vs haversine) —
        // it narrows, it never decides. When the spec declined to compile the
        // store never saw a geo predicate and truthfully claims decided; the
        // API layer forfeits every pushdown in that case (geo_uncompiled gate
        // in filter_entities_paged) — asserted by the all_ids check below.
        if spec.is_some() {
            assert!(!outcome.decided, "geo must never claim decided");
        }
        let got = ids(&outcome.rows);

        for want in &expected {
            assert!(
                got.contains(want),
                "SQL dropped {want}, which the evaluator matches, for {case:?}\n  sql set: {got:?}"
            );
        }
        if case.iter().any(|(k, _)| *k == "geoproperty") {
            // no extracted column for a non-default GeoProperty: the compiler
            // must decline entirely rather than filter on the wrong geometry
            assert!(spec.is_none(), "non-default geoproperty must not compile");
            assert_eq!(got, all_ids, "declining to compile must not narrow");
        } else {
            assert!(
                !expected.is_empty(),
                "fixture set must exercise {case:?} — an all-empty case proves nothing"
            );
        }
    }

    // scopeQ, same contract
    for sq in ["/A/B", "/A/#", "/A/+/C", "/X/Y,/A/B", "/A/B;/A/B", "/#"] {
        let mut expected: Vec<String> = docs
            .iter()
            .filter(|(_, d)| antares_api::scope_matches(sq, d))
            .map(|(id, _)| (*id).to_owned())
            .collect();
        expected.sort();

        let outcome = store
            .query_entities(
                &t,
                &EntityFilter {
                    scope_q: Some(sq),
                    ..Default::default()
                },
            )
            .expect("sql query");
        assert!(!outcome.decided, "scopeQ is loose-or-equal, never decided");
        let got = ids(&outcome.rows);

        for want in &expected {
            assert!(
                got.contains(want),
                "SQL dropped {want} for scopeQ={sq}\n  sql set: {got:?}"
            );
        }
        assert!(
            !expected.is_empty(),
            "scopeQ={sq} matches nothing in the fixtures"
        );
    }

    // and the compiled path must actually be doing work — a query that
    // narrows to nothing proves the predicate reached the database
    let params: std::collections::HashMap<String, String> = [
        ("georel", "near;maxDistance==1"),
        ("geometry", "Point"),
        ("coordinates", "[80,80]"),
    ]
    .iter()
    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
    .collect();
    let gq = antares_api::geo::GeoQuery::from_params(&params)
        .expect("valid")
        .expect("present");
    let spec = gq.to_sql_spec(&ctx);
    let got = ids(&store
        .query_entities(
            &t,
            &EntityFilter {
                geo: spec.as_ref(),
                ..Default::default()
            },
        )
        .expect("sql query")
        .rows);
    // only the two rows with no extractable geometry survive the guard
    assert_eq!(
        got,
        ["urn:p:multi", "urn:p:nogeo"],
        "the geo predicate must run in SQL, keeping only the IS NULL guard rows"
    );
}

/// C11: the pushdown ladder — when every present predicate compiles exactly,
/// SQL DECIDES (equality, not superset), pages, counts and projects; any
/// inexact predicate forfeits all of it and falls back to narrowing.
#[tokio::test(flavor = "multi_thread")]
async fn exactness_gated_pushdown_pages_projects_and_counts() {
    let url = require_db!();
    let pool = antares_sql::pg::connect(&url, 5).await.expect("connect");
    let t = TenantId::new("pushdown").expect("tenant");
    antares_sql::pg::ensure_tenant(&pool, &t)
        .await
        .expect("tenant row");
    let store = AnyStore::Pg(PgBackend::new(pool));

    // ten Rooms with a temperature, one Place without
    let temp = format!("{NS}temperature");
    let mut docs: Vec<(String, Value)> = (0..10)
        .map(|i| {
            (
                format!("urn:room:{i:02}"),
                json!({
                    "id": format!("urn:room:{i:02}"),
                    "type": [format!("{NS}Room")],
                    "createdAt": "2026-08-05T09:00:00Z",
                    "modifiedAt": "2026-08-05T09:00:00Z",
                    &temp: [{"type": "Property", "value": i}]
                }),
            )
        })
        .collect();
    docs.push((
        "urn:room:zz-place".into(),
        json!({
            "id": "urn:room:zz-place",
            "type": [format!("{NS}Place")],
            "createdAt": "2026-08-05T09:00:00Z",
            "modifiedAt": "2026-08-05T09:00:00Z"
        }),
    ));
    for (id, doc) in &docs {
        let _ = store.delete(&t, antares_sql::store::Kind::Entity, id);
        assert!(store
            .create(&t, antares_sql::store::Kind::Entity, id, doc.clone())
            .expect("seed"));
    }

    let room_types = vec![vec![format!("{NS}Room")]];

    // 1. type filter is exact ⇒ page + total in SQL
    let outcome = store
        .query_entities(
            &t,
            &EntityFilter {
                types: Some(&room_types),
                page: Some(antares_sql::store::pg_entity::Page {
                    offset: 2,
                    limit: 3,
                }),
                ..Default::default()
            },
        )
        .expect("paged query");
    assert!(outcome.decided && outcome.paged);
    assert_eq!(outcome.total, Some(10), "pre-LIMIT match count");
    assert_eq!(
        ids(&outcome.rows),
        ["urn:room:02", "urn:room:03", "urn:room:04"],
        "ORDER BY id, OFFSET 2 LIMIT 3"
    );

    // 2. an off-the-end page still reports the true total for links/count
    let outcome = store
        .query_entities(
            &t,
            &EntityFilter {
                types: Some(&room_types),
                page: Some(antares_sql::store::pg_entity::Page {
                    offset: 50,
                    limit: 3,
                }),
                ..Default::default()
            },
        )
        .expect("off-the-end page");
    assert!(outcome.paged && outcome.rows.is_empty());
    assert_eq!(outcome.total, Some(10));

    // 3. compiled q= keeps decided; SQL answer EQUALS the evaluator's
    let ast = antares_ql::parse_q("temperature>=5").expect("parses");
    let expand = |t: &str| format!("{NS}{t}");
    let outcome = store
        .query_entities(
            &t,
            &EntityFilter {
                types: Some(&room_types),
                q: Some(&ast),
                expand: &expand,
                ..Default::default()
            },
        )
        .expect("q query");
    assert!(outcome.decided, "compiled q= is exact by contract");
    assert_eq!(
        ids(&outcome.rows),
        (5..10)
            .map(|i| format!("urn:room:{i:02}"))
            .collect::<Vec<_>>(),
        "decided means equality, not superset"
    );

    // 4. projection: pick keeps the picked attr + every non-IRI member
    let keep = vec![temp.clone()];
    let outcome = store
        .query_entities(
            &t,
            &EntityFilter {
                types: Some(&room_types),
                keep_attrs: Some(&keep),
                ..Default::default()
            },
        )
        .expect("projected query");
    assert!(outcome.decided);
    for row in &outcome.rows {
        let obj = row.as_object().expect("object");
        assert!(obj.contains_key("id") && obj.contains_key("type"));
        assert!(obj.contains_key(&temp), "picked attr survives");
    }

    // 5. any inexact predicate (scopeQ) forfeits paging even when requested
    let outcome = store
        .query_entities(
            &t,
            &EntityFilter {
                types: Some(&room_types),
                scope_q: Some("/A/#"),
                page: Some(antares_sql::store::pg_entity::Page {
                    offset: 0,
                    limit: 3,
                }),
                ..Default::default()
            },
        )
        .expect("inexact query");
    assert!(!outcome.decided && !outcome.paged && outcome.total.is_none());
}

/// C11: temporal instance pruning is byte-exact against instance_matches, and
/// the lastN RANK() cap keeps timestamp ties.
#[tokio::test(flavor = "multi_thread")]
async fn temporal_pruning_matches_the_window_and_keeps_ties() {
    let url = require_db!();
    let pool = antares_sql::pg::connect(&url, 5).await.expect("connect");
    let t = TenantId::new("tpruning").expect("tenant");
    antares_sql::pg::ensure_tenant(&pool, &t)
        .await
        .expect("tenant row");
    let store = AnyStore::Pg(PgBackend::new(pool));

    let temp = format!("{NS}temperature");
    let inst = |v: i64, at: &str, iid: &str| {
        json!({"type": "Property", "value": v, "observedAt": at,
               "instanceId": format!("urn:ngsi-ld:Instance:{iid}"),
               "createdAt": at, "modifiedAt": at})
    };
    let doc = json!({
        "id": "urn:troom:1",
        "type": [format!("{NS}Room")],
        "createdAt": "2026-01-01T00:00:00Z",
        "modifiedAt": "2026-01-01T00:00:00Z",
        &temp: [
            inst(1, "2026-01-01T00:00:00Z", "a"),
            inst(2, "2026-02-01T00:00:00Z", "b"),
            // a timestamp TIE at the newest instant — RANK must keep both
            inst(3, "2026-03-01T00:00:00Z", "c"),
            inst(4, "2026-03-01T00:00:00Z", "d"),
        ]
    });
    let _ = store.delete(&t, antares_sql::store::Kind::Temporal, "urn:troom:1");
    assert!(store
        .create(&t, antares_sql::store::Kind::Temporal, "urn:troom:1", doc)
        .expect("seed temporal"));

    // range: between [Feb, Mar) keeps exactly the February instance
    let tf = antares_sql::store::pg_temporal::TemporalFilter {
        range: Some(antares_sql::compile::temporal::InstanceRange {
            timerel: "between",
            time_at: "2026-02-01T00:00:00Z",
            end_time_at: Some("2026-03-01T00:00:00Z"),
            timeproperty: "observedAt",
        }),
        ..Default::default()
    };
    let got = store
        .get_temporal(&t, "urn:troom:1", &tf)
        .expect("get")
        .expect("present");
    let arr = got[&temp].as_array().expect("array");
    assert_eq!(
        arr.len(),
        1,
        "between [Feb,Mar) is exactly February: {arr:?}"
    );
    assert_eq!(arr[0]["value"], 2);

    // lastN=1 with a tie at the top: BOTH tied instances survive the SQL cap
    // (the API's per-attr lastN then picks per its own stable order)
    let tf = antares_sql::store::pg_temporal::TemporalFilter {
        last_n: Some(1),
        ..Default::default()
    };
    let got = store
        .get_temporal(&t, "urn:troom:1", &tf)
        .expect("get")
        .expect("present");
    let arr = got[&temp].as_array().expect("array");
    assert_eq!(arr.len(), 2, "RANK keeps the tie: {arr:?}");
    for i in arr {
        assert_eq!(i["observedAt"], "2026-03-01T00:00:00Z");
    }

    // meta members are never pruned
    assert_eq!(got["id"], "urn:troom:1");
    assert!(got.get("type").is_some() && got.get("createdAt").is_some());
}
