// SPDX-License-Identifier: EUPL-1.2
//! Postgres foundation: ONE shared pool, embedded migrations,
//! transaction-scoped tenancy. Store implementations build on top.

pub mod doc;
pub mod entity;
pub mod maintenance;
pub mod outbox;
pub mod temporal;

use antares_model::TenantId;
use serde_json::Value;
use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::{Postgres, Transaction};

/// Embedded migrations, run at start (like Scorpio's Flyway, but once).
///
/// `ANTARES_MIGRATE=0`/`false` skips that run on this process, so serving
/// replicas do not race the same DDL and a deployment can migrate once from a
/// separate job or init container against the same database. Unset — the
/// default — migrates on boot exactly as before.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Is this connect failure a database whose migration history can never match
/// this binary? A database still booting is worth waiting for; a history from
/// another release is not, and a caller that retries it reports the timeout
/// instead of the cause. Only the history errors are permanent: an execution
/// failure can be a lock the next attempt gets.
pub fn is_schema_mismatch(e: &sqlx::Error) -> bool {
    use sqlx::migrate::MigrateError::{
        Dirty, VersionMismatch, VersionMissing, VersionNotPresent, VersionTooNew, VersionTooOld,
    };
    match e {
        sqlx::Error::Migrate(m) => matches!(
            **m,
            VersionMissing(_)
                | VersionMismatch(_)
                | VersionNotPresent(_)
                | VersionTooOld(..)
                | VersionTooNew(..)
                | Dirty(_)
        ),
        _ => false,
    }
}

/// The `ANTARES_MIGRATE` switch: off only for an explicit `0`/`false`.
fn migrate_enabled(v: Option<&str>) -> bool {
    !matches!(v, Some("0" | "false"))
}

/// One shared pool for all tenants — never per-tenant pools.
/// `max_connections` ≈ 2× the PG box's cores; the default suits a small dev
/// Postgres, deployments size it via config.
///
/// Every acquisition and session is bounded: a saturated pool fails the one
/// request after 5 s instead of queueing forever; idle/aged connections are
/// recycled; and each session carries `statement_timeout`/`lock_timeout` so
/// a runaway query or lost lock can never wedge a pooled connection.
pub async fn connect(url: &str, max_connections: u32) -> Result<PgPool, sqlx::Error> {
    connect_with(url, max_connections, std::time::Duration::from_secs(30)).await
}

/// [`connect`] with the per-session `statement_timeout` chosen by the
/// deployment (`ANTARES_PG_STATEMENT_TIMEOUT_MS`).
pub async fn connect_with(
    url: &str,
    max_connections: u32,
    statement_timeout: std::time::Duration,
) -> Result<PgPool, sqlx::Error> {
    use std::time::Duration;
    let session = format!(
        "SET statement_timeout = '{}ms'; SET lock_timeout = '5s'",
        statement_timeout.as_millis()
    );
    let pool = PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(Duration::from_secs(5))
        .idle_timeout(Duration::from_secs(600))
        .max_lifetime(Duration::from_secs(1800))
        .after_connect(move |conn, _meta| {
            let session = session.clone();
            Box::pin(async move {
                use sqlx::Executor;
                // the only interpolated value is a validated integer of
                // milliseconds (parsed by the broker), never client text
                conn.execute(sqlx::raw_sql(sqlx::AssertSqlSafe(session)))
                    .await?;
                Ok(())
            })
        })
        .connect(url)
        .await?;
    // DDL is exempt from the per-session statement timeout: building an index
    // on a large attr_instances legitimately runs longer than a query ever
    // should. The lock timeout stays — a migration blocked on a lock must
    // fail rather than hold the boot path.
    //
    // `SET` is session-scoped and the pool has no release hook, so the
    // connection is DETACHED first: it never goes back into the pool, and no
    // request is ever served by a connection with the runaway-query wall
    // switched off. The pool opens a fresh one (running `after_connect`) on
    // demand.
    if migrate_enabled(std::env::var("ANTARES_MIGRATE").ok().as_deref()) {
        use sqlx::{Connection, Executor};
        let mut migrate_conn = pool.acquire().await?.detach();
        migrate_conn
            .execute(sqlx::raw_sql("SET statement_timeout = 0"))
            .await?;
        MIGRATOR.run(&mut migrate_conn).await?;
        migrate_conn.close().await?;
    }
    Ok(pool)
}

/// Make RLS effective for this transaction — SET LOCAL only (transaction
/// scoped), so a recycled pooled connection carries no tenant residue.
/// Call first in EVERY transaction that touches tenant data.
pub async fn set_tenant(
    tx: &mut Transaction<'_, Postgres>,
    tenant: &TenantId,
) -> Result<(), sqlx::Error> {
    sqlx::query(crate::SET_TENANT_SQL)
        .bind(tenant.as_str())
        .execute(&mut **tx)
        .await
        .map(|_| ())
}

/// Does the connected role bypass RLS (superuser or BYPASSRLS)? RLS is
/// a belt only when the role wears it — the broker warns at startup when the
/// belt is off.
///
/// A probe that cannot answer fails CLOSED: an unreachable or erroring
/// database is reported as bypassing, so the strict gate refuses the boot
/// instead of passing on an error.
pub async fn role_bypasses_rls(pool: &PgPool) -> bool {
    bypasses(
        sqlx::query_scalar(
            "SELECT rolsuper OR rolbypassrls FROM pg_roles WHERE rolname = current_user",
        )
        .fetch_one(pool)
        .await,
    )
}

