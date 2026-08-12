//! Postgres foundation integration tests (tasks.md C1–C4). Need a live
//! PostGIS; skip (pass vacuously, loudly) when ANTARES_TEST_DATABASE_URL is
//! unset so plain `cargo test` stays service-free (§9.5).
//!
//! Local:  docker run -d --name pgdev -e POSTGRES_USER=antares \
//!           -e POSTGRES_PASSWORD=antares -e POSTGRES_DB=antares \
//!           -p 15432:5432 ghcr.io/baosystems/postgis:17-3.5
//!         ANTARES_TEST_DATABASE_URL=postgresql://antares:antares@localhost:15432/antares \
//!           cargo test -p antares-sql --test pg

use antares_model::TenantId;
use antares_sql::pg;
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
    let store = antares_sql::store::any::AnyStore::Pg(
        antares_sql::store::any::PgBackend::new(pool.clone()),
    );
    let run = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_millis();
    let t = TenantId::new(&format!("pgte{run}")).expect("tenant");
    assert!(
        !store.tenant_exists(&t).expect("tenant_exists must not error"),
        "never-seen tenant reads as existing"
    );
    pg::ensure_tenant(&pool, &t).await.expect("tenant row");
    assert!(store.tenant_exists(&t).expect("tenant_exists must not error"));
}

#[tokio::test]
async fn migrations_apply_and_indexes_exist() {
    let url = require_db!();
    let pool = pg::connect(&url, 5).await.expect("connect+migrate");

    // C3: the LIVE index set on entities — the exact indexes the compiled
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

    // C2: every §8.3 table exists.
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
    // §3.1.4: two concurrent first-writes both succeed.
    let (a, b) = tokio::join!(pg::ensure_tenant(&pool, &t), pg::ensure_tenant(&pool, &t));
    a.expect("first");
    b.expect("second");
}

/// §16.1.3 RLS denial: connect as a NON-superuser role (superusers bypass
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
