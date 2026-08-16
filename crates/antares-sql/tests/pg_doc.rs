//! PgDocStore integration. Skips loudly without
//! ANTARES_TEST_DATABASE_URL (see tests/pg.rs recipe).

use antares_model::TenantId;
use antares_sql::pg;
use antares_sql::store::pg_doc::{DocKind, PgDocStore};
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

#[tokio::test(flavor = "multi_thread")]
async fn jsonld_contexts_cross_tenant_roundtrip() {
    let url = require_db!();
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
