//! PgDocStore integration. Skips loudly without
//! ANTARES_TEST_DATABASE_URL (see tests/pg.rs recipe).

use antares_model::TenantId;
use antares_sql::store::pg;
use antares_sql::store::pg::doc::{DocKind, PgDocStore};
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

#[tokio::test(flavor = "multi_thread")]
async fn doc_kinds_roundtrip_and_extract_bookkeeping() {
    let url = require_db!();
    let pool = pg::connect(&url, 5).await.expect("connect");
    let s = PgDocStore::new(pool.clone());
    let t = TenantId::new("pgdoc").expect("tenant");
    pg::ensure_tenant(&pool, &t).await.expect("tenant row");

    // Registration must be created BEFORE any csource_index rows reference it
    // (FK) — plain doc roundtrip here.
    for kind in [
        DocKind::Registration,
        DocKind::Subscription,
        DocKind::CSourceSubscription,
    ] {
        let id = format!("urn:x:{kind:?}");
        let _ = s.delete(&t, kind, &id);
        let doc = json!({"id": id, "type": "doc", "n": 1});
        assert!(
            !s.upsert(&t, kind, &id, &doc).expect("insert"),
            "fresh insert"
        );
        assert!(s
            .upsert(&t, kind, &id, &json!({"id": id, "n": 2}))
            .expect("update"));
        assert_eq!(s.get(&t, kind, &id).expect("get").expect("present")["n"], 2);
        assert_eq!(s.list(&t, kind).expect("list").len(), 1);
        // cross-tenant invisible
        let other = TenantId::new("pgdoc_other").expect("t");
        assert!(s.get(&other, kind, &id).expect("get").is_none());
        assert!(s.delete(&t, kind, &id).expect("delete"));
    }

    // Rows-are-truth: bookkeeping columns really extracted from the doc.
    let id = "urn:ngsi-ld:Subscription:bk";
    let _ = s.delete(&t, DocKind::Subscription, id);
    let doc = json!({
        "id": id, "type": "Subscription", "isActive": false,
        "expiresAt": "2027-01-01T00:00:00Z",
        "notification": {
            "timesSent": 7,
            "lastNotification": "2026-08-04T09:00:00Z",
            "lastSuccess": "2026-08-04T09:00:00Z"
        }
    });
    s.upsert(&t, DocKind::Subscription, id, &doc)
        .expect("upsert");
    let (active, sent) = s
        .status_row(&t, DocKind::Subscription, id)
        .expect("status")
        .expect("row");
    assert!(!active, "isActive:false extracted");
    assert_eq!(sent, 7, "notification.timesSent extracted");
    s.delete(&t, DocKind::Subscription, id).expect("cleanup");
}

/// `jsonld_contexts` is ONE cross-tenant keyspace and the Cached ceiling
/// evicts across the whole table, so the two tests that write rows there
/// cannot run concurrently: the capping test would evict the roundtrip test's
/// row out from under it.
static CONTEXT_ROWS: std::sync::LazyLock<tokio::sync::Mutex<()>> =
    std::sync::LazyLock::new(|| tokio::sync::Mutex::new(()));

#[tokio::test(flavor = "multi_thread")]
async fn jsonld_contexts_cross_tenant_roundtrip() {
    let url = require_db!();
    let _rows = CONTEXT_ROWS.lock().await;
    let pool = pg::connect(&url, 5).await.expect("connect");
    let s = PgDocStore::new(pool);
    let id = "https://example.org/ctx/test.jsonld";
    let _ = s.context_delete(id);
    s.context_put(id, &json!({"@context": {"n": "https://x/n"}}), "Cached")
        .expect("put");
    assert!(s.context_get(id).expect("get").is_some());
    assert!(s
        .context_list()
        .expect("list")
        .iter()
        .any(|c| c["@context"]["n"] == "https://x/n"));
    assert!(s.context_delete(id).expect("delete"));
    assert!(s.context_get(id).expect("get").is_none());
}

