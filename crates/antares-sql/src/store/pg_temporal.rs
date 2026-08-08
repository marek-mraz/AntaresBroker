//! PgStore slice three (tasks.md §C-ii): the temporal store over
//! `attr_instances` ROWS (C9/D read cutover, audit 2026-08-08). The 0002
//! bridge doc is gone — `temporal_entities` holds only the small `meta`
//! document; every instance lives as a row, reads RECONSTRUCT the doc shape
//! the API layer consumes (so window()/aggregation/presentation are
//! untouched), and writes are deltas, never a full-history rewrite.
//!
//! What this buys (the §8.2 promises, now real): the hypertable/partition
//! machinery acts on the data queries actually read; retention shortens
//! query results; instance pruning and entity paging run in SQL with the
//! `(tenant_id, entity_id, attr_id, observed_at DESC)` index.

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
            Value::Array(a) => a
                .iter()
                .filter_map(|x| x.as_str().map(str::to_owned))
                .collect(),
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

/// Entity-doc members that are NOT temporal attributes. `scope` may itself be
/// instance-shaped (deletion instances, 020_19/20) — it lives in `meta`
/// verbatim either way, exactly as the bridge kept it unpruned.
const DOC_META: &[&str] = &[
    "id",
    "type",
    "scope",
    "@context",
    "createdAt",
    "modifiedAt",
    "deletedAt",
    "expiresAt",
];

/// The meta-only document stored in `temporal_entities.meta`.
fn meta_of(doc: &Value) -> Value {
    let mut m = serde_json::Map::new();
    if let Some(obj) = doc.as_object() {
        for k in DOC_META {
            if let Some(v) = obj.get(*k) {
                m.insert((*k).to_owned(), v.clone());
            }
        }
    }
    Value::Object(m)
}

/// Decompose a doc's attribute arrays into row JSON for the multi-row
/// INSERT. The `observed_at` string is derived ONLY from the instance
/// document, so decomposing a reconstructed doc reproduces byte-identical
/// keys (what the mutate diff relies on).
fn decompose(doc: &Value) -> Vec<Value> {
    let mut rows: Vec<Value> = Vec::new();
    if let Some(obj) = doc.as_object() {
        for (attr, instances) in obj {
            if DOC_META.contains(&attr.as_str()) {
                continue;
            }
            let Some(arr) = instances.as_array() else {
                continue;
            };
            for i in arr {
                let s = |k: &str| i.get(k).and_then(Value::as_str);
                let Some(instance_id) = s("instanceId") else {
                    continue; // stamped by the API layer; belt only
                };
                // §8.2: observed_at falls back through the instance's own
                // timestamps — deletion instances carry only deletedAt and
                // must NOT collapse onto the epoch (retention would reap them)
                let observed = s("observedAt")
                    .or_else(|| s("modifiedAt"))
                    .or_else(|| s("deletedAt"))
                    .or_else(|| s("createdAt"))
                    .unwrap_or("1970-01-01T00:00:00Z");
                rows.push(serde_json::json!({
                    "attr_id": attr,
                    "instance_id": instance_id,
                    "dataset_id": s("datasetId"),
                    "observed_at": observed,
                    "created_at": s("createdAt").unwrap_or(observed),
                    "modified_at": s("modifiedAt").unwrap_or(observed),
                    "data": i,
                }));
            }
        }
    }
    rows
}

/// Multi-row instance upsert inside the caller's transaction.
async fn insert_rows(
    tx: &mut sqlx::PgConnection,
    tenant: &TenantId,
    entity_id: &str,
    rows: Vec<Value>,
) -> Result<(), sqlx::Error> {
    if rows.is_empty() {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO attr_instances
           (tenant_id, entity_id, attr_id, instance_id, dataset_id, observed_at,
            created_at, modified_at, data)
         SELECT $1, $2, e->>'attr_id', e->>'instance_id', e->>'dataset_id',
                (e->>'observed_at')::timestamptz, (e->>'created_at')::timestamptz,
                (e->>'modified_at')::timestamptz, e->'data'
         FROM jsonb_array_elements($3::jsonb) AS e
         ON CONFLICT (tenant_id, entity_id, attr_id, instance_id, observed_at)
           DO UPDATE SET data = EXCLUDED.data, modified_at = EXCLUDED.modified_at,
                         dataset_id = EXCLUDED.dataset_id",
    )
    .bind(tenant.as_str())
    .bind(entity_id)
    .bind(Value::Array(rows))
    .execute(&mut *tx)
    .await?;
    Ok(())
}

