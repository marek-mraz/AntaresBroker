//! Temporal maintenance: the broker's own scheduled
//! job replaces TimescaleDB background workers in plain mode, and drives the
//! retention knob in both modes.
//!
//! Single-winner rule: the run is claimed via
//! `SELECT … FOR UPDATE SKIP LOCKED` on the `maintenance_jobs` row — N
//! instances race, one wins, the rest skip. No coordinator.
//!
//! Plain-mode partitioning: weekly partitions are pre-created for a window
//! around now; everything else (historic backfill) lands in the DEFAULT
//! partition. Creating a partition whose range already has rows sitting in
//! the DEFAULT partition fails in PostgreSQL, so a single row written with an
//! `observedAt` past the window permanently blocks that week's partition and
//! sends all of its later traffic to DEFAULT as well. Such a range is
//! recovered rather than skipped: the rows are moved out of DEFAULT into a
//! standalone table, which is then ATTACHed as the partition.
//!
//! The recovery belongs here and NOT at ingest, for two reasons. The first is
//! internal: clamping an `observedAt` outside a horizon would let the
//! `observed_at` column disagree with the raw timestamp string in `data`, and
//! `compile::temporal::column_range_bound` may prune on that column only
//! because it is a superset of the byte-exact text window (4.11). The second
//! is normative: 4.8 defines `observedAt` as "the temporal Property at which a
//! certain Property or Relationship became valid or was observed" and requires
//! only that it be a 4.6.3 DateTime — there is no horizon and no error type
//! for one, and a forecast Property legitimately becomes valid in the future.
//! A well-formed future `observedAt` is therefore valid input the broker
//! stores, not input it may refuse.
//!
//! Residual, deliberate: rows whose `observed_at` lies beyond the pre-created
//! window stay in DEFAULT until their week enters it, and rows dated far
//! enough ahead stay there indefinitely — retention only purges DEFAULT rows
//! that are already OLD. Auto-adopting arbitrary future weeks is not the fix
//! either: one row per week over a century would trade an oversized DEFAULT
//! for thousands of partitions. The condition is reported instead (see
//! `default_partition_load`), so an operator sees it rather than discovering
//! it as a query slowdown.

use sqlx::postgres::PgPool;
use sqlx::{Acquire, Row};

/// True when the timescaledb extension is CREATED in this database
/// (per-database `pg_extension`, not "installed on the server").
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
    crate::store::pg::set_service(&mut tx).await?;
    let mut done: Vec<String> = Vec::new();
    // The 4.22 reaps run in their own transactions AFTER this one commits: both
    // DELETEs grow with stored volume and can exceed the connection's
    // statement_timeout, and a reap that times out must not abort the partition
    // pre-creation that keeps ingest writable.
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
            // The failure below is EXPECTED (see module docs), and in
            // PostgreSQL a failed statement aborts the whole transaction —
            // every later one then returns 25P02. Tolerating an error means
            // owning a savepoint to roll back to; without it the first
            // already-occupied range poisons the entire maintenance pass.
            let mut sp = tx.begin().await?;
            match sqlx::query(sqlx::AssertSqlSafe(create_partition_sql(&suffix, &lo, &hi)))
                .execute(&mut *sp)
                .await
            {
                Ok(_) => {
                    sp.commit().await?;
                    done.push(format!("partition {suffix}: ok"));
                    continue;
                }
                Err(_) => sp.rollback().await?,
            }
            // Rows for this range already sit in DEFAULT. Adopt them: the move
            // empties the range, so the ATTACH's revalidation of DEFAULT
            // passes. Its ACCESS EXCLUSIVE lock is bounded by the connection's
            // lock_timeout, and a loser simply retries on the next tick.
            let mut sp = tx.begin().await?;
            let mut adopted = Ok(());
            for stmt in adopt_default_rows_sql(&suffix, &lo, &hi) {
                adopted = sqlx::query(sqlx::AssertSqlSafe(stmt))
                    .execute(&mut *sp)
                    .await
                    .map(|_| ());
                if adopted.is_err() {
                    break;
                }
            }
            match adopted {
                Ok(()) => {
                    sp.commit().await?;
                    done.push(format!("partition {suffix}: adopted from default"));
                }
                Err(e) => {
                    // warn, not debug: a PERMANENTLY failing create
                    // (permissions) must be visible at default log level, or it
                    // silently degrades to "everything lands in DEFAULT".
                    sp.rollback().await?;
                    tracing::warn!("partition attr_instances_{suffix} not created: {e}");
                    done.push(format!("partition {suffix}: left in default"));
                }
            }
        }
    }
    sqlx::query("UPDATE maintenance_jobs SET last_run = now() WHERE name = 'temporal_partitions'")
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    // Plain-mode retention runs AFTER the claim transaction commits, for the
    // same reason as the 4.22 reaps: DROP TABLE needs ACCESS EXCLUSIVE on the
    // parent and the DEFAULT purge grows with stored volume, so one lock
    // contention or one statement timeout would otherwise roll back the
    // partition pre-creation that keeps ingest writable.
    if backend == TemporalBackend::Partitioned {
        if let Some(days) = retention_days {
            match plain_retention(pool, days).await {
                Ok(lines) => done.extend(lines),
                Err(e) => done.push(format!("retention skipped ({e})")),
            }
        }
        if let Ok(Some(line)) = default_partition_load(pool).await {
            done.push(line);
        }
    }
    // 4.22 garbage collection, on both backends: reads already refuse expired
    // entities and instances, so these reaps only bound storage — the clause
    // itself sanctions deletion lagging expiresAt.
    match reap_expired_entities(pool).await {
        Ok(0) => {}
        Ok(n) => done.push(format!("reaped {n} expired transient entities (4.22)")),
        Err(e) => done.push(format!("entity reap skipped ({e})")),
    }
    // 4.22 also names Properties/Relationships: an attribute instance whose
    // expiresAt has passed "should be deleted from an NGSI-LD system". This
    // DELETE additionally contends with concurrent ingest on the same rows, and
    // a deadlock must cost only the reap (observed under ~1.2k msg/s ingest).
    match reap_expired_instances(pool).await {
        Ok(0) => {}
        Ok(n) => done.push(format!("reaped {n} expired attribute instances (4.22)")),
        Err(e) => done.push(format!("instance reap skipped ({e})")),
    }
    if done.is_empty() {
        done.push("nothing to do".into());
    }
    Ok(done.join("; "))
}

