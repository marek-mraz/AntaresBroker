// SPDX-License-Identifier: EUPL-1.2
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

/// `list_page` is the read the must-see-everything callers use, so its
/// contract is: ids strictly greater than `after`, id-ordered, at most
/// `limit`, a short page means the end — and NO row ceiling, because a page
/// bounds the allocation by construction and refusing here would let one
/// tenant's stored volume decide whether another tenant is served at all.
#[tokio::test(flavor = "multi_thread")]
async fn list_page_walks_every_row_in_id_order_without_a_ceiling() {
    let url = require_db!();
    let pool = pg::connect(&url, 5).await.expect("pool");
    let s = PgDocStore::new(pool.clone());
    let t = TenantId::new("pgpage").expect("tenant");
    pg::ensure_tenant(&pool, &t).await.expect("tenant row");
    sqlx::query("DELETE FROM subscriptions WHERE tenant_id = 'pgpage'")
        .execute(&pool)
        .await
        .expect("clean");

    for i in 0..25 {
        let id = format!("urn:ngsi-ld:Subscription:page-{i:03}");
        s.upsert(
            &t,
            DocKind::Subscription,
            &id,
            &json!({"id": id, "type": "Subscription"}),
        )
        .await
        .expect("insert");
    }

    // Walk it the way the mirror seed does.
    let mut seen: Vec<String> = Vec::new();
    let mut after: Option<String> = None;
    loop {
        let page = s
            .list_page(&t, DocKind::Subscription, after.as_deref(), 10)
            .await
            .expect("page");
        let short = page.len() < 10;
        for d in &page {
            let id = d["id"].as_str().expect("id").to_owned();
            after = Some(id.clone());
            seen.push(id);
        }
        if short {
            break;
        }
    }
    assert_eq!(seen.len(), 25, "every row, exactly once: {}", seen.len());
    let mut sorted = seen.clone();
    sorted.sort();
    assert_eq!(seen, sorted, "id order");
    sorted.dedup();
    assert_eq!(sorted.len(), 25, "no row served twice");

    // `after` is EXCLUSIVE: the row it names is not repeated.
    let page = s
        .list_page(&t, DocKind::Subscription, Some(&seen[0]), 5)
        .await
        .expect("page");
    assert_eq!(page[0]["id"].as_str(), Some(seen[1].as_str()), "{page:?}");

    // Past the end is empty, not an error.
    let page = s
        .list_page(
            &t,
            DocKind::Subscription,
            Some("urn:ngsi-ld:Subscription:zzz"),
            10,
        )
        .await
        .expect("page");
    assert!(page.is_empty(), "{page:?}");

    sqlx::query("DELETE FROM subscriptions WHERE tenant_id = 'pgpage'")
        .execute(&pool)
        .await
        .expect("cleanup");
}

