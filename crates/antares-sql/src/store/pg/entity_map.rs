//! EntityMap rows (`entity_maps`, 5.5.9.3 distributed
//! pagination). One row per (map, position): the id→source materialization
//! that keeps broad federation pageable without re-fanning per page.
//!
//! The EntityMaps API resource (5.14) is a known gap; this store is
//! its storage. Per-row registration ids (never the
//! first row's) are enforced by shape: registration_id is a per-row column
//! bound per element.

use antares_model::TenantId;
use serde_json::Value;
use sqlx::postgres::PgPool;
use sqlx::Row;

use super::entity::wait;

pub struct EntityMapStore {
    pool: PgPool,
}

/// Every statement this module issues. Values are bound as `$n` — the strings
/// are compile-time constants, so no request data ever reaches the parser, and
/// every one of them carries `tenant_id = $1` next to the RLS policy.
const CLEAR_SQL: &str = "DELETE FROM entity_maps WHERE tenant_id = $1 AND map_id = $2";
const INSERT_SQL: &str =
    "INSERT INTO entity_maps (tenant_id, map_id, pos, query_checksum, entity_id,
                              remote_query, registration_id, last_access, expires_at)
     SELECT $1, $2, (e->>'pos')::bigint, $3, e->>'entity_id',
            e->>'remote_query', e->>'registration_id', now(), $4::timestamptz
     FROM jsonb_array_elements($5::jsonb) AS e";
/// 5.5.14: "If an EntityMap has expired, or cannot be accessed, no inference
/// can be made as to which entities are held within the Context Sources and a
/// new one shall be created." The TTL sweep is a timer, so the expiry belongs
/// in the read itself — never serve a page from a map past `expires_at`.
const PAGE_SQL: &str = "SELECT entity_id, registration_id FROM entity_maps
     WHERE tenant_id = $1 AND map_id = $2 AND expires_at > now()
     ORDER BY pos OFFSET $3 LIMIT $4";
const SWEEP_SQL: &str = "DELETE FROM entity_maps WHERE tenant_id = $1 AND expires_at < now()";

/// One JSONB element per map entry: the position IS the array index, and the
/// registration id is a per-entry member — a map whose rows all carried the
/// first entry's registration would send every later page to the wrong
/// Context Source (5.5.9.3).
fn put_payload(entries: &[(String, String, Option<String>)]) -> Vec<Value> {
    entries
        .iter()
        .enumerate()
        .map(|(pos, (eid, rid, rq))| {
            serde_json::json!({"pos": pos as i64, "entity_id": eid,
                               "registration_id": rid, "remote_query": rq})
        })
        .collect()
}

