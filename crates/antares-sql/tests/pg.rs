//! Postgres foundation integration tests. Need a live
//! PostGIS; skip (pass vacuously, loudly) when ANTARES_TEST_DATABASE_URL is
//! unset so plain `cargo test` stays service-free.
//!
//! Local:  docker run -d --name pgdev -e POSTGRES_USER=antares \
//!           -e POSTGRES_PASSWORD=antares -e POSTGRES_DB=antares \
//!           -p 15432:5432 ghcr.io/baosystems/postgis:17-3.5
//!         ANTARES_TEST_DATABASE_URL=postgresql://antares:antares@localhost:15432/antares \
//!           cargo test -p antares-sql --test pg

use antares_model::TenantId;
use antares_sql::store::pg;
use sqlx::Row;

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

/// 5.5.10: tenant existence — false for a never-seen Tenant, true once its
/// row exists. Regression: the Pg arm never bound $1, so EVERY call errored
/// ("bind message supplies 0 parameters") and the NonexistentTenant
/// middleware 500'd every non-default-tenant request on the Pg store.
#[tokio::test(flavor = "multi_thread")]
async fn tenant_exists_answers_for_the_actual_tenant() {
    let url = require_db!();
    let pool = pg::connect(&url, 5).await.expect("connect+migrate");
    let store = antares_sql::store::any::AnyStore::Pg(antares_sql::store::any::PgBackend::new(
        pool.clone(),
    ));
    let run = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_millis();
    let t = TenantId::new(&format!("pgte{run}")).expect("tenant");
    assert!(
        !store
            .tenant_exists(&t)
            .expect("tenant_exists must not error"),
        "never-seen tenant reads as existing"
    );
    pg::ensure_tenant(&pool, &t).await.expect("tenant row");
    assert!(store
        .tenant_exists(&t)
        .expect("tenant_exists must not error"));
}

#[tokio::test]
async fn migrations_apply_and_indexes_exist() {
    let url = require_db!();
    let pool = pg::connect(&url, 5).await.expect("connect+migrate");

    // The LIVE index set on entities — the exact indexes the compiled
    // statements route through (0005 dropped the dead scopes/modified pair;
    // pg_explain.rs proves usability, this proves existence).
    for idx in [
        "i_entities_location",
        "i_entities_jsonb",
        "i_entities_types",
        "i_entities_loc_ambiguous",
    ] {
        let n: i64 = sqlx::query(
            "SELECT count(*) FROM pg_indexes WHERE tablename = 'entities' AND indexname = $1",
        )
        .bind(idx)
        .fetch_one(&pool)
        .await
        .expect("pg_indexes")
        .get(0);
        assert_eq!(n, 1, "index {idx} missing on entities");
    }

    // Every schema table exists.
    for table in [
        "tenants",
        "entities",
        "subscriptions",
        "csource_subscriptions",
        "csource_registrations",
        "csource_index",
        "jsonld_contexts",
        "entity_maps",
        "outbox",
    ] {
        let ok: bool = sqlx::query("SELECT to_regclass($1) IS NOT NULL")
            .bind(table)
            .fetch_one(&pool)
            .await
            .expect("to_regclass")
            .get(0);
        assert!(ok, "missing table {table}");
    }

    // Idempotence: running the migrator again is a no-op, not an error.
    pg::MIGRATOR.run(&pool).await.expect("re-run migrations");
}

#[tokio::test]
async fn tenant_upsert_is_idempotent() {
    let url = require_db!();
    let pool = pg::connect(&url, 5).await.expect("connect");
    let t = TenantId::new("race_tenant").expect("tenant");
    // Two concurrent first-writes both succeed.
    let (a, b) = tokio::join!(pg::ensure_tenant(&pool, &t), pg::ensure_tenant(&pool, &t));
    a.expect("first");
    b.expect("second");
}

