// SPDX-License-Identifier: EUPL-1.2
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
            .await
            .expect("tenant_exists must not error"),
        "never-seen tenant reads as existing"
    );
    pg::ensure_tenant(&pool, &t).await.expect("tenant row");
    assert!(store
        .tenant_exists(&t)
        .await
        .expect("tenant_exists must not error"));
}

#[tokio::test]
async fn migrations_apply_and_indexes_exist() {
    let url = require_db!();
    let pool = pg::connect(&url, 5).await.expect("connect+migrate");

    // The LIVE index set on entities — the exact indexes the compiled
    // statements route through (pg_explain.rs proves usability, this proves
    // existence).
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

/// ADR-0021 + 4.14: a stored @context is under the same Row-Level Security as
/// every other Tenant-bearing row. A Hosted document holds term mappings
/// authored through one Tenant's requests, and 5.5.7 makes those mappings
/// decide what that Tenant's payloads mean, so another Tenant must not read
/// it, rewrite it or delete it. A Cached copy is a public document the broker
/// downloaded (5.13.1): it belongs to no Tenant and every Tenant reaches it.
///
/// Proven as the non-superuser role the deployment runs under, with SQL that
/// names no tenant at all — the belt has to bite where the code forgot to.
#[tokio::test]
async fn rls_scopes_hosted_contexts_and_shares_cached_ones() {
    let url = require_db!();
    let admin = pg::connect(&url, 5).await.expect("connect");
    for stmt in [
        "DO $$ BEGIN CREATE ROLE antares_app LOGIN PASSWORD 'app';
         EXCEPTION WHEN duplicate_object THEN NULL; END $$",
        "GRANT USAGE ON SCHEMA public TO antares_app",
        "GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO antares_app",
    ] {
        sqlx::query(stmt).execute(&admin).await.expect(stmt);
    }
    let app_url = url.replace("antares:antares@", "antares_app:app@");
    let app = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .connect(&app_url)
        .await
        .expect("app-role connect");

    let ta = TenantId::new("ctxrls_a").expect("ta");
    let tb = TenantId::new("ctxrls_b").expect("tb");
    let hosted = "ctxrls:hosted";
    let cached = "ctxrls:cached";
    for id in [hosted, cached] {
        sqlx::query("DELETE FROM jsonld_contexts WHERE id = $1")
            .bind(id)
            .execute(&admin)
            .await
            .expect("clean");
    }

    // Tenant a stores one Hosted @context and one Cached copy.
    let mut tx = app.begin().await.expect("tx");
    pg::set_tenant(&mut tx, &ta).await.expect("set_tenant a");
    sqlx::query(
        "INSERT INTO jsonld_contexts (id, body, kind) VALUES
           ($1, jsonb_build_object('owner', $3::text, 'kind', 'Hosted'), 'Hosted'),
           ($2, jsonb_build_object('kind', 'Cached'), 'Cached')",
    )
    .bind(hosted)
    .bind(cached)
    .bind(ta.as_str())
    .execute(&mut *tx)
    .await
    .expect("insert as a");
    tx.commit().await.expect("commit");

    let seen = |t: Option<&TenantId>, id: &'static str| {
        let app = app.clone();
        let t = t.map(|t| t.as_str().to_owned());
        async move {
            let mut tx = app.begin().await.expect("tx");
            if let Some(t) = &t {
                let tid = TenantId::new(t).expect("tenant");
                pg::set_tenant(&mut tx, &tid).await.expect("set_tenant");
            }
            let n: i64 = sqlx::query("SELECT count(*) FROM jsonld_contexts WHERE id = $1")
                .bind(id)
                .fetch_one(&mut *tx)
                .await
                .expect("count")
                .get(0);
            n
        }
    };
    assert_eq!(seen(Some(&ta), hosted).await, 1, "its owner reads it");
    assert_eq!(
        seen(Some(&tb), hosted).await,
        0,
        "another Tenant's Hosted @context must be invisible"
    );
    assert_eq!(
        seen(None, hosted).await,
        0,
        "no Tenant in scope reaches no Tenant's documents"
    );
    for t in [Some(&ta), Some(&tb), None] {
        assert_eq!(
            seen(t, cached).await,
            1,
            "a Cached copy is a public document and belongs to no Tenant"
        );
    }

    // Tenant b can neither rewrite nor delete what it cannot see, and cannot
    // forge a row into another Tenant either (WITH CHECK).
    let mut tx = app.begin().await.expect("tx");
    pg::set_tenant(&mut tx, &tb).await.expect("set_tenant b");
    let touched = sqlx::query("UPDATE jsonld_contexts SET body = '{}'::jsonb WHERE id = $1")
        .bind(hosted)
        .execute(&mut *tx)
        .await
        .expect("update")
        .rows_affected();
    assert_eq!(touched, 0, "RLS must hide the row from an UPDATE");
    let removed = sqlx::query("DELETE FROM jsonld_contexts WHERE id = $1")
        .bind(hosted)
        .execute(&mut *tx)
        .await
        .expect("delete")
        .rows_affected();
    assert_eq!(removed, 0, "RLS must hide the row from a DELETE");
    let forged = sqlx::query(
        "INSERT INTO jsonld_contexts (id, body, kind)
         VALUES ('ctxrls:forged', jsonb_build_object('owner', $1::text), 'Hosted')",
    )
    .bind(ta.as_str())
    .execute(&mut *tx)
    .await;
    assert!(
        forged.is_err(),
        "WITH CHECK must reject an @context forged into another Tenant"
    );
    drop(tx); // rollback

    assert_eq!(
        seen(Some(&ta), hosted).await,
        1,
        "the owner's @context survived every foreign attempt"
    );
    for id in [hosted, cached] {
        sqlx::query("DELETE FROM jsonld_contexts WHERE id = $1")
            .bind(id)
            .execute(&admin)
            .await
            .expect("cleanup");
    }
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
    // `b` extends `a`: a purge that ever matched its tenant by prefix
    // instead of by equality would take this one with it.
    let b = TenantId::new(&format!("pgpa{run}x")).expect("tenant");
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
                CurrentStateDriver::create(&store, t, kind, id, d)
                    .await
                    .expect("create"),
                "{kind:?}"
            );
        }
        // ADR-0021: a Hosted @context belongs to the Tenant that stored it,
        // so it is one more table the purge has to reach.
        CurrentStateDriver::context_put(
            &store,
            Some(t),
            &format!("ctx-{}", t.as_str()),
            serde_json::json!({"url": format!("http://b.example/ngsi-ld/v1/jsonldContexts/ctx-{}", t.as_str()),
                               "localId": format!("ctx-{}", t.as_str()), "kind": "Hosted",
                               "owner": t.as_str(), "createdAt": "2026-08-04T09:00:00Z",
                               "body": {"@context": {"n": "https://x/n"}}}),
        )
        .await.expect("hosted @context");
        TemporalDriver::create(&store, t, "urn:x:1", doc("urn:x:1"))
            .await
            .expect("temporal");
        store
            .temporal_append(
                t,
                "urn:x:1",
                &doc("urn:x:1"),
                &serde_json::json!({"n": [{"type": "Property", "value": 2,
                    "observedAt": "2026-08-04T09:01:00Z", "instanceId": "urn:i:1"}]}),
            )
            .await
            .expect("append");
    }
    const TABLES: [&str; 13] = [
        "entities",
        "jsonld_contexts",
        "subscriptions",
        "csource_subscriptions",
        "csource_registrations",
        "csource_index",
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
    // The list above is hand-written, and so is the one `purge_tenant`
    // deletes from. A new tenant-bearing table added to the schema without a
    // purge branch would keep that tenant's rows after its deletion, and no
    // assertion below would notice. Ask the catalogue instead: every table
    // with a `tenant_id` column is either purged here or `tenants` itself,
    // whose row purge_tenant removes and `tenant_exists` reports on.
    let bearing: Vec<String> = sqlx::query_scalar(
        "SELECT c.relname::text FROM pg_class c
           JOIN pg_namespace n ON n.oid = c.relnamespace AND n.nspname = 'public'
           JOIN pg_attribute a ON a.attrelid = c.oid AND a.attname = 'tenant_id'
          WHERE c.relkind IN ('r', 'p') AND NOT c.relispartition
          ORDER BY 1",
    )
    .fetch_all(&pool)
    .await
    .expect("catalogue");
    let mut want: Vec<String> = TABLES.iter().map(|t| (*t).to_owned()).collect();
    want.push("tenants".to_owned());
    want.sort();
    assert_eq!(
        bearing, want,
        "a tenant-bearing table is missing from the purge"
    );
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
    let ids = store.tenant_ids().await.expect("ids");
    assert!(ids.iter().any(|t| t == a.as_str()), "listed: {ids:?}");
    assert!(
        ids.iter().any(|t| t == antares_model::TenantId::DEFAULT),
        "5.5.10: the default tenant is always listed: {ids:?}"
    );
    let row = store
        .tenant_stats_one(&a)
        .await
        .expect("stats")
        .expect("tenant exists");
    assert_eq!(
        (row.entities, row.subscriptions, row.registrations),
        (1, 1, 1)
    );
    assert_eq!(row.dist_subs, 1);
    assert!(
        row.created_at.is_some(),
        "the tenants row carries its stamp"
    );
    assert!(
        store
            .tenant_stats_one(&TenantId::new("never-seen").expect("tenant"))
            .await
            .expect("stats")
            .is_none(),
        "an unknown tenant has no stats to report"
    );
    assert!(
        TemporalDriver::attr_instance_count(&store, &a)
            .await
            .expect("n")
            >= 1
    );

    assert!(CurrentStateDriver::purge_tenant(&store, &a)
        .await
        .expect("purge"));
    TemporalDriver::purge_tenant(&store, &a)
        .await
        .expect("purge history");
    for table in TABLES {
        assert_eq!(
            count(table, &a).await,
            0,
            "{table} still holds rows of the purged tenant"
        );
    }
    assert!(!store.tenant_exists(&a).await.expect("exists"));
    for (table, before) in TABLES.iter().zip(before_b) {
        assert_eq!(
            count(table, &b).await,
            before,
            "{table} of the other tenant changed"
        );
    }
    assert!(
        !CurrentStateDriver::purge_tenant(&store, &a)
            .await
            .expect("purge"),
        "nothing left"
    );
    // cleanup
    assert!(CurrentStateDriver::purge_tenant(&store, &b)
        .await
        .expect("purge b"));
    TemporalDriver::purge_tenant(&store, &b)
        .await
        .expect("purge b history");
}

