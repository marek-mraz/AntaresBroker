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

/// Oldest-first page for the drain loop (F3). `seq` is the dedup id.
pub fn peek(pool: &PgPool, limit: i64) -> Result<Vec<(i64, String, Value)>, sqlx::Error> {
    wait(async {
        let rows = sqlx::query("SELECT seq, tenant_id, event FROM outbox ORDER BY seq LIMIT $1")
            .bind(limit)
            .fetch_all(pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|r| (r.get(0), r.get(1), r.get(2)))
            .collect())
    })
}

/// Ack everything published up to and including `seq`.
pub fn ack(pool: &PgPool, seq: i64) -> Result<u64, sqlx::Error> {
    wait(async {
        Ok(sqlx::query("DELETE FROM outbox WHERE seq <= $1")
            .bind(seq)
            .execute(pool)
            .await?
            .rows_affected())
    })
}
