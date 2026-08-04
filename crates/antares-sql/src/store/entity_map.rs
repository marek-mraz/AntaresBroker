//! EntityMap rows (tasks.md C8; §8.3 `entity_maps`, 5.5.9.3 distributed
//! pagination). One row per (map, position): the id→source materialization
//! that keeps broad federation pageable without re-fanning per page (§16.7).
//!
//! The EntityMaps API resource (5.14) is an H1-recorded gap; this store is
//! its §8.3 storage. The B1 regression (per-row registration_id, never the
//! first row's) is enforced by shape: registration_id is a per-row column
//! bound per element.

use antares_model::TenantId;
use serde_json::Value;
use sqlx::postgres::PgPool;
use sqlx::Row;

use super::pg_entity::wait;

pub struct EntityMapStore {
    pool: PgPool,
}

impl EntityMapStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Materialize a map: rows carry (pos, entity_id, registration_id) —
    /// per-element registration ids (the B1 fix class), one statement.
    pub fn put(
        &self,
        tenant: &TenantId,
        map_id: &str,
        query_checksum: &str,
        expires_at: &str,
        entries: &[(String, String, Option<String>)], // (entity_id, registration_id, remote_query)
    ) -> Result<(), sqlx::Error> {
        let payload: Vec<Value> = entries
            .iter()
            .enumerate()
            .map(|(pos, (eid, rid, rq))| {
                serde_json::json!({"pos": pos as i64, "entity_id": eid,
                                    "registration_id": rid, "remote_query": rq})
            })
            .collect();
        wait(async {
            let mut tx = self.pool.begin().await?;
            crate::pg::set_tenant(&mut tx, tenant).await?;
            sqlx::query("DELETE FROM entity_maps WHERE tenant_id = $1 AND map_id = $2")
                .bind(tenant.as_str())
                .bind(map_id)
                .execute(&mut *tx)
                .await?;
            sqlx::query(
                "INSERT INTO entity_maps (tenant_id, map_id, pos, query_checksum, entity_id,
                                          remote_query, registration_id, last_access, expires_at)
                 SELECT $1, $2, (e->>'pos')::bigint, $3, e->>'entity_id',
                        e->>'remote_query', e->>'registration_id', now(), $4::timestamptz
                 FROM jsonb_array_elements($5::jsonb) AS e",
            )
            .bind(tenant.as_str())
            .bind(map_id)
            .bind(query_checksum)
            .bind(expires_at)
            .bind(Value::Array(payload))
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            Ok(())
        })
    }

    /// Page of a map in position order; bumps `last_access`.
    pub fn page(
        &self,
        tenant: &TenantId,
        map_id: &str,
        offset: i64,
        limit: i64,
    ) -> Result<Vec<(String, String)>, sqlx::Error> {
        wait(async {
            let mut tx = self.pool.begin().await?;
            crate::pg::set_tenant(&mut tx, tenant).await?;
            sqlx::query(
                "UPDATE entity_maps SET last_access = now() WHERE tenant_id = $1 AND map_id = $2",
            )
            .bind(tenant.as_str())
            .bind(map_id)
            .execute(&mut *tx)
            .await?;
            let rows = sqlx::query(
                "SELECT entity_id, registration_id FROM entity_maps
                 WHERE tenant_id = $1 AND map_id = $2
                 ORDER BY pos OFFSET $3 LIMIT $4",
            )
            .bind(tenant.as_str())
            .bind(map_id)
            .bind(offset)
            .bind(limit)
            .fetch_all(&mut *tx)
            .await?;
            tx.commit().await?;
            Ok(rows.into_iter().map(|r| (r.get(0), r.get(1))).collect())
        })
    }

    /// TTL sweep (§8.3: Scorpio default 90 s TTL / 30 s sweep) — run by a
    /// broker job; returns swept row count. Per-tenant loop ON PURPOSE: the
    /// RLS policy makes a tenant-less DELETE silently match zero rows for a
    /// non-superuser broker role — a sweep that "works" only as superuser is
    /// the kind of silent no-op §16.1.3 exists to catch.
    pub fn sweep(&self) -> Result<u64, sqlx::Error> {
        wait(async {
            let tenants: Vec<String> = sqlx::query_scalar("SELECT tenant_id FROM tenants")
                .fetch_all(&self.pool)
                .await?;
            let mut swept = 0;
            for t in tenants {
                let Ok(tid) = antares_model::TenantId::new(&t) else {
                    continue;
                };
                let mut tx = self.pool.begin().await?;
                crate::pg::set_tenant(&mut tx, &tid).await?;
                swept += sqlx::query(
                    "DELETE FROM entity_maps WHERE tenant_id = $1 AND expires_at < now()",
                )
                .bind(&t)
                .execute(&mut *tx)
                .await?
                .rows_affected();
                tx.commit().await?;
            }
            Ok(swept)
        })
    }
}
