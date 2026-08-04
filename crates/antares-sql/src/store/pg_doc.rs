//! PgStore slice two (tasks.md C6/C7/C8 partial): the doc-table kinds —
//! `subscriptions`, `csource_registrations`, `csource_subscriptions` — plus
//! cross-tenant `jsonld_contexts`. Same sync-facade shape as `pg_entity`.
//!
//! The v0 interchange form stores ONE doc per resource; §8.3's bookkeeping
//! columns (`expires_at`, `is_active`, `times_sent`, `last_*`) are EXTRACTED
//! from the doc on every write, so the row stays the truth (§14.1) while the
//! API layer keeps its doc-shaped view until the C13 cutover completes.

use antares_model::TenantId;
use serde_json::Value;
use sqlx::postgres::PgPool;
use sqlx::Row;

use super::pg_entity::wait;

/// Which doc table a resource kind lives in.
#[derive(Clone, Copy, Debug)]
pub enum DocKind {
    Subscription,
    Registration,
    CSourceSubscription,
}

impl DocKind {
    fn table(self) -> &'static str {
        match self {
            DocKind::Subscription => "subscriptions",
            DocKind::Registration => "csource_registrations",
            DocKind::CSourceSubscription => "csource_subscriptions",
        }
    }
    fn doc_column(self) -> &'static str {
        match self {
            DocKind::Subscription | DocKind::CSourceSubscription => "subscription",
            DocKind::Registration => "registration",
        }
    }
    fn has_bookkeeping(self) -> bool {
        !matches!(self, DocKind::Registration)
    }
}

/// §8.3 bookkeeping, derived from the doc (5.2.14.2 output members).
fn bookkeeping(doc: &Value) -> (Option<String>, bool, i64, Option<String>, Option<String>, Option<String>) {
    let s = |v: Option<&Value>| v.and_then(Value::as_str).map(str::to_owned);
    let n = doc.get("notification");
    (
        s(doc.get("expiresAt")),
        doc.get("isActive") != Some(&Value::Bool(false)),
        n.and_then(|n| n.get("timesSent")).and_then(Value::as_i64).unwrap_or(0),
        s(n.and_then(|n| n.get("lastNotification"))),
        s(n.and_then(|n| n.get("lastSuccess"))),
        s(n.and_then(|n| n.get("lastFailure"))),
    )
}

pub struct PgDocStore {
    pool: PgPool,
}

