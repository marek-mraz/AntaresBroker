//! Transactional outbox: the change event is
//! INSERTed in the SAME transaction as the entity write, so a broker crash
//! between commit and publish can never lose an event. The drain loop
//! publishes rows to the bus with `Nats-Msg-Id` = `seq` for dedup, then acks
//! by deleting up to the published seq.
//!
//! Producer wiring into the entity write paths lands WITH the drain:
//! enqueuing events nothing consumes would only grow the table
//! without bound.

use antares_model::TenantId;
use serde_json::Value;
use sqlx::postgres::{PgConnection, PgPool};
use sqlx::Row;

use super::pg_entity::wait;

/// Every statement this module issues. Values are bound as `$n` — the strings
/// are compile-time constants, so no request data ever reaches the parser.
const ENQUEUE_SQL: &str = "INSERT INTO outbox (tenant_id, event) VALUES ($1, $2) RETURNING seq";
const ENQUEUE_MANY_SQL: &str = "INSERT INTO outbox (tenant_id, event)
     SELECT $1, e FROM jsonb_array_elements($2::jsonb) AS e";
const PEEK_SQL: &str = "SELECT seq, tenant_id, event FROM outbox ORDER BY seq LIMIT $1";
const ACK_SQL: &str = "DELETE FROM outbox WHERE seq = ANY($1)";

/// Enqueue one event INSIDE the caller's transaction (same-tx INSERT).
pub async fn enqueue(
    tx: &mut PgConnection,
    tenant: &TenantId,
    event: &Value,
) -> Result<i64, sqlx::Error> {
    let row = sqlx::query(ENQUEUE_SQL)
        .bind(tenant.as_str())
        .bind(event)
        .fetch_one(tx)
        .await?;
    Ok(row.get::<i64, _>(0))
}

/// Enqueue a whole batch in ONE multi-row INSERT (a
/// per-item loop cost N round-trips inside every batch transaction).
pub async fn enqueue_many(
    tx: &mut PgConnection,
    tenant: &TenantId,
    events: &[Value],
) -> Result<(), sqlx::Error> {
    if events.is_empty() {
        return Ok(());
    }
    sqlx::query(ENQUEUE_MANY_SQL)
        .bind(tenant.as_str())
        .bind(Value::Array(events.to_vec()))
        .execute(tx)
        .await
        .map(|_| ())
}

/// Oldest-first page for the drain loop. `seq` is the dedup id.
///
/// The drain is cross-tenant by nature — it runs under the transaction-scoped
/// `antares.service` escape (migration 0005) so it stays correct under a
/// non-superuser role, where the plain tenant policy would silently return
/// zero rows forever (the very failure this table exists to prevent).
pub fn peek(pool: &PgPool, limit: i64) -> Result<Vec<(i64, String, Value)>, sqlx::Error> {
    wait(async {
        let mut tx = pool.begin().await?;
        crate::pg::set_service(&mut tx).await?;
        let rows = sqlx::query(PEEK_SQL)
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

/// Ack EXACTLY the published seqs: bigserial
/// allocates at INSERT and commits land out of order, so a blanket
/// `seq <= max` deletes a lower-seq row that commits between peek and ack —
/// an event lost unpublished. Deleting by exact seq can never touch a row
/// the drain did not publish.
pub fn ack(pool: &PgPool, seqs: &[i64]) -> Result<u64, sqlx::Error> {
    if seqs.is_empty() {
        return Ok(0);
    }
    wait(async {
        let mut tx = pool.begin().await?;
        crate::pg::set_service(&mut tx).await?;
        let n = sqlx::query(ACK_SQL)
            .bind(seqs)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        tx.commit().await?;
        Ok(n)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    /// A pool that has never connected: any statement issued through it fails
    /// immediately, so a test that succeeds proves no statement was issued.
    fn unreachable_pool() -> PgPool {
        PgPoolOptions::new()
            .connect_lazy("postgres://nobody@127.0.0.1:1/antares_no_such_db")
            .expect("lazy pool")
    }

    /// Acking an empty page must short-circuit: without the guard the drain
    /// opens a transaction (and `seq = ANY('{}')` scans) on every idle tick.
    #[test]
    fn ack_of_nothing_issues_no_statement() {
        assert_eq!(ack(&unreachable_pool(), &[]).expect("noop ack"), 0);
    }

    /// The ack deletes the published seqs one by one. A range form
    /// (`seq <= max`) also deletes a lower-seq row that committed between peek
    /// and ack — an event dropped without ever being published.
    #[test]
    fn ack_deletes_by_exact_seq_never_by_range() {
        assert!(ACK_SQL.contains("seq = ANY($1)"));
        assert!(
            !ACK_SQL.contains("<="),
            "range ack loses gap rows: {ACK_SQL}"
        );
        assert!(!ACK_SQL.contains('<'));
    }

    /// The drain's page size is bound, not interpolated, and the page is
    /// oldest-first — publishing order is the commit order the bus dedups on.
    #[test]
    fn peek_is_bounded_and_oldest_first() {
        assert!(PEEK_SQL.contains("ORDER BY seq"));
        assert!(PEEK_SQL.contains("LIMIT $1"));
        assert!(
            !PEEK_SQL.contains("LIMIT {"),
            "page size must not be formatted in"
        );
    }

    /// Both enqueue paths write the tenant column from a bound parameter; the
    /// batch form stays ONE statement (a per-item loop would cost N round
    /// trips inside every write transaction).
    #[test]
    fn enqueue_binds_the_tenant_and_batches_in_one_statement() {
        for sql in [ENQUEUE_SQL, ENQUEUE_MANY_SQL] {
            assert!(sql.contains("INSERT INTO outbox (tenant_id, event)"));
            assert!(sql.contains("$1"));
        }
        assert!(ENQUEUE_MANY_SQL.contains("jsonb_array_elements($2::jsonb)"));
        assert_eq!(ENQUEUE_MANY_SQL.matches("INSERT").count(), 1);
    }
}
