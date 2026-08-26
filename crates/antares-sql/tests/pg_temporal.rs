//! PgTemporalStore integration (the temporal bridge). Skips
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

/// `occupied_range_…` drops the current week's partition as setup — destroying
/// any sibling's rows parked there (the decomposition doc's modifiedAt
/// fallback lands in the current week whenever "now" crosses into its ISO
/// week) — and conversely a sibling's maintenance pass can recreate the
/// partition it just dropped. Tests that insert into or DDL the current-week
/// partition therefore serialize on this lock.
static PARTITION_DDL: std::sync::LazyLock<tokio::sync::Mutex<()>> =
    std::sync::LazyLock::new(|| tokio::sync::Mutex::new(()));

/// Maintenance is single-winner by design (`FOR UPDATE SKIP LOCKED`),
/// so a concurrent caller legitimately gets "skipped" — including a sibling
/// test. A real deployment retries on the next tick; so do we, rather than
/// weakening an assertion about what a winning pass must do.
async fn maintenance_winning(
    pool: &sqlx::PgPool,
    retention_days: Option<i64>,
) -> Result<String, sqlx::Error> {
    let backend = antares_sql::maintenance::detect_temporal_backend(pool)
        .await
        .expect("temporal backend");
    for _ in 0..50 {
        let msg =
            antares_sql::maintenance::temporal_maintenance(pool, backend, retention_days).await?;
        if !msg.starts_with("skipped") {
            return Ok(msg);
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("never won the maintenance claim in 50 tries");
}

/// The current-state row an auto-recorded append hangs off. Written raw: the
/// entity store is a sibling slice, and what the append needs to see is the
/// ROW, not the write path that produced it.
async fn ensure_entity(pool: &sqlx::PgPool, tenant: &TenantId, id: &str) {
    sqlx::query(
        "INSERT INTO entities (tenant_id, id, entity, types, created_at, modified_at)
         VALUES ($1, $2, jsonb_build_object('id', $2::text), ARRAY['T'], now(), now())
         ON CONFLICT (tenant_id, id) DO NOTHING",
    )
    .bind(tenant.as_str())
    .bind(id)
    .execute(pool)
    .await
    .expect("entity row");
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

/// The bridge doc decomposes into attr_instances rows in the same tx,
/// and the maintenance pass creates plain-mode partitions under the
/// SKIP LOCKED claim.
#[tokio::test(flavor = "multi_thread")]
async fn attr_instances_decomposition_and_maintenance() {
    let url = require_db!();
    let _ddl = PARTITION_DDL.lock().await;
    let pool = pg::connect(&url, 5).await.expect("pool");
    let t = TenantId::new("pgtempdec").expect("tenant");
    pg::ensure_tenant(&pool, &t).await.expect("tenant row");
    // self-clean: an interrupted earlier run must not poison this one
    for table in ["attr_instances", "temporal_entities"] {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DELETE FROM {table} WHERE tenant_id = 'pgtempdec'"
        )))
        .execute(&pool)
        .await
        .expect("clean");
    }
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
    // observed_at falls back to modified_at when absent
    let obs: String = sqlx::query_scalar(
        "SELECT observed_at::text FROM attr_instances
         WHERE tenant_id = 'pgtempdec' AND instance_id = 'urn:i:2'",
    )
    .fetch_one(&pool)
    .await
    .expect("obs");
    assert!(
        obs.replace(' ', "T").starts_with("2026-08-04T10:00"),
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
    let msg = maintenance_winning(&pool, None).await.expect("maintenance");
    if ts {
        // no partition/chunk work without a retention horizon — but the 4.22
        // expired-instance reap legitimately runs on every pass
        assert!(
            msg.contains("nothing to do") || msg.contains("(4.22)"),
            "timescale, no retention: {msg}"
        );
        assert!(
            !msg.contains("partition") && !msg.contains("drop_chunks"),
            "timescale must not do partition work without retention: {msg}"
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

/// A range whose rows already sit in the DEFAULT partition cannot be
/// partitioned off, and the module contract is that maintenance SKIPS it and
/// carries on. PostgreSQL aborts the whole transaction on any failed
/// statement, so "carries on" only holds if that one statement runs under its
/// own savepoint — without it the pass dies with 25P02 on the NEXT statement
/// and no partition is ever created on a live database.
#[tokio::test(flavor = "multi_thread")]
async fn occupied_range_is_skipped_without_poisoning_the_pass() {
    let url = require_db!();
    let _ddl = PARTITION_DDL.lock().await;
    let pool = pg::connect(&url, 5).await.expect("connect");
    if antares_sql::maintenance::timescale_present(&pool)
        .await
        .expect("detect")
    {
        return; // plain-mode-only contract; timescale has chunks
    }
    let t = TenantId::new("pgpart").expect("tenant");
    pg::ensure_tenant(&pool, &t).await.expect("tenant row");

    // Drop any weekly partition covering now, then park a row in DEFAULT for
    // that exact range — the conflict maintenance must tolerate.
    let this_week: String = sqlx::query_scalar(
        "SELECT 'attr_instances_' || to_char(date_trunc('week', now()), 'IYYY\"w\"IW')",
    )
    .fetch_one(&pool)
    .await
    .expect("suffix");
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "DROP TABLE IF EXISTS {this_week}"
    )))
    .execute(&pool)
    .await
    .expect("drop");
    sqlx::query(
        "INSERT INTO attr_instances
           (tenant_id, entity_id, attr_id, instance_id, observed_at, created_at, modified_at, data)
         VALUES ($1, 'urn:e:part', 'urn:a:part', 'urn:i:part', now(), now(), now(), '{}'::jsonb)
         ON CONFLICT DO NOTHING",
    )
    .bind(t.as_str())
    .execute(&pool)
    .await
    .expect("park row in default");

    let msg = maintenance_winning(&pool, None)
        .await
        .expect("maintenance must survive an occupied range");
    assert!(
        msg.contains("adopted from default"),
        "an occupied range must be recovered, not abandoned: {msg}"
    );
    assert!(
        msg.contains(": ok"),
        "the pass continued past the occupied range: {msg}"
    );

    sqlx::query("DELETE FROM attr_instances WHERE tenant_id = $1")
        .bind(t.as_str())
        .execute(&pool)
        .await
        .expect("cleanup");
}

/// 4.22: expiresAt marks the point where an instance "should be deleted from
/// an NGSI-LD system" — the maintenance pass physically reaps attr_instances
/// rows whose instance-level expiresAt has passed. Durable siblings stay.
/// (Reads already refuse expired instances; this bounds the storage.)
#[tokio::test(flavor = "multi_thread")]
async fn clause_4_22_expired_attr_instances_reaped() {
    let url = require_db!();
    let _ddl = PARTITION_DDL.lock().await;
    let pool = pg::connect(&url, 5).await.expect("pool");
    let t = TenantId::new("pgtempexp").expect("tenant");
    pg::ensure_tenant(&pool, &t).await.expect("tenant row");
    for table in ["attr_instances", "temporal_entities"] {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DELETE FROM {table} WHERE tenant_id = 'pgtempexp'"
        )))
        .execute(&pool)
        .await
        .expect("clean");
    }
    let s = PgTemporalStore::new(pool.clone());
    let attr = "https://uri.etsi.org/ngsi-ld/default-context/speed";
    let doc = json!({
        "id": "urn:t:exp422", "type": ["T"],
        "createdAt": "2026-08-04T09:00:00Z", "modifiedAt": "2026-08-04T09:00:00Z",
        attr: [
            {"type": "Property", "value": 1, "observedAt": "2026-08-04T09:00:00Z",
             "instanceId": "urn:ngsi-ld:Instance:reapme",
             "expiresAt": "2020-01-01T00:00:00Z"},
            {"type": "Property", "value": 2, "observedAt": "2026-08-04T09:01:00Z",
             "instanceId": "urn:ngsi-ld:Instance:durable"}
        ]
    });
    assert!(s.create(&t, "urn:t:exp422", &doc).expect("create"));
    let before: i64 =
        sqlx::query_scalar("SELECT count(*) FROM attr_instances WHERE tenant_id = 'pgtempexp'")
            .fetch_one(&pool)
            .await
            .expect("count");
    assert_eq!(before, 2, "both instances decomposed");

    maintenance_winning(&pool, None).await.expect("maintenance");

    let left: Vec<String> =
        sqlx::query_scalar("SELECT instance_id FROM attr_instances WHERE tenant_id = 'pgtempexp'")
            .fetch_all(&pool)
            .await
            .expect("rows");
    assert_eq!(
        left,
        vec!["urn:ngsi-ld:Instance:durable".to_string()],
        "expired instance reaped, durable sibling kept (4.22)"
    );

    sqlx::query("DELETE FROM attr_instances WHERE tenant_id = 'pgtempexp'")
        .execute(&pool)
        .await
        .expect("cleanup");
    let _ = s.delete(&t, "urn:t:exp422");
}

