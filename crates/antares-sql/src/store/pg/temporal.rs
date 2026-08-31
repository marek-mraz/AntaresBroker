// SPDX-License-Identifier: EUPL-1.2
//! PgStore slice three: the temporal store over
//! `attr_instances` ROWS. The 0002
//! bridge doc is gone — `temporal_entities` holds only the small `meta`
//! document; every instance lives as a row, reads RECONSTRUCT the doc shape
//! the API layer consumes (so window()/aggregation/presentation are
//! untouched), and writes are deltas, never a full-history rewrite.
//!
//! What this buys: the hypertable/partition
//! machinery acts on the data queries actually read; retention shortens
//! query results; instance pruning and entity paging run in SQL with the
//! `(tenant_id, entity_id, attr_id, observed_at DESC)` index.

use antares_model::TenantId;
use serde_json::Value;
use sqlx::postgres::PgPool;
use sqlx::Row;

use super::entity::wait;

pub struct PgTemporalStore {
    pool: PgPool,
    /// Set when this instance serves only the temporal seam: it never holds
    /// the entities, so the append guard must not look for them here.
    pub temporal_only: bool,
}

/// 4.22: "expiresAt is defined as the system temporal Property at which a
/// certain Entity, Property or Relationship shall become invalid" — an
/// expired temporal entity is absent from every read, ahead of the retention
/// sweep. One shared literal so `query`, its count fallback and `get_range`
/// can never disagree.
///
/// The stamp is jsonb TEXT, so it goes through `try_timestamptz`
/// (0001_init.sql): a bare cast RAISES on anything it cannot parse, and
/// these reads are
/// tenant-wide — one bad stamp would take down the whole tenant's temporal
/// API rather than hide one entity. An unusable stamp reads as no expiry, the
/// same direction the memory arm takes in `filter::expired_at`.
const NOT_EXPIRED: &str =
    "(try_timestamptz(m.meta->>'expiresAt') IS NULL OR try_timestamptz(m.meta->>'expiresAt') > now())";

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
    // 4.6.3 comma fraction: both feed `::timestamptz` binds.
    let ts = |k: &str| {
        doc.get(k)
            .and_then(Value::as_str)
            .map(|s| antares_store::filter::canonical_datetime(s).into_owned())
    };
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
                // 4.6.3: a comma seconds-fraction is legal in a request and
                // these stamps go straight into `::timestamptz` casts, which
                // refuse it — and the raise lands in the temporal drain,
                // which absorbs it, so the whole request's history would be
                // lost with a 2xx already returned. Timestamps only:
                // `datasetId` is a URI, where a comma is an ordinary
                // character and rewriting it would corrupt the id.
                let ts = |k: &str| s(k).map(antares_store::filter::canonical_datetime);
                let Some(instance_id) = s("instanceId") else {
                    continue; // stamped by the API layer; belt only
                };
                // observed_at falls back through the instance's own
                // timestamps — deletion instances carry only deletedAt and
                // must NOT collapse onto the epoch (retention would reap them)
                let observed = ts("observedAt")
                    .or_else(|| ts("modifiedAt"))
                    .or_else(|| ts("deletedAt"))
                    .or_else(|| ts("createdAt"))
                    .unwrap_or(std::borrow::Cow::Borrowed("1970-01-01T00:00:00Z"));
                rows.push(serde_json::json!({
                    "attr_id": attr,
                    "instance_id": instance_id,
                    "dataset_id": s("datasetId"),
                    "observed_at": observed,
                    "created_at": ts("createdAt").unwrap_or_else(|| observed.clone()),
                    "modified_at": ts("modifiedAt").unwrap_or(observed),
                    "deleted_at": ts("deletedAt"),
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
    // geo_value: extracted per instance when the value LOOKS like a GeoJSON
    // geometry; try_geomfromgeojson (0001_init.sql) maps anything PostGIS rejects to
    // NULL, which the S3 prefilter treats as "reaches the evaluator".
    sqlx::query(
        "INSERT INTO attr_instances
           (tenant_id, entity_id, attr_id, instance_id, dataset_id, observed_at,
            created_at, modified_at, deleted_at, data, geo_value)
         SELECT $1, $2, e->>'attr_id', e->>'instance_id', e->>'dataset_id',
                (e->>'observed_at')::timestamptz, (e->>'created_at')::timestamptz,
                (e->>'modified_at')::timestamptz, (e->>'deleted_at')::timestamptz,
                e->'data',
                CASE WHEN jsonb_typeof(e->'data'->'value') = 'object'
                      AND e->'data'->'value'->>'type' IN
                          ('Point','MultiPoint','LineString','MultiLineString',
                           'Polygon','MultiPolygon')
                     THEN try_geomfromgeojson((e->'data'->'value')::text) END
         FROM jsonb_array_elements($3::jsonb) AS e
         ON CONFLICT (tenant_id, entity_id, attr_id, instance_id, observed_at)
           DO UPDATE SET data = EXCLUDED.data, modified_at = EXCLUDED.modified_at,
                         dataset_id = EXCLUDED.dataset_id,
                         deleted_at = EXCLUDED.deleted_at,
                         geo_value = EXCLUDED.geo_value",
    )
    .bind(tenant.as_str())
    .bind(entity_id)
    .bind(Value::Array(rows))
    .execute(&mut *tx)
    .await?;
    Ok(())
}

// TemporalFilter lives in `store::filter`; re-exported for path compat.
pub use crate::store::filter::{TemporalFilter, TemporalOutcome};

/// The correlated subquery reconstructing the attribute object for the meta
/// row aliased `m`, with the 4.11 range and the lastN RANK() cap applied over
/// the rows (byte-exact against the API window: predicates and ordering run
/// on the instance JSON with COLLATE "C", never on the partition column).
/// Returns the SQL fragment + its text binds, numbered from `first_bind`;
/// `None` when a range is present but outside the compiler's exact subset —
/// the caller then reconstructs unpruned and the window stays the arbiter.
fn attr_object_expr(f: &TemporalFilter<'_>, first_bind: usize) -> Option<(String, Vec<String>)> {
    let (range_and, mut binds) = window_sql(f, first_bind)?;
    let tp = first_bind;
    let expr = match f.last_n {
        Some(n) => {
            let n_bind = first_bind + binds.len();
            binds.push(n.to_string());
            // 4.11 lastN keeps the N most recent INSTANTS, so the rank orders
            // on the same canonical key the window compares on — raw bytes
            // put "…00.000Z" after "…00Z" and kept the wrong N.
            let order_key =
                crate::compile::temporal::dt_key_sql(&format!("(ai.data ->> ${tp})"));
            format!(
                "COALESCE((SELECT jsonb_object_agg(g.attr_id, g.insts) FROM (\
                   SELECT s.attr_id, jsonb_agg(s.data ORDER BY s.created_at, s.observed_at, s.instance_id) AS insts \
                   FROM (SELECT ai.*, rank() OVER (PARTITION BY ai.attr_id, ai.data ->> 'datasetId' \
                             ORDER BY {order_key} DESC NULLS LAST) AS rk \
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

/// The 4.11 window over the instance rows `ai` of the meta row `m`: the
/// ` AND …` fragment plus its text binds — $first_bind is always the
/// timeproperty (predicate member / order key), the range binds follow.
/// `None` when the range is outside the compiler's exact subset.
fn window_sql(f: &TemporalFilter<'_>, first_bind: usize) -> Option<(String, Vec<String>)> {
    let mut binds = vec![f.timeproperty.to_owned()];
    let mut range_and = String::new();
    if let Some(r) = &f.range {
        let c = crate::compile::temporal::compile_instance_range(r, "ai.data", first_bind)?;
        range_and = format!(" AND {}", c.sql);
        debug_assert_eq!(c.binds[0], f.timeproperty);
        binds.extend(c.binds.into_iter().skip(1));
        // widened COLUMN bound on the SAME binds ($first_bind+1 = timeAt):
        // lets the (tenant, entity, attr, observed_at) btree serve the range;
        // the byte-exact text predicate above still decides membership
        if let Some(cb) = crate::compile::temporal::column_range_bound(r, "ai", first_bind + 1) {
            range_and.push_str(&format!(" AND {cb}"));
        }
    }
    Some((range_and, binds))
}

/// Timestamp text in the shape the API's aggregated rows carry.
const BUCKET_TS: &str = r#"to_char({} AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS"Z"')"#;

/// 4.5.19 / 5.7.4.4 aggregated temporal representation computed in SQL for
/// the meta row `m`: per attribute the bucket matrix of every requested
/// method as `[value, start, end]` rows, in one object
/// `{"bad": <any windowed value non-numeric>, "attrs": {iri: {"type":
/// "Property", method: rows…}}}`. Only the numeric class (numbers and
/// booleans, Table 4.5.19.1-1) is computed here; `bad` tells the caller to
/// reconstruct instead so the API keeps every other class and the
/// eligibility errors. Buckets: `period_secs` wide from the anchor (the
/// request's timeAt, else the attribute's first instant); no period = one
/// bucket spanning the query's whole time range (4.5.19.1 PT0S), with the
/// edge 4.11 leaves open closed by the attribute's own first or last
/// instant — the API's own rule.
fn aggregate_expr(
    f: &TemporalFilter<'_>,
    agg: &crate::store::filter::Aggregate<'_>,
    first_bind: usize,
) -> Option<(String, Vec<String>)> {
    let (range_and, mut binds) = window_sql(f, first_bind)?;
    let tp = first_bind;
    let col = match f.timeproperty {
        "observedAt" => "observed_at",
        "createdAt" => "created_at",
        "modifiedAt" => "modified_at",
        _ => return None,
    };
    let anchor = match agg.anchor {
        Some(a) => {
            binds.push(antares_store::filter::canonical_datetime(a).into_owned());
            format!("${}::timestamptz", first_bind + binds.len() - 1)
        }
        None => format!("min(ai.{col}) OVER (PARTITION BY ai.attr_id)"),
    };
    let (bs, be) = match agg.period_secs {
        None => {
            // 4.5.19.1: a zero duration "is interpreted as a duration
            // spanning the whole time range specified by the temporal
            // query". `before` names only the range's end and `after` only
            // its start (4.11), so the data closes the other edge.
            let (qs, qe) = match &f.range {
                Some(r) if r.timerel == "before" => (None, Some(r.time_at)),
                Some(r) if r.timerel == "between" => (Some(r.time_at), r.end_time_at),
                Some(r) if r.timerel == "after" => (Some(r.time_at), None),
                _ => (None, None),
            };
            let start = match qs {
                Some(v) => {
                    binds.push(antares_store::filter::canonical_datetime(v).into_owned());
                    format!("${}::timestamptz", first_bind + binds.len() - 1)
                }
                None => "s0.first_ts".to_owned(),
            };
            let end = match qe {
                Some(v) => {
                    binds.push(antares_store::filter::canonical_datetime(v).into_owned());
                    format!("${}::timestamptz", first_bind + binds.len() - 1)
                }
                None => "s0.last + interval '1 second'".to_owned(),
            };
            (start, end)
        }
        Some(sc) => {
            binds.push(sc.to_string());
            let n = first_bind + binds.len() - 1;
            let start = format!("date_bin(make_interval(secs => ${n}::bigint), s0.ts, s0.anchor)");
            (
                start.clone(),
                format!("{start} + make_interval(secs => ${n}::bigint)"),
            )
        }
    };
    let mut aggs = String::new();
    let mut rows = String::new();
    let mut pairs = String::new();
    for (i, m) in agg.methods.iter().enumerate() {
        let sql = match m.as_str() {
            "totalCount" => "count(*)",
            "distinctCount" => "count(DISTINCT s.v)",
            "min" => "min(s.v)",
            "max" => "max(s.v)",
            "sum" => "sum(s.v)",
            "avg" => "avg(s.v)",
            "stddev" => "stddev_pop(s.v)",
            "sumsq" => "sum(s.v * s.v)",
            _ => return None,
        };
        aggs.push_str(&format!(", {sql} AS m{i}"));
        rows.push_str(&format!(
            ", jsonb_agg(jsonb_build_array(g.m{i}, {}, {}) ORDER BY g.bs) AS r{i}",
            BUCKET_TS.replace("{}", "g.bs"),
            BUCKET_TS.replace("{}", "g.be")
        ));
        // method names are the allowlist above, never request text
        pairs.push_str(&format!(", '{m}', b.r{i}"));
    }
    let expr = format!(
        "(SELECT jsonb_build_object('bad', bool_or(b.bad), 'attrs', \
            jsonb_object_agg(b.attr_id, jsonb_build_object('type', 'Property'{pairs}))) \
          FROM (SELECT g.attr_id, bool_or(g.bad) AS bad{rows} \
                FROM (SELECT s.attr_id, s.bs, s.be, bool_or(s.bad) AS bad{aggs} \
                      FROM (SELECT s0.attr_id, s0.v, s0.bad, {bs} AS bs, {be} AS be \
                            FROM (SELECT ai.attr_id, \
                                   CASE WHEN jsonb_typeof(ai.data -> 'value') = 'boolean' \
                                          THEN (CASE WHEN (ai.data ->> 'value')::boolean THEN 1 ELSE 0 END)::float8 \
                                        WHEN jsonb_typeof(ai.data -> 'value') = 'number' \
                                          THEN (ai.data ->> 'value')::float8 END AS v, \
                                   jsonb_typeof(ai.data -> 'value') IS DISTINCT FROM 'number' \
                                     AND jsonb_typeof(ai.data -> 'value') IS DISTINCT FROM 'boolean' AS bad, \
                                   ai.{col} AS ts, {anchor} AS anchor, \
                                   min(ai.{col}) OVER (PARTITION BY ai.attr_id) AS first_ts, \
                                   max(ai.{col}) OVER (PARTITION BY ai.attr_id) AS last \
                                  FROM attr_instances ai \
                                  WHERE ai.tenant_id = m.tenant_id AND ai.entity_id = m.id \
                                    AND jsonb_typeof(ai.data -> ${tp}) = 'string'{range_and}) s0) s \
                      GROUP BY s.attr_id, s.bs, s.be) g \
                GROUP BY g.attr_id) b)"
    );
    Some((expr, binds))
}

impl PgTemporalStore {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            temporal_only: false,
        }
    }

    /// `false` when the id already exists (create semantics, like the memory
    /// store's `create`).
    ///
    /// 4.22: an expired one does NOT exist, so it is dropped here — history
    /// included — and the insert below is then an ordinary create. Leaving it
    /// in place would make `create` report a conflict for an id every read
    /// calls absent, and `AnyStore::upsert` reads that `false` as "already
    /// there" and falls through to `mutate`, which refuses an expired entity
    /// too: the upsert would write nothing and report success.
    pub fn create(&self, tenant: &TenantId, id: &str, doc: &Value) -> Result<bool, sqlx::Error> {
        let (types, scopes, created, modified) = extract(doc);
        let meta = meta_of(doc);
        let rows = decompose(doc);
        wait(async {
            let mut tx = self.pool.begin().await?;
            crate::store::pg::set_tenant(&mut tx, tenant).await?;
            let reaped = sqlx::query(sqlx::AssertSqlSafe(format!(
                "DELETE FROM temporal_entities m \
                 WHERE m.tenant_id = $1 AND m.id = $2 AND NOT {NOT_EXPIRED}"
            )))
            .bind(tenant.as_str())
            .bind(id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            if reaped == 1 {
                sqlx::query("DELETE FROM attr_instances WHERE tenant_id = $1 AND entity_id = $2")
                    .bind(tenant.as_str())
                    .bind(id)
                    .execute(&mut *tx)
                    .await?;
            }
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
    ///
    /// Conditional on the entity still existing. 5.6.6 Delete Entity removes
    /// the entity and the temporal evolution recorded for it, in that order and
    /// in two transactions; an auto-recording append that overlaps the delete
    /// would otherwise commit history for an entity that is gone, and no later
    /// delete would ever clean it. The `FOR KEY SHARE` lock is what makes the
    /// check hold: it lets concurrent updates of the same entity through and
    /// makes a concurrent DELETE wait until this append has committed.
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
            crate::store::pg::set_tenant(&mut tx, tenant).await?;
            if !self.temporal_only {
                let live = sqlx::query(
                    "SELECT 1 FROM entities WHERE tenant_id = $1 AND id = $2 FOR KEY SHARE",
                )
                .bind(tenant.as_str())
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?;
                if live.is_none() {
                    tx.commit().await?;
                    return Ok(());
                }
            }
            // DO NOTHING froze types/scopes at first touch — an
            // entity gaining a type stayed invisible to type-filtered
            // temporal queries forever. The shell carries the CURRENT
            // entity, so refresh on change; the IS DISTINCT FROM guard keeps
            // the common no-change append from churning the meta row.
            sqlx::query(
                "INSERT INTO temporal_entities
                   (tenant_id, id, types, scopes, meta, created_at, modified_at)
                 VALUES ($1, $2, $3, $4, $5, $6::timestamptz, $7::timestamptz)
                 ON CONFLICT (tenant_id, id) DO UPDATE SET
                   types = EXCLUDED.types,
                   scopes = EXCLUDED.scopes,
                   meta = EXCLUDED.meta,
                   modified_at = EXCLUDED.modified_at
                 WHERE temporal_entities.types IS DISTINCT FROM EXCLUDED.types
                    OR temporal_entities.scopes IS DISTINCT FROM EXCLUDED.scopes
                    OR temporal_entities.meta IS DISTINCT FROM EXCLUDED.meta",
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

    /// 5.6.16 Delete Temporal Evolution: `true` when it was there, `false`
    /// for "no existing Entity whose id (URI) is equivalent held locally",
    /// which the caller answers as ResourceNotFound. 4.22 decides what "held
    /// locally" means, the same way `get_range` and `query` decide it.
    pub fn delete(&self, tenant: &TenantId, id: &str) -> Result<bool, sqlx::Error> {
        wait(async {
            let mut tx = self.pool.begin().await?;
            crate::store::pg::set_tenant(&mut tx, tenant).await?;
            let n = sqlx::query(sqlx::AssertSqlSafe(format!(
                "DELETE FROM temporal_entities m \
                 WHERE m.tenant_id = $1 AND m.id = $2 AND {NOT_EXPIRED}"
            )))
            .bind(tenant.as_str())
            .bind(id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            // no FK: a partitioned table cannot be the referencing side of a
            // cascade from temporal_entities — clean the instances explicitly,
            // and only for the row this call actually removed. An expired
            // entity is refused above and keeps its history until the 4.22
            // reap or a create replaces it; wiping it here would destroy the
            // history behind a 404.
            if n == 1 {
                sqlx::query("DELETE FROM attr_instances WHERE tenant_id = $1 AND entity_id = $2")
                    .bind(tenant.as_str())
                    .bind(id)
                    .execute(&mut *tx)
                    .await?;
            }
            tx.commit().await?;
            Ok(n == 1)
        })
    }

    /// Temporal query: entity narrowing (ids/types/attrs) in the WHERE,
    /// instance pruning (range + lastN cap) in the reconstruction, and —
    /// when the caller passes a page — entity qualification + LIMIT/OFFSET
    /// in SQL, so a temporal query no longer materializes the whole tenant.
    pub fn query(
        &self,
        tenant: &TenantId,
        f: &TemporalFilter<'_>,
    ) -> Result<TemporalOutcome, sqlx::Error> {
        self.query_inner(tenant, f, f.aggregate.is_some())
    }

    /// `push_agg`: compute the filter's 4.5.19 aggregation in SQL; a row
    /// whose windowed values are not all numeric makes the whole query fall
    /// back to instance reconstruction (one extra round trip, only then).
    fn query_inner(
        &self,
        tenant: &TenantId,
        f: &TemporalFilter<'_>,
        push_agg: bool,
    ) -> Result<TemporalOutcome, sqlx::Error> {
        enum B {
            Text(String),
            Arr(Vec<String>),
            Num(i64),
            Float(f64),
        }
        let mut binds: Vec<B> = vec![B::Text(tenant.as_str().to_owned())];
        // 4.22: an expired ENTITY is invalid on temporal reads too — filter it
        // in SQL (no bind) so paging/totals stay exact. Expired instances are
        // stripped at the read boundary (any.rs). Literal, applies to the
        // fallback count query too.
        let mut wheres = vec!["m.tenant_id = $1".to_owned(), NOT_EXPIRED.to_owned()];
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
        // None: no range to compile. Some(None): a range that does not
        // compile, which is the case page pushdown must not take — the SQL
        // would qualify entities on a window it cannot express.
        let compiled_range = f.range.as_ref().map(|r| {
            crate::compile::temporal::compile_instance_range(r, "ai.data", binds.len() + 1)
        });
        let mut paged = false;
        if f.page.is_some() && !matches!(compiled_range, Some(None)) {
            let mut qual = "EXISTS (SELECT 1 FROM attr_instances ai WHERE \
                            ai.tenant_id = m.tenant_id AND ai.entity_id = m.id"
                .to_owned();
            if let (Some(r), Some(Some(c))) = (&f.range, compiled_range) {
                let n = binds.len() + 1;
                for b in c.binds {
                    binds.push(B::Text(b));
                }
                qual.push_str(&format!(" AND {}", c.sql));
                // index-serving widened bound on the same binds ($n+1 = timeAt)
                if let Some(cb) = crate::compile::temporal::column_range_bound(r, "ai", n + 1) {
                    qual.push_str(&format!(" AND {cb}"));
                }
            }
            qual.push(')');
            wheres.push(qual);
            paged = true;
        }
        // 5.7.4.4 S2 superset prefilter: entities with no windowed instance
        // satisfying the compilable part of q= are never reconstructed. The
        // API arbiter re-evaluates q on every row that comes back, so this
        // can only narrow (compile::qprefilter invariant), never decide.
        if let Some(qn) = f.q {
            if let Some(c) = crate::compile::qprefilter::compile_prefilter(
                qn,
                f.range.as_ref(),
                "m",
                binds.len() + 1,
                f.expand,
            ) {
                for b in c.binds {
                    binds.push(B::Text(b));
                }
                wheres.push(c.sql);
            }
        }
        // 5.7.4.4 S3 superset prefilter: entities with no windowed instance
        // of the geoproperty possibly satisfying the geoquery are never
        // reconstructed. NULL geo_value (rows with no extracted geometry) always
        // survives; GeoQuery::matches stays the arbiter.
        if let Some((spec, iri)) = f.geo {
            let attr_bind = binds.len() + 1;
            let mut win_binds: Vec<String> = Vec::new();
            let mut window = String::new();
            if let Some(r) = &f.range {
                if let Some(cb) =
                    crate::compile::temporal::column_range_bound(r, "gi", attr_bind + 1)
                {
                    win_binds.push(r.time_at.to_owned());
                    if r.timerel == "between" {
                        if let Some(e) = r.end_time_at {
                            win_binds.push(e.to_owned());
                        }
                    }
                    window = format!(" AND {cb}");
                }
            }
            if let Some(c) = crate::compile::geo::compile_geo_instance(
                spec,
                "gi.geo_value",
                attr_bind + 1 + win_binds.len(),
            ) {
                binds.push(B::Text(iri.to_owned()));
                for b in win_binds {
                    binds.push(B::Text(b));
                }
                for b in c.geo_binds {
                    binds.push(B::Text(b));
                }
                for n in c.num_binds {
                    binds.push(B::Float(n));
                }
                wheres.push(format!(
                    "EXISTS (SELECT 1 FROM attr_instances gi \
                     WHERE gi.tenant_id = m.tenant_id AND gi.entity_id = m.id \
                     AND gi.attr_id = ${attr_bind}{window} AND {})",
                    c.sql
                ));
            }
        }
        let n_where = binds.len();
        let where_sql = wheres.join(" AND ");
        let agg = if push_agg {
            f.aggregate
                .as_ref()
                .and_then(|a| aggregate_expr(f, a, n_where + 1))
        } else {
            None
        };
        let aggregated = agg.is_some();
        let (attr_expr, extra) = match agg {
            Some((e, b)) => (format!("jsonb_build_object('$agg', {e})"), b),
            None => match attr_object_expr(f, n_where + 1) {
                Some(v) => v,
                // refused range shape: reconstruct unpruned, window arbitrates
                None => attr_object_expr(&TemporalFilter::default(), n_where + 1)
                    .expect("no range/lastN always compiles"),
            },
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
        } else {
            // No page pushed down: the caller still has to filter, so the only
            // bound on this statement is the safety ceiling — without it a
            // bare `?timerel=…` reconstructs every temporal entity of the
            // tenant into one Vec.
            binds.push(B::Num(super::entity::MAX_UNDECIDED_ROWS));
            tail.push_str(&format!(" LIMIT ${}", binds.len()));
        }
        let sql = format!(
            "SELECT m.meta || {attr_expr}{select_total} FROM temporal_entities m \
             WHERE {where_sql}{tail}"
        );
        wait(async {
            let mut tx = self.pool.begin().await?;
            crate::store::pg::set_tenant(&mut tx, tenant).await?;
            // `sql` is compiler literals + $n placeholders only.
            let mut qy = sqlx::query(sqlx::AssertSqlSafe(sql.clone()));
            for b in &binds {
                qy = match b {
                    B::Text(s) => qy.bind(s),
                    B::Arr(v) => qy.bind(v),
                    B::Num(n) => qy.bind(n),
                    B::Float(x) => qy.bind(x),
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
                        B::Float(x) => cq.bind(x),
                    };
                }
                total = Some(cq.fetch_one(&mut *tx).await?);
            }
            tx.commit().await?;
            super::entity::check_ceiling(paged, rows.len(), super::entity::MAX_UNDECIDED_ROWS)?;
            let mut docs: Vec<Value> = rows.into_iter().map(|r| r.get::<Value, _>(0)).collect();
            if aggregated {
                for d in &mut docs {
                    let Some(o) = d.as_object_mut() else { continue };
                    let Some(agg) = o.remove("$agg") else {
                        continue;
                    };
                    if agg.get("bad").and_then(Value::as_bool) == Some(true) {
                        return Ok(None);
                    }
                    if let Some(attrs) = agg.get("attrs").and_then(Value::as_object) {
                        for (k, v) in attrs {
                            o.insert(k.clone(), v.clone());
                        }
                    }
                }
            }
            Ok(Some(TemporalOutcome {
                rows: docs,
                paged,
                total,
                aggregated,
            }))
        })
        .and_then(|out| match out {
            Some(out) => Ok(out),
            None => self.query_inner(tenant, f, false),
        })
    }

    /// Single-entity fetch with the same instance pruning (Retrieve
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
        // 4.22: an expired entity is invalid → None (404), same as get().
        let sql = format!(
            "SELECT m.meta || {attr_expr} FROM temporal_entities m \
             WHERE m.tenant_id = $1 AND m.id = $2 AND {NOT_EXPIRED}"
        );
        wait(async {
            let mut tx = self.pool.begin().await?;
            crate::store::pg::set_tenant(&mut tx, tenant).await?;
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
            crate::store::pg::set_tenant(&mut tx, tenant).await?;
            // the meta row is the serialization point (FOR UPDATE). 4.22
            // qualifies it: 5.6.12 to 5.6.15 reach the history through here,
            // and none of them may modify an entity every read calls absent.
            let row = sqlx::query(sqlx::AssertSqlSafe(format!(
                "SELECT m.meta FROM temporal_entities m \
                 WHERE m.tenant_id = $1 AND m.id = $2 AND {NOT_EXPIRED} FOR UPDATE"
            )))
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
