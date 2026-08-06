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

/// Entity-doc members that are NOT temporal attributes.
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

/// C9: decompose the bridge doc into `attr_instances` rows and resync them
/// inside the caller's transaction. Full resync (delete + one multi-row
/// insert): the doc is authoritative, so the rows can never drift from it.
/// (`ponytail:` full resync per write; delta upserts arrive with the F8
/// durable recorder, whose events carry per-instance deltas.)
async fn sync_instances(
    tx: &mut sqlx::PgConnection,
    tenant: &TenantId,
    entity_id: &str,
    doc: &Value,
) -> Result<(), sqlx::Error> {
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
                // §8.2: observed_at falls back to modified_at
                let observed = s("observedAt")
                    .or_else(|| s("modifiedAt"))
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
    sqlx::query("DELETE FROM attr_instances WHERE tenant_id = $1 AND entity_id = $2")
        .bind(tenant.as_str())
        .bind(entity_id)
        .execute(&mut *tx)
        .await?;
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
pub use super::filter::TemporalFilter;

/// The SELECT expression that prunes instance arrays inside the doc, plus its
/// text binds (numbered from `first_bind`). `None` = nothing to prune (no
/// range, no lastN) or a range shape the compiler refuses — select `doc`.
///
/// Meta members (including instance-shaped `scope`, which is in DOC_META)
/// pass through unpruned — looser is fine, the window refilters. The DOC_META
/// name list is inlined as literals: it is a compiler constant, never user
/// input (§16.2). lastN uses RANK(), not ROW_NUMBER(): ties share a rank, so
/// every instance the API's per-attr lastN could keep survives the cut.
fn pruned_doc_expr(f: &TemporalFilter<'_>, first_bind: usize) -> Option<(String, Vec<String>)> {
    if f.range.is_none() && f.last_n.is_none() {
        return None;
    }
    // $first_bind is always the timeproperty (predicate member / order key)
    let mut binds = vec![f.timeproperty.to_owned()];
    let tp = first_bind;
    let mut where_clause = String::new();
    if let Some(r) = &f.range {
        let c = crate::compile::temporal::compile_instance_range(r, "el", first_bind)?;
        where_clause = format!("WHERE {}", c.sql);
        debug_assert_eq!(c.binds[0], f.timeproperty);
        binds.extend(c.binds.into_iter().skip(1));
    }
    let arr = match f.last_n {
        Some(n) => {
            let n_bind = first_bind + binds.len();
            binds.push(n.to_string());
            format!(
                "(SELECT COALESCE(jsonb_agg(s.el), '[]'::jsonb) FROM \
                   (SELECT el, rank() OVER (PARTITION BY el ->> 'datasetId' \
                      ORDER BY (el ->> ${tp}) COLLATE \"C\" DESC NULLS LAST) AS rk \
                    FROM jsonb_array_elements(t.v) AS el {where_clause}) AS s \
                 WHERE s.rk <= ${n_bind}::bigint)"
            )
        }
        None => format!(
            "(SELECT COALESCE(jsonb_agg(el), '[]'::jsonb) \
              FROM jsonb_array_elements(t.v) AS el {where_clause})"
        ),
    };
    let meta = DOC_META
        .iter()
        .map(|m| format!("'{m}'"))
        .collect::<Vec<_>>()
        .join(", ");
    let expr = format!(
        "(SELECT COALESCE(jsonb_object_agg(t.k, CASE \
            WHEN jsonb_typeof(t.v) = 'array' AND t.k NOT IN ({meta}) \
            THEN {arr} ELSE t.v END), '{{}}'::jsonb) \
          FROM jsonb_each(doc) AS t(k, v))"
    );
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
            if n == 1 {
                sync_instances(&mut tx, tenant, id, doc).await?;
            }
            tx.commit().await?;
            Ok(n == 1)
        })
    }

    pub fn get(&self, tenant: &TenantId, id: &str) -> Result<Option<Value>, sqlx::Error> {
        wait(async {
            let mut tx = self.pool.begin().await?;
            crate::pg::set_tenant(&mut tx, tenant).await?;
            let row =
                sqlx::query("SELECT doc FROM temporal_entities WHERE tenant_id = $1 AND id = $2")
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

    /// C11 temporal query pushdown: entity narrowing (ids/types/attrs) in the
    /// WHERE, instance pruning (range + lastN cap) in the SELECT. Rows come
    /// back as bridge-shaped docs — the API's window() stays the arbiter.
    pub fn query(
        &self,
        tenant: &TenantId,
        f: &TemporalFilter<'_>,
    ) -> Result<Vec<Value>, sqlx::Error> {
        enum B {
            Text(String),
            Arr(Vec<String>),
        }
        let mut binds: Vec<B> = vec![B::Text(tenant.as_str().to_owned())];
        let mut wheres = vec!["tenant_id = $1".to_owned()];
        if let Some(ids) = f.ids {
            binds.push(B::Arr(ids.iter().map(|s| s.to_string()).collect()));
            wheres.push(format!("id = ANY(${})", binds.len()));
        }
        if let Some(types) = f.types {
            // overlap: entity has ANY of the wanted types (flat OR list)
            binds.push(B::Arr(types.to_vec()));
            wheres.push(format!("types && ${}", binds.len()));
        }
        if let Some(attrs) = f.attrs {
            binds.push(B::Arr(attrs.to_vec()));
            wheres.push(format!("doc ?| ${}", binds.len()));
        }
        let select = match pruned_doc_expr(f, binds.len() + 1) {
            Some((expr, extra)) => {
                binds.extend(extra.into_iter().map(B::Text));
                expr
            }
            None => "doc".to_owned(),
        };
        let sql = format!(
            "SELECT {select} FROM temporal_entities WHERE {} ORDER BY id",
            wheres.join(" AND ")
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
                };
            }
            let rows = qy.fetch_all(&mut *tx).await?;
            tx.commit().await?;
            Ok(rows.into_iter().map(|r| r.get::<Value, _>(0)).collect())
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
        let mut binds: Vec<String> = Vec::new();
        let select = match pruned_doc_expr(f, 3) {
            Some((expr, extra)) => {
                binds = extra;
                expr
            }
            None => "doc".to_owned(),
        };
        let sql =
            format!("SELECT {select} FROM temporal_entities WHERE tenant_id = $1 AND id = $2");
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
        wait(async {
            let mut tx = self.pool.begin().await?;
            crate::pg::set_tenant(&mut tx, tenant).await?;
            let rows =
                sqlx::query("SELECT doc FROM temporal_entities WHERE tenant_id = $1 ORDER BY id")
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
                    sync_instances(&mut tx, tenant, id, &doc).await?;
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
    fn no_window_means_no_pruning_expression() {
        assert!(pruned_doc_expr(&TemporalFilter::default(), 2).is_none());
    }

    #[test]
    fn range_prunes_arrays_but_never_meta_members() {
        let (expr, binds) = pruned_doc_expr(&between(), 2).expect("prunes");
        // meta members pass through the ELSE branch untouched
        for m in DOC_META {
            assert!(expr.contains(&format!("'{m}'")), "meta {m} missing: {expr}");
        }
        assert!(expr.contains("jsonb_typeof(t.v) = 'array'"), "{expr}");
        // binds: timeproperty, timeAt, endTimeAt — numbered from first_bind
        assert_eq!(
            binds,
            vec!["observedAt", "2026-01-01T00:00:00Z", "2026-02-01T00:00:00Z"]
        );
        assert!(
            expr.contains("$2") && expr.contains("$3") && expr.contains("$4"),
            "{expr}"
        );
    }

    #[test]
    fn last_n_caps_with_rank_per_dataset_never_row_number() {
        let f = TemporalFilter {
            last_n: Some(5),
            ..Default::default()
        };
        let (expr, binds) = pruned_doc_expr(&f, 1).expect("prunes");
        // RANK keeps timestamp ties; ROW_NUMBER would cut an instance the
        // API-side per-attr lastN still wants (the tie-break divergence bug)
        assert!(expr.contains("rank() OVER"), "{expr}");
        assert!(!expr.contains("row_number"), "{expr}");
        assert!(expr.contains("PARTITION BY el ->> 'datasetId'"), "{expr}");
        assert!(expr.contains("COLLATE \"C\" DESC NULLS LAST"), "{expr}");
        assert_eq!(binds, vec!["observedAt", "5"]);
    }

    #[test]
    fn refused_range_shape_refuses_the_whole_pruning() {
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
        assert!(pruned_doc_expr(&f, 1).is_none());
    }
}