/// The startup probe reads what the server actually is, and the store serves
/// that copy afterwards. `/q/health` is polled, so a health body that named
/// the server by querying for it would put a database round trip on every
/// probe; the version is captured once, here.
#[tokio::test(flavor = "multi_thread")]
async fn the_server_version_is_probed_once_and_served_from_the_store() {
    let url = require_db!();
    let pool = pg::connect(&url, 5).await.expect("connect+migrate");
    let probed = pg::version_info(&pool).await;
    assert_eq!(probed["engine"], "postgres", "{probed}");
    assert!(
        probed["server"].as_str().is_some_and(|v| !v.is_empty()),
        "the server version must be named: {probed}"
    );
    assert!(
        probed["postgis"].as_str().is_some(),
        "the broker needs PostGIS and must report which one: {probed}"
    );

    let plain = antares_sql::store::any::AnyStore::Pg(antares_sql::store::any::PgBackend::new(
        pool.clone(),
    ));
    assert_eq!(
        plain.version_info(),
        serde_json::json!({}),
        "a store built without the probe reports nothing rather than guessing"
    );
    let probed_store = antares_sql::store::any::AnyStore::Pg(
        antares_sql::store::any::PgBackend::new(pool).with_version(probed.clone()),
    );
    assert_eq!(probed_store.version_info(), probed);
}

