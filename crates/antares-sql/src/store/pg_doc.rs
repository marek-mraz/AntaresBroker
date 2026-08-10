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
fn bookkeeping(
    doc: &Value,
) -> (
    Option<String>,
    bool,
    i64,
    Option<String>,
    Option<String>,
    Option<String>,
) {
    let s = |v: Option<&Value>| v.and_then(Value::as_str).map(str::to_owned);
    let n = doc.get("notification");
    (
        s(doc.get("expiresAt")),
        doc.get("isActive") != Some(&Value::Bool(false)),
        n.and_then(|n| n.get("timesSent"))
            .and_then(Value::as_i64)
            .unwrap_or(0),
        s(n.and_then(|n| n.get("lastNotification"))),
        s(n.and_then(|n| n.get("lastSuccess"))),
        s(n.and_then(|n| n.get("lastFailure"))),
    )
}

// ---- C7: csource_index maintenance (§8.3) ----------------------------------
// The flattened federation match table, rebuilt in Rust inside the same
// transaction as every registration write (§4: no triggers). Deleting a
// registration cleans its rows via the FK ON DELETE CASCADE.

/// Table 4.20-1 operation names; bit position in `csource_index.ops` = index.
/// Order is append-only: a bitmask is stored data, so renumbering is a
/// migration.
pub const OPERATIONS: &[&str] = &[
    "createEntity",
    "updateEntity",
    "appendAttrs",
    "updateAttrs",
    "deleteAttrs",
    "deleteEntity",
    "createBatch",
    "upsertBatch",
    "updateBatch",
    "deleteBatch",
    "upsertTemporal",
    "appendAttrsTemporal",
    "deleteAttrsTemporal",
    "updateAttrInstanceTemporal",
    "deleteAttrInstanceTemporal",
    "deleteTemporal",
    "mergeEntity",
    "replaceEntity",
    "replaceAttrs",
    "mergeBatch",
    "purgeEntity",
    "retrieveEntity",
    "queryEntity",
    "queryBatch",
    "retrieveTemporal",
    "queryTemporal",
    "retrieveEntityTypes",
    "retrieveEntityTypeDetails",
    "retrieveEntityTypeInfo",
    "retrieveAttrTypes",
    "retrieveAttrTypeDetails",
    "retrieveAttrTypeInfo",
    "createSubscription",
    "updateSubscription",
    "retrieveSubscription",
    "querySubscription",
    "deleteSubscription",
    "retrieveEntityMap",
    "updateEntityMap",
    "deleteEntityMap",
    "createEntityMapQueryEntity",
    "createEntityMapQueryTemporal",
    "retrieveContextSourceIdentity",
];

/// Table 4.20-2 named groups, expanded to their members before masking.
fn group_members(name: &str) -> Option<&'static [&'static str]> {
    const FEDERATION: &[&str] = &[
        "retrieveEntity",
        "queryEntity",
        "queryBatch",
        "retrieveEntityTypes",
        "retrieveEntityTypeDetails",
        "retrieveEntityTypeInfo",
        "retrieveAttrTypes",
        "retrieveAttrTypeDetails",
        "retrieveAttrTypeInfo",
        "createSubscription",
        "updateSubscription",
        "retrieveSubscription",
        "querySubscription",
        "deleteSubscription",
        "retrieveEntityMap",
        "updateEntityMap",
        "deleteEntityMap",
        "createEntityMapQueryEntity",
        "retrieveContextSourceIdentity",
    ];
    const ASSOCIATION: &[&str] = &[
        "retrieveEntity",
        "queryEntity",
        "queryBatch",
        "retrieveEntityTypes",
        "retrieveEntityTypeDetails",
        "retrieveEntityTypeInfo",
        "retrieveAttrTypes",
        "retrieveAttrTypeDetails",
        "retrieveAttrTypeInfo",
        "createSubscription",
        "updateSubscription",
        "retrieveSubscription",
        "querySubscription",
        "deleteSubscription",
        "retrieveContextSourceIdentity",
    ];
    const UPDATE: &[&str] = &[
        "updateEntity",
        "updateAttrs",
        "replaceEntity",
        "replaceAttrs",
    ];
    const RETRIEVE: &[&str] = &["retrieveEntity", "queryEntity"];
    const REDIRECTION: &[&str] = &[
        "createEntity",
        "updateEntity",
        "appendAttrs",
        "updateAttrs",
        "deleteAttrs",
        "deleteEntity",
        "mergeEntity",
        "replaceEntity",
        "replaceAttrs",
        "retrieveEntity",
        "queryEntity",
        "purgeEntity",
    ];
    match name {
        "federationOps" => Some(FEDERATION),
        "associationOps" => Some(ASSOCIATION),
        "updateOps" => Some(UPDATE),
        "retrieveOps" => Some(RETRIEVE),
        "redirectionOps" => Some(REDIRECTION),
        _ => None,
    }
}