/// Plain-mode retention, in its own transaction: drop every weekly partition
/// whose whole range is older than the horizon, then purge the DEFAULT
/// partition (which has no upper bound and is therefore never dropped) of the
/// historic-backfill rows that landed there before their week existed.
///
/// Both statements are idempotent, so a run that loses a lock simply repeats
/// on the next tick. Service role: retention is cross-tenant work.
async fn plain_retention(pool: &PgPool, days: i64) -> Result<Vec<String>, sqlx::Error> {
    let mut done: Vec<String> = Vec::new();
    let mut tx = pool.begin().await?;
    crate::store::pg::set_service(&mut tx).await?;
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
        // bound looks like: FOR VALUES FROM ('<lo>') TO ('<hi>')
        let Some(hi) = bound
            .split("TO ('")
            .nth(1)
            .and_then(|s| s.split('\'').next())
        else {
            continue; // DEFAULT partition — never dropped
        };
        let expired: bool =
            sqlx::query_scalar("SELECT $1::timestamptz < now() - make_interval(days => $2::int)")
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
    let purged = sqlx::query(
        "DELETE FROM attr_instances_default
         WHERE observed_at < now() - make_interval(days => $1::int)",
    )
    .bind(days)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    tx.commit().await?;
    if purged > 0 {
        done.push(format!(
            "purged {purged} expired rows from DEFAULT partition"
        ));
    }
    Ok(done)
}

/// Rows the DEFAULT partition holds, and a warning once it stops being
/// incidental. Every row there is one no weekly partition covers — historic
/// backfill, or an `observedAt` dated beyond the pre-created window — so the
/// count is what makes an unpartitioned pile visible before it shows up as a
/// query that stopped pruning. `reltuples` is the planner's estimate, so this
/// costs a catalog lookup rather than a scan of the pile it is measuring;
/// `-1` means "never analysed", which is reported as nothing.
const DEFAULT_PARTITION_WARN_ROWS: i64 = 100_000;

async fn default_partition_load(pool: &PgPool) -> Result<Option<String>, sqlx::Error> {
    let rows: Option<f32> = sqlx::query_scalar(
        "SELECT reltuples FROM pg_class
         WHERE relname = 'attr_instances_default' AND relnamespace = 'public'::regnamespace",
    )
    .fetch_optional(pool)
    .await?;
    let est = rows.unwrap_or(-1.0) as i64;
    if est < DEFAULT_PARTITION_WARN_ROWS {
        return Ok(None);
    }
    tracing::warn!(
        "attr_instances_default holds ~{est} rows: instances are being written outside the \
         maintained partition window (historic backfill, or an observedAt far in the future); \
         queries over those ranges cannot prune"
    );
    Ok(Some(format!("default partition ~{est} rows")))
}