/// RLS denial: connect as a NON-superuser role (superusers bypass
/// RLS) and prove a tenant sees zero foreign rows even with tenant-less SQL.
#[tokio::test]
async fn rls_denies_cross_tenant_reads_and_writes() {
    let url = require_db!();
    let admin = pg::connect(&url, 5).await.expect("connect");

    // Non-superuser app role (idempotent), least privilege on the data tables.
    for stmt in [
        "DO $$ BEGIN CREATE ROLE antares_app LOGIN PASSWORD 'app';
         EXCEPTION WHEN duplicate_object THEN NULL; END $$",
        "GRANT USAGE ON SCHEMA public TO antares_app",
        "GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO antares_app",
    ] {
        sqlx::query(stmt).execute(&admin).await.expect(stmt);
    }

    let app_url = url.replace("antares:antares@", "antares_app:app@");
    assert_ne!(
        app_url, url,
        "test URL must embed antares:antares credentials"
    );
    let app = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .connect(&app_url)
        .await
        .expect("app-role connect");

    let ta = TenantId::new("rls_a").expect("ta");
    let tb = TenantId::new("rls_b").expect("tb");

    // Write one entity as tenant a.
    let mut tx = app.begin().await.expect("tx");
    pg::set_tenant(&mut tx, &ta).await.expect("set_tenant");
    sqlx::query(
        "INSERT INTO entities (tenant_id, id, entity, types, created_at, modified_at)
         VALUES ($1, 'urn:rls:1', '{}'::jsonb, '{T}', now(), now())
         ON CONFLICT (tenant_id, id) DO NOTHING",
    )
    .bind(ta.as_str())
    .execute(&mut *tx)
    .await
    .expect("insert as a");
    tx.commit().await.expect("commit");

    // Tenant b, deliberately tenant-less SELECT: RLS must return zero rows.
    let mut tx = app.begin().await.expect("tx");
    pg::set_tenant(&mut tx, &tb).await.expect("set_tenant b");
    let n: i64 = sqlx::query("SELECT count(*) FROM entities WHERE id = 'urn:rls:1'")
        .fetch_one(&mut *tx)
        .await
        .expect("select as b")
        .get(0);
    assert_eq!(n, 0, "RLS must hide tenant a's row from tenant b");

    // WITH CHECK: tenant b cannot forge a row claiming tenant a.
    let forged = sqlx::query(
        "INSERT INTO entities (tenant_id, id, entity, types, created_at, modified_at)
         VALUES ($1, 'urn:rls:forged', '{}'::jsonb, '{T}', now(), now())",
    )
    .bind(ta.as_str())
    .execute(&mut *tx)
    .await;
    assert!(
        forged.is_err(),
        "WITH CHECK must reject cross-tenant inserts"
    );
    drop(tx); // rollback

    // Tenant a still sees its row.
    let mut tx = app.begin().await.expect("tx");
    pg::set_tenant(&mut tx, &ta).await.expect("set_tenant a");
    let n: i64 = sqlx::query("SELECT count(*) FROM entities WHERE id = 'urn:rls:1'")
        .fetch_one(&mut *tx)
        .await
        .expect("select as a")
        .get(0);
    assert_eq!(n, 1);

    // No tenant set at all (current_setting returns NULL): zero rows.
    let n: i64 = sqlx::query("SELECT count(*) FROM entities")
        .fetch_one(&app)
        .await
        .expect("tenant-less select")
        .get(0);
    assert_eq!(n, 0, "no tenant in scope must mean no rows");
}