impl PgDocStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Upsert one doc, refreshing the extracted columns. `Ok(true)` = it
    /// existed before (create paths check existence via `create`).
    pub fn upsert(
        &self,
        tenant: &TenantId,
        kind: DocKind,
        id: &str,
        doc: &Value,
    ) -> Result<bool, sqlx::Error> {
        let context = doc.get("@context").cloned().unwrap_or(Value::Object(Default::default()));
        let (expires, active, sent, last_n, last_s, last_f) = bookkeeping(doc);
        let table = kind.table();
        let col = kind.doc_column();
        let sql = if kind.has_bookkeeping() {
            format!(
                "INSERT INTO {table} (tenant_id, id, {col}, context, expires_at, is_active,
                   times_sent, last_notification, last_success, last_failure)
                 VALUES ($1, $2, $3, $4, $5::timestamptz, $6, $7,
                   $8::timestamptz, $9::timestamptz, $10::timestamptz)
                 ON CONFLICT (tenant_id, id) DO UPDATE SET {col} = EXCLUDED.{col},
                   context = EXCLUDED.context, expires_at = EXCLUDED.expires_at,
                   is_active = EXCLUDED.is_active, times_sent = EXCLUDED.times_sent,
                   last_notification = EXCLUDED.last_notification,
                   last_success = EXCLUDED.last_success, last_failure = EXCLUDED.last_failure
                 RETURNING (xmax <> 0) AS existed"
            )
        } else {
            format!(
                "INSERT INTO {table} (tenant_id, id, {col})
                 VALUES ($1, $2, $3)
                 ON CONFLICT (tenant_id, id) DO UPDATE SET {col} = EXCLUDED.{col}
                 RETURNING (xmax <> 0) AS existed"
            )
        };
        wait(async {
            let mut tx = self.pool.begin().await?;
            crate::pg::set_tenant(&mut tx, tenant).await?;
            let mut q = sqlx::query(sqlx::AssertSqlSafe(sql.clone())).bind(tenant.as_str()).bind(id).bind(doc);
            if kind.has_bookkeeping() {
                q = q
                    .bind(&context)
                    .bind(&expires)
                    .bind(active)
                    .bind(sent)
                    .bind(&last_n)
                    .bind(&last_s)
                    .bind(&last_f);
            }
            let existed: bool = q.fetch_one(&mut *tx).await?.get("existed");
            tx.commit().await?;
            Ok(existed)
        })
    }

    pub fn get(&self, tenant: &TenantId, kind: DocKind, id: &str) -> Result<Option<Value>, sqlx::Error> {
        let sql = format!(
            "SELECT {} FROM {} WHERE tenant_id = $1 AND id = $2",
            kind.doc_column(),
            kind.table()
        );
        wait(async {
            let mut tx = self.pool.begin().await?;
            crate::pg::set_tenant(&mut tx, tenant).await?;
            let row = sqlx::query(sqlx::AssertSqlSafe(sql.clone()))
                .bind(tenant.as_str())
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?;
            tx.commit().await?;
            Ok(row.map(|r| r.get::<Value, _>(0)))
        })
    }

    pub fn delete(&self, tenant: &TenantId, kind: DocKind, id: &str) -> Result<bool, sqlx::Error> {
        let sql = format!(
            "DELETE FROM {} WHERE tenant_id = $1 AND id = $2",
            kind.table()
        );
        wait(async {
            let mut tx = self.pool.begin().await?;
            crate::pg::set_tenant(&mut tx, tenant).await?;
            let done = sqlx::query(sqlx::AssertSqlSafe(sql.clone()))
                .bind(tenant.as_str())
                .bind(id)
                .execute(&mut *tx)
                .await?
                .rows_affected();
            tx.commit().await?;
            Ok(done == 1)
        })
    }

    pub fn list(&self, tenant: &TenantId, kind: DocKind) -> Result<Vec<Value>, sqlx::Error> {
        let sql = format!(
            "SELECT {} FROM {} WHERE tenant_id = $1 ORDER BY id",
            kind.doc_column(),
            kind.table()
        );
        wait(async {
            let mut tx = self.pool.begin().await?;
            crate::pg::set_tenant(&mut tx, tenant).await?;
            let rows = sqlx::query(sqlx::AssertSqlSafe(sql.clone()))
                .bind(tenant.as_str())
                .fetch_all(&mut *tx)
                .await?;
            tx.commit().await?;
            Ok(rows.into_iter().map(|r| r.get::<Value, _>(0)).collect())
        })
    }

    /// Bookkeeping columns straight from the row (test hook: rows are truth).
    pub fn status_row(
        &self,
        tenant: &TenantId,
        kind: DocKind,
        id: &str,
    ) -> Result<Option<(bool, i64)>, sqlx::Error> {
        let sql = format!(
            "SELECT is_active, times_sent FROM {} WHERE tenant_id = $1 AND id = $2",
            kind.table()
        );
        wait(async {
            let mut tx = self.pool.begin().await?;
            crate::pg::set_tenant(&mut tx, tenant).await?;
            let row = sqlx::query(sqlx::AssertSqlSafe(sql.clone()))
                .bind(tenant.as_str())
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?;
            tx.commit().await?;
            Ok(row.map(|r| (r.get(0), r.get(1))))
        })
    }

    // jsonldContexts — the ONE cross-tenant table (§8.3), no RLS by design.
    pub fn context_put(&self, id: &str, doc: &Value, kind: &str) -> Result<(), sqlx::Error> {
        wait(async {
            sqlx::query(
                "INSERT INTO jsonld_contexts (id, body, kind) VALUES ($1, $2, $3)
                 ON CONFLICT (id) DO UPDATE SET body = EXCLUDED.body, kind = EXCLUDED.kind",
            )
            .bind(id)
            .bind(doc)
            .bind(kind)
            .execute(&self.pool)
            .await
            .map(|_| ())
        })
    }

    pub fn context_get(&self, id: &str) -> Result<Option<Value>, sqlx::Error> {
        wait(async {
            let row = sqlx::query("SELECT body FROM jsonld_contexts WHERE id = $1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;
            Ok(row.map(|r| r.get::<Value, _>(0)))
        })
    }

    pub fn context_delete(&self, id: &str) -> Result<bool, sqlx::Error> {
        wait(async {
            Ok(sqlx::query("DELETE FROM jsonld_contexts WHERE id = $1")
                .bind(id)
                .execute(&self.pool)
                .await?
                .rows_affected()
                == 1)
        })
    }

    pub fn context_list(&self) -> Result<Vec<Value>, sqlx::Error> {
        wait(async {
            let rows = sqlx::query("SELECT body FROM jsonld_contexts ORDER BY id")
                .fetch_all(&self.pool)
                .await?;
            Ok(rows.into_iter().map(|r| r.get::<Value, _>(0)).collect())
        })
    }
}