/// The 047_06 leftover-subscription bug: a bookkeeping writeback racing a
/// DELETE must never resurrect the row. mutate holds the row lock (FOR
/// UPDATE) for its whole read-modify-write, so whichever order the two land
/// in, the row is GONE afterwards — and a mutate after the delete is a None.
#[tokio::test(flavor = "multi_thread")]
async fn mutate_never_resurrects_a_deleted_row() {
    let url = require_db!();
    let pool = pg::connect(&url, 5).await.expect("connect");
    let s = std::sync::Arc::new(PgDocStore::new(pool.clone()));
    let t = TenantId::new("pgdoc_race").expect("tenant");
    pg::ensure_tenant(&pool, &t).await.expect("tenant row");
    let id = "urn:ngsi-ld:Subscription:race";

    // plain sequential: mutate after delete is a None, not an insert
    s.upsert(&t, DocKind::CSourceSubscription, id, &json!({"id": id}))
        .expect("insert");
    assert!(s.delete(&t, DocKind::CSourceSubscription, id).expect("del"));
    let r = s
        .mutate(&t, DocKind::CSourceSubscription, id, |d| {
            d["status"] = json!("failed");
            Ok::<(), ()>(())
        })
        .expect("mutate");
    assert!(r.is_none(), "mutate on a deleted row must be None");
    assert!(s
        .get(&t, DocKind::CSourceSubscription, id)
        .expect("get")
        .is_none());

    // racing: closure holds the row lock while a delete lands concurrently
    s.upsert(&t, DocKind::CSourceSubscription, id, &json!({"id": id}))
        .expect("insert");
    let (s1, s2) = (s.clone(), s.clone());
    let (t1, t2) = (t.clone(), t.clone());
    let m = tokio::task::spawn_blocking(move || {
        s1.mutate(&t1, DocKind::CSourceSubscription, id, |d| {
            std::thread::sleep(std::time::Duration::from_millis(300));
            d["status"] = json!("failed");
            Ok::<(), ()>(())
        })
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let d = tokio::task::spawn_blocking(move || s2.delete(&t2, DocKind::CSourceSubscription, id));
    let (m, d) = (m.await.expect("join"), d.await.expect("join"));
    m.expect("mutate ok");
    d.expect("delete ok");
    assert!(
        s.get(&t, DocKind::CSourceSubscription, id)
            .expect("get")
            .is_none(),
        "row must be gone after mutate+delete in any interleaving"
    );
}

/// 5.13.1: "@contexts implicitly and automatically fetched by the broker from
/// external URLs during normal NGSI-LD operations are flagged as 'Cached' …
/// Implementations shall periodically invalidate the 'Cached' @contexts."
/// One row is written per distinct URL a client references, so without a
/// ceiling a loop over fresh URLs grows the table forever — and the broker
/// warms every stored row at startup. Eviction is oldest-first and applies to
/// Cached rows ONLY: "Hosted" entries are the ones users explicitly added
/// (5.13.2/5.13.3) and losing one would delete client-owned data.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_13_1_cached_contexts_are_capped_oldest_first() {
    let url = require_db!();
    let _rows = CONTEXT_ROWS.lock().await;
    let pool = pg::connect(&url, 5).await.expect("connect");
    let s = PgDocStore::new(pool.clone());
    let cap = antares_sql::store::MAX_CACHED_CONTEXTS as i64;
    // the ceiling counts the whole (cross-tenant) table — start from empty so
    // the counts below are about this test's rows only
    sqlx::query("DELETE FROM jsonld_contexts")
        .execute(&pool)
        .await
        .expect("clean");
    // exactly `cap` Cached rows, ascending age: g = 1 is the oldest
    sqlx::query(
        "INSERT INTO jsonld_contexts (id, body, kind, created_at)
         SELECT 'ctxcap:' || g, '{}'::jsonb, 'Cached',
                now() - make_interval(secs => (100000 - g)::int)
           FROM generate_series(1, $1::bigint) g",
    )
    .bind(cap)
    .execute(&pool)
    .await
    .expect("seed cached");
    // …and one Hosted row OLDER than every Cached one: age alone must not
    // decide, the kind does
    sqlx::query(
        "INSERT INTO jsonld_contexts (id, body, kind, created_at)
         VALUES ('ctxcap:hosted', '{}'::jsonb, 'Hosted', now() - interval '200000 seconds')",
    )
    .execute(&pool)
    .await
    .expect("seed hosted");

    s.context_put("ctxcap:new", &json!({"@context": {}}), "Cached")
        .expect("put one over the ceiling");
    let count = |kind: &'static str| {
        let pool = pool.clone();
        async move {
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM jsonld_contexts WHERE kind = $1")
                .bind(kind)
                .fetch_one(&pool)
                .await
                .expect("count")
        }
    };
    assert_eq!(
        count("Cached").await,
        cap,
        "the ceiling holds after the put"
    );
    assert!(
        s.context_get("ctxcap:new").expect("get").is_some(),
        "the new entry is the one kept"
    );
    assert!(
        s.context_get("ctxcap:1").expect("get").is_none(),
        "the oldest Cached entry is the one evicted"
    );
    assert!(
        s.context_get("ctxcap:2").expect("get").is_some(),
        "eviction stops at the ceiling — the second-oldest stays"
    );
    assert!(
        s.context_get("ctxcap:hosted").expect("get").is_some(),
        "a Hosted entry is tenant-authored and must never be evicted"
    );

    // and a Hosted put never triggers eviction of anything
    s.context_put("ctxcap:hosted2", &json!({"@context": {}}), "Hosted")
        .expect("put hosted");
    assert_eq!(count("Hosted").await, 2, "both Hosted entries stay");
    assert_eq!(count("Cached").await, cap, "a Hosted put evicts nothing");

    sqlx::query("DELETE FROM jsonld_contexts")
        .execute(&pool)
        .await
        .expect("cleanup");
}