/// The Postgres arms answer the same driver contract as the in-process ones
/// (`antares_store::contract`). A backend's own tests assert what that
/// backend does; this asserts what the seam promises, which is what
/// `antares-api` was written against.
#[tokio::test(flavor = "multi_thread")]
async fn the_postgres_store_keeps_the_driver_contract() {
    let url = require_db!();
    let pool = pg::connect(&url, 5).await.expect("connect+migrate");
    let run = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let a = TenantId::new(&format!("ctra{run}")).expect("tenant");
    let b = TenantId::new(&format!("ctrb{run}")).expect("tenant");
    for t in [&a, &b] {
        pg::ensure_tenant(&pool, t).await.expect("tenant row");
    }
    let store = antares_sql::store::any::AnyStore::Pg(antares_sql::store::any::PgBackend::new(
        pool.clone(),
    ));
    let prefix = format!("pgctr{run}");
    antares_store::contract::run_current_state_contract(&store, &a, &b, &prefix).await;
    antares_store::contract::run_temporal_contract(&store, &a, &b, &prefix).await;
}

/// The tenants the broker mints for itself are not customer accounts, and
/// `/q/tenants` reads this list. They still hold a row: on Postgres this table
/// IS the broker's tenant enumeration — `subscription_tenants` reads it,
/// because `subscriptions` sits outside the `antares.service` escape (a
/// subscription document carries the credentials its notification is sent
/// with), and the mirror seed and the notification sweep walk what it answers.
/// So the row is written and the READ is what leaves it out.
#[tokio::test(flavor = "multi_thread")]
async fn an_internal_tenant_holds_a_row_and_is_left_out_of_the_inventory() {
    use antares_store::{CurrentStateDriver, Kind};
    let url = require_db!();
    let pool = pg::connect(&url, 5).await.expect("connect+migrate");
    let store = antares_sql::store::any::AnyStore::Pg(antares_sql::store::any::PgBackend::new(
        pool.clone(),
    ));
    let run = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_millis();
    let synth = TenantId::new_internal(&format!("snap-{run}")).expect("synthetic tenant");
    assert!(synth.is_internal());
    let id = "urn:ngsi-ld:Vehicle:frozen";
    store
        .create(
            &synth,
            Kind::Entity,
            id,
            serde_json::json!({"id": id, "type": ["urn:x:Vehicle"]}),
        )
        .await
        .expect("write into the synthetic tenant");

    let row: Option<i32> = sqlx::query_scalar("SELECT 1 FROM tenants WHERE tenant_id = $1")
        .bind(synth.as_str())
        .fetch_optional(&pool)
        .await
        .expect("inventory read");
    assert!(
        row.is_some(),
        "{} lost the row the tenant enumeration reads",
        synth.as_str()
    );
    assert!(
        store
            .subscription_tenants()
            .await
            .expect("enumeration")
            .iter()
            .any(|t| t == synth.as_str()),
        "the notification pipeline cannot reach a tenant it cannot enumerate"
    );

    let inventory = store.tenant_ids().await.expect("inventory");
    assert!(
        !inventory.iter().any(|t| t == synth.as_str()),
        "the broker's own tenant is listed as an account: {inventory:?}"
    );

    assert!(
        CurrentStateDriver::purge_tenant(&store, &synth)
            .await
            .expect("purge"),
        "the teardown purges the synthetic tenant"
    );
    assert!(
        store
            .list(&synth, Kind::Entity)
            .await
            .expect("list")
            .is_empty(),
        "the copy survived its teardown"
    );
}

