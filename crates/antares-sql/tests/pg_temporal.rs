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
        d[attr].as_array_mut().expect("instance array").push(
            json!({"type": "Property", "value": 85,
                         "observedAt": "2026-08-04T09:05:00Z",
                         "instanceId": "urn:ngsi-ld:Instance:2"}),
        );
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

/// C9: the bridge doc decomposes into attr_instances rows in the same tx,
/// and the maintenance pass creates plain-mode partitions under the
/// SKIP LOCKED claim (§3.1.6).
#[tokio::test(flavor = "multi_thread")]
async fn attr_instances_decomposition_and_maintenance() {
    let url = require_db!();
    let pool = pg::connect(&url, 5).await.expect("pool");
    let t = TenantId::new("pgtempdec").expect("tenant");
    pg::ensure_tenant(&pool, &t).await.expect("tenant row");
    let s = PgTemporalStore::new(pool.clone());

    let doc = serde_json::json!({
        "id": "urn:t:dec1", "type": ["T"],
        "createdAt": "2026-08-04T10:00:00Z", "modifiedAt": "2026-08-04T10:00:00Z",
        "https://ex.org/speed": [
            {"type": "Property", "value": 1, "instanceId": "urn:i:1",
             "observedAt": "2026-08-01T00:00:00Z", "modifiedAt": "2026-08-04T10:00:00Z"},
            {"type": "Property", "value": 2, "instanceId": "urn:i:2",
             "datasetId": "urn:ds:1", "modifiedAt": "2026-08-04T10:00:00Z"}
        ]
    });
    assert!(s.create(&t, "urn:t:dec1", &doc).expect("create"));
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM attr_instances WHERE tenant_id = 'pgtempdec' AND entity_id = 'urn:t:dec1'",
    )
    .fetch_one(&pool)
    .await
    .expect("count");
    assert_eq!(n, 2, "one row per instance");
    // observed_at falls back to modified_at when absent (§8.2)
    let obs: String = sqlx::query_scalar(
        "SELECT observed_at::text FROM attr_instances
         WHERE tenant_id = 'pgtempdec' AND instance_id = 'urn:i:2'",
    )
    .fetch_one(&pool)
    .await
    .expect("obs");
    assert!(
        obs.starts_with("2026-08-04"),
        "fallback to modifiedAt: {obs}"
    );

    // delete cleans the instances (no FK on a partitioned table)
    assert!(s.delete(&t, "urn:t:dec1").expect("delete"));
    let n: i64 =
        sqlx::query_scalar("SELECT count(*) FROM attr_instances WHERE tenant_id = 'pgtempdec'")
            .fetch_one(&pool)
            .await
            .expect("count2");
    assert_eq!(n, 0);

    // maintenance: plain mode pre-creates weekly partitions, single-winner;
    // timescale mode has chunks instead and reports a no-op without a
    // retention horizon.
    let ts = antares_sql::maintenance::timescale_present(&pool)
        .await
        .expect("detect");
    let msg = antares_sql::maintenance::temporal_maintenance(&pool, None)
        .await
        .expect("maintenance");
    if ts {
        assert!(
            msg.contains("nothing to do"),
            "timescale, no retention: {msg}"
        );
    } else {
        assert!(msg.contains("partition"), "plain-mode partitions: {msg}");
        let parts: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_inherits i JOIN pg_class p ON p.oid = i.inhparent
             WHERE p.relname = 'attr_instances'",
        )
        .fetch_one(&pool)
        .await
        .expect("parts");
        assert!(parts >= 5, "default + >=4 weekly partitions, got {parts}");
    }
}
