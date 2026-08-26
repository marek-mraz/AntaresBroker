//! Store-mode parity for the compiled query path. ONE fixture set, run
//! through BOTH engines:
//!
//! * the in-memory evaluator (`geo::GeoQuery::matches`, `scope_matches`,
//!   `qeval::eval_q`) — the arbiter, and what `memory`/`file` modes use;
//! * the compiled SQL (`antares-sql/src/compile/*`) executed by PostGIS.
//!
//! The invariant under test is the one the whole pushdown rests on: **SQL may
//! only narrow.** Every entity the evaluator accepts must come back from SQL;
//! extra rows are fine because the caller re-filters. A violation here is a
//! compliance bug — two store modes answering the same query differently —
//! which is exactly what this suite exists to catch.
//!
//! Skips loudly without ANTARES_TEST_DATABASE_URL (see antares-sql/tests/pg.rs
//! for the container recipe).

use antares_jsonld::Loader;
use antares_model::TenantId;
use antares_sql::store::any::{AnyStore, PgBackend};
use antares_sql::store::pg::entity::EntityFilter;
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
    let pool = antares_sql::store::pg::connect(&url, 5)
        .await
        .expect("connect");
    let t = TenantId::new("parity").expect("tenant");
    antares_sql::store::pg::ensure_tenant(&pool, &t)
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
        // Geo carries a metric residual (near geography vs haversine) —
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
    // only the row CARRYING an unextractable geoproperty survives (via the
    // location_ambiguous flag); a row with no geoproperty at all can never
    // match the evaluator and is excluded in SQL — exact, and index-shaped
    assert_eq!(
        got,
        ["urn:p:multi"],
        "the geo predicate must run in SQL, keeping only the ambiguous rows"
    );
}