impl EntityMapStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Materialize a map: rows carry (pos, entity_id, registration_id) —
    /// per-element registration ids, one statement.
    pub fn put(
        &self,
        tenant: &TenantId,
        map_id: &str,
        query_checksum: &str,
        expires_at: &str,
        entries: &[(String, String, Option<String>)], // (entity_id, registration_id, remote_query)
    ) -> Result<(), sqlx::Error> {
        let payload = put_payload(entries);
        wait(async {
            let mut tx = self.pool.begin().await?;
            crate::store::pg::set_tenant(&mut tx, tenant).await?;
            sqlx::query(CLEAR_SQL)
                .bind(tenant.as_str())
                .bind(map_id)
                .execute(&mut *tx)
                .await?;
            sqlx::query(INSERT_SQL)
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

    /// Page of a map in position order, refusing a map past its TTL (5.5.14).
    pub fn page(
        &self,
        tenant: &TenantId,
        map_id: &str,
        offset: i64,
        limit: i64,
    ) -> Result<Vec<(String, String)>, sqlx::Error> {
        wait(async {
            let mut tx = self.pool.begin().await?;
            crate::store::pg::set_tenant(&mut tx, tenant).await?;
            let rows = sqlx::query(PAGE_SQL)
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

    /// TTL sweep (Scorpio default 90 s TTL / 30 s sweep) — run by a
    /// broker job; returns swept row count. Per-tenant loop ON PURPOSE: the
    /// RLS policy makes a tenant-less DELETE silently match zero rows for a
    /// non-superuser broker role — a sweep that "works" only as superuser is
    /// exactly the kind of silent no-op to guard against.
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
                crate::store::pg::set_tenant(&mut tx, &tid).await?;
                swept += sqlx::query(SWEEP_SQL)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 5.5.9.3: the map's order IS the pagination order, so positions are the
    /// entry indices, densely, from zero.
    #[test]
    fn put_payload_numbers_entries_by_position_from_zero() {
        let entries: Vec<(String, String, Option<String>)> = (0..3)
            .map(|i| (format!("urn:e:{i}"), format!("urn:reg:{i}"), None))
            .collect();
        let payload = put_payload(&entries);
        assert_eq!(payload.len(), 3);
        for (i, row) in payload.iter().enumerate() {
            assert_eq!(row["pos"], i as i64);
            assert_eq!(row["entity_id"], format!("urn:e:{i}"));
        }
    }

    /// Each row keeps ITS OWN registration id: a map that stamped every row
    /// with the first entry's registration would forward later pages to a
    /// Context Source that never held those entities.
    #[test]
    fn put_payload_keeps_a_registration_id_per_entry() {
        let payload = put_payload(&[
            ("urn:e:1".into(), "urn:reg:A".into(), None),
            ("urn:e:2".into(), "urn:reg:B".into(), Some("type=T".into())),
        ]);
        assert_eq!(payload[0]["registration_id"], "urn:reg:A");
        assert_eq!(payload[1]["registration_id"], "urn:reg:B");
        assert_ne!(payload[1]["registration_id"], payload[0]["registration_id"]);
        // an absent remote query stays absent (SQL NULL), never the string "null"
        assert!(payload[0]["remote_query"].is_null());
        assert_eq!(payload[1]["remote_query"], "type=T");
    }

    /// An empty map still materializes: `put` clears the old rows and inserts
    /// nothing, so the previous map's entries cannot be paged afterwards.
    #[test]
    fn put_payload_of_no_entries_is_empty() {
        assert!(put_payload(&[]).is_empty());
    }

    /// Every statement is tenant-scoped in its own text, next to the RLS
    /// policy — a read or a sweep must never reach another tenant's rows.
    #[test]
    fn every_statement_carries_the_tenant_predicate() {
        for sql in [CLEAR_SQL, PAGE_SQL, SWEEP_SQL] {
            assert!(sql.contains("tenant_id = $1"), "untenanted: {sql}");
        }
        assert!(
            INSERT_SQL.contains("SELECT $1, $2,"),
            "tenant is bound, not literal"
        );
    }

    /// 5.5.14: an expired EntityMap cannot be accessed. The TTL sweep lags by
    /// design, so the read itself must refuse rows past `expires_at` instead
    /// of serving a stale page until the sweep catches up.
    #[test]
    fn a_page_never_serves_rows_past_the_ttl() {
        assert!(
            PAGE_SQL.contains("expires_at > now()"),
            "expired rows pageable: {PAGE_SQL}"
        );
    }

    /// Reading a page must not write: `last_access` was maintained by an
    /// UPDATE of every row of the map before each page, which nothing ever
    /// read and which serialized concurrent paging behind a row-level write
    /// lock over the whole map.
    #[test]
    fn no_statement_writes_on_the_read_path() {
        for sql in [CLEAR_SQL, INSERT_SQL, PAGE_SQL, SWEEP_SQL] {
            assert!(!sql.contains("UPDATE"), "a read takes a write lock: {sql}");
            assert!(!sql.contains("last_access = "), "dead column write: {sql}");
        }
    }

    /// Paging is bound, not interpolated: OFFSET/LIMIT arrive as parameters.
    #[test]
    fn page_binds_offset_and_limit() {
        assert!(PAGE_SQL.contains("OFFSET $3 LIMIT $4"));
        assert!(
            PAGE_SQL.contains("ORDER BY pos"),
            "map order is position order"
        );
    }
}
