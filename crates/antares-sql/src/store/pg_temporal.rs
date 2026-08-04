//! PgStore slice three (tasks.md §C-ii): temporal docs over the
//! `temporal_entities` bridge (migration 0002). Same sync facade; same
//! extraction discipline as `pg_entity` (types/scopes/timestamps computed in
//! Rust at write time). The doc-shaped bridge keeps the suite green through
//! the C13 cutover; C9/D replace it with real attr_instances rows.

use antares_model::TenantId;
use serde_json::Value;
use sqlx::postgres::PgPool;
use sqlx::Row;

use super::pg_entity::wait;

pub struct PgTemporalStore {
    pool: PgPool,
}

fn extract(doc: &Value) -> (Vec<String>, Option<Vec<String>>, String, String) {
    let as_vec = |v: &Value| -> Vec<String> {
        match v {
            Value::String(s) => vec![s.clone()],
            Value::Array(a) => a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect(),
            _ => vec![],
        }
    };
    let ts = |k: &str| doc.get(k).and_then(Value::as_str).map(str::to_owned);
    let now = || "1970-01-01T00:00:00Z".to_owned();
    (
        doc.get("type").map(&as_vec).unwrap_or_default(),
        doc.get("scope").map(&as_vec),
        ts("createdAt").unwrap_or_else(now),
        ts("modifiedAt").unwrap_or_else(now),
    )
}

impl PgTemporalStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// `false` when the id already exists (create semantics, like the memory
    /// store's `create`).
    pub fn create(&self, tenant: &TenantId, id: &str, doc: &Value) -> Result<bool, sqlx::Error> {
        let (types, scopes, created, modified) = extract(doc);
        wait(async {
            let mut tx = self.pool.begin().await?;
            crate::pg::set_tenant(&mut tx, tenant).await?;
            let n = sqlx::query(
                "INSERT INTO temporal_entities
                   (tenant_id, id, types, scopes, doc, created_at, modified_at)
                 VALUES ($1, $2, $3, $4, $5, $6::timestamptz, $7::timestamptz)
                 ON CONFLICT (tenant_id, id) DO NOTHING",
            )
            .bind(tenant.as_str())
            .bind(id)
            .bind(&types)
            .bind(&scopes)
            .bind(doc)
            .bind(&created)
            .bind(&modified)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            tx.commit().await?;
            Ok(n == 1)
        })
    }

    pub fn get(&self, tenant: &TenantId, id: &str) -> Result<Option<Value>, sqlx::Error> {
        wait(async {
            let mut tx = self.pool.begin().await?;
            crate::pg::set_tenant(&mut tx, tenant).await?;
            let row = sqlx::query("SELECT doc FROM temporal_entities WHERE tenant_id = $1 AND id = $2")
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
            let n = sqlx::query("DELETE FROM temporal_entities WHERE tenant_id = $1 AND id = $2")
                .bind(tenant.as_str())
                .bind(id)
                .execute(&mut *tx)
                .await?
                .rows_affected();
            tx.commit().await?;
            Ok(n == 1)
        })
    }

    pub fn list(&self, tenant: &TenantId) -> Result<Vec<Value>, sqlx::Error> {
        wait(async {
            let mut tx = self.pool.begin().await?;
            crate::pg::set_tenant(&mut tx, tenant).await?;
            let rows = sqlx::query("SELECT doc FROM temporal_entities WHERE tenant_id = $1 ORDER BY id")
                .bind(tenant.as_str())
                .fetch_all(&mut *tx)
                .await?;
            tx.commit().await?;
            Ok(rows.into_iter().map(|r| r.get::<Value, _>(0)).collect())
        })
    }

    /// Row-locked read-modify-write, same shape as PgEntityStore::mutate
    /// (no version column on temporal docs — history is the versioning).
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
                "SELECT doc FROM temporal_entities WHERE tenant_id = $1 AND id = $2 FOR UPDATE",
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
                    let (types, scopes, _created, modified) = extract(&doc);
                    sqlx::query(
                        "UPDATE temporal_entities SET doc = $3, types = $4, scopes = $5,
                           modified_at = $6::timestamptz
                         WHERE tenant_id = $1 AND id = $2",
                    )
                    .bind(tenant.as_str())
                    .bind(id)
                    .bind(&doc)
                    .bind(&types)
                    .bind(&scopes)
                    .bind(&modified)
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
}