/// A repeat write must not rewrite the Tenant row. `claim_tenant` runs in
/// every document transaction, so an upsert that takes the exclusive row lock
/// serialises every write in a Tenant behind every other and leaves one dead
/// tuple per write on a table with one row per Tenant. The lock the purge
/// waits on is still taken; only the row version is not.
#[tokio::test(flavor = "multi_thread")]
async fn a_repeat_write_does_not_rewrite_the_tenant_row() {
    let url = require_db!();
    let pool = pg::connect(&url, 5).await.expect("connect+migrate");
    let run = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_millis();
    let t = TenantId::new(&format!("pgclaim{run}")).expect("tenant");

    let xmin = |pool: sqlx::PgPool, t: TenantId| async move {
        sqlx::query("SELECT xmin::text AS x FROM tenants WHERE tenant_id = $1")
            .bind(t.as_str())
            .fetch_one(&pool)
            .await
            .expect("xmin")
            .get::<String, _>("x")
    };
    let claim = |pool: sqlx::PgPool, t: TenantId| async move {
        let mut tx = pool.begin().await.expect("begin");
        pg::set_tenant(&mut tx, &t).await.expect("set_tenant");
        pg::claim_tenant(&mut tx, &t).await.expect("claim");
        tx.commit().await.expect("commit");
    };

    claim(pool.clone(), t.clone()).await;
    let first = xmin(pool.clone(), t.clone()).await;
    claim(pool.clone(), t.clone()).await;
    let second = xmin(pool.clone(), t.clone()).await;
    assert_eq!(
        first, second,
        "the second write rewrote the tenant row (xmin moved), so every write \
         in this tenant takes the exclusive row lock"
    );

    sqlx::query("DELETE FROM tenants WHERE tenant_id = $1")
        .bind(t.as_str())
        .execute(&pool)
        .await
        .expect("cleanup");
}