/// The pushdown ladder — when every present predicate compiles exactly,
/// SQL DECIDES (equality, not superset), pages, counts and projects; any
/// inexact predicate forfeits all of it and falls back to narrowing.
#[tokio::test(flavor = "multi_thread")]
async fn exactness_gated_pushdown_pages_projects_and_counts() {
    let url = require_db!();
    let pool = antares_sql::store::pg::connect(&url, 5)
        .await
        .expect("connect");
    let t = TenantId::new("pushdown").expect("tenant");
    antares_sql::store::pg::ensure_tenant(&pool, &t)
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
                page: Some(antares_sql::store::pg::entity::Page {
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
                page: Some(antares_sql::store::pg::entity::Page {
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
                page: Some(antares_sql::store::pg::entity::Page {
                    offset: 0,
                    limit: 3,
                }),
                ..Default::default()
            },
        )
        .expect("inexact query");
    assert!(!outcome.decided && !outcome.paged && outcome.total.is_none());
}

/// 5.7.4.4 S2 — the temporal q= prefilter obeys the same contract as every
/// pushdown in this file: **SQL may only narrow.** ONE fixture set with
/// deliberately awkward shapes (relationships, languageMaps, array values,
/// datasetId multi-instance, out-of-window decoys), a battery of q filters
/// spanning the 4.9 grammar, and for each:
///
/// * every entity whose WINDOWED doc `qeval::eval_q` accepts MUST come back
///   from SQL (superset — a drop here is two store modes disagreeing);
/// * for filters whose every leaf compiles, known non-matches must be
///   EXCLUDED SQL-side (tightness — proves the prefilter reached the DB).
#[tokio::test(flavor = "multi_thread")]
async fn temporal_q_prefilter_narrows_but_never_drops() {
    let url = require_db!();
    let pool = antares_sql::store::pg::connect(&url, 5)
        .await
        .expect("connect");
    let t = TenantId::new("tqprefilter").expect("tenant");
    antares_sql::store::pg::ensure_tenant(&pool, &t)
        .await
        .expect("tenant row");
    let store = AnyStore::Pg(PgBackend::new(pool));
    let ctx = Loader::new().core();

    // window: [Mar 1, Mar 2). Decoy instances sit in January — far outside
    // the 48 h widened column bound, so tightness assertions stay valid.
    const T0: &str = "2026-03-01T00:00:00Z";
    const T1: &str = "2026-03-02T00:00:00Z";
    const OUT: &str = "2026-01-05T00:00:00Z";
    const IN: &str = "2026-03-01T12:00:00Z";

    let a = |name: &str| format!("{NS}{name}");
    let inst = |body: Value, at: &str, iid: &str| {
        let mut o = body;
        o["observedAt"] = json!(at);
        o["instanceId"] = json!(format!("urn:ngsi-ld:Instance:{iid}"));
        o["createdAt"] = json!(at);
        o["modifiedAt"] = json!(at);
        o
    };
    let prop = |v: Value| json!({"type": "Property", "value": v});
    let docs: Vec<(&str, Value)> = vec![
        // in-window speed 30, heading 45, route "550"; out-window speed 90
        (
            "urn:tq:fast",
            json!({
                "id": "urn:tq:fast", "type": [format!("{NS}Vehicle")],
                "createdAt": OUT, "modifiedAt": IN,
                a("speed"): [inst(prop(json!(30)), IN, "f1"), inst(prop(json!(90)), OUT, "f2")],
                a("heading"): [inst(prop(json!(45)), IN, "f3")],
                a("route"): [inst(prop(json!("550")), IN, "f4")],
            }),
        ),
        // in-window speed 10 (fails >25), heading 170; out-window speed 80
        // (would pass >25 — the window inside the EXISTS must ignore it)
        (
            "urn:tq:slow",
            json!({
                "id": "urn:tq:slow", "type": [format!("{NS}Vehicle")],
                "createdAt": OUT, "modifiedAt": IN,
                a("speed"): [inst(prop(json!(10)), IN, "s1"), inst(prop(json!(80)), OUT, "s2")],
                a("heading"): [inst(prop(json!(170)), IN, "s3")],
            }),
        ),
        // string value, relationship object, array value, languageMap
        (
            "urn:tq:shapes",
            json!({
                "id": "urn:tq:shapes", "type": [format!("{NS}Vehicle")],
                "createdAt": OUT, "modifiedAt": IN,
                a("name"): [inst(prop(json!("m")), IN, "h1")],
                a("ref"): [inst(json!({"type": "Relationship", "object": "urn:dest:1"}), IN, "h2")],
                a("tags"): [inst(prop(json!(["a", "b"])), IN, "h3")],
                a("label"): [inst(json!({"type": "LanguageProperty",
                                         "languageMap": {"en": "hi"}}), IN, "h4")],
            }),
        ),
        // multi-instance datasetId: only the datasetId'd instance matches
        ("urn:tq:multi", {
            let mut m2 = inst(prop(json!(60)), IN, "m2");
            m2["datasetId"] = json!("urn:d:1");
            json!({
                "id": "urn:tq:multi", "type": [format!("{NS}Vehicle")],
                "createdAt": OUT, "modifiedAt": IN,
                a("speed"): [inst(prop(json!(5)), IN, "m1"), m2],
            })
        }),
        // everything out-of-window: never part of any windowed result
        (
            "urn:tq:nowin",
            json!({
                "id": "urn:tq:nowin", "type": [format!("{NS}Vehicle")],
                "createdAt": OUT, "modifiedAt": OUT,
                a("speed"): [inst(prop(json!(99)), OUT, "n1")],
            }),
        ),
    ];
    for (id, doc) in &docs {
        let _ = store.delete(&t, antares_sql::store::Kind::Temporal, id);
        assert!(store
            .create(&t, antares_sql::store::Kind::Temporal, id, doc.clone())
            .expect("seed temporal"));
    }

    // the arbiter's view: instances retained by the [T0, T1) byte-compare
    // window (instance_matches semantics for `between`), then eval_q
    let windowed = |doc: &Value| -> Value {
        let mut d = doc.clone();
        if let Some(o) = d.as_object_mut() {
            for (k, v) in o.iter_mut() {
                if !k.starts_with("http") {
                    continue;
                }
                if let Some(arr) = v.as_array_mut() {
                    arr.retain(|i| {
                        i.get("observedAt")
                            .and_then(Value::as_str)
                            .is_some_and(|s| (T0..T1).contains(&s))
                    });
                }
            }
        }
        d
    };
    let expand = |t: &str| format!("{NS}{t}");

    // (q, entities that must be EXCLUDED SQL-side; empty = superset-only —
    // the filter contains a leaf the compiler rightly refuses)
    let cases: Vec<(&str, Vec<&str>)> = vec![
        (
            "speed>25",
            vec!["urn:tq:slow", "urn:tq:nowin", "urn:tq:shapes"],
        ),
        ("speed>=5;heading<90", vec!["urn:tq:nowin", "urn:tq:shapes"]),
        (
            "speed>25|heading>100",
            vec!["urn:tq:nowin", "urn:tq:shapes"],
        ),
        (r#"speed>25|name~="^x""#, vec![]), // Or with a refused branch: trivial
        (r#"name=="m""#, vec!["urn:tq:fast", "urn:tq:slow"]),
        (r#"ref=="urn:dest:1""#, vec!["urn:tq:fast", "urn:tq:nowin"]),
        (r#"tags=="a""#, vec!["urn:tq:fast", "urn:tq:nowin"]),
        ("speed", vec!["urn:tq:shapes", "urn:tq:nowin"]),
        ("!speed", vec![]), // negated existence: trivial
        // != compiles as NOT-of-Eq over the instance (universal over arrays,
        // datatype-mismatch matches — both fall out of the NOT form)
        (
            "speed!=10",
            vec!["urn:tq:slow", "urn:tq:nowin", "urn:tq:shapes"],
        ),
        // Ne+List: unequal to ALL listed values — 10 and 30 knock out both
        // slow (10) and fast (30); a string "10" would NOT (p.92: datatype
        // mismatch matches !=), which is why the list is all-numeric here
        (
            "speed!=10,30",
            vec![
                "urn:tq:fast",
                "urn:tq:slow",
                "urn:tq:nowin",
                "urn:tq:shapes",
            ],
        ),
        // [lang] brackets compile to a languageMap wildcard (superset: any
        // language — case-insensitive tag matching stays in memory)
        (
            r#"label[en]=="hi""#,
            vec!["urn:tq:fast", "urn:tq:slow", "urn:tq:nowin", "urn:tq:multi"],
        ),
        (
            r#"label[*]=="hi""#,
            vec!["urn:tq:fast", "urn:tq:slow", "urn:tq:nowin", "urn:tq:multi"],
        ),
        // string ordering: COLLATE "C" byte compare (p.89 SHALL), arrays
        // pass through to the evaluator
        (
            r#"name>="m""#,
            vec!["urn:tq:fast", "urn:tq:slow", "urn:tq:nowin", "urn:tq:multi"],
        ),
        (r#"label=="hi""#, vec![]), // languageMap semantics stay in memory? superset holds either way
        ("speed==10..40", vec!["urn:tq:nowin", "urn:tq:shapes"]),
        (
            r#"route=="550","551""#,
            vec!["urn:tq:slow", "urn:tq:nowin", "urn:tq:shapes"],
        ),
    ];

    for (q, excluded) in &cases {
        let ast = antares_ql::parse_q(q).expect("parse");
        let expected: Vec<&str> = docs
            .iter()
            .filter(|(_, d)| antares_api::qeval::eval_q(&ast, &windowed(d), &ctx, &|_| None))
            .map(|(id, _)| *id)
            .collect();
        let tf = antares_sql::store::filter::TemporalFilter {
            range: Some(antares_sql::compile::temporal::InstanceRange {
                timerel: "between",
                time_at: T0,
                end_time_at: Some(T1),
                timeproperty: "observedAt",
            }),
            q: Some(&ast),
            expand: &expand,
            ..Default::default()
        };
        let got = ids(&store.query_temporal(&t, &tf).expect("query").rows);
        for want in &expected {
            assert!(
                got.contains(&(*want).to_string()),
                "SQL dropped {want}, which the windowed evaluator matches, for q={q}\n  sql set: {got:?}"
            );
        }
        for out in excluded {
            assert!(
                !got.contains(&(*out).to_string()),
                "prefilter failed to exclude {out} for q={q} — it never reached the DB?\n  sql set: {got:?}"
            );
            // an excluded id must really be a non-match, or the case is wrong
            let d = &docs.iter().find(|(id, _)| id == out).expect("fixture").1;
            assert!(
                !antares_api::qeval::eval_q(&ast, &windowed(d), &ctx, &|_| None),
                "case bug: {out} actually matches q={q}"
            );
        }
    }

    // the 48 h widening: an instance whose stamp is textually inside the
    // window but whose parsed instant sits BEFORE the window start (+03:00
    // offset) — the byte-exact text predicate keeps it, so an unwidened
    // column bound would drop it and flip both the verdict and the payload
    let off = "2026-03-01T02:00:00+03:00"; // instant 2026-02-28T23:00:00Z
    let doc = json!({
        "id": "urn:tq:offset", "type": [format!("{NS}Vehicle")],
        "createdAt": OUT, "modifiedAt": IN,
        a("speed"): [inst(prop(json!(50)), off, "o1")],
    });
    let _ = store.delete(&t, antares_sql::store::Kind::Temporal, "urn:tq:offset");
    assert!(store
        .create(&t, antares_sql::store::Kind::Temporal, "urn:tq:offset", doc)
        .expect("seed"));
    let ast = antares_ql::parse_q("speed>25").expect("parse");
    let tf = antares_sql::store::filter::TemporalFilter {
        range: Some(antares_sql::compile::temporal::InstanceRange {
            timerel: "between",
            time_at: T0,
            end_time_at: Some(T1),
            timeproperty: "observedAt",
        }),
        q: Some(&ast),
        expand: &expand,
        ..Default::default()
    };
    let rows = store.query_temporal(&t, &tf).expect("query").rows;
    let offset_doc = rows
        .iter()
        .find(|r| r["id"] == "urn:tq:offset")
        .expect("widened column bound must admit the textually-in-window instance");
    let arr = offset_doc[&a("speed")].as_array().expect("speed array");
    assert!(
        arr.iter().any(|i| i["value"] == 50),
        "range pruning dropped the offset-stamp instance: {arr:?}"
    );
}

/// Temporal instance pruning is byte-exact against instance_matches, and
/// the lastN RANK() cap keeps timestamp ties.
#[tokio::test(flavor = "multi_thread")]
async fn temporal_pruning_matches_the_window_and_keeps_ties() {
    let url = require_db!();
    let pool = antares_sql::store::pg::connect(&url, 5)
        .await
        .expect("connect");
    let t = TenantId::new("tpruning").expect("tenant");
    antares_sql::store::pg::ensure_tenant(&pool, &t)
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
    let tf = antares_sql::store::pg::temporal::TemporalFilter {
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
    let tf = antares_sql::store::pg::temporal::TemporalFilter {
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

/// 5.7.4.4 S3 — the geoquery compiled to a SUPERSET SQL prefilter over the
/// per-instance `attr_instances.geo_value` column:
///
/// * every entity the windowed evaluator (`GeoQuery::matches`) accepts MUST
///   come back from SQL (superset);
/// * far-away and out-of-window entities must be EXCLUDED SQL-side
///   (tightness — proves the windowed EXISTS reached the DB);
/// * a row whose `geo_value` is NULL (unextracted / pre-fill data) always
///   survives to the evaluator.
#[tokio::test(flavor = "multi_thread")]
async fn temporal_geo_prefilter_narrows_but_never_drops() {
    let url = require_db!();
    let pool = antares_sql::store::pg::connect(&url, 5)
        .await
        .expect("connect");
    let t = TenantId::new("tgeopre").expect("tenant");
    antares_sql::store::pg::ensure_tenant(&pool, &t)
        .await
        .expect("tenant row");
    let store = AnyStore::Pg(PgBackend::new(pool.clone()));

    const T0: &str = "2026-03-01T00:00:00Z";
    const T1: &str = "2026-03-02T00:00:00Z";
    const OUT: &str = "2026-01-05T00:00:00Z";
    const IN: &str = "2026-03-01T12:00:00Z";
    let paris = json!({"type": "Point", "coordinates": [2.29, 48.85]});
    let far = json!({"type": "Point", "coordinates": [10.0, 50.0]});
    let ginst = |geom: &Value, at: &str, iid: &str| {
        json!({"type": "GeoProperty", "value": geom.clone(), "observedAt": at,
               "instanceId": format!("urn:ngsi-ld:Instance:{iid}"),
               "createdAt": at, "modifiedAt": at})
    };
    let obs = format!("{NS}observationSpace");
    let docs: Vec<(&str, Value)> = vec![
        (
            "urn:tg:near",
            json!({
                "id": "urn:tg:near", "type": [format!("{NS}Vehicle")],
                "createdAt": OUT, "modifiedAt": IN,
                LOC: [ginst(&paris, IN, "g1")],
            }),
        ),
        (
            "urn:tg:far",
            json!({
                "id": "urn:tg:far", "type": [format!("{NS}Vehicle")],
                "createdAt": OUT, "modifiedAt": IN,
                LOC: [ginst(&far, IN, "g2")],
            }),
        ),
        // near Paris — but only OUTSIDE the window (S3: windowed instances)
        (
            "urn:tg:outwin",
            json!({
                "id": "urn:tg:outwin", "type": [format!("{NS}Vehicle")],
                "createdAt": OUT, "modifiedAt": OUT,
                LOC: [ginst(&paris, OUT, "g3")],
            }),
        ),
        // near Paris, in-window, but geo_value forced NULL below (pre-fill row)
        (
            "urn:tg:nullgeo",
            json!({
                "id": "urn:tg:nullgeo", "type": [format!("{NS}Vehicle")],
                "createdAt": OUT, "modifiedAt": IN,
                LOC: [ginst(&paris, IN, "g4")],
            }),
        ),
        // near Paris under a NON-default geoproperty, in-window
        (
            "urn:tg:custgp",
            json!({
                "id": "urn:tg:custgp", "type": [format!("{NS}Vehicle")],
                "createdAt": OUT, "modifiedAt": IN,
                &obs: [ginst(&paris, IN, "g5")],
            }),
        ),
    ];
    for (id, doc) in &docs {
        let _ = store.delete(&t, antares_sql::store::Kind::Temporal, id);
        assert!(store
            .create(&t, antares_sql::store::Kind::Temporal, id, doc.clone())
            .expect("seed temporal"));
    }
    // simulate a pre-fill row: extraction never happened for this entity
    antares_sql::sqlx::query("UPDATE attr_instances SET geo_value = NULL WHERE tenant_id = $1 AND entity_id = 'urn:tg:nullgeo'")
        .bind(t.as_str())
        .execute(&pool)
        .await
        .expect("null out geo_value");

    let range = antares_sql::compile::temporal::InstanceRange {
        timerel: "between",
        time_at: T0,
        end_time_at: Some(T1),
        timeproperty: "observedAt",
    };
    use antares_sql::compile::geo::{GeoSpec, Rel, LOCATION_IRI};
    let expand = |s: &str| format!("{NS}{s}");
    let coords = json!([2.29, 48.85]);
    let poly = json!([[
        [2.0, 48.5],
        [2.6, 48.5],
        [2.6, 49.2],
        [2.0, 49.2],
        [2.0, 48.5]
    ]]);

    // (spec, geoproperty IRI, must-return, must-exclude)
    let near_spec = GeoSpec {
        rel: Rel::Near {
            max: Some(2000.0),
            min: None,
        },
        geometry: "Point",
        coordinates: &coords,
        geoproperty_iri: "",
    };
    let within_spec = GeoSpec {
        rel: Rel::Within,
        geometry: "Polygon",
        coordinates: &poly,
        geoproperty_iri: "",
    };
    let cust_spec = GeoSpec {
        rel: Rel::Near {
            max: Some(2000.0),
            min: None,
        },
        geometry: "Point",
        coordinates: &coords,
        geoproperty_iri: "",
    };
    let cases: Vec<(&GeoSpec, &str, Vec<&str>, Vec<&str>)> = vec![
        (
            &near_spec,
            LOCATION_IRI,
            vec!["urn:tg:near", "urn:tg:nullgeo"],
            vec!["urn:tg:far", "urn:tg:outwin", "urn:tg:custgp"],
        ),
        (
            &within_spec,
            LOCATION_IRI,
            vec!["urn:tg:near", "urn:tg:nullgeo"],
            vec!["urn:tg:far", "urn:tg:outwin", "urn:tg:custgp"],
        ),
        (
            &cust_spec,
            &obs,
            vec!["urn:tg:custgp"],
            vec!["urn:tg:near", "urn:tg:far", "urn:tg:outwin"],
        ),
    ];
    for (n, (spec, iri, keep, drop)) in cases.iter().enumerate() {
        let tf = antares_sql::store::filter::TemporalFilter {
            range: Some(antares_sql::compile::temporal::InstanceRange { ..range }),
            geo: Some((spec, iri)),
            expand: &expand,
            ..Default::default()
        };
        let got = ids(&store.query_temporal(&t, &tf).expect("query").rows);
        for want in keep {
            assert!(
                got.contains(&(*want).to_string()),
                "case {n}: SQL dropped {want}, which the windowed evaluator matches\n  sql set: {got:?}"
            );
        }
        for out in drop {
            assert!(
                !got.contains(&(*out).to_string()),
                "case {n}: geo prefilter failed to exclude {out}\n  sql set: {got:?}"
            );
        }
    }
}

/// 5.7.4.4 — entity paging WITH a values filter: safe only when the
/// prefilter is EXACT, i.e. the windowed EXISTS carries the byte-exact text
/// predicate, not just the ±48 h widened column bound. An entity whose only
/// q-matching instance lies in the slack zone (outside the true window,
/// inside the widened bound) must NOT consume the page slot.
#[tokio::test(flavor = "multi_thread")]
async fn temporal_exact_prefilter_pages_correctly() {
    let url = require_db!();
    let pool = antares_sql::store::pg::connect(&url, 5)
        .await
        .expect("connect");
    let t = TenantId::new("tqpage").expect("tenant");
    antares_sql::store::pg::ensure_tenant(&pool, &t)
        .await
        .expect("tenant row");
    let store = AnyStore::Pg(PgBackend::new(pool));

    const T0: &str = "2026-03-01T00:00:00Z";
    const T1: &str = "2026-03-02T00:00:00Z";
    const SLACK: &str = "2026-02-28T23:00:00Z"; // outside window, inside 48 h
    const IN: &str = "2026-03-01T12:00:00Z";
    let a = |name: &str| format!("{NS}{name}");
    let inst = |v: Value, at: &str, iid: &str| {
        json!({"type": "Property", "value": v, "observedAt": at,
               "instanceId": format!("urn:ngsi-ld:Instance:{iid}"),
               "createdAt": at, "modifiedAt": at})
    };
    // urn:tp:a sorts FIRST: q-matching speed only in the slack zone, plus an
    // unrelated in-window instance so the entity-qualification EXISTS passes
    let docs: Vec<(&str, Value)> = vec![
        (
            "urn:tp:a",
            json!({
                "id": "urn:tp:a", "type": [format!("{NS}Vehicle")],
                "createdAt": SLACK, "modifiedAt": IN,
                a("speed"): [inst(json!(90), SLACK, "p1")],
                a("heading"): [inst(json!(10), IN, "p2")],
            }),
        ),
        (
            "urn:tp:b",
            json!({
                "id": "urn:tp:b", "type": [format!("{NS}Vehicle")],
                "createdAt": SLACK, "modifiedAt": IN,
                a("speed"): [inst(json!(30), IN, "p3")],
            }),
        ),
    ];
    for (id, doc) in &docs {
        let _ = store.delete(&t, antares_sql::store::Kind::Temporal, id);
        assert!(store
            .create(&t, antares_sql::store::Kind::Temporal, id, doc.clone())
            .expect("seed temporal"));
    }

    let ast = antares_ql::parse_q("speed>25").expect("parse");
    let expand = |s: &str| format!("{NS}{s}");
    let tf = antares_sql::store::filter::TemporalFilter {
        range: Some(antares_sql::compile::temporal::InstanceRange {
            timerel: "between",
            time_at: T0,
            end_time_at: Some(T1),
            timeproperty: "observedAt",
        }),
        q: Some(&ast),
        expand: &expand,
        page: Some(antares_sql::store::filter::Page {
            offset: 0,
            limit: 1,
        }),
        ..Default::default()
    };
    let out = store.query_temporal(&t, &tf).expect("query");
    assert!(out.paged, "page must be honoured");
    let got = ids(&out.rows);
    assert_eq!(
        got,
        vec!["urn:tp:b".to_string()],
        "the slack-zone-only match must not consume the page slot"
    );
    assert_eq!(out.total, Some(1), "total counts only true matches");
}