// N2: TemporalFilter moved to `store::filter`; re-exported for path compat.
pub use super::filter::{TemporalFilter, TemporalOutcome};

/// The correlated subquery reconstructing the attribute object for the meta
/// row aliased `m`, with the 4.11 range and the lastN RANK() cap applied over
/// the rows (byte-exact against the API window: predicates and ordering run
/// on the instance JSON with COLLATE "C", never on the partition column).
/// Returns the SQL fragment + its text binds, numbered from `first_bind`;
/// `None` when a range is present but outside the compiler's exact subset —
/// the caller then reconstructs unpruned and the window stays the arbiter.
fn attr_object_expr(f: &TemporalFilter<'_>, first_bind: usize) -> Option<(String, Vec<String>)> {
    // $first_bind is always the timeproperty (predicate member / order key)
    let mut binds = vec![f.timeproperty.to_owned()];
    let tp = first_bind;
    let mut range_and = String::new();
    if let Some(r) = &f.range {
        let c = crate::compile::temporal::compile_instance_range(r, "ai.data", first_bind)?;
        range_and = format!(" AND {}", c.sql);
        debug_assert_eq!(c.binds[0], f.timeproperty);
        binds.extend(c.binds.into_iter().skip(1));
    }
    let expr = match f.last_n {
        Some(n) => {
            let n_bind = first_bind + binds.len();
            binds.push(n.to_string());
            format!(
                "COALESCE((SELECT jsonb_object_agg(g.attr_id, g.insts) FROM (\
                   SELECT s.attr_id, jsonb_agg(s.data ORDER BY s.created_at, s.observed_at, s.instance_id) AS insts \
                   FROM (SELECT ai.*, rank() OVER (PARTITION BY ai.attr_id, ai.data ->> 'datasetId' \
                             ORDER BY (ai.data ->> ${tp}) COLLATE \"C\" DESC NULLS LAST) AS rk \
                         FROM attr_instances ai \
                         WHERE ai.tenant_id = m.tenant_id AND ai.entity_id = m.id{range_and}) s \
                   WHERE s.rk <= ${n_bind}::bigint GROUP BY s.attr_id) g), '{{}}'::jsonb)"
            )
        }
        None => format!(
            "COALESCE((SELECT jsonb_object_agg(g.attr_id, g.insts) FROM (\
               SELECT ai.attr_id, jsonb_agg(ai.data ORDER BY ai.created_at, ai.observed_at, ai.instance_id) AS insts \
               FROM attr_instances ai \
               WHERE ai.tenant_id = m.tenant_id AND ai.entity_id = m.id{range_and} \
               GROUP BY ai.attr_id) g), '{{}}'::jsonb)"
        ),
    };
    Some((expr, binds))
}

