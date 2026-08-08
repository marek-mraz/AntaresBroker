//! Transactional outbox (tasks.md C8; deep-analysis §10): the change event is
//! INSERTed in the SAME transaction as the entity write, so a broker crash
//! between commit and publish can never lose an event. The drain loop (F3)
//! publishes rows to the bus with `Nats-Msg-Id` = `seq` for dedup, then acks
//! by deleting up to the published seq.
//!
//! Producer wiring into the entity write paths lands WITH the F3 drain:
//! enqueuing events nothing consumes would only grow the table (the R4
//! unbounded-growth lesson, §4.1).

use antares_model::TenantId;
use serde_json::Value;
use sqlx::postgres::{PgConnection, PgPool};
use sqlx::Row;

use super::pg_entity::wait;

/// Enqueue one event INSIDE the caller's transaction (§10: same-tx INSERT).
pub async fn enqueue(
    tx: &mut PgConnection,
    tenant: &TenantId,
    event: &Value,
) -> Result<i64, sqlx::Error> {
    let row = sqlx::query("INSERT INTO outbox (tenant_id, event) VALUES ($1, $2) RETURNING seq")
        .bind(tenant.as_str())
        .bind(event)
        .fetch_one(tx)
        .await?;
    Ok(row.get::<i64, _>(0))
}

/// Enqueue a whole batch in ONE multi-row INSERT (audit 2026-08-08: the
/// per-item loop cost N round-trips inside every batch transaction).
pub async fn enqueue_many(
    tx: &mut PgConnection,
    tenant: &TenantId,
    events: &[Value],
) -> Result<(), sqlx::Error> {
    if events.is_empty() {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO outbox (tenant_id, event)
         SELECT $1, e FROM jsonb_array_elements($2::jsonb) AS e",
    )
    .bind(tenant.as_str())
    .bind(Value::Array(events.to_vec()))
    .execute(tx)
    .await
    .map(|_| ())
}

/// Oldest-first page for the drain loop (F3). `seq` is the dedup id.
///
/// The drain is cross-tenant by nature — it runs under the transaction-scoped
/// `antares.service` escape (migration 0005) so it stays correct under a
/// non-superuser role, where the plain tenant policy would silently return
/// zero rows forever (the R4 failure this table exists to prevent).
pub fn peek(pool: &PgPool, limit: i64) -> Result<Vec<(i64, String, Value)>, sqlx::Error> {
    wait(async {
        let mut tx = pool.begin().await?;
        crate::pg::set_service(&mut tx).await?;
        let rows = sqlx::query("SELECT seq, tenant_id, event FROM outbox ORDER BY seq LIMIT $1")
            .bind(limit)
            .fetch_all(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(rows
            .into_iter()
            .map(|r| (r.get(0), r.get(1), r.get(2)))
            .collect())
    })
}

/// Ack everything published up to and including `seq`.
pub fn ack(pool: &PgPool, seq: i64) -> Result<u64, sqlx::Error> {
    wait(async {
        let mut tx = pool.begin().await?;
        crate::pg::set_service(&mut tx).await?;
        let n = sqlx::query("DELETE FROM outbox WHERE seq <= $1")
            .bind(seq)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        tx.commit().await?;
        Ok(n)
    })
}
