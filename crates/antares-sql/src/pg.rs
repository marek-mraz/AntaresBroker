//! Postgres foundation (tasks.md §C-i): ONE shared pool, embedded migrations,
//! transaction-scoped tenancy. Store implementations land per §C-ii.

use antares_model::TenantId;
use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::{Postgres, Transaction};

/// Embedded migrations, run at start (§8: like Scorpio's Flyway, but once).
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// One shared pool for all tenants (§6.2 — never per-tenant pools, §14.2).
/// `max_connections` ≈ 2× the PG box's cores; the default suits a small dev
/// Postgres, deployments size it via config.
pub async fn connect(url: &str, max_connections: u32) -> Result<PgPool, sqlx::Error> {
    let pool = PgPoolOptions::new()
        .max_connections(max_connections)
        .connect(url)
        .await?;
    MIGRATOR.run(&pool).await?;
    Ok(pool)
}

/// §3: make RLS effective for this transaction — SET LOCAL only (transaction
/// scoped), so a recycled pooled connection carries no tenant residue
/// (§3.1.5). Call first in EVERY transaction that touches tenant data.
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

/// §3.1.4: tenant auto-create is one idempotent upsert — two concurrent
/// first-writes both succeed (vs Scorpio's CREATE DATABASE + Flyway deadlock).
pub async fn ensure_tenant(pool: &PgPool, tenant: &TenantId) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO tenants (tenant_id) VALUES ($1) ON CONFLICT DO NOTHING")
        .bind(tenant.as_str())
        .execute(pool)
        .await
        .map(|_| ())
}
