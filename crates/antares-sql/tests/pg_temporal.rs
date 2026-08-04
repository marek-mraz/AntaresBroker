//! PgTemporalStore integration (tasks.md §C-ii temporal bridge). Skips
//! loudly without ANTARES_TEST_DATABASE_URL (see tests/pg.rs recipe).

use antares_model::TenantId;
use antares_sql::pg;
use antares_sql::store::pg_temporal::PgTemporalStore;
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
async fn temporal_doc_roundtrip_and_instance_append() {
    let url = require_db!();
    let pool = pg::connect(&url, 5).await.expect("connect");
    let s = PgTemporalStore::new(pool.clone());
    let t = TenantId::new("pgtemp").expect("tenant");
    pg::ensure_tenant(&pool, &t).await.expect("tenant row");
    let id = "urn:ngsi-ld:Vehicle:temp1";
    let _ = s.delete(&t, id);

    let attr = "https://uri.etsi.org/ngsi-ld/default-context/speed";
    let doc = json!({
        "id": id, "type": "Vehicle",
        "createdAt": "2026-08-04T09:00:00Z", "modifiedAt": "2026-08-04T09:00:00Z",
        attr: [{"type": "Property", "value": 80, "observedAt": "2026-08-04T09:00:00Z",
                "instanceId": "urn:ngsi-ld:Instance:1"}]
    });
    assert!(s.create(&t, id, &doc).expect("create"));
    assert!(!s.create(&t, id, &doc).expect("dup"), "existing id → false");

    // byte-faithful doc roundtrip (the property the bridge must hold)
    assert_eq!(s.get(&t, id).expect("get").expect("present"), doc);

    // append an instance under the row lock — the 5.6.12 shape
    let r = s.mutate(&t, id, |d| {
        d[attr]
            .as_array_mut()
            .expect("instance array")
            .push(json!({"type": "Property", "value": 85,
                         "observedAt": "2026-08-04T09:05:00Z",
                         "instanceId": "urn:ngsi-ld:Instance:2"}));
        d["modifiedAt"] = json!("2026-08-04T09:05:00Z");
        Ok::<(), ()>(())
    });
    assert!(matches!(r, Ok(Some(Ok(())))));
    let got = s.get(&t, id).expect("get").expect("present");
    assert_eq!(got[attr].as_array().expect("arr").len(), 2);

    // cross-tenant invisible; list sees exactly one
    let other = TenantId::new("pgtemp_other").expect("t");
    assert!(s.get(&other, id).expect("get").is_none());
    assert_eq!(s.list(&t).expect("list").len(), 1);

    assert!(s.delete(&t, id).expect("delete"));
    assert!(s.get(&t, id).expect("get").is_none());
}
