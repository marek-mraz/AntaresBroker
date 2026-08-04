//! Temporal maintenance (tasks.md C9/D4; §8.2): the broker's own scheduled
//! job replaces TimescaleDB background workers in plain mode, and drives the
//! retention knob in both modes.
//!
//! Single-winner rule (§3.1.6): the run is claimed via
//! `SELECT … FOR UPDATE SKIP LOCKED` on the `maintenance_jobs` row — N
//! instances race, one wins, the rest skip. No coordinator.
//!
//! Plain-mode partitioning: weekly partitions are pre-created for a window
//! around now; everything else (historic backfill) lands in the DEFAULT
//! partition. Creating a partition whose range already has rows sitting in
//! the DEFAULT partition fails in PostgreSQL — such ranges are logged and
//! skipped, and those rows simply stay in the default partition (correct,
//! just unpartitioned).

use sqlx::postgres::PgPool;
use sqlx::Row;

/// True when the timescaledb extension is CREATED in this database (D3 —
/// per-database `pg_extension`, not "installed on the server").
pub async fn timescale_present(pool: &PgPool) -> Result<bool, sqlx::Error> {
    let row = sqlx::query("SELECT 1 FROM pg_extension WHERE extname = 'timescaledb'")
        .fetch_optional(pool)
        .await?;
    Ok(row.is_some())
}

/// One maintenance pass. Returns a short human-readable summary ("skipped"
/// when another instance holds the claim). `retention_days = None` keeps
/// history forever — retention is a deliberate deployment knob, never a
/// default.
pub async fn temporal_maintenance(
    pool: &PgPool,
    retention_days: Option<i64>,
) -> Result<String, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let claimed = sqlx::query(
        "SELECT name FROM maintenance_jobs WHERE name = 'temporal_partitions'
         FOR UPDATE SKIP LOCKED",
    )
    .fetch_optional(&mut *tx)
    .await?;
    if claimed.is_none() {
        return Ok("skipped: another instance holds the claim".into());
    }
    let timescale = timescale_present(pool).await?;
    let mut done: Vec<String> = Vec::new();
    if timescale {
        if let Some(days) = retention_days {
            sqlx::query("SELECT public.drop_chunks('attr_instances', older_than => make_interval(days => $1::int))")
                .bind(days)
                .execute(&mut *tx)
                .await?;
            done.push(format!("timescale drop_chunks older than {days}d"));
        }
    } else {
        // weekly partitions for [now-1w, now+4w)
        for off in -1i64..4 {
            let row = sqlx::query(
                "SELECT to_char(date_trunc('week', now()) + make_interval(weeks => $1::int), 'IYYY\"w\"IW') AS suffix,
                        (date_trunc('week', now()) + make_interval(weeks => $1::int))::text AS lo,
                        (date_trunc('week', now()) + make_interval(weeks => ($1::int) + 1))::text AS hi",
            )
            .bind(off)
            .fetch_one(&mut *tx)
            .await?;
            let (suffix, lo, hi): (String, String, String) =
                (row.get("suffix"), row.get("lo"), row.get("hi"));
            let ddl = format!(
                "CREATE TABLE IF NOT EXISTS attr_instances_{suffix} PARTITION OF attr_instances \
                 FOR VALUES FROM ('{lo}') TO ('{hi}')"
            );
            if let Err(e) = sqlx::query(sqlx::AssertSqlSafe(ddl))
                .execute(&mut *tx)
                .await
            {
                // rows for this range already sit in the DEFAULT partition —
                // fine, they stay there (see module docs).
                tracing::debug!("partition attr_instances_{suffix} not created: {e}");
                done.push(format!("partition {suffix}: left in default"));
            } else {
                done.push(format!("partition {suffix}: ok"));
            }
        }
        if let Some(days) = retention_days {
            // drop whole partitions strictly older than the horizon
            let parts = sqlx::query(
                "SELECT c.relname,
                        pg_get_expr(c.relpartbound, c.oid) AS bound
                 FROM pg_inherits i
                 JOIN pg_class c ON c.oid = i.inhrelid
                 JOIN pg_class p ON p.oid = i.inhparent
                 WHERE p.relname = 'attr_instances'",
            )
            .fetch_all(&mut *tx)
            .await?;
            for r in parts {
                let name: String = r.get("relname");
                let bound: String = r.get::<Option<String>, _>("bound").unwrap_or_default();
                // bound looks like: FOR VALUES FROM ('2026-07-27 ...') TO ('2026-08-03 ...')
                let Some(hi) = bound
                    .split("TO ('")
                    .nth(1)
                    .and_then(|s| s.split('\'').next())
                else {
                    continue; // DEFAULT partition — never dropped
                };
                let expired: bool = sqlx::query_scalar(
                    "SELECT $1::timestamptz < now() - make_interval(days => $2::int)",
                )
                .bind(hi)
                .bind(days)
                .fetch_one(&mut *tx)
                .await?;
                if expired {
                    sqlx::query(sqlx::AssertSqlSafe(format!("DROP TABLE {name}")))
                        .execute(&mut *tx)
                        .await?;
                    done.push(format!("dropped expired partition {name}"));
                }
            }
        }
    }
    sqlx::query("UPDATE maintenance_jobs SET last_run = now() WHERE name = 'temporal_partitions'")
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    if done.is_empty() {
        done.push("nothing to do".into());
    }
    Ok(done.join("; "))
}