/// What the connected server is, read ONCE at startup and served from
/// `/q/health` afterwards: server version plus the version of each extension
/// the broker's behaviour depends on. A probe that fails is not fatal — the
/// broker serves fine without knowing its own server version — so the caller
/// gets an empty object and health simply says nothing.
pub async fn version_info(pool: &PgPool) -> Value {
    let row: Result<(String, Option<String>, Option<String>), sqlx::Error> = sqlx::query_as(
        "SELECT current_setting('server_version'), \
         (SELECT extversion FROM pg_extension WHERE extname = 'postgis'), \
         (SELECT extversion FROM pg_extension WHERE extname = 'timescaledb')",
    )
    .fetch_one(pool)
    .await;
    match row {
        Ok((server, postgis, timescale)) => {
            let mut m = serde_json::Map::new();
            m.insert("engine".into(), "postgres".into());
            m.insert("server".into(), server.into());
            if let Some(v) = postgis {
                m.insert("postgis".into(), v.into());
            }
            if let Some(v) = timescale {
                m.insert("timescaledb".into(), v.into());
            }
            Value::Object(m)
        }
        Err(e) => {
            tracing::warn!("server version probe failed ({e}); /q/health will not report it");
            Value::Object(serde_json::Map::new())
        }
    }
}

fn bypasses(probe: Result<bool, sqlx::Error>) -> bool {
    probe.unwrap_or_else(|e| {
        tracing::error!(
            "row-level-security probe failed ({e}) — treating the role as RLS-bypassing"
        );
        true
    })
}

/// Arm the transaction-scoped `antares.service` escape (0001_init.sql) for
/// the two internal cross-tenant jobs: outbox drain and temporal retention.
/// NEVER call from a request path — request queries carry explicit tenant
/// predicates and run under `set_tenant` only.
pub async fn set_service(tx: &mut Transaction<'_, Postgres>) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT set_config('antares.service', 'on', true)")
        .execute(&mut **tx)
        .await
        .map(|_| ())
}

/// The write's own claim on its tenant (ADR-0001: "every implicit tenant
/// creation inserts its row in the same transaction as the document"). Two
/// things ride on it being IN the document's transaction rather than before
/// it: the tenant row and the rows it accounts for commit together, and the
/// `DO UPDATE` takes the row lock, so the purge's `SELECT … FOR UPDATE`
/// waits for an in-flight first write instead of stepping over it. Committed
/// separately, a write racing a purge leaves rows behind for a tenant the
/// inventory no longer names — readable to whoever sends that tenant header,
/// listed by nothing, and reclaimable by no further purge.
///
/// Idempotent, so two concurrent first writes both succeed (vs Scorpio's
/// CREATE DATABASE + Flyway deadlock).
pub async fn claim_tenant(
    tx: &mut Transaction<'_, Postgres>,
    tenant: &TenantId,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO tenants (tenant_id) VALUES ($1) \
         ON CONFLICT (tenant_id) DO UPDATE SET tenant_id = EXCLUDED.tenant_id",
    )
    .bind(tenant.as_str())
    .execute(&mut **tx)
    .await
    .map(|_| ())
}

/// The same claim outside a document transaction, for the callers that have
/// no document to pair it with (test seeds, the maintenance paths).
pub async fn ensure_tenant(pool: &PgPool, tenant: &TenantId) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO tenants (tenant_id) VALUES ($1) ON CONFLICT DO NOTHING")
        .bind(tenant.as_str())
        .execute(pool)
        .await
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::{bypasses, is_schema_mismatch, migrate_enabled};

    /// The RLS probe answers a security gate: a probe that errors must read as
    /// "unsafe", never as "the role does not bypass RLS".
    #[test]
    fn rls_probe_fails_closed() {
        assert!(!bypasses(Ok(false)), "a role without BYPASSRLS is safe");
        assert!(bypasses(Ok(true)), "superuser/BYPASSRLS bypasses");
        assert!(
            bypasses(Err(sqlx::Error::PoolTimedOut)),
            "an unanswerable probe must not pass the gate"
        );
        assert!(bypasses(Err(sqlx::Error::RowNotFound)));
    }

    /// A database from another release is a permanent failure; a database
    /// that is not up yet is not. The boot path retries the second and dies on
    /// the first, so the classification decides which cause an operator is
    /// shown — the squash of the pre-1.0 migration set left a 0.1.0 database
    /// reporting "not reachable after 30 s" for a schema it would never match.
    #[test]
    fn a_history_from_another_release_is_permanent_and_a_booting_database_is_not() {
        use sqlx::migrate::MigrateError;
        for e in [
            MigrateError::VersionMissing(5),
            MigrateError::VersionMismatch(1),
            MigrateError::VersionNotPresent(9),
            MigrateError::VersionTooOld(1, 4),
            MigrateError::VersionTooNew(9, 4),
            MigrateError::Dirty(3),
        ] {
            let shown = e.to_string();
            assert!(
                is_schema_mismatch(&sqlx::Error::Migrate(Box::new(e))),
                "{shown} can never be resolved by waiting"
            );
        }
        assert!(
            !is_schema_mismatch(&sqlx::Error::PoolTimedOut),
            "a database still booting is worth another attempt"
        );
        assert!(!is_schema_mismatch(&sqlx::Error::RowNotFound));
        assert!(
            !is_schema_mismatch(&sqlx::Error::Migrate(Box::new(MigrateError::Execute(
                sqlx::Error::PoolTimedOut
            )))),
            "a migration that failed to execute may be a lock the next attempt gets"
        );
    }

    /// Migrations stay on unless a deployment explicitly turns them off.
    #[test]
    fn migrate_switch_defaults_on() {
        assert!(migrate_enabled(None));
        assert!(migrate_enabled(Some("1")));
        assert!(migrate_enabled(Some("true")));
        assert!(!migrate_enabled(Some("0")));
        assert!(!migrate_enabled(Some("false")));
    }
}