/// The 4.22 expired-entity DELETE, isolated so a reap that outruns
/// `statement_timeout` costs only itself. Served by the partial index on
/// `expires_at` (migration 0010) — without it this is a sequential scan of
/// every entity in the deployment. Service role: the reap is cross-tenant work
/// (RLS would hide other tenants' rows).
async fn reap_expired_entities(pool: &PgPool) -> Result<u64, sqlx::Error> {
    let mut tx = pool.begin().await?;
    crate::store::pg::set_service(&mut tx).await?;
    let n = sqlx::query("DELETE FROM entities WHERE expires_at IS NOT NULL AND expires_at < now()")
        .execute(&mut *tx)
        .await?
        .rows_affected();
    tx.commit().await?;
    Ok(n)
}

/// One weekly partition, created directly under the parent. Fails while the
/// DEFAULT partition still holds a row in `[lo, hi)`.
fn create_partition_sql(suffix: &str, lo: &str, hi: &str) -> String {
    format!(
        "CREATE TABLE IF NOT EXISTS attr_instances_{suffix} PARTITION OF attr_instances \
         FOR VALUES FROM ('{lo}') TO ('{hi}')"
    )
}

/// Recovery for a range DEFAULT already holds rows for, in execution order:
/// build the week's table STANDALONE (a `PARTITION OF` would fail again), move
/// exactly `[lo, hi)` out of DEFAULT into it, then ATTACH. The move is what
/// makes the ATTACH legal, and its bounds are what keep every other row in
/// DEFAULT. Run as one unit — a partial application would strand rows in an
/// unattached table.
fn adopt_default_rows_sql(suffix: &str, lo: &str, hi: &str) -> [String; 3] {
    [
        format!(
            "CREATE TABLE IF NOT EXISTS attr_instances_{suffix} \
             (LIKE attr_instances INCLUDING DEFAULTS INCLUDING CONSTRAINTS)"
        ),
        format!(
            "WITH moved AS (DELETE FROM attr_instances_default \
               WHERE observed_at >= '{lo}' AND observed_at < '{hi}' RETURNING *) \
             INSERT INTO attr_instances_{suffix} SELECT * FROM moved"
        ),
        format!(
            "ALTER TABLE attr_instances ATTACH PARTITION attr_instances_{suffix} \
             FOR VALUES FROM ('{lo}') TO ('{hi}')"
        ),
    ]
}

/// The 4.22 expired-instance DELETE, isolated so a deadlock with concurrent
/// ingest never poisons the main maintenance transaction. Service role: the
/// reap is cross-tenant work (RLS would hide other tenants' rows).
async fn reap_expired_instances(pool: &PgPool) -> Result<u64, sqlx::Error> {
    let mut tx = pool.begin().await?;
    crate::store::pg::set_service(&mut tx).await?;
    let n = sqlx::query(
        // try_timestamptz (migration 0011), not a bare cast: `expiresAt` is
        // jsonb TEXT and a stamp PostgreSQL cannot parse would abort this
        // DELETE for the whole deployment, every tick, forever.
        "DELETE FROM attr_instances
         WHERE try_timestamptz(data->>'expiresAt') < now()",
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();
    tx.commit().await?;
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LO: &str = "2026-08-17T00:00:00+00";
    const HI: &str = "2026-08-24T00:00:00+00";

    #[test]
    fn create_partition_covers_exactly_the_week() {
        let sql = create_partition_sql("2026w34", LO, HI);
        assert!(sql.contains("attr_instances_2026w34 PARTITION OF attr_instances"));
        assert!(sql.contains(&format!("FROM ('{LO}') TO ('{HI}')")));
    }

    /// The week's table must be built standalone: `CREATE ... PARTITION OF`
    /// is the statement that just failed, so repeating it cannot recover the
    /// range.
    #[test]
    fn adopt_builds_the_table_standalone_then_attaches_it() {
        let [create, _move, attach] = adopt_default_rows_sql("2026w34", LO, HI);
        assert!(create.contains("LIKE attr_instances"), "{create}");
        assert!(!create.contains("PARTITION OF"), "{create}");
        assert!(
            attach
                .starts_with("ALTER TABLE attr_instances ATTACH PARTITION attr_instances_2026w34"),
            "{attach}"
        );
        assert!(
            attach.contains(&format!("FROM ('{LO}') TO ('{HI}')")),
            "{attach}"
        );
    }

    /// The move is bounded by the partition range on BOTH sides. Unbounded, it
    /// would empty the DEFAULT partition of every historic-backfill row in the
    /// deployment and stuff them into one week.
    #[test]
    fn adopt_moves_only_the_partition_range_out_of_default() {
        let [_create, mv, _attach] = adopt_default_rows_sql("2026w34", LO, HI);
        assert!(mv.contains("DELETE FROM attr_instances_default"), "{mv}");
        assert!(mv.contains(&format!("observed_at >= '{LO}'")), "{mv}");
        assert!(mv.contains(&format!("observed_at < '{HI}'")), "{mv}");
        assert!(
            mv.contains("INSERT INTO attr_instances_2026w34"),
            "moved rows must land in the week's table: {mv}"
        );
    }
}