/// The invariant the row lock exists for: a purge must wait for a write that
/// is already in flight in the same Tenant, so it cannot step over it and
/// leave rows behind for a Tenant the inventory no longer names. The write
/// holds a shared lock, the purge asks for `FOR UPDATE`, and the two
/// conflict — proven here by giving the purge a deadline it must miss.
#[tokio::test(flavor = "multi_thread")]
async fn a_purge_waits_for_an_in_flight_write_in_the_same_tenant() {
    let url = require_db!();
    let pool = pg::connect(&url, 5).await.expect("connect+migrate");
    let run = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_millis();
    let t = TenantId::new(&format!("pgrace{run}")).expect("tenant");

    // the tenant exists before the race, so the purge has a row to lock
    let mut seed = pool.begin().await.expect("begin");
    pg::set_tenant(&mut seed, &t).await.expect("set_tenant");
    pg::claim_tenant(&mut seed, &t).await.expect("claim");
    seed.commit().await.expect("commit");

    // a write in flight: claimed, not yet committed
    let mut writing = pool.begin().await.expect("begin");
    pg::set_tenant(&mut writing, &t).await.expect("set_tenant");
    pg::claim_tenant(&mut writing, &t).await.expect("claim");

    let mut purge = pool.begin().await.expect("begin");
    sqlx::query("SET LOCAL lock_timeout = '750ms'")
        .execute(&mut *purge)
        .await
        .expect("lock_timeout");
    pg::set_tenant(&mut purge, &t).await.expect("set_tenant");
    let blocked =
        sqlx::query_scalar::<_, i32>("SELECT 1 FROM tenants WHERE tenant_id = $1 FOR UPDATE")
            .bind(t.as_str())
            .fetch_optional(&mut *purge)
            .await;
    assert!(
        blocked.is_err(),
        "the purge took the tenant row while a write held it, so it can step \
         over an in-flight write"
    );
    drop(purge);

    // once the write commits the purge gets the row
    writing.commit().await.expect("commit");
    let mut after = pool.begin().await.expect("begin");
    pg::set_tenant(&mut after, &t).await.expect("set_tenant");
    let got = sqlx::query_scalar::<_, i32>("SELECT 1 FROM tenants WHERE tenant_id = $1 FOR UPDATE")
        .bind(t.as_str())
        .fetch_optional(&mut *after)
        .await
        .expect("after the write commits the purge proceeds");
    assert_eq!(got, Some(1));
    after.commit().await.expect("commit");

    sqlx::query("DELETE FROM tenants WHERE tenant_id = $1")
        .bind(t.as_str())
        .execute(&pool)
        .await
        .expect("cleanup");
}