/// Registration `operations` → bitmask; absent defaults to federationOps
/// (5.2.9).
pub fn ops_mask(reg: &Value) -> i64 {
    let names: Vec<&str> = reg
        .get("operations")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_else(|| vec!["federationOps"]);
    let mut mask = 0i64;
    let mut set = |op: &str| {
        if let Some(bit) = OPERATIONS.iter().position(|o| *o == op) {
            mask |= 1 << bit;
        }
    };
    for n in names {
        match group_members(n) {
            Some(members) => members.iter().for_each(|m| set(m)),
            None => set(n),
        }
    }
    mask
}

fn mode_code(reg: &Value) -> i16 {
    match reg.get("mode").and_then(Value::as_str) {
        Some("auxiliary") => 0,
        Some("redirect") => 2,
        Some("exclusive") => 3,
        _ => 1, // inclusive is the default (5.2.9)
    }
}

/// Hard ceiling on rows one registration may explode into. The API caps
/// cardinality at the validation boundary (§16.3), but a document written
/// through any other path — a restored dump, a future importer — must not be
/// able to drive this quadratically. Truncating loses federation matches for
/// an absurd registration; OOM loses the process.
pub const MAX_INDEX_ROWS: usize = 10_000;

/// Explode one registration document into csource_index rows: each
/// RegistrationInfo element yields entities × (propertyNames ∪
/// relationshipNames) rows, with NULL placeholders when a dimension is
/// absent — the Scorpio csourceinformation shape (§14.8) minus the 46
/// boolean columns. Attribute/type names are stored as they appear in the
/// document; canonical-IRI storage lands with the SQL matching path (§16.7).
/// NGSI-LD 2.0 readiness (#31, §8.3): when propertyNames/relationshipNames
/// merge into attributeNames, the migration is a coalesce of the two name
/// columns into one attribute_name column — no reshape.
pub fn index_rows(reg: &Value) -> Vec<Value> {
    let endpoint = reg
        .get("endpoint")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let common = |entity: Option<&Value>, prop: Option<&str>, rel: Option<&str>| {
        serde_json::json!({
            "entity_id": entity.and_then(|e| e.get("id")).and_then(Value::as_str),
            "id_pattern": entity.and_then(|e| e.get("idPattern")).and_then(Value::as_str),
            "entity_type": entity.and_then(|e| e.get("type")).and_then(Value::as_str),
            "property_name": prop,
            "relationship_name": rel,
            // C11b: a registration carries its geo scope as a RAW GeoJSON
            // geometry under `location` (not instance-wrapped like an entity
            // attribute) — see antares_api::csource::csr_matches_subscription,
            // which hands exactly this value to `matches_geometry`.
            "location": reg.get("location").filter(|g| g.get("type").is_some())
                           .map(|g| g.to_string()),
            "scopes": reg.get("scope").map(|s| match s {
                Value::String(one) => vec![one.clone()],
                Value::Array(a) => a.iter().filter_map(Value::as_str).map(str::to_owned).collect(),
                _ => vec![],
            }),
            "expires_at": reg.get("expiresAt").and_then(Value::as_str),
            "endpoint": endpoint,
            "mode": mode_code(reg),
            "ops": ops_mask(reg),
            "tenant_at_peer": reg.get("tenant").and_then(Value::as_str),
            "headers": reg.get("contextSourceInfo"),
            // Table 5.2.9-1 names this member `contextSourceAlias` — the
            // peer's tenant-specific loop pseudonym. (`hostAlias` is the
            // prose spelling 6.3.18 uses and the csource_index column name;
            // it is not a payload member and was never sent by any client.)
            "host_alias": reg.get("contextSourceAlias").and_then(Value::as_str),
        })
    };
    let mut rows = Vec::new();
    let infos = reg
        .get("information")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for info in &infos {
        let names = |k: &str| -> Vec<String> {
            info.get(k)
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default()
        };
        let props = names("propertyNames");
        let rels = names("relationshipNames");
        let entities: Vec<Option<&Value>> = match info.get("entities").and_then(Value::as_array) {
            Some(a) if !a.is_empty() => a.iter().map(Some).collect(),
            _ => vec![None],
        };
        for ent in entities {
            if rows.len() >= MAX_INDEX_ROWS {
                tracing::warn!(
                    "registration explodes past {MAX_INDEX_ROWS} index rows; truncating"
                );
                return rows;
            }
            if props.is_empty() && rels.is_empty() {
                rows.push(common(ent, None, None));
            }
            for p in &props {
                rows.push(common(ent, Some(p), None));
            }
            for r in &rels {
                rows.push(common(ent, None, Some(r)));
            }
        }
    }
    if infos.is_empty() {
        rows.push(common(None, None, None));
    }
    rows
}