impl PgTemporalStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// `false` when the id already exists (create semantics, like the memory
    /// store's `create`).
    pub fn create(&self, tenant: &TenantId, id: &str, doc: &Value) -> Result<bool, sqlx::Error> {
        let (types, scopes, created, modified) = extract(doc);
        let meta = meta_of(doc);
        let rows = decompose(doc);
        wait(async {
            let mut tx = self.pool.begin().await?;
            crate::pg::set_tenant(&mut tx, tenant).await?;
            let n = sqlx::query(
                "INSERT INTO temporal_entities
                   (tenant_id, id, types, scopes, meta, created_at, modified_at)
                 VALUES ($1, $2, $3, $4, $5, $6::timestamptz, $7::timestamptz)
                 ON CONFLICT (tenant_id, id) DO NOTHING",
            )
            .bind(tenant.as_str())
            .bind(id)
            .bind(&types)
            .bind(&scopes)
            .bind(&meta)
            .bind(&created)
            .bind(&modified)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            if n == 1 {
                insert_rows(&mut tx, tenant, id, rows).await?;
            }
            tx.commit().await?;
            Ok(n == 1)
        })
    }

    /// Append-only fast path (auto-recording, 5.6.12 adds): NO reconstruction,
    /// no history read — a shell meta insert (first touch) plus one multi-row
    /// instance upsert. This is the write the old full-resync made O(history).
    pub fn append(
        &self,
        tenant: &TenantId,
        id: &str,
        shell: &Value,
        additions: &Value,
    ) -> Result<(), sqlx::Error> {
        let (types, scopes, created, modified) = extract(shell);
        let meta = meta_of(shell);
        let rows = decompose(additions);
        wait(async {
            let mut tx = self.pool.begin().await?;
            crate::pg::set_tenant(&mut tx, tenant).await?;
            sqlx::query(
                "INSERT INTO temporal_entities
                   (tenant_id, id, types, scopes, meta, created_at, modified_at)
                 VALUES ($1, $2, $3, $4, $5, $6::timestamptz, $7::timestamptz)
                 ON CONFLICT (tenant_id, id) DO NOTHING",
            )
            .bind(tenant.as_str())
            .bind(id)
            .bind(&types)
            .bind(&scopes)
            .bind(&meta)
            .bind(&created)
            .bind(&modified)
            .execute(&mut *tx)
            .await?;
            insert_rows(&mut tx, tenant, id, rows).await?;
            tx.commit().await?;
            Ok(())
        })
    }

    pub fn get(&self, tenant: &TenantId, id: &str) -> Result<Option<Value>, sqlx::Error> {
        self.get_range(tenant, id, &TemporalFilter::default())
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
            // no FK: a partitioned table cannot be the referencing side of a
            // cascade from temporal_entities — clean the instances explicitly.
            sqlx::query("DELETE FROM attr_instances WHERE tenant_id = $1 AND entity_id = $2")
                .bind(tenant.as_str())
                .bind(id)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            Ok(n == 1)
        })
    }

    /// C11 temporal query: entity narrowing (ids/types/attrs) in the WHERE,
    /// instance pruning (range + lastN cap) in the reconstruction, and —
    /// when the caller passes a page — entity qualification + LIMIT/OFFSET
    /// in SQL, so a temporal query no longer materializes the whole tenant.
    pub fn query(
        &self,
        tenant: &TenantId,
        f: &TemporalFilter<'_>,
    ) -> Result<TemporalOutcome, sqlx::Error> {
        enum B {
            Text(String),
            Arr(Vec<String>),
            Num(i64),
        }
        let mut binds: Vec<B> = vec![B::Text(tenant.as_str().to_owned())];
        let mut wheres = vec!["m.tenant_id = $1".to_owned()];
        if let Some(ids) = f.ids {
            binds.push(B::Arr(ids.iter().map(|s| s.to_string()).collect()));
            wheres.push(format!("m.id = ANY(${})", binds.len()));
        }
        if let Some(types) = f.types {
            // overlap: entity has ANY of the wanted types (flat OR list)
            binds.push(B::Arr(types.to_vec()));
            wheres.push(format!("m.types && ${}", binds.len()));
        }
        if let Some(attrs) = f.attrs {
            binds.push(B::Arr(attrs.to_vec()));
            wheres.push(format!(
                "EXISTS (SELECT 1 FROM attr_instances x WHERE x.tenant_id = m.tenant_id \
                 AND x.entity_id = m.id AND x.attr_id = ANY(${}))",
                binds.len()
            ));
        }
        // page pushdown: only when the caller passed one AND the range (if
        // any) compiles — SQL then also applies the evaluator's entity-
        // qualification rule (≥1 instance, in-window when ranged). WHERE
        // binds come FIRST so the fallback count query (offset past the end)
        // can reuse them with identical numbering.
        let range_compiled = f.range.is_none()
            || f.range.as_ref().is_some_and(|r| {
                crate::compile::temporal::compile_instance_range(r, "ai.data", 1).is_some()
            });
        let mut paged = false;
        if f.page.is_some() && range_compiled {
            let mut qual = "EXISTS (SELECT 1 FROM attr_instances ai WHERE \
                            ai.tenant_id = m.tenant_id AND ai.entity_id = m.id"
                .to_owned();
            if let Some(r) = &f.range {
                let c =
                    crate::compile::temporal::compile_instance_range(r, "ai.data", binds.len() + 1)
                        .expect("range_compiled checked above");
                for b in c.binds {
                    binds.push(B::Text(b));
                }
                qual.push_str(&format!(" AND {}", c.sql));
            }
            qual.push(')');
            wheres.push(qual);
            paged = true;
        }
        let n_where = binds.len();
        let where_sql = wheres.join(" AND ");
        let (attr_expr, extra) = match attr_object_expr(f, n_where + 1) {
            Some(v) => v,
            // refused range shape: reconstruct unpruned, window arbitrates
            None => attr_object_expr(&TemporalFilter::default(), n_where + 1)
                .expect("no range/lastN always compiles"),
        };
        binds.extend(extra.into_iter().map(B::Text));
        let mut select_total = String::new();
        let mut tail = " ORDER BY m.id".to_owned();
        if paged {
            let page = f.page.as_ref().expect("paged implies page");
            select_total = ", count(*) OVER () AS total".into();
            binds.push(B::Num(page.limit));
            tail.push_str(&format!(" LIMIT ${}", binds.len()));
            binds.push(B::Num(page.offset));
            tail.push_str(&format!(" OFFSET ${}", binds.len()));
        }
        let sql = format!(
            "SELECT m.meta || {attr_expr}{select_total} FROM temporal_entities m \
             WHERE {where_sql}{tail}"
        );
        wait(async {
            let mut tx = self.pool.begin().await?;
            crate::pg::set_tenant(&mut tx, tenant).await?;
            // §16.2 audit: `sql` is compiler literals + $n placeholders only.
            let mut qy = sqlx::query(sqlx::AssertSqlSafe(sql.clone()));
            for b in &binds {
                qy = match b {
                    B::Text(s) => qy.bind(s),
                    B::Arr(v) => qy.bind(v),
                    B::Num(n) => qy.bind(n),
                };
            }
            let rows = qy.fetch_all(&mut *tx).await?;
            let mut total = if paged {
                rows.first().map(|r| r.get::<i64, _>(1))
            } else {
                None
            };
            if paged && total.is_none() {
                // offset past the end: the window function came back with the
                // page, which is empty — count separately with the WHERE binds
                let count_sql =
                    format!("SELECT count(*) FROM temporal_entities m WHERE {where_sql}");
                let mut cq = sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(count_sql));
                for b in binds.iter().take(n_where) {
                    cq = match b {
                        B::Text(s) => cq.bind(s),
                        B::Arr(v) => cq.bind(v),
                        B::Num(n) => cq.bind(n),
                    };
                }
                total = Some(cq.fetch_one(&mut *tx).await?);
            }
            tx.commit().await?;
            Ok(TemporalOutcome {
                rows: rows.into_iter().map(|r| r.get::<Value, _>(0)).collect(),
                paged,
                total,
            })
        })
    }

    /// C11: single-entity fetch with the same instance pruning (Retrieve
    /// Temporal Evolution, 5.7.3). `None` = entity absent.
    pub fn get_range(
        &self,
        tenant: &TenantId,
        id: &str,
        f: &TemporalFilter<'_>,
    ) -> Result<Option<Value>, sqlx::Error> {
        let (attr_expr, binds) = match attr_object_expr(f, 3) {
            Some(v) => v,
            None => attr_object_expr(&TemporalFilter::default(), 3)
                .expect("no range/lastN always compiles"),
        };
        let sql = format!(
            "SELECT m.meta || {attr_expr} FROM temporal_entities m \
             WHERE m.tenant_id = $1 AND m.id = $2"
        );
        wait(async {
            let mut tx = self.pool.begin().await?;
            crate::pg::set_tenant(&mut tx, tenant).await?;
            let mut qy = sqlx::query(sqlx::AssertSqlSafe(sql.clone()))
                .bind(tenant.as_str())
                .bind(id);
            for b in &binds {
                qy = qy.bind(b);
            }
            let row = qy.fetch_optional(&mut *tx).await?;
            tx.commit().await?;
            Ok(row.map(|r| r.get::<Value, _>(0)))
        })
    }

    pub fn list(&self, tenant: &TenantId) -> Result<Vec<Value>, sqlx::Error> {
        Ok(self.query(tenant, &TemporalFilter::default())?.rows)
    }

    /// Row-locked read-modify-write over the RECONSTRUCTED doc, written back
    /// as a DELTA: rows are diffed by (attr, instanceId) and only moved,
    /// changed or removed instances touch the table — never a full-history
    /// rewrite (the old resync's O(history) write amplification, and the
    /// thing that fought hypertable compression).
    pub fn mutate<T, E>(
        &self,
        tenant: &TenantId,
        id: &str,
        f: impl FnOnce(&mut Value) -> Result<T, E>,
    ) -> Result<Option<Result<T, E>>, sqlx::Error> {
        wait(async {
            let mut tx = self.pool.begin().await?;
            crate::pg::set_tenant(&mut tx, tenant).await?;
            // the meta row is the serialization point (FOR UPDATE)
            let row = sqlx::query(
                "SELECT meta FROM temporal_entities WHERE tenant_id = $1 AND id = $2 FOR UPDATE",
            )
            .bind(tenant.as_str())
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?;
            let Some(row) = row else {
                tx.commit().await?;
                return Ok(None);
            };
            let meta: Value = row.get(0);
            // reconstruct the full doc inside the same transaction
            let attrs_row = sqlx::query(
                "SELECT COALESCE((SELECT jsonb_object_agg(g.attr_id, g.insts) FROM (
                    SELECT ai.attr_id,
                           jsonb_agg(ai.data ORDER BY ai.created_at, ai.observed_at, ai.instance_id) AS insts
                    FROM attr_instances ai
                    WHERE ai.tenant_id = $1 AND ai.entity_id = $2
                    GROUP BY ai.attr_id) g), '{}'::jsonb)",
            )
            .bind(tenant.as_str())
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
            let attrs: Value = attrs_row.get(0);
            let mut doc = meta;
            if let (Some(d), Some(a)) = (doc.as_object_mut(), attrs.as_object()) {
                for (k, v) in a {
                    d.insert(k.clone(), v.clone());
                }
            }
            let before_rows = decompose(&doc);
            match f(&mut doc) {
                Ok(t) => {
                    let (types, scopes, _created, modified) = extract(&doc);
                    let after_rows = decompose(&doc);
                    // diff by logical identity (attr, instanceId); a changed
                    // observed_at moves the physical row → delete + insert
                    let key = |r: &Value| -> (String, String) {
                        (
                            r["attr_id"].as_str().unwrap_or("").to_owned(),
                            r["instance_id"].as_str().unwrap_or("").to_owned(),
                        )
                    };
                    let old: std::collections::HashMap<(String, String), &Value> =
                        before_rows.iter().map(|r| (key(r), r)).collect();
                    let new: std::collections::HashMap<(String, String), &Value> =
                        after_rows.iter().map(|r| (key(r), r)).collect();
                    let mut deletes: Vec<Value> = Vec::new();
                    for (k, o) in &old {
                        match new.get(k) {
                            None => deletes.push(serde_json::json!({"a": k.0, "i": k.1})),
                            Some(n) if n["observed_at"] != o["observed_at"] => {
                                deletes.push(serde_json::json!({"a": k.0, "i": k.1}))
                            }
                            _ => {}
                        }
                    }
                    let upserts: Vec<Value> = after_rows
                        .iter()
                        .filter(|r| old.get(&key(r)).is_none_or(|o| *o != *r))
                        .cloned()
                        .collect();
                    if !deletes.is_empty() {
                        sqlx::query(
                            "DELETE FROM attr_instances t
                             USING jsonb_array_elements($3::jsonb) AS e
                             WHERE t.tenant_id = $1 AND t.entity_id = $2
                               AND t.attr_id = e->>'a' AND t.instance_id = e->>'i'",
                        )
                        .bind(tenant.as_str())
                        .bind(id)
                        .bind(Value::Array(deletes))
                        .execute(&mut *tx)
                        .await?;
                    }
                    insert_rows(&mut tx, tenant, id, upserts).await?;
                    sqlx::query(
                        "UPDATE temporal_entities SET meta = $3, types = $4, scopes = $5,
                           modified_at = $6::timestamptz
                         WHERE tenant_id = $1 AND id = $2",
                    )
                    .bind(tenant.as_str())
                    .bind(id)
                    .bind(meta_of(&doc))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile::temporal::InstanceRange;

    fn between<'a>() -> TemporalFilter<'a> {
        TemporalFilter {
            range: Some(InstanceRange {
                timerel: "between",
                time_at: "2026-01-01T00:00:00Z",
                end_time_at: Some("2026-02-01T00:00:00Z"),
                timeproperty: "observedAt",
            }),
            ..Default::default()
        }
    }

    #[test]
    fn meta_of_keeps_only_meta_members() {
        let doc = serde_json::json!({
            "id": "urn:x", "type": ["T"], "createdAt": "c", "modifiedAt": "m",
            "https://a/attr": [{"instanceId": "i1"}]
        });
        let m = meta_of(&doc);
        assert!(m.get("id").is_some() && m.get("https://a/attr").is_none());
    }

    #[test]
    fn range_binds_are_numbered_from_first_bind() {
        let (expr, binds) = attr_object_expr(&between(), 2).expect("compiles");
        assert_eq!(
            binds,
            vec!["observedAt", "2026-01-01T00:00:00Z", "2026-02-01T00:00:00Z"]
        );
        assert!(
            expr.contains("$2") && expr.contains("$3") && expr.contains("$4"),
            "{expr}"
        );
        // predicates run on the instance JSON, never the partition column
        assert!(expr.contains("ai.data ->>"), "{expr}");
    }

    #[test]
    fn last_n_caps_with_rank_per_attr_and_dataset_never_row_number() {
        let f = TemporalFilter {
            last_n: Some(5),
            ..Default::default()
        };
        let (expr, binds) = attr_object_expr(&f, 1).expect("compiles");
        // RANK keeps timestamp ties; ROW_NUMBER would cut an instance the
        // API-side per-attr lastN still wants (the tie-break divergence bug)
        assert!(expr.contains("rank() OVER"), "{expr}");
        assert!(!expr.contains("row_number"), "{expr}");
        assert!(
            expr.contains("PARTITION BY ai.attr_id, ai.data ->> 'datasetId'"),
            "{expr}"
        );
        assert!(expr.contains("COLLATE \"C\" DESC NULLS LAST"), "{expr}");
        assert_eq!(binds, vec!["observedAt", "5"]);
    }

    #[test]
    fn refused_range_shape_refuses_the_pruning() {
        let f = TemporalFilter {
            range: Some(InstanceRange {
                timerel: "since", // not a 4.11 relation
                time_at: "t",
                end_time_at: None,
                timeproperty: "observedAt",
            }),
            last_n: Some(3),
            ..Default::default()
        };
        // half-pruning would silently skip the range: refuse instead
        assert!(attr_object_expr(&f, 1).is_none());
    }

    #[test]
    fn decompose_never_parks_deletion_instances_on_the_epoch() {
        let doc = serde_json::json!({
            "id": "urn:x",
            "https://a/attr": [{
                "type": "Property", "value": "urn:ngsi-ld:null",
                "instanceId": "urn:ngsi-ld:Instance:d1",
                "deletedAt": "2026-08-08T10:00:00Z"
            }]
        });
        let rows = decompose(&doc);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["observed_at"], "2026-08-08T10:00:00Z");
    }
}
