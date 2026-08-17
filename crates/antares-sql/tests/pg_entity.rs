//! PgEntityStore integration, including the concurrency guarantees.
//! Skips loudly without ANTARES_TEST_DATABASE_URL (see tests/pg.rs recipe).

use antares_model::TenantId;
use antares_sql::pg;
use antares_sql::store::pg_entity::PgEntityStore;
use serde_json::json;

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

fn doc(id: &str, n: i64) -> serde_json::Value {
    json!({
        "id": id, "type": "Test",
        "createdAt": "2026-08-04T09:00:00Z", "modifiedAt": "2026-08-04T09:00:00Z",
        "n": {"type": "Property", "value": n}
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn entity_crud_roundtrip_with_extracted_columns() {
    let url = require_db!();
    let pool = pg::connect(&url, 5).await.expect("connect");
    let s = PgEntityStore::new(pool.clone());
    let t = TenantId::new("pgcrud").expect("tenant");
    pg::ensure_tenant(&pool, &t).await.expect("tenant row");
    let id = "urn:ngsi-ld:Test:crud1";
    let _ = s.delete(&t, id);

    assert!(s.create(&t, id, &doc(id, 1)).expect("create"));
    assert!(
        !s.create(&t, id, &doc(id, 1)).expect("dup"),
        "AlreadyExists → false"
    );
    assert_eq!(
        s.get(&t, id).expect("get").expect("present")["n"]["value"],
        1
    );
    assert_eq!(s.version(&t, id).expect("v"), Some(1));

    // extracted columns really extracted (types tenant-scoped index shape)
    let other = TenantId::new("pgcrud_other").expect("tenant");
    assert!(s.get(&other, id).expect("cross-tenant get").is_none());
    assert_eq!(s.list(&t).expect("list").len(), 1);

    let r: Option<Result<(), ()>> = match s.mutate(&t, id, |d| {
        d["n"]["value"] = json!(2);
        d["modifiedAt"] = json!("2026-08-04T09:01:00Z");
        Ok(())
    }) {
        Ok(x) => x,
        Err(e) => panic!("mutate: {e}"),
    };
    assert!(matches!(r, Some(Ok(()))));
    assert_eq!(
        s.get(&t, id).expect("get").expect("present")["n"]["value"],
        2
    );
    assert_eq!(
        s.version(&t, id).expect("v"),
        Some(2),
        "version bumped under the lock"
    );

    // closure error rolls back, version untouched
    let r: Option<Result<(), &str>> = match s.mutate(&t, id, |d| {
        d["n"]["value"] = json!(99);
        Err("nope")
    }) {
        Ok(x) => x,
        Err(e) => panic!("mutate: {e}"),
    };
    assert!(matches!(r, Some(Err("nope"))));
    assert_eq!(
        s.get(&t, id).expect("get").expect("present")["n"]["value"],
        2
    );
    assert_eq!(s.version(&t, id).expect("v"), Some(2));

    assert!(s.delete(&t, id).expect("delete").is_some());
    assert!(s.get(&t, id).expect("get").is_none());
    assert!(s
        .mutate(&t, id, |_| Ok::<(), ()>(()))
        .expect("mutate absent")
        .is_none());
}

/// Parallel PATCH storm against ONE entity — no lost updates,
/// version strictly monotone, final state = sum of all increments.
#[tokio::test(flavor = "multi_thread")]
async fn concurrent_mutations_lose_nothing() {
    let url = require_db!();
    let pool = pg::connect(&url, 10).await.expect("connect");
    let t = TenantId::new("pgstorm").expect("tenant");
    pg::ensure_tenant(&pool, &t).await.expect("tenant row");
    let s = std::sync::Arc::new(PgEntityStore::new(pool));
    let id = "urn:ngsi-ld:Test:storm";
    let _ = s.delete(&t, id);
    assert!(s.create(&t, id, &doc(id, 0)).expect("create"));

    const WRITERS: i64 = 8;
    const ROUNDS: i64 = 10;
    let mut tasks = Vec::new();
    for _ in 0..WRITERS {
        let (s, t) = (s.clone(), t.clone());
        tasks.push(tokio::task::spawn_blocking(move || {
            for _ in 0..ROUNDS {
                let r = s
                    .mutate(&t, id, |d| {
                        let n = d["n"]["value"].as_i64().expect("n");
                        d["n"]["value"] = serde_json::json!(n + 1);
                        Ok::<(), ()>(())
                    })
                    .expect("mutate");
                assert!(matches!(r, Some(Ok(()))), "row must exist throughout");
            }
        }));
    }
    for task in tasks {
        task.await.expect("writer");
    }

    let n = s.get(&t, id).expect("get").expect("present")["n"]["value"]
        .as_i64()
        .expect("n");
    assert_eq!(
        n,
        WRITERS * ROUNDS,
        "every increment survived (no lost updates)"
    );
    assert_eq!(
        s.version(&t, id).expect("v"),
        Some(1 + WRITERS * ROUNDS),
        "version = create + one bump per mutate"
    );
}

/// Batch create/delete as single multi-row statements — created flags in
/// input order, duplicate ids deduped (5.5.11.1/.4), delete returns prevs.
#[tokio::test(flavor = "multi_thread")]
async fn batch_create_and_delete_multirow() {
    let url = require_db!();
    let pool = pg::connect(&url, 5).await.expect("pool");
    let t = TenantId::new("pgbatch").expect("tenant");
    pg::ensure_tenant(&pool, &t).await.expect("tenant row");
    let s = PgEntityStore::new(pool);

    // pre-existing entity: its batch item must report false
    assert!(s.create(&t, "urn:b:0", &doc("urn:b:0", 0)).expect("pre"));

    let items = vec![
        ("urn:b:0".to_owned(), doc("urn:b:0", 9)), // exists → false
        ("urn:b:1".to_owned(), doc("urn:b:1", 1)),
        ("urn:b:2".to_owned(), doc("urn:b:2", 2)),
        ("urn:b:1".to_owned(), doc("urn:b:1", 99)), // duplicate → false
    ];
    let flags = s.batch_create(&t, &items).expect("batch create");
    assert_eq!(flags, vec![false, true, true, false]);
    // first instance won (5.5.11.1): value 1, not 99
    let stored = s.get(&t, "urn:b:1").expect("get").expect("present");
    assert_eq!(stored["n"]["value"], 1);

    let deleted = s
        .batch_delete(
            &t,
            &["urn:b:0".into(), "urn:b:1".into(), "urn:b:missing".into()],
        )
        .expect("batch delete");
    let mut ids: Vec<&str> = deleted.iter().map(|(id, _)| id.as_str()).collect();
    ids.sort();
    assert_eq!(ids, vec!["urn:b:0", "urn:b:1"]);
    // prev doc travels with the delete (change-hook before-image)
    let prev1 = &deleted
        .iter()
        .find(|(id, _)| id == "urn:b:1")
        .expect("b1")
        .1;
    assert_eq!(prev1["n"]["value"], 1);
    assert!(s.get(&t, "urn:b:1").expect("get").is_none());

    // cleanup
    let _ = s.batch_delete(&t, &["urn:b:2".into()]);
}

// ---- query pushdown ------------------------------------------------------
// The contract is one-directional and it is the whole reason the pushdown is
// safe: SQL may only NARROW. Every row the in-memory evaluator would accept
// must survive the WHERE clause; extra rows are fine because the caller
// re-filters exactly. So each case below asserts the expected set is present,
// and the refusal cases assert the query widens to everything rather than
// guessing a translation.

const NS: &str = "https://uri.etsi.org/ngsi-ld/default-context/";

fn ex(t: &str) -> String {
    format!("{NS}{t}")
}

/// The shape the broker actually stores: expanded IRI keys, each holding an
/// ARRAY of instances. Testing against the internal shape, not a convenient
/// one, is the point — the jsonpath addresses instances.
fn expanded(id: &str, ty: &str, attrs: serde_json::Value) -> serde_json::Value {
    let mut doc = json!({
        "id": id,
        "type": format!("{NS}{ty}"),
        "createdAt": "2026-08-04T09:00:00Z",
        "modifiedAt": "2026-08-04T09:00:00Z"
    });
    for (k, v) in attrs.as_object().expect("attrs object") {
        doc[ex(k)] = json!([v]);
    }
    doc
}

fn ids_of(rows: &[serde_json::Value]) -> Vec<String> {
    let mut v: Vec<String> = rows
        .iter()
        .map(|r| r["id"].as_str().unwrap_or_default().to_owned())
        .collect();
    v.sort();
    v
}

#[tokio::test(flavor = "multi_thread")]
async fn query_pushdown_narrows_without_dropping_matches() {
    let url = require_db!();
    let pool = pg::connect(&url, 5).await.expect("connect");
    let s = PgEntityStore::new(pool.clone());
    let t = TenantId::new("pgquery").expect("tenant");
    pg::ensure_tenant(&pool, &t).await.expect("tenant row");

    let seed = [
        (
            "urn:ngsi-ld:Room:1",
            "Room",
            json!({"temperature": {"type": "Property", "value": 30},
                   "name": {"type": "Property", "value": "north"}}),
        ),
        (
            "urn:ngsi-ld:Room:2",
            "Room",
            json!({"temperature": {"type": "Property", "value": 10}}),
        ),
        (
            "urn:ngsi-ld:Vehicle:1",
            "Vehicle",
            json!({"speed": {"type": "Property", "value": 60},
                   "name": {"type": "Property", "value": "south"}}),
        ),
        (
            "urn:ngsi-ld:Vehicle:2",
            "Vehicle",
            json!({"brandName": {"type": "Property", "value": "Mercedes"}}),
        ),
    ];
    for (id, ty, attrs) in &seed {
        let _ = s.delete(&t, id);
        assert!(
            s.create(&t, id, &expanded(id, ty, attrs.clone()))
                .expect("create"),
            "seed {id}"
        );
    }
    let all: Vec<String> = seed.iter().map(|(id, ..)| (*id).to_owned()).collect();

    let q = |f: &antares_sql::store::pg_entity::EntityFilter<'_>| {
        ids_of(&s.query(&t, f).expect("query").rows)
    };
    let base = || antares_sql::store::pg_entity::EntityFilter {
        expand: &ex,
        ..Default::default()
    };

    // no filter: everything this tenant holds
    assert_eq!(q(&base()), all);

    // ids
    let want = ["urn:ngsi-ld:Room:2"];
    assert_eq!(
        q(&antares_sql::store::pg_entity::EntityFilter {
            ids: Some(&want),
            ..base()
        }),
        want
    );

    // type selection (OR of AND-groups, expanded IRIs)
    let groups = vec![vec![ex("Vehicle")]];
    assert_eq!(
        q(&antares_sql::store::pg_entity::EntityFilter {
            types: Some(&groups),
            ..base()
        }),
        ["urn:ngsi-ld:Vehicle:1", "urn:ngsi-ld:Vehicle:2"]
    );

    // attrs: carries at least one of them
    let attrs = vec![ex("speed"), ex("brandName")];
    assert_eq!(
        q(&antares_sql::store::pg_entity::EntityFilter {
            attrs: Some(&attrs),
            ..base()
        }),
        ["urn:ngsi-ld:Vehicle:1", "urn:ngsi-ld:Vehicle:2"]
    );

    // q=: numeric comparison over instance values
    let ast = antares_ql::parse_q("temperature>20").expect("parse");
    assert_eq!(
        q(&antares_sql::store::pg_entity::EntityFilter {
            q: Some(&ast),
            ..base()
        }),
        ["urn:ngsi-ld:Room:1"]
    );

    // q=: string equality, and the AND of two predicates
    let ast = antares_ql::parse_q("name==\"south\";speed==60").expect("parse");
    assert_eq!(
        q(&antares_sql::store::pg_entity::EntityFilter {
            q: Some(&ast),
            ..base()
        }),
        ["urn:ngsi-ld:Vehicle:1"]
    );

    // q=: existence, and negated existence (true when the attribute is ABSENT
    // — the case a naive `NOT jsonb_path_exists` on the wrong path gets wrong)
    let ast = antares_ql::parse_q("brandName").expect("parse");
    assert_eq!(
        q(&antares_sql::store::pg_entity::EntityFilter {
            q: Some(&ast),
            ..base()
        }),
        ["urn:ngsi-ld:Vehicle:2"]
    );
    let ast = antares_ql::parse_q("!name").expect("parse");
    assert_eq!(
        q(&antares_sql::store::pg_entity::EntityFilter {
            q: Some(&ast),
            ..base()
        }),
        ["urn:ngsi-ld:Room:2", "urn:ngsi-ld:Vehicle:2"]
    );

    // q= the compiler REFUSES (dotted path, regex, string ordering): the row
    // set must widen to everything, never narrow on a guess.
    for refused in ["address.city==\"Bonn\"", "name~=\"^so\"", "name>\"m\""] {
        let ast = antares_ql::parse_q(refused).expect("parse");
        assert_eq!(
            q(&antares_sql::store::pg_entity::EntityFilter {
                q: Some(&ast),
                ..base()
            }),
            all,
            "{refused} must fall back to the full set, not a guess"
        );
    }

    // and the filters compose: type AND q
    let groups = vec![vec![ex("Room")]];
    let ast = antares_ql::parse_q("temperature<20").expect("parse");
    assert_eq!(
        q(&antares_sql::store::pg_entity::EntityFilter {
            types: Some(&groups),
            q: Some(&ast),
            ..base()
        }),
        ["urn:ngsi-ld:Room:2"]
    );
}

/// 5.5.6: "When a query operation is producing so many results that can
/// potentially exhaust client or server resources, or it can be just
/// impractical to be managed, implementations shall raise an error of type
/// TooManyResults. The threshold conditions used as criteria to raise such
/// error is up to each implementation."
///
/// A `q=` shape the compiler declines leaves the request undecided, so no
/// caller page can be pushed and the statement is bounded by the store's own
/// safety LIMIT. Reaching it means the answer was cut at a bound nobody
/// chose: refused, never served as a silent prefix. The same oversized match
/// set stays perfectly pageable through a decided query — the ceiling bounds
/// what one statement materializes, it does not cap the tenant.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_5_6_undecided_query_past_the_ceiling_is_refused() {
    let url = require_db!();
    let pool = pg::connect(&url, 5).await.expect("connect");
    let t = TenantId::new("pgceiling").expect("tenant");
    pg::ensure_tenant(&pool, &t).await.expect("tenant row");
    let clean = || {
        let pool = pool.clone();
        async move {
            sqlx::query("DELETE FROM entities WHERE tenant_id = 'pgceiling'")
                .execute(&pool)
                .await
                .expect("clean");
        }
    };
    clean().await;
    let ceiling = antares_sql::store::pg_entity::MAX_UNDECIDED_ROWS;
    // One statement, ceiling+1 rows: one more than the safety LIMIT can ever
    // return, so the cut is provable rather than incidental.
    sqlx::query(
        "INSERT INTO entities (tenant_id, id, entity, types, created_at, modified_at)
         SELECT 'pgceiling', 'urn:ngsi-ld:Ceil:' || g,
                jsonb_build_object('id', 'urn:ngsi-ld:Ceil:' || g, 'type', $1::text),
                ARRAY[$1::text], now(), now()
           FROM generate_series(1, $2::bigint) g",
    )
    .bind(ex("Ceil"))
    .bind(ceiling + 1)
    .execute(&pool)
    .await
    .expect("seed");

    let s = PgEntityStore::new(pool.clone());
    // a dotted path is one of the shapes compile_q declines (see the pushdown
    // test above) — the request is undecided and therefore unpaged
    let ast = antares_ql::parse_q("address.city==\"Bonn\"").expect("parse");
    let err = match s.query(
        &t,
        &antares_sql::store::pg_entity::EntityFilter {
            q: Some(&ast),
            expand: &ex,
            ..Default::default()
        },
    ) {
        Ok(out) => panic!(
            "a cut result set must be refused, got {} rows (decided={}, paged={})",
            out.rows.len(),
            out.decided,
            out.paged
        ),
        Err(e) => e,
    };
    let ngsi =
        antares_sql::store::pg_entity::ngsi_error(&err).expect("a spec error, not a driver error");
    assert_eq!(ngsi.kind(), "TooManyResults");
    assert_eq!(
        ngsi.status(),
        403,
        "Table 6.3.2-1 status for TooManyResults"
    );

    // and the negative: the same oversized set answers a DECIDED paged query
    // exactly — the ceiling refuses statements, not tenants
    let groups = vec![vec![ex("Ceil")]];
    let out = s
        .query(
            &t,
            &antares_sql::store::pg_entity::EntityFilter {
                types: Some(&groups),
                page: Some(antares_sql::store::pg_entity::Page {
                    offset: 0,
                    limit: 5,
                }),
                expand: &ex,
                ..Default::default()
            },
        )
        .expect("a paged query over the same set is answerable");
    assert_eq!(out.rows.len(), 5, "exactly the requested page");
    assert!(out.paged && out.decided);
    assert_eq!(
        out.total,
        Some(ceiling + 1),
        "pre-LIMIT total, past the ceiling"
    );
    clean().await;
}
