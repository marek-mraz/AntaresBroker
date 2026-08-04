//! PgDocStore integration (tasks.md C6/C7/C8 partial). Skips loudly without
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
    for kind in [DocKind::Registration, DocKind::Subscription, DocKind::CSourceSubscription] {
        let id = format!("urn:x:{kind:?}");
        let _ = s.delete(&t, kind, &id);
        let doc = json!({"id": id, "type": "doc", "n": 1});
        assert!(!s.upsert(&t, kind, &id, &doc).expect("insert"), "fresh insert");
        assert!(s.upsert(&t, kind, &id, &json!({"id": id, "n": 2})).expect("update"));
        assert_eq!(s.get(&t, kind, &id).expect("get").expect("present")["n"], 2);
        assert_eq!(s.list(&t, kind).expect("list").len(), 1);
        // cross-tenant invisible
        let other = TenantId::new("pgdoc_other").expect("t");
        assert!(s.get(&other, kind, &id).expect("get").is_none());
        assert!(s.delete(&t, kind, &id).expect("delete"));
    }

    // §14.1 rows-are-truth: bookkeeping columns really extracted from the doc.
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
    s.upsert(&t, DocKind::Subscription, id, &doc).expect("upsert");
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
    assert!(s.context_list().expect("list").iter().any(|c| c["@context"]["n"] == "https://x/n"));
    assert!(s.context_delete(id).expect("delete"));
    assert!(s.context_get(id).expect("get").is_none());
}
