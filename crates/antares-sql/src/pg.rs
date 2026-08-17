//! Postgres foundation: ONE shared pool, embedded migrations,
//! transaction-scoped tenancy. Store implementations build on top.

use antares_model::TenantId;
use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::{Postgres, Transaction};

/// Embedded migrations, run at start (like Scorpio's Flyway, but once).
///
/// `ANTARES_MIGRATE=0`/`false` skips that run on this process, so serving
/// replicas do not race the same DDL and a deployment can migrate once from a
/// separate job or init container against the same database. Unset — the
/// default — migrates on boot exactly as before.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

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
    use std::time::Duration;
    let pool = PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(Duration::from_secs(5))
        .idle_timeout(Duration::from_secs(600))
        .max_lifetime(Duration::from_secs(1800))
        .after_connect(|conn, _meta| {
            Box::pin(async move {
                use sqlx::Executor;
                conn.execute(sqlx::raw_sql(
                    "SET statement_timeout = '30s'; SET lock_timeout = '5s'",
                ))
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
    if migrate_enabled(std::env::var("ANTARES_MIGRATE").ok().as_deref()) {
        use sqlx::Executor;
        let mut migrate_conn = pool.acquire().await?;
        migrate_conn
            .execute(sqlx::raw_sql("SET statement_timeout = 0"))
            .await?;
        MIGRATOR.run(&mut *migrate_conn).await?;
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

fn bypasses(probe: Result<bool, sqlx::Error>) -> bool {
    probe.unwrap_or_else(|e| {
        tracing::error!(
            "row-level-security probe failed ({e}) — treating the role as RLS-bypassing"
        );
        true
    })
}

/// Arm the transaction-scoped `antares.service` escape (migration 0005) for
/// the two internal cross-tenant jobs: outbox drain and temporal retention.
/// NEVER call from a request path — request queries carry explicit tenant
/// predicates and run under `set_tenant` only.
pub async fn set_service(tx: &mut Transaction<'_, Postgres>) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT set_config('antares.service', 'on', true)")
        .execute(&mut **tx)
        .await
        .map(|_| ())
}

/// Tenant auto-create is one idempotent upsert — two concurrent
/// first-writes both succeed (vs Scorpio's CREATE DATABASE + Flyway deadlock).
pub async fn ensure_tenant(pool: &PgPool, tenant: &TenantId) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO tenants (tenant_id) VALUES ($1) ON CONFLICT DO NOTHING")
        .bind(tenant.as_str())
        .execute(pool)
        .await
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::{bypasses, migrate_enabled};

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