/// Regression: the append fast-path's shell insert was ON CONFLICT DO
/// NOTHING — an entity that gained a type after first touch stayed frozen
/// with its original types and was invisible to type-filtered temporal
/// queries forever. The shell upsert must carry new types/scopes forward.
#[tokio::test(flavor = "multi_thread")]
async fn append_refreshes_types_after_first_touch() {
    let url = require_db!();
    let pool = pg::connect(&url, 5).await.expect("pool");
    let s = PgTemporalStore::new(pool.clone());
    let t = TenantId::new("pgtempretype").expect("tenant");
    pg::ensure_tenant(&pool, &t).await.expect("tenant row");
    let id = "urn:ngsi-ld:Vehicle:retype";
    let _ = s.delete(&t, id);
    ensure_entity(&pool, &t, id).await;
    let attr = "https://uri.etsi.org/ngsi-ld/default-context/speed";
    let shell1 = json!({"id": id, "type": "Vehicle",
        "createdAt": "2026-08-15T09:00:00Z", "modifiedAt": "2026-08-15T09:00:00Z"});
    let adds = json!({attr: [{"type": "Property", "value": 1,
        "observedAt": "2026-08-15T09:00:00Z",
        "instanceId": "urn:ngsi-ld:Instance:rt1"}]});
    s.append(&t, id, &shell1, &adds).expect("append 1");

    let shell2 = json!({"id": id, "type": ["Vehicle", "Camera"],
        "createdAt": "2026-08-15T09:00:00Z", "modifiedAt": "2026-08-15T09:01:00Z"});
    let adds2 = json!({attr: [{"type": "Property", "value": 2,
        "observedAt": "2026-08-15T09:01:00Z",
        "instanceId": "urn:ngsi-ld:Instance:rt2"}]});
    s.append(&t, id, &shell2, &adds2).expect("append 2");

    let got = s.get(&t, id).expect("get").expect("present");
    assert!(
        got["type"].to_string().contains("Camera"),
        "a type gained after first touch must reach the temporal evolution: {}",
        got["type"]
    );
    let _ = s.delete(&t, id);
    sqlx::query("DELETE FROM entities WHERE tenant_id = $1")
        .bind(t.as_str())
        .execute(&pool)
        .await
        .expect("cleanup");
}

