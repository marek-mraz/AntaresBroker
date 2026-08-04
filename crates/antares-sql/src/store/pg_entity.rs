//! PgStore, first slice (tasks.md C5): entity CRUD over the §8.1 `entities`
//! table. Sync facade — same signatures as the in-memory `Store`, sqlx driven
//! internally via `block_in_place` + `Handle::block_on`, so the 63 existing
//! call sites in `antares-api` never change when the cutover (C13) lands.
//!
//! Extracted columns are computed in Rust at write time (§4 — no triggers):
//! `types`, `scopes`, `created_at`, `modified_at`, `expires_at` from the
//! internal doc form. `location` stays NULL in this slice.
//! (`ponytail:` geo extraction lands with C11, when compiled geoQ actually
//! reads the column — nothing consumes it before then.)

use antares_model::TenantId;
use serde_json::Value;
use sqlx::postgres::PgPool;
use sqlx::Row;

pub struct PgEntityStore {
    pool: PgPool,
}

/// Run an async block from sync code without stalling a tokio worker
/// (same rationale as the redb shadow's `on_blocking`, B1b).
fn wait<T>(fut: impl std::future::Future<Output = T>) -> T {
    match tokio::runtime::Handle::try_current() {
        Ok(h) if h.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(|| h.block_on(fut))
        }
        Ok(h) => h.block_on(fut),
        Err(_) => tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("mini runtime")
            .block_on(fut),
    }
}

/// The internal doc's members that become extracted columns (§8.1).
fn extract(doc: &Value) -> (Vec<String>, Option<Vec<String>>, String, String, Option<String>) {
    let as_vec = |v: &Value| -> Vec<String> {
        match v {
            Value::String(s) => vec![s.clone()],
            Value::Array(a) => a
                .iter()
                .filter_map(|x| x.as_str().map(str::to_owned))
                .collect(),
            _ => vec![],
        }
    };
    let types = doc.get("type").map(&as_vec).unwrap_or_default();
    let scopes = doc.get("scope").map(&as_vec);
    let ts = |k: &str| doc.get(k).and_then(Value::as_str).map(str::to_owned);
    let now = || "1970-01-01T00:00:00Z".to_owned(); // caller always stamps; belt only
    (
        types,
        scopes,
        ts("createdAt").unwrap_or_else(now),
        ts("modifiedAt").unwrap_or_else(now),
        ts("expiresAt"),
    )
}

impl PgEntityStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// 5.6.1-shaped create: `false` when the id already exists (→ 409).
    pub fn create(&self, tenant: &TenantId, id: &str, doc: &Value) -> Result<bool, sqlx::Error> {
        let (types, scopes, created, modified, expires) = extract(doc);
        wait(async {
            let mut tx = self.pool.begin().await?;
            crate::pg::set_tenant(&mut tx, tenant).await?;
            let done = sqlx::query(
                "INSERT INTO entities
                   (tenant_id, id, entity, types, scopes, created_at, modified_at, expires_at)
                 VALUES ($1, $2, $3, $4, $5, $6::timestamptz, $7::timestamptz, $8::timestamptz)
                 ON CONFLICT (tenant_id, id) DO NOTHING",
            )
            .bind(tenant.as_str())
            .bind(id)
            .bind(doc)
            .bind(&types)
            .bind(&scopes)
            .bind(&created)
            .bind(&modified)
            .bind(&expires)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            tx.commit().await?;
            Ok(done == 1)
        })
    }

    pub fn get(&self, tenant: &TenantId, id: &str) -> Result<Option<Value>, sqlx::Error> {
        wait(async {
            let mut tx = self.pool.begin().await?;
            crate::pg::set_tenant(&mut tx, tenant).await?;
            let row = sqlx::query("SELECT entity FROM entities WHERE tenant_id = $1 AND id = $2")
                .bind(tenant.as_str())
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?;
            tx.commit().await?;
            Ok(row.map(|r| r.get::<Value, _>(0)))
        })
    }

    pub fn delete(&self, tenant: &TenantId, id: &str) -> Result<bool, sqlx::Error> {
        wait(async {
            let mut tx = self.pool.begin().await?;
            crate::pg::set_tenant(&mut tx, tenant).await?;
            let n = sqlx::query("DELETE FROM entities WHERE tenant_id = $1 AND id = $2")
                .bind(tenant.as_str())
                .bind(id)
                .execute(&mut *tx)
                .await?
                .rows_affected();
            tx.commit().await?;
            Ok(n == 1)
        })
    }

    /// Id-ordered snapshot for one tenant (v0 `list` shape; compiled-SQL
    /// querying replaces call sites one by one in C10/C11).
    pub fn list(&self, tenant: &TenantId) -> Result<Vec<Value>, sqlx::Error> {
        wait(async {
            let mut tx = self.pool.begin().await?;
            crate::pg::set_tenant(&mut tx, tenant).await?;
            let rows = sqlx::query(
                "SELECT entity FROM entities WHERE tenant_id = $1 ORDER BY id",
            )
            .bind(tenant.as_str())
            .fetch_all(&mut *tx)
            .await?;
            tx.commit().await?;
            Ok(rows.into_iter().map(|r| r.get::<Value, _>(0)).collect())
        })
    }

    /// §3.1.2 read-modify-write: row lock via `SELECT … FOR UPDATE`, closure
    /// applied in Rust, `version` bumped under the lock. Two racing PATCHes
    /// serialize in Postgres, neither is lost. `Ok(None)` = entity absent.
    pub fn mutate<T, E>(
        &self,
        tenant: &TenantId,
        id: &str,
        f: impl FnOnce(&mut Value) -> Result<T, E>,
    ) -> Result<Option<Result<T, E>>, sqlx::Error> {
        wait(async {
            let mut tx = self.pool.begin().await?;
            crate::pg::set_tenant(&mut tx, tenant).await?;
            let row = sqlx::query(
                "SELECT entity FROM entities WHERE tenant_id = $1 AND id = $2 FOR UPDATE",
            )
            .bind(tenant.as_str())
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?;
            let Some(row) = row else {
                tx.commit().await?;
                return Ok(None);
            };
            let mut doc: Value = row.get(0);
            match f(&mut doc) {
                Ok(t) => {
                    let (types, scopes, _created, modified, expires) = extract(&doc);
                    sqlx::query(
                        "UPDATE entities SET entity = $3, types = $4, scopes = $5,
                           modified_at = $6::timestamptz, expires_at = $7::timestamptz,
                           version = version + 1
                         WHERE tenant_id = $1 AND id = $2",
                    )
                    .bind(tenant.as_str())
                    .bind(id)
                    .bind(&doc)
                    .bind(&types)
                    .bind(&scopes)
                    .bind(&modified)
                    .bind(&expires)
                    .execute(&mut *tx)
                    .await?;
                    tx.commit().await?;
                    Ok(Some(Ok(t)))
                }
                Err(e) => {
                    tx.rollback().await?;
                    Ok(Some(Err(e)))
                }
            }
        })
    }

    /// Current row version (test hook for the §3.1 monotonicity assertions).
    pub fn version(&self, tenant: &TenantId, id: &str) -> Result<Option<i64>, sqlx::Error> {
        wait(async {
            let mut tx = self.pool.begin().await?;
            crate::pg::set_tenant(&mut tx, tenant).await?;
            let row = sqlx::query("SELECT version FROM entities WHERE tenant_id = $1 AND id = $2")
                .bind(tenant.as_str())
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?;
            tx.commit().await?;
            Ok(row.map(|r| r.get::<i64, _>(0)))
        })
    }
}
