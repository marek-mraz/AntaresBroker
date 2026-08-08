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
use sqlx::{Acquire, Row};

/// True when the timescaledb extension is CREATED in this database (D3 —
/// per-database `pg_extension`, not "installed on the server").
pub async fn timescale_present(pool: &PgPool) -> Result<bool, sqlx::Error> {
    let row = sqlx::query("SELECT 1 FROM pg_extension WHERE extname = 'timescaledb'")
        .fetch_optional(pool)
        .await?;
    Ok(row.is_some())
}

/// What the migrations actually built `attr_instances` as. Detected ONCE at
/// startup from the catalog and pinned — never re-probed per tick, so the
/// maintenance branch can never disagree with the DDL on disk (the
/// "extension installed after first boot" trap).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TemporalBackend {
    /// timescale hypertable (relkind 'r' + timescaledb catalog entry)
    Hypertable,
    /// native PARTITION BY RANGE (relkind 'p')
    Partitioned,
}

/// Inspect the catalog. Errors when `attr_instances` is neither a hypertable
/// nor a partitioned table — that means the database was migrated under one
/// extension state and is now running under another; refusing beats running
/// the wrong maintenance jobs against mismatched DDL.
pub async fn detect_temporal_backend(pool: &PgPool) -> Result<TemporalBackend, String> {
    let relkind: Option<String> = sqlx::query_scalar(
        "SELECT c.relkind::text FROM pg_class c
         WHERE c.relname = 'attr_instances' AND c.relnamespace = 'public'::regnamespace",
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;
    match relkind.as_deref() {
        Some("p") => Ok(TemporalBackend::Partitioned),
        Some("r") => {
            let hyper = sqlx::query(
                "SELECT 1 FROM timescaledb_information.hypertables
                 WHERE hypertable_name = 'attr_instances'",
            )
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?;
            if hyper.is_some() {
                Ok(TemporalBackend::Hypertable)
            } else {
                Err(
                    "attr_instances is a plain table — the database was migrated as a \
                     hypertable and the timescaledb extension has since been removed, or \
                     the catalog is damaged; refusing to run temporal maintenance"
                        .into(),
                )
            }
        }
        other => Err(format!(
            "attr_instances has unexpected relkind {other:?} — migrations did not run?"
        )),
    }
}

/// One maintenance pass. Returns a short human-readable summary ("skipped"
/// when another instance holds the claim). `retention_days = None` keeps
/// history forever — retention is a deliberate deployment knob, never a
/// default.
pub async fn temporal_maintenance(
    pool: &PgPool,
    backend: TemporalBackend,
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
    // retention DML is cross-tenant service work (see migration 0005)
    crate::pg::set_service(&mut tx).await?;
    let mut done: Vec<String> = Vec::new();
    // 4.22 garbage collection: expired transient entities are reaped here —
    // reads already refuse them, so the lag this job runs at is invisible
    // (the clause itself sanctions lagging deletion). Runs on both backends.
    let reaped =
        sqlx::query("DELETE FROM entities WHERE expires_at IS NOT NULL AND expires_at < now()")
            .execute(&mut *tx)
            .await?
            .rows_affected();
    if reaped > 0 {
        done.push(format!("reaped {reaped} expired transient entities (4.22)"));
    }
    if backend == TemporalBackend::Hypertable {
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
            // The failure below is EXPECTED (see module docs), and in
            // PostgreSQL a failed statement aborts the whole transaction —
            // every later one then returns 25P02. Tolerating an error means
            // owning a savepoint to roll back to; without it the first
            // already-occupied range poisons the entire maintenance pass.
            let mut sp = tx.begin().await?;
            match sqlx::query(sqlx::AssertSqlSafe(ddl))
                .execute(&mut *sp)
                .await
            {
                Ok(_) => {
                    sp.commit().await?;
                    done.push(format!("partition {suffix}: ok"));
                }
                Err(e) => {
                    // rows for this range already sit in the DEFAULT partition —
                    // fine, they stay there (see module docs). warn, not debug:
                    // a PERMANENTLY failing create (permissions) must be visible
                    // at default log level, or it silently degrades to
                    // "everything lands in DEFAULT".
                    sp.rollback().await?;
                    tracing::warn!("partition attr_instances_{suffix} not created: {e}");
                    done.push(format!("partition {suffix}: left in default"));
                }
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
            // The DEFAULT partition has no upper bound and is never dropped —
            // reclaim its expired rows (historic backfill that landed there
            // before its week partition existed) by DELETE, or they are
            // retained forever.
            let purged = sqlx::query(
                "DELETE FROM attr_instances_default
                 WHERE observed_at < now() - make_interval(days => $1::int)",
            )
            .bind(days)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            if purged > 0 {
                done.push(format!(
                    "purged {purged} expired rows from DEFAULT partition"
                ));
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