/// 5.12 registration matching: the store narrows the candidate set with the
/// `csource_index` rows the registration writes, instead of listing every
/// registration document for the tenant. The narrowing is one-directional —
/// only rows the Rust matcher would reject anyway are removed, so an index
/// dimension left NULL (unconstrained) always survives, and an `idPattern`
/// row survives every id query because only the matcher owns the regex.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_12_matching_registrations_narrows_by_type_and_id() {
    let url = require_db!();
    let pool = pg::connect(&url, 5).await.expect("pool");
    let t = TenantId::new("pgcsmatch").expect("tenant");
    let other = TenantId::new("pgcsmatch_other").expect("tenant");
    for t in [&t, &other] {
        pg::ensure_tenant(&pool, t).await.expect("tenant row");
    }
    let s = PgDocStore::new(pool.clone());
    let reg = |id: &str, info: serde_json::Value| {
        json!({"id": id, "type": "ContextSourceRegistration",
               "endpoint": "http://cs:9090", "information": [info]})
    };
    let seed = [
        // type only
        (
            "urn:csr:m:vehicle",
            json!({"entities": [{"type": "Vehicle"}]}),
        ),
        // explicit id + type
        (
            "urn:csr:m:room1",
            json!({"entities": [{"id": "urn:e:room1", "type": "Room"}]}),
        ),
        // another explicit id, same type as the first
        (
            "urn:csr:m:vehicle9",
            json!({"entities": [{"id": "urn:e:v9", "type": "Vehicle"}]}),
        ),
        // idPattern, no type
        (
            "urn:csr:m:pattern",
            json!({"entities": [{"idPattern": "urn:e:.*"}]}),
        ),
        // attributes only: no entity dimension at all
        ("urn:csr:m:attrs", json!({"propertyNames": ["speed"]})),
    ];
    for (id, info) in &seed {
        let _ = s.delete(&t, DocKind::Registration, id);
        s.upsert(&t, DocKind::Registration, id, &reg(id, info.clone()))
            .expect("seed");
    }
    // a same-shaped registration in a DIFFERENT tenant must never surface
    let foreign = "urn:csr:m:foreign";
    let _ = s.delete(&other, DocKind::Registration, foreign);
    s.upsert(
        &other,
        DocKind::Registration,
        foreign,
        &reg(foreign, json!({"entities": [{"type": "Vehicle"}]})),
    )
    .expect("seed foreign");

    let ids_of = |ids: Option<Vec<String>>, types: Option<Vec<String>>| {
        let mut got: Vec<String> = s
            .matching_registrations(&t, ids.as_deref(), types.as_deref())
            .expect("matching")
            .iter()
            .map(|d| d["id"].as_str().unwrap_or_default().to_owned())
            .collect();
        got.sort();
        got
    };
    let ty = |t: &str| Some(vec![t.to_owned()]);

    // no narrowing at all: every registration of this tenant, and nothing else
    assert_eq!(
        ids_of(None, None),
        [
            "urn:csr:m:attrs",
            "urn:csr:m:pattern",
            "urn:csr:m:room1",
            "urn:csr:m:vehicle",
            "urn:csr:m:vehicle9"
        ]
    );

    // by type: the Room registration drops out; the unconstrained ones stay
    assert_eq!(
        ids_of(None, ty("Vehicle")),
        [
            "urn:csr:m:attrs",
            "urn:csr:m:pattern",
            "urn:csr:m:vehicle",
            "urn:csr:m:vehicle9"
        ]
    );
    // a type nobody registered for keeps only the type-less registrations
    assert_eq!(
        ids_of(None, ty("Bridge")),
        ["urn:csr:m:attrs", "urn:csr:m:pattern"]
    );

    // by id: a registration bound to a DIFFERENT explicit id drops out,
    // the idPattern row survives (the regex is the matcher's business)
    assert_eq!(
        ids_of(Some(vec!["urn:e:room1".into()]), None),
        [
            "urn:csr:m:attrs",
            "urn:csr:m:pattern",
            "urn:csr:m:room1",
            "urn:csr:m:vehicle"
        ]
    );

    // both dimensions compose
    assert_eq!(
        ids_of(Some(vec!["urn:e:v9".into()]), ty("Vehicle")),
        [
            "urn:csr:m:attrs",
            "urn:csr:m:pattern",
            "urn:csr:m:vehicle",
            "urn:csr:m:vehicle9"
        ]
    );

    for (id, _) in &seed {
        s.delete(&t, DocKind::Registration, id).expect("cleanup");
    }
    s.delete(&other, DocKind::Registration, foreign)
        .expect("cleanup");
}