/// 5.6.6 Delete Entity removes the entity, and with it the temporal evolution
/// recorded for it (5.6.11 auto-recording). Those are two transactions, so an
/// auto-recording append that started before the delete can commit after it —
/// and the history it writes then belongs to an entity that no longer exists,
/// with no later delete to clean it. The append is conditional on the entity
/// row for exactly that reason, and it takes a KEY SHARE lock on it so a
/// delete cannot slip between the check and the insert.
#[tokio::test(flavor = "multi_thread")]
async fn append_for_a_deleted_entity_does_not_resurrect_its_history() {
    let url = require_db!();
    // the appended instances are stamped "now", so they land in the current
    // week's partition — the one the maintenance tests drop and re-create
    let _ddl = PARTITION_DDL.lock().await;
    let pool = pg::connect(&url, 5).await.expect("pool");
    let s = PgTemporalStore::new(pool.clone());
    let t = TenantId::new("pgtempghost").expect("tenant");
    pg::ensure_tenant(&pool, &t).await.expect("tenant row");
    let id = "urn:ngsi-ld:Vehicle:ghost";
    let _ = s.delete(&t, id);
    let attr = "https://uri.etsi.org/ngsi-ld/default-context/speed";
    let shell = json!({"id": id, "type": "Vehicle",
        "createdAt": "2026-08-17T09:00:00Z", "modifiedAt": "2026-08-17T09:00:00Z"});
    let adds = |n: i64, inst: &str| {
        json!({attr: [{"type": "Property", "value": n,
                       "observedAt": "2026-08-17T09:00:00Z",
                       "instanceId": inst}]})
    };
    let instances = |pool: sqlx::PgPool| async move {
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM attr_instances
             WHERE tenant_id = 'pgtempghost' AND entity_id = 'urn:ngsi-ld:Vehicle:ghost'",
        )
        .fetch_one(&pool)
        .await
        .expect("count")
    };

    // control: with the entity present the append records normally
    ensure_entity(&pool, &t, id).await;
    s.append(&t, id, &shell, &adds(1, "urn:ngsi-ld:Instance:live"))
        .expect("append while the entity lives");
    assert!(s.get(&t, id).expect("get").is_some(), "history recorded");
    assert_eq!(instances(pool.clone()).await, 1);

    // the delete, as the API does it: the entity row, then its evolution
    sqlx::query("DELETE FROM entities WHERE tenant_id = $1 AND id = $2")
        .bind(t.as_str())
        .bind(id)
        .execute(&pool)
        .await
        .expect("delete entity");
    assert!(s.delete(&t, id).expect("delete temporal"));

    // the late hook: an append for an entity that is gone records NOTHING
    s.append(&t, id, &shell, &adds(2, "urn:ngsi-ld:Instance:ghost"))
        .expect("a late append is a no-op, not an error");
    assert!(
        s.get(&t, id).expect("get").is_none(),
        "no temporal evolution may exist for a deleted entity"
    );
    assert_eq!(
        instances(pool.clone()).await,
        0,
        "and no instance rows either"
    );
}