/// The walk is keyset — `WHERE id > $after ORDER BY id` — so the comparison
/// that cuts the page and the one that orders it have to be the SAME
/// comparison. The `id` column carries the database collation, which is not
/// byte order: under `en_US.utf8` an underscore and letter case sort where a
/// `memcmp` would not. A cursor compared one way and a sort done the other
/// loses rows in the middle of the walk and never says so, and this walk is
/// the one that seeds the subscription mirror — a lost row is a subscription
/// that exists and never notifies. Ids that the two orders disagree about,
/// walked one page at a time.
#[tokio::test(flavor = "multi_thread")]
async fn the_page_cursor_and_the_page_order_are_the_same_comparison() {
    let url = require_db!();
    let pool = pg::connect(&url, 5).await.expect("pool");
    let s = PgDocStore::new(pool.clone());
    let t = TenantId::new("pgcollate").expect("tenant");
    pg::ensure_tenant(&pool, &t).await.expect("tenant row");
    sqlx::query("DELETE FROM subscriptions WHERE tenant_id = 'pgcollate'")
        .execute(&pool)
        .await
        .expect("clean");

    // Case, underscore, hyphen and digits: byte order puts every upper-case
    // letter before every lower-case one and `_` between them, the collation
    // does neither. The last two also carry the LIKE metacharacters and a
    // quote, which no statement on this path may treat as anything but text.
    let ids = [
        "urn:ngsi-ld:Subscription:Alpha",
        "urn:ngsi-ld:Subscription:alpha",
        "urn:ngsi-ld:Subscription:_alpha",
        "urn:ngsi-ld:Subscription:alpha-2",
        "urn:ngsi-ld:Subscription:alpha_2",
        "urn:ngsi-ld:Subscription:ALPHA2",
        "urn:ngsi-ld:Subscription:100%_x",
        "urn:ngsi-ld:Subscription:o'brien",
    ];
    for id in ids {
        s.upsert(
            &t,
            DocKind::Subscription,
            id,
            &json!({"id": id, "type": "Subscription"}),
        )
        .await
        .expect("seed");
    }

    let mut seen: Vec<String> = Vec::new();
    let mut after: Option<String> = None;
    loop {
        let page = s
            .list_page(&t, DocKind::Subscription, after.as_deref(), 3)
            .await
            .expect("page");
        let short = page.len() < 3;
        for d in &page {
            let id = d["id"].as_str().expect("id").to_owned();
            after = Some(id.clone());
            seen.push(id);
        }
        if short {
            break;
        }
    }
    let mut once = seen.clone();
    once.sort();
    once.dedup();
    assert_eq!(
        (seen.len(), once.len()),
        (ids.len(), ids.len()),
        "the walk lost or repeated a row: {seen:?}"
    );

    // Ordered by the DATABASE's comparison, which is the one the statement
    // used — asserting Rust's byte order here would assert the bug.
    for w in seen.windows(2) {
        let ordered: bool = sqlx::query_scalar("SELECT $1::text < $2::text")
            .bind(&w[0])
            .bind(&w[1])
            .fetch_one(&pool)
            .await
            .expect("compare");
        assert!(ordered, "{:?} was served before {:?}", w[0], w[1]);
    }

    // A page wider than the set ends the walk in one read, and the narrowest
    // page that still advances reaches the last row.
    assert_eq!(
        s.list_page(&t, DocKind::Subscription, None, 100)
            .await
            .expect("wide")
            .len(),
        ids.len()
    );
    let mut cursor = None;
    let mut steps = 0;
    while let Some(d) = s
        .list_page(&t, DocKind::Subscription, cursor.as_deref(), 1)
        .await
        .expect("one")
        .pop()
    {
        cursor = Some(d["id"].as_str().expect("id").to_owned());
        steps += 1;
    }
    assert_eq!(steps, ids.len(), "a page of one walked {steps} rows");

    for id in ids {
        assert!(
            s.delete(&t, DocKind::Subscription, id)
                .await
                .expect("delete"),
            "the row is addressed by its own id: {id}"
        );
    }
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
        let _ = s.delete(&t, kind, &id).await;
        let doc = json!({"id": id, "type": "doc", "n": 1});
        assert!(
            !s.upsert(&t, kind, &id, &doc).await.expect("insert"),
            "fresh insert"
        );
        assert!(s
            .upsert(&t, kind, &id, &json!({"id": id, "n": 2}))
            .await
            .expect("update"));
        assert_eq!(
            s.get(&t, kind, &id).await.expect("get").expect("present")["n"],
            2
        );
        assert_eq!(s.list(&t, kind).await.expect("list").len(), 1);
        // cross-tenant invisible
        let other = TenantId::new("pgdoc_other").expect("t");
        assert!(s.get(&other, kind, &id).await.expect("get").is_none());
        assert!(s.delete(&t, kind, &id).await.expect("delete"));
    }

    // Rows-are-truth: bookkeeping columns really extracted from the doc.
    let id = "urn:ngsi-ld:Subscription:bk";
    let _ = s.delete(&t, DocKind::Subscription, id).await;
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
        .await
        .expect("upsert");
    let (active, sent) = s
        .status_row(&t, DocKind::Subscription, id)
        .await
        .expect("status")
        .expect("row");
    assert!(!active, "isActive:false extracted");
    assert_eq!(sent, 7, "notification.timesSent extracted");
    s.delete(&t, DocKind::Subscription, id)
        .await
        .expect("cleanup");
}