/// Registration writes rebuild csource_index in the same transaction;
/// deleting the registration cascades its rows away.
#[tokio::test(flavor = "multi_thread")]
async fn registration_maintains_csource_index() {
    let url = require_db!();
    let pool = pg::connect(&url, 5).await.expect("pool");
    let t = TenantId::new("pgcsidx").expect("tenant");
    pg::ensure_tenant(&pool, &t).await.expect("tenant row");
    let s = PgDocStore::new(pool.clone());

    let reg = serde_json::json!({
        "id": "urn:csr:idx1", "type": "ContextSourceRegistration",
        "endpoint": "http://cs1:9090",
        "mode": "redirect",
        "operations": ["retrieveOps"],
        "information": [{
            "entities": [{"id": "urn:e:1", "type": "T"}],
            "propertyNames": ["speed", "heading"]
        }]
    });
    s.upsert(&t, DocKind::Registration, "urn:csr:idx1", &reg)
        .expect("upsert");
    let count = |pool: &sqlx::PgPool| {
        let pool = pool.clone();
        async move {
            sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM csource_index WHERE tenant_id = 'pgcsidx' AND registration_id = 'urn:csr:idx1'",
            )
            .fetch_one(&pool)
            .await
            .expect("count")
        }
    };
    assert_eq!(count(&pool).await, 2, "one row per propertyName");
    let mode: i16 = sqlx::query_scalar(
        "SELECT mode FROM csource_index WHERE tenant_id = 'pgcsidx' AND registration_id = 'urn:csr:idx1' LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("mode");
    assert_eq!(mode, 2, "redirect");

    // update narrows the info: rows are REBUILT, not appended
    let reg2 = serde_json::json!({
        "id": "urn:csr:idx1", "type": "ContextSourceRegistration",
        "endpoint": "http://cs1:9090",
        "information": [{"entities": [{"id": "urn:e:1", "type": "T"}]}]
    });
    s.upsert(&t, DocKind::Registration, "urn:csr:idx1", &reg2)
        .expect("update");
    assert_eq!(count(&pool).await, 1, "rebuilt to the narrowed shape");

    // delete cascades the index rows away (FK ON DELETE CASCADE)
    assert!(s
        .delete(&t, DocKind::Registration, "urn:csr:idx1")
        .expect("delete"));
    assert_eq!(count(&pool).await, 0, "cascade cleaned the index");
    // A registration's own `location` becomes an indexed geometry so
    // federation matching can be a GIST lookup rather than a scan.
    let geo_id = "urn:ngsi-ld:ContextSourceRegistration:geo1";
    let geo_reg = serde_json::json!({
        "id": geo_id, "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": "Vehicle"}]}],
        "endpoint": "http://peer.example/ngsi-ld/v1",
        "location": {"type": "Polygon",
                     "coordinates": [[[0, 0], [4, 0], [4, 4], [0, 4], [0, 0]]]}
    });
    s.upsert(&t, DocKind::Registration, geo_id, &geo_reg)
        .expect("upsert geo reg");
    let inside: bool = sqlx::query_scalar(
        "SELECT bool_or(ST_Within(ST_SetSRID(ST_Point(2, 2), 4326), location))
           FROM csource_index WHERE tenant_id = $1 AND registration_id = $2",
    )
    .bind(t.as_str())
    .bind(geo_id)
    .fetch_one(&pool)
    .await
    .expect("geo query");
    assert!(inside, "the registration geometry must be queryable in SQL");
}