/// 4.5.19 / 5.7.4.4 aggregation pushed into SQL: the numeric bucket matrix
/// the API would compute over reconstructed instances comes back from the
/// store already aggregated; a non-numeric windowed value makes the store
/// fall back to instance rows so the API keeps its eligibility verdict.
#[tokio::test(flavor = "multi_thread")]
async fn aggregation_pushdown_buckets_like_the_api() {
    use antares_sql::compile::temporal::InstanceRange;
    use antares_sql::store::filter::{Aggregate, TemporalFilter};
    let url = require_db!();
    let pool = pg::connect(&url, 5).await.expect("connect");
    let s = PgTemporalStore::new(pool.clone());
    let t = TenantId::new("pgaggr").expect("tenant");
    pg::ensure_tenant(&pool, &t).await.expect("tenant row");
    let id = "urn:ngsi-ld:Vehicle:aggr1";
    let _ = s.delete(&t, id);
    let speed = "https://uri.etsi.org/ngsi-ld/default-context/speed";
    let inst = |v: serde_json::Value, at: &str, n: u32| {
        json!({"type": "Property", "value": v, "observedAt": at,
               "instanceId": format!("urn:ngsi-ld:Instance:a{n}")})
    };
    let doc = json!({
        "id": id, "type": "Vehicle",
        "createdAt": "2026-01-01T00:00:00Z", "modifiedAt": "2026-01-01T00:00:00Z",
        speed: [inst(json!(10), "2026-01-01T00:00:00Z", 1),
                inst(json!(20), "2026-01-01T00:30:00Z", 2),
                inst(json!(30), "2026-01-01T01:15:00Z", 3),
                inst(json!(99), "2025-12-31T23:00:00Z", 4)]
    });
    assert!(s.create(&t, id, &doc).expect("create"));
    let methods = ["avg".to_owned(), "max".to_owned(), "totalCount".to_owned()];
    let expand = |t: &str| t.to_owned();
    let range = InstanceRange {
        timerel: "after",
        time_at: "2026-01-01T00:00:00Z",
        end_time_at: None,
        timeproperty: "observedAt",
    };
    let f = TemporalFilter {
        ids: Some(&[id]),
        range: Some(range),
        expand: &expand,
        aggregate: Some(Aggregate {
            methods: &methods,
            period_secs: Some(3600),
            anchor: Some("2026-01-01T00:00:00Z"),
        }),
        ..TemporalFilter::default()
    };
    let out = s.query(&t, &f).expect("query");
    assert!(out.aggregated, "the store aggregated");
    assert_eq!(out.rows.len(), 1);
    let a = &out.rows[0][speed];
    assert_eq!(a["type"], "Property");
    // [value, start, end] rows; the value compares as a number (SQL spells
    // 15 where the API spells 15.0 — the same JSON number)
    let rows = |v: &serde_json::Value| -> Vec<(f64, String, String)> {
        v.as_array()
            .expect("rows")
            .iter()
            .map(|r| {
                (
                    r[0].as_f64().expect("number"),
                    r[1].as_str().expect("start").to_owned(),
                    r[2].as_str().expect("end").to_owned(),
                )
            })
            .collect()
    };
    let b0 = (
        "2026-01-01T00:00:00Z".to_owned(),
        "2026-01-01T01:00:00Z".to_owned(),
    );
    let b1 = (
        "2026-01-01T01:00:00Z".to_owned(),
        "2026-01-01T02:00:00Z".to_owned(),
    );
    // hour buckets anchored at timeAt; the 23:00 instance is outside the window
    assert_eq!(
        rows(&a["avg"]),
        vec![
            (15.0, b0.0.clone(), b0.1.clone()),
            (30.0, b1.0.clone(), b1.1.clone())
        ]
    );
    assert_eq!(rows(&a["max"])[0].0, 20.0);
    assert_eq!(
        rows(&a["totalCount"]),
        vec![(2.0, b0.0, b0.1), (1.0, b1.0, b1.1)]
    );
    assert!(
        a.get("value").is_none(),
        "no instance members leak into an aggregate: {a}"
    );

    // PT0S: one bucket from the first instant to the last + 1 s, no anchor
    let f0 = TemporalFilter {
        ids: Some(&[id]),
        expand: &expand,
        aggregate: Some(Aggregate {
            methods: &methods,
            period_secs: None,
            anchor: None,
        }),
        ..TemporalFilter::default()
    };
    let out = s.query(&t, &f0).expect("query");
    assert!(out.aggregated);
    assert_eq!(
        rows(&out.rows[0][speed]["totalCount"]),
        vec![(
            4.0,
            "2025-12-31T23:00:00Z".to_owned(),
            "2026-01-01T01:15:01Z".to_owned()
        )]
    );

    // a string-valued windowed instance: not the store's class to judge —
    // plain instance rows come back and the API decides eligibility
    let r = s.mutate(&t, id, |d| {
        d[speed].as_array_mut().expect("arr").push(
            json!({"type": "Property", "value": "fast", "observedAt": "2026-01-01T00:45:00Z",
                   "instanceId": "urn:ngsi-ld:Instance:a5"}),
        );
        Ok::<(), ()>(())
    });
    assert!(matches!(r, Ok(Some(Ok(())))));
    let out = s.query(&t, &f).expect("query");
    assert!(!out.aggregated, "mixed classes fall back to instances");
    assert!(out.rows[0][speed].is_array(), "{}", out.rows[0]);
    assert!(s.delete(&t, id).expect("delete"));
}