/// `jsonld_contexts` is ONE cross-tenant keyspace and the Cached ceiling
/// evicts across the whole table, so the two tests that write rows there
/// cannot run concurrently: the capping test would evict the roundtrip test's
/// row out from under it.
static CONTEXT_ROWS: std::sync::LazyLock<tokio::sync::Mutex<()>> =
    std::sync::LazyLock::new(|| tokio::sync::Mutex::new(()));

#[tokio::test(flavor = "multi_thread")]
async fn jsonld_contexts_roundtrip() {
    let url = require_db!();
    let _rows = CONTEXT_ROWS.lock().await;
    let pool = pg::connect(&url, 5).await.expect("connect");
    let s = PgDocStore::new(pool);
    // A Cached row belongs to no Tenant (ADR-0021) and is written and read
    // with none; the Hosted probe below carries no `owner` member, so its
    // generated tenant is the default Tenant's.
    let owner = TenantId::default();
    let id = "https://example.org/ctx/test.jsonld";
    let _ = s.context_delete(None, id).await;
    s.context_put(
        None,
        id,
        &json!({"@context": {"n": "https://x/n"}}),
        "Cached",
    )
    .await
    .expect("put");
    assert!(s.context_get(None, id).await.expect("get").is_some());
    // The listing read carries metadata and never the stored document: a
    // body is accepted up to 5 MiB and only the Cached rows are capped in
    // number, so a read that carried bodies was gigabytes on the boot path.
    // The row is found by id, and the document comes back only from `get`.
    s.context_put(
        Some(&owner),
        "urn:meta:probe",
        &json!({"localId": "urn:meta:probe", "kind": "Hosted",
                "body": {"@context": {"big": "https://x/big"}}}),
        "Hosted",
    )
    .await
    .expect("put");
    let meta = s.context_list_meta(Some(&owner)).await.expect("list");
    let probe = meta
        .iter()
        .find(|c| c["localId"] == "urn:meta:probe")
        .expect("the row is listed by its metadata");
    assert!(
        probe.get("body").is_none(),
        "the metadata read carried the @context document: {probe}"
    );
    assert_eq!(probe["kind"], "Hosted", "the metadata itself survives");
    assert_eq!(
        s.context_get(Some(&owner), "urn:meta:probe")
            .await
            .expect("get")
            .expect("row")["body"]["@context"]["big"],
        "https://x/big",
        "the document is still readable by id"
    );
    assert!(s
        .context_delete(Some(&owner), "urn:meta:probe")
        .await
        .expect("delete"));
    assert!(s.context_delete(None, id).await.expect("delete"));
    assert!(s.context_get(None, id).await.expect("get").is_none());
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
        .await
        .expect("insert");
    assert!(s
        .delete(&t, DocKind::CSourceSubscription, id)
        .await
        .expect("del"));
    let r = s
        .mutate(&t, DocKind::CSourceSubscription, id, |d| {
            d["status"] = json!("failed");
            Ok::<(), ()>(())
        })
        .await
        .expect("mutate");
    assert!(r.is_none(), "mutate on a deleted row must be None");
    assert!(s
        .get(&t, DocKind::CSourceSubscription, id)
        .await
        .expect("get")
        .is_none());

    // racing: closure holds the row lock while a delete lands concurrently
    s.upsert(&t, DocKind::CSourceSubscription, id, &json!({"id": id}))
        .await
        .expect("insert");
    let (s1, s2) = (s.clone(), s.clone());
    let (t1, t2) = (t.clone(), t.clone());
    let m = tokio::spawn(async move {
        s1.mutate(&t1, DocKind::CSourceSubscription, id, |d| {
            std::thread::sleep(std::time::Duration::from_millis(300));
            d["status"] = json!("failed");
            Ok::<(), ()>(())
        })
        .await
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let d = tokio::spawn(async move { s2.delete(&t2, DocKind::CSourceSubscription, id).await });
    let (m, d) = (m.await.expect("join"), d.await.expect("join"));
    m.expect("mutate ok");
    d.expect("delete ok");
    assert!(
        s.get(&t, DocKind::CSourceSubscription, id)
            .await
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

    s.context_put(None, "ctxcap:new", &json!({"@context": {}}), "Cached")
        .await
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
        s.context_get(None, "ctxcap:new")
            .await
            .expect("get")
            .is_some(),
        "the new entry is the one kept"
    );
    assert!(
        s.context_get(None, "ctxcap:1")
            .await
            .expect("get")
            .is_none(),
        "the oldest Cached entry is the one evicted"
    );
    assert!(
        s.context_get(None, "ctxcap:2")
            .await
            .expect("get")
            .is_some(),
        "eviction stops at the ceiling — the second-oldest stays"
    );
    assert!(
        s.context_get(Some(&TenantId::default()), "ctxcap:hosted")
            .await
            .expect("get")
            .is_some(),
        "a Hosted entry is tenant-authored and must never be evicted"
    );

    // and a Hosted put never triggers eviction of anything
    s.context_put(
        Some(&TenantId::default()),
        "ctxcap:hosted2",
        &json!({"@context": {}}),
        "Hosted",
    )
    .await
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
        let _ = s.delete(&t, DocKind::Registration, id).await;
        s.upsert(&t, DocKind::Registration, id, &reg(id, info.clone()))
            .await
            .expect("seed");
    }
    // a same-shaped registration in a DIFFERENT tenant must never surface
    let foreign = "urn:csr:m:foreign";
    let _ = s.delete(&other, DocKind::Registration, foreign).await;
    s.upsert(
        &other,
        DocKind::Registration,
        foreign,
        &reg(foreign, json!({"entities": [{"type": "Vehicle"}]})),
    )
    .await
    .expect("seed foreign");

    // a macro, not a closure: the query is awaited and an async closure is not
    // a stable language feature
    macro_rules! ids_of {
        ($ids:expr, $types:expr) => {{
            let ids: Option<Vec<String>> = $ids;
            let types: Option<Vec<String>> = $types;
            let mut got: Vec<String> = s
                .matching_registrations(&t, ids.as_deref(), types.as_deref())
                .await
                .expect("matching")
                .iter()
                .map(|d| d["id"].as_str().unwrap_or_default().to_owned())
                .collect();
            got.sort();
            got
        }};
    }
    let ty = |t: &str| Some(vec![t.to_owned()]);

    // no narrowing at all: every registration of this tenant, and nothing else
    assert_eq!(
        ids_of!(None, None),
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
        ids_of!(None, ty("Vehicle")),
        [
            "urn:csr:m:attrs",
            "urn:csr:m:pattern",
            "urn:csr:m:vehicle",
            "urn:csr:m:vehicle9"
        ]
    );
    // a type nobody registered for keeps only the type-less registrations
    assert_eq!(
        ids_of!(None, ty("Bridge")),
        ["urn:csr:m:attrs", "urn:csr:m:pattern"]
    );

    // by id: a registration bound to a DIFFERENT explicit id drops out,
    // the idPattern row survives (the regex is the matcher's business)
    assert_eq!(
        ids_of!(Some(vec!["urn:e:room1".into()]), None),
        [
            "urn:csr:m:attrs",
            "urn:csr:m:pattern",
            "urn:csr:m:room1",
            "urn:csr:m:vehicle"
        ]
    );

    // both dimensions compose
    assert_eq!(
        ids_of!(Some(vec!["urn:e:v9".into()]), ty("Vehicle")),
        [
            "urn:csr:m:attrs",
            "urn:csr:m:pattern",
            "urn:csr:m:vehicle",
            "urn:csr:m:vehicle9"
        ]
    );

    for (id, _) in &seed {
        s.delete(&t, DocKind::Registration, id)
            .await
            .expect("cleanup");
    }
    s.delete(&other, DocKind::Registration, foreign)
        .await
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
        .await
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
        .await
        .expect("update");
    assert_eq!(count(&pool).await, 1, "rebuilt to the narrowed shape");

    // delete cascades the index rows away (FK ON DELETE CASCADE)
    assert!(s
        .delete(&t, DocKind::Registration, "urn:csr:idx1")
        .await
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
        .await
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

/// 5.2.14.2 delivery bookkeeping: `record_delivery` hands back the
/// `lastSuccess` it overwrote, and a failed attempt puts that value back.
/// Under fan-out two attempts on ONE subscription run at once, so "the value
/// it overwrote" has to be the one that was there when the UPDATE landed —
/// not the one that was there when the statement began. A pre-image read from
/// the statement's own snapshot is the older of the two whenever another
/// attempt commits in between, and the rollback then rewinds `lastSuccess`
/// past a delivery that did succeed.
#[tokio::test(flavor = "multi_thread")]
async fn record_delivery_returns_the_pre_image_it_actually_overwrote() {
    let url = require_db!();
    let pool = pg::connect(&url, 5).await.expect("pool");
    let s = std::sync::Arc::new(PgDocStore::new(pool.clone()));
    let t = TenantId::new("pgpreimage").expect("tenant");
    pg::ensure_tenant(&pool, &t).await.expect("tenant row");
    let id = "urn:ngsi-ld:Subscription:preimage";
    let (t0, t1, t2) = (
        "2026-01-01T00:00:00.000Z",
        "2026-01-01T00:00:01.000Z",
        "2026-01-01T00:00:02.000Z",
    );
    s.upsert(
        &t,
        DocKind::Subscription,
        id,
        &json!({"id": id, "type": "Subscription",
                "notification": {"endpoint": {"uri": "http://127.0.0.1:9"},
                                 "lastSuccess": t0}}),
    )
    .await
    .expect("seed");

    // The other attempt's UPDATE, landed but not yet committed: it holds the
    // row lock, so the delivery below blocks with its snapshot already taken.
    let mut holder = pool.begin().await.expect("begin");
    sqlx::query(
        "UPDATE subscriptions
            SET subscription = jsonb_set(subscription, '{notification,lastSuccess}',
                                         to_jsonb($3::text))
          WHERE tenant_id = $1 AND id = $2",
    )
    .bind(t.as_str())
    .bind(id)
    .bind(t1)
    .execute(&mut *holder)
    .await
    .expect("the competing attempt writes");

    let (s2, t2s) = (s.clone(), t.clone());
    let recording = tokio::spawn(async move {
        s2.record_delivery(&t2s, DocKind::Subscription, id, t2)
            .await
    });
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert!(
        !recording.is_finished(),
        "the delivery stamp did not wait for the row lock"
    );
    holder.commit().await.expect("commit the competing attempt");

    let (doc, prev) = recording
        .await
        .expect("join")
        .expect("record_delivery")
        .expect("the subscription is still there");
    assert_eq!(
        prev.as_ref().and_then(serde_json::Value::as_str),
        Some(t1),
        "the pre-image is the value this attempt overwrote, not the one its \
         statement started with — a rollback to {t0:?} would erase a delivery \
         that succeeded"
    );
    assert_eq!(doc["notification"]["lastSuccess"], json!(t2));
    assert_eq!(doc["notification"]["timesSent"], json!(1));
    let _ = s.delete(&t, DocKind::Subscription, id).await;
}