pub struct PgDocStore {
    pool: PgPool,
}

impl PgDocStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
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
        let context = doc
            .get("@context")
            .cloned()
            .unwrap_or(Value::Object(Default::default()));
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
            let mut q = sqlx::query(sqlx::AssertSqlSafe(sql.clone()))
                .bind(tenant.as_str())
                .bind(id)
                .bind(doc);
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
            if matches!(kind, DocKind::Registration) {
                // C7: rebuild this registration's csource_index rows in the
                // SAME transaction (delete + multi-row insert; §8.3).
                sqlx::query(
                    "DELETE FROM csource_index WHERE tenant_id = $1 AND registration_id = $2",
                )
                .bind(tenant.as_str())
                .bind(id)
                .execute(&mut *tx)
                .await?;
                sqlx::query(
                    "INSERT INTO csource_index
                       (tenant_id, registration_id, entity_id, id_pattern, entity_type,
                        property_name, relationship_name, scopes, expires_at, endpoint,
                        mode, ops, tenant_at_peer, headers, host_alias, location)
                     SELECT $1, $2, e->>'entity_id', e->>'id_pattern', e->>'entity_type',
                            e->>'property_name', e->>'relationship_name',
                            CASE WHEN e->'scopes' = 'null'::jsonb THEN NULL
                                 ELSE ARRAY(SELECT jsonb_array_elements_text(e->'scopes')) END,
                            (e->>'expires_at')::timestamptz, e->>'endpoint',
                            (e->>'mode')::smallint, (e->>'ops')::bigint,
                            e->>'tenant_at_peer', e->'headers', e->>'host_alias',
                            CASE WHEN ST_IsValid(ST_SetSRID(ST_GeomFromGeoJSON(e->>'location'), 4326))
                                 THEN ST_SetSRID(ST_GeomFromGeoJSON(e->>'location'), 4326) END
                     FROM jsonb_array_elements($3::jsonb) AS e",
                )
                .bind(tenant.as_str())
                .bind(id)
                .bind(Value::Array(index_rows(doc)))
                .execute(&mut *tx)
                .await?;
            }
            tx.commit().await?;
            Ok(existed)
        })
    }

    pub fn get(
        &self,
        tenant: &TenantId,
        kind: DocKind,
        id: &str,
    ) -> Result<Option<Value>, sqlx::Error> {
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

    /// Read-modify-write in ONE transaction under the row lock (§3.1.2
    /// applied to doc kinds): `SELECT … FOR UPDATE` → apply → `UPDATE`.
    /// A missing row returns `None` and is NEVER inserted — a concurrent
    /// DELETE must win, not be resurrected by a bookkeeping writeback
    /// (the 047_06 leftover-subscription bug).
    pub fn mutate<T, E>(
        &self,
        tenant: &TenantId,
        kind: DocKind,
        id: &str,
        f: impl FnOnce(&mut Value) -> Result<T, E>,
    ) -> Result<Option<Result<T, E>>, sqlx::Error> {
        let col = kind.doc_column();
        let table = kind.table();
        let select =
            format!("SELECT {col} FROM {table} WHERE tenant_id = $1 AND id = $2 FOR UPDATE");
        let update = if kind.has_bookkeeping() {
            format!(
                "UPDATE {table} SET {col} = $3, expires_at = $4::timestamptz,
                   is_active = $5, times_sent = $6, last_notification = $7::timestamptz,
                   last_success = $8::timestamptz, last_failure = $9::timestamptz,
                   context = $10
                 WHERE tenant_id = $1 AND id = $2"
            )
        } else {
            format!("UPDATE {table} SET {col} = $3 WHERE tenant_id = $1 AND id = $2")
        };
        wait(async move {
            let mut tx = self.pool.begin().await?;
            crate::pg::set_tenant(&mut tx, tenant).await?;
            let row = sqlx::query(sqlx::AssertSqlSafe(select.clone()))
                .bind(tenant.as_str())
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?;
            let Some(row) = row else {
                return Ok(None);
            };
            let mut doc: Value = row.get(0);
            match f(&mut doc) {
                Ok(t) => {
                    let context = doc
                        .get("@context")
                        .cloned()
                        .unwrap_or(Value::Object(Default::default()));
                    let (expires, active, sent, last_n, last_s, last_f) = bookkeeping(&doc);
                    let mut q = sqlx::query(sqlx::AssertSqlSafe(update.clone()))
                        .bind(tenant.as_str())
                        .bind(id)
                        .bind(&doc);
                    if kind.has_bookkeeping() {
                        q = q
                            .bind(&expires)
                            .bind(active)
                            .bind(sent)
                            .bind(&last_n)
                            .bind(&last_s)
                            .bind(&last_f)
                            .bind(&context);
                    }
                    q.execute(&mut *tx).await?;
                    tx.commit().await?;
                    Ok(Some(Ok(t)))
                }
                // closure rejected the change: nothing written, lock released
                Err(e) => Ok(Some(Err(e))),
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ops_mask_expands_groups_and_defaults() {
        // absent operations = federationOps (5.2.9)
        let default_mask = ops_mask(&json!({}));
        let fed_mask = ops_mask(&json!({"operations": ["federationOps"]}));
        assert_eq!(default_mask, fed_mask);
        let bit = |op: &str| 1i64 << OPERATIONS.iter().position(|o| *o == op).expect(op);
        assert_ne!(fed_mask & bit("retrieveEntity"), 0);
        assert_eq!(
            fed_mask & bit("createEntity"),
            0,
            "provision op not in federationOps"
        );
        let m = ops_mask(&json!({"operations": ["createEntity", "retrieveOps"]}));
        assert_ne!(m & bit("createEntity"), 0);
        assert_ne!(m & bit("queryEntity"), 0, "retrieveOps expands");
        assert_eq!(m & bit("deleteEntity"), 0);
        assert_eq!(ops_mask(&json!({"operations": ["notARealOp"]})), 0);
    }

    #[test]
    fn index_rows_explode_information() {
        // 2 entities × (1 property + 1 relationship) = 4 rows
        let reg = json!({
            "endpoint": "http://cs.example:9090",
            "mode": "exclusive",
            "information": [{
                "entities": [
                    {"id": "urn:a", "type": "T1"},
                    {"idPattern": "urn:.*", "type": "T2"}
                ],
                "propertyNames": ["speed"],
                "relationshipNames": ["isParked"]
            }],
            "expiresAt": "2030-01-01T00:00:00Z"
        });
        let rows = index_rows(&reg);
        assert_eq!(rows.len(), 4);
        assert!(rows
            .iter()
            .all(|r| r["endpoint"] == "http://cs.example:9090"
                && r["mode"] == 3
                && r["expires_at"] == "2030-01-01T00:00:00Z"));
        assert!(rows
            .iter()
            .any(|r| r["entity_id"] == "urn:a" && r["property_name"] == "speed"));
        assert!(rows
            .iter()
            .any(|r| r["id_pattern"] == "urn:.*" && r["relationship_name"] == "isParked"));
        // attribute-less info: one row per entity, both attr columns NULL
        let bare = json!({"endpoint": "e", "information": [{"entities": [{"type": "T"}]}]});
        let rows = index_rows(&bare);
        assert_eq!(rows.len(), 1);
        assert!(rows[0]["property_name"].is_null() && rows[0]["relationship_name"].is_null());
    }
}
