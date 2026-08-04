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
