//! PgEntityStore integration (tasks.md C5 first slice + §3.1 concurrency).
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

    assert!(s.delete(&t, id).expect("delete"));
    assert!(s.get(&t, id).expect("get").is_none());
    assert!(s
        .mutate(&t, id, |_| Ok::<(), ()>(()))
        .expect("mutate absent")
        .is_none());
}

/// §3.1 / §9.5: parallel PATCH storm against ONE entity — no lost updates,
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

/// C5: batch create/delete as single multi-row statements — created flags in
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

// ---- C10: query pushdown -------------------------------------------------
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
        ids_of(&s.query(&t, f).expect("query"))
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