/// Purging a tenant empties every tenant-bearing table for it in one
/// transaction and touches no row of another tenant.
#[tokio::test(flavor = "multi_thread")]
async fn purge_tenant_empties_every_tenant_table() {
    use antares_store::{CurrentStateDriver, Kind, TemporalDriver};
    let url = require_db!();
    let pool = pg::connect(&url, 5).await.expect("connect+migrate");
    let store = antares_sql::store::any::AnyStore::Pg(antares_sql::store::any::PgBackend::new(
        pool.clone(),
    ));
    store.set_outbox(true);
    let run = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_millis();
    let a = TenantId::new(&format!("pgpa{run}")).expect("tenant");
    let b = TenantId::new(&format!("pgpb{run}")).expect("tenant");
    let doc = |id: &str| {
        serde_json::json!({
            "id": id, "type": "Test",
            "createdAt": "2026-08-04T09:00:00Z", "modifiedAt": "2026-08-04T09:00:00Z",
            "n": {"type": "Property", "value": 1, "observedAt": "2026-08-04T09:00:00Z"}
        })
    };
    for t in [&a, &b] {
        pg::ensure_tenant(&pool, t).await.expect("tenant row");
        for kind in [
            Kind::Entity,
            Kind::Subscription,
            Kind::Registration,
            Kind::CSourceSubscription,
            Kind::Snapshot,
            Kind::EntityMap,
            Kind::DistSub,
            Kind::DeadLetter,
        ] {
            let id = "urn:x:1";
            let d = if kind == Kind::Registration {
                serde_json::json!({"id": id, "type": "ContextSourceRegistration",
                    "information": [{"entities": [{"type": "Test"}]}],
                    "endpoint": "http://127.0.0.1:9"})
            } else {
                doc(id)
            };
            assert!(
                CurrentStateDriver::create(&store, t, kind, id, d).expect("create"),
                "{kind:?}"
            );
        }
        TemporalDriver::create(&store, t, "urn:x:1", doc("urn:x:1")).expect("temporal");
        store
            .temporal_append(
                t,
                "urn:x:1",
                &doc("urn:x:1"),
                &serde_json::json!({"n": [{"type": "Property", "value": 2,
                    "observedAt": "2026-08-04T09:01:00Z", "instanceId": "urn:i:1"}]}),
            )
            .expect("append");
        sqlx::query(
            "INSERT INTO entity_maps (tenant_id, map_id, pos, query_checksum, entity_id,
             registration_id, last_access, expires_at)
             VALUES ($1, 'm', 0, 'c', 'urn:x:1', 'r', now(), now() + interval '1 hour')",
        )
        .bind(t.as_str())
        .execute(&pool)
        .await
        .expect("entity map row");
    }
    const TABLES: [&str; 13] = [
        "entities",
        "subscriptions",
        "csource_subscriptions",
        "csource_registrations",
        "csource_index",
        "entity_maps",
        "outbox",
        "snapshots",
        "entity_map_docs",
        "dist_subs",
        "dead_letters",
        "temporal_entities",
        "attr_instances",
    ];
    let count = |table: &'static str, t: &TenantId| {
        let pool = pool.clone();
        let t = t.as_str().to_string();
        async move {
            let n: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
                "SELECT count(*) FROM {table} WHERE tenant_id = $1"
            )))
            .bind(t)
            .fetch_one(&pool)
            .await
            .expect("count");
            n
        }
    };
    for table in TABLES {
        assert!(count(table, &a).await > 0, "seed missing in {table}");
    }
    let before_b: Vec<i64> = {
        let mut v = Vec::new();
        for table in TABLES {
            v.push(count(table, &b).await);
        }
        v
    };
    let stats = store.tenant_stats().expect("stats");
    let row = stats
        .iter()
        .find(|r| r.tenant == a.as_str())
        .expect("listed");
    assert_eq!(
        (row.entities, row.subscriptions, row.registrations),
        (1, 1, 1)
    );
    assert_eq!(row.dist_subs, 1);
    assert!(TemporalDriver::attr_instance_count(&store, &a).expect("n") >= 1);

    assert!(CurrentStateDriver::purge_tenant(&store, &a).expect("purge"));
    TemporalDriver::purge_tenant(&store, &a).expect("purge history");
    for table in TABLES {
        assert_eq!(
            count(table, &a).await,
            0,
            "{table} still holds rows of the purged tenant"
        );
    }
    assert!(!store.tenant_exists(&a).expect("exists"));
    for (table, before) in TABLES.iter().zip(before_b) {
        assert_eq!(
            count(table, &b).await,
            before,
            "{table} of the other tenant changed"
        );
    }
    assert!(
        !CurrentStateDriver::purge_tenant(&store, &a).expect("purge"),
        "nothing left"
    );
    // cleanup
    assert!(CurrentStateDriver::purge_tenant(&store, &b).expect("purge b"));
    TemporalDriver::purge_tenant(&store, &b).expect("purge b history");
}
