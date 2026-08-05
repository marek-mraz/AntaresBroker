//! PgStore, first slice (tasks.md C5): entity CRUD over the §8.1 `entities`
//! table. Sync facade — same signatures as the in-memory `Store`, sqlx driven
//! internally via `block_in_place` + `Handle::block_on`, so the 63 existing
//! call sites in `antares-api` never change when the cutover (C13) lands.
//!
//! Extracted columns are computed in Rust at write time (§4 — no triggers):
//! `types`, `scopes`, `created_at`, `modified_at`, `expires_at` and (C11b)
//! `location`, the default GeoProperty, converted by PostGIS itself from
//! bound GeoJSON text (`ponytail:` `ST_GeomFromGeoJSON` over a geozero
//! dependency — the DB already owns the conversion, and the value still
//! travels as a bind).

use antares_model::TenantId;
use serde_json::Value;
use sqlx::postgres::PgPool;
use sqlx::Row;

pub struct PgEntityStore {
    pool: PgPool,
    /// F3: when on, every entity write enqueues its change event into the
    /// outbox INSIDE the write transaction (§10 — a crash between commit and
    /// publish can never lose an event). Off by default: with `bus = local`
    /// events flow through the in-process hook and undrained rows would only
    /// grow the table (R4). The broker turns this on when `bus = nats`.
    outbox: std::sync::atomic::AtomicBool,
}

/// What a Query Entities call can push into SQL (C10). Every member is
/// optional and every omission is safe: a filter left out only widens the
/// result set, and the caller filters exactly afterwards.
pub struct EntityFilter<'a> {
    /// exact entity ids (`id=` / the ids of a batch query)
    pub ids: Option<&'a [&'a str]>,
    /// Entity Type Selection (4.17) as OR-of-AND groups, expanded IRIs
    pub types: Option<&'a [Vec<String>]>,
    /// `attrs=`: the entity must carry at least one, expanded IRIs
    pub attrs: Option<&'a [String]>,
    /// `q=` AST; compiled when its shape is exactly reproducible, else skipped
    pub q: Option<&'a antares_ql::QNode>,
    /// `scopeQ=` verbatim (4.19); compiled over the `scopes` column (C11)
    pub scope_q: Option<&'a str>,
    /// `georel`/`geometry`/`coordinates`/`geoproperty` (4.10), compiled over
    /// the extracted `location` column (C11b)
    pub geo: Option<&'a crate::compile::geo::GeoSpec<'a>>,
    /// term → IRI, the request context's expander (the AST holds terms)
    pub expand: &'a dyn Fn(&str) -> String,
    /// C11 pagination pushdown: applied ONLY when every present predicate
    /// compiled exactly (`decided`) — otherwise the caller's evaluator still
    /// has rows to drop and a SQL LIMIT would page over the wrong set. The
    /// caller passes it only when its own store-invisible filters (idPattern,
    /// federation, orderBy) are absent.
    pub page: Option<Page>,
    /// C11 projection pushdown (4.21 `pick`, top-level): keep these expanded
    /// attr IRIs + every non-attribute member. Applied only when `decided` —
    /// a projected doc can no longer answer a q= re-check.
    pub keep_attrs: Option<&'a [String]>,
    /// C11 projection pushdown (`omit`, top-level entries only): drop exactly
    /// these attr IRIs. Same `decided` gate.
    pub drop_attrs: Option<&'a [String]>,
}

/// One page: OFFSET/LIMIT in row units, ORDER BY id (the store's stable
/// default order, same as the memory snapshot).
pub struct Page {
    pub offset: i64,
    pub limit: i64,
}

/// What `query` produced. `decided` = SQL applied every present predicate
/// exactly, so re-evaluation cannot drop a row; `paged` = LIMIT/OFFSET
/// happened in SQL (implies `decided`), `total` = the pre-LIMIT match count.
pub struct QueryOutcome {
    pub rows: Vec<Value>,
    pub decided: bool,
    pub paged: bool,
    pub total: Option<i64>,
}

impl Default for EntityFilter<'_> {
    fn default() -> Self {
        Self {
            ids: None,
            types: None,
            attrs: None,
            q: None,
            scope_q: None,
            geo: None,
            expand: &|t: &str| t.to_owned(),
            page: None,
            keep_attrs: None,
            drop_attrs: None,
        }
    }
}

/// A bound value. Enumerated because the bind list is built dynamically while
/// the SQL is assembled — the alternative is string interpolation (§16.2: no).
enum Bind {
    Text(String),
    TextArr(Vec<String>),
    /// jsonpath; bound as text and cast with `$n::jsonpath` in the SQL
    Path(String),
    /// a distance in metres (C11b `near`)
    Num(f64),
    /// LIMIT/OFFSET (C11 pagination pushdown)
    Int(i64),
}

/// Run an async block from sync code without stalling a tokio worker
/// (same rationale as the redb shadow's `on_blocking`, B1b).
pub(crate) fn wait<T>(fut: impl std::future::Future<Output = T>) -> T {
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
pub(crate) struct Extracted {
    types: Vec<String>,
    scopes: Option<Vec<String>>,
    created: String,
    modified: String,
    expires: Option<String>,
    /// C11b: the default GeoProperty as GeoJSON text, for `ST_GeomFromGeoJSON`.
    /// `None` (→ SQL NULL) whenever it cannot be represented as ONE geometry —
    /// the compiled geoquery guards on `location IS NULL` so those rows still
    /// reach the evaluator.
    location: Option<String>,
}

fn extract(doc: &Value) -> Extracted {
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
    let now = || "1970-01-01T00:00:00Z".to_owned(); // caller always stamps; belt only
    Extracted {
        types: doc.get("type").map(&as_vec).unwrap_or_default(),
        scopes: doc.get("scope").map(&as_vec),
        created: ts("createdAt").unwrap_or_else(now),
        modified: ts("modifiedAt").unwrap_or_else(now),
        expires: ts("expiresAt"),
        location: crate::compile::geo::extract_location(doc),
    }
}

/// The outbox row's event JSON (F2/F3): what the drain turns into a
/// `ChangeEvent`. Field names are the bus crate's serde names; `seq` and the
/// claim check are the drain's business.
#[allow(clippy::too_many_arguments)]
async fn enqueue_change(
    tx: &mut sqlx::postgres::PgConnection,
    tenant: &TenantId,
    op: &str,
    id: &str,
    types: &[String],
    prev: Option<&Value>,
    next: Option<&Value>,
    version: i64,
    incarnation: &str,
) -> Result<(), sqlx::Error> {
    let ev = serde_json::json!({
        "tenant": tenant.as_str(),
        "entity_id": id,
        "types": types,
        "op": op,
        "changed_attrs": [],
        "payload": next,
        "prev_payload": prev,
        "version": version,
        "incarnation": incarnation,
    });
    super::outbox::enqueue(tx, tenant, &ev).await.map(|_| ())
}

impl PgEntityStore {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            outbox: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// F3 producer switch — the broker enables this exactly when `bus=nats`.
    pub fn set_outbox(&self, on: bool) {
        self.outbox.store(on, std::sync::atomic::Ordering::Relaxed);
    }

    fn outbox_on(&self) -> bool {
        self.outbox.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// 5.6.1-shaped create: `false` when the id already exists (→ 409).
    pub fn create(&self, tenant: &TenantId, id: &str, doc: &Value) -> Result<bool, sqlx::Error> {
        let e = extract(doc);
        wait(async {
            let mut tx = self.pool.begin().await?;
            crate::pg::set_tenant(&mut tx, tenant).await?;
            let done = sqlx::query(
                "INSERT INTO entities
                   (tenant_id, id, entity, types, scopes, created_at, modified_at, expires_at,
                    location)
                 VALUES ($1, $2, $3, $4, $5, $6::timestamptz, $7::timestamptz, $8::timestamptz,
                         CASE WHEN ST_IsValid(ST_SetSRID(ST_GeomFromGeoJSON($9), 4326))
                              THEN ST_SetSRID(ST_GeomFromGeoJSON($9), 4326) END)
                 ON CONFLICT (tenant_id, id) DO NOTHING",
            )
            .bind(tenant.as_str())
            .bind(id)
            .bind(doc)
            .bind(&e.types)
            .bind(&e.scopes)
            .bind(&e.created)
            .bind(&e.modified)
            .bind(&e.expires)
            .bind(&e.location)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            if done == 1 && self.outbox_on() {
                enqueue_change(
                    &mut tx,
                    tenant,
                    "create",
                    id,
                    &e.types,
                    None,
                    Some(doc),
                    1,
                    &e.created,
                )
                .await?;
            }
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
            let row = sqlx::query(
                "DELETE FROM entities WHERE tenant_id = $1 AND id = $2
                 RETURNING entity, types, version, created_at::text",
            )
            .bind(tenant.as_str())
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?;
            if let Some(r) = &row {
                if self.outbox_on() {
                    let prev: Value = r.get(0);
                    let types: Vec<String> = r.get(1);
                    enqueue_change(
                        &mut tx,
                        tenant,
                        "delete",
                        id,
                        &types,
                        Some(&prev),
                        None,
                        r.get::<i64, _>(2),
                        r.get::<&str, _>(3),
                    )
                    .await?;
                }
            }
            tx.commit().await?;
            Ok(row.is_some())
        })
    }

    /// C10 query pushdown. The predicates that compile EXACTLY go to
    /// Postgres; everything else is simply left out of the WHERE clause, so
    /// the result is always a superset of the answer and the caller's
    /// in-memory evaluator remains the arbiter. That is the property that
    /// makes store modes agree: SQL removes rows, it never decides them.
    ///
    /// §16.2: every value here is a bind. The only text this function
    /// concatenates is its own operators and `$n` placeholders.
    pub fn query(
        &self,
        tenant: &TenantId,
        f: &EntityFilter<'_>,
    ) -> Result<QueryOutcome, sqlx::Error> {
        let mut binds: Vec<Bind> = vec![Bind::Text(tenant.as_str().to_owned())];
        let mut wheres = vec!["tenant_id = $1".to_owned()];
        // C11 exactness ledger: ids/types/attrs translate exactly by
        // construction; q is exact IF it compiles (the compiler's contract);
        // scopeQ is documented loose-or-equal, geo has a metric residual
        // (`near` geography vs haversine) — both therefore forfeit
        // `decided`, they only narrow.
        let mut decided = true;

        if let Some(ids) = f.ids {
            binds.push(Bind::TextArr(ids.iter().map(|s| s.to_string()).collect()));
            wheres.push(format!("id = ANY(${})", binds.len()));
        }
        // OR of AND-groups, mirroring the Entity Type Selection Language
        // (4.17) the caller already parsed: `types @> ARRAY[…]` per group.
        if let Some(groups) = f.types {
            let mut ors = Vec::with_capacity(groups.len());
            for g in groups {
                binds.push(Bind::TextArr(g.clone()));
                ors.push(format!("types @> ${}", binds.len()));
            }
            if !ors.is_empty() {
                wheres.push(format!("({})", ors.join(" OR ")));
            }
        }
        // `attrs`: the entity carries at least one of them — jsonb `?|`,
        // exactly the evaluator's `any(|a| doc.get(a).is_some())`.
        if let Some(attrs) = f.attrs {
            binds.push(Bind::TextArr(attrs.to_vec()));
            wheres.push(format!("entity ?| ${}", binds.len()));
        }
        if let Some(node) = f.q {
            match crate::compile::q::compile_q(node, "entity", binds.len() + 1, f.expand) {
                Some(c) => {
                    wheres.push(c.sql);
                    binds.extend(c.binds.into_iter().map(Bind::Path));
                }
                None => decided = false,
            }
        }
        if let Some(sq) = f.scope_q {
            decided = false;
            if let Some(c) = crate::compile::scope::compile_scope_q(sq, "scopes", binds.len() + 1) {
                wheres.push(c.sql);
                binds.extend(c.binds.into_iter().map(Bind::Text));
            }
        }
        if let Some(spec) = f.geo {
            decided = false;
            // A client may send a self-intersecting polygon. GEOS raises on
            // one (`side location conflict`), which would turn a query into a
            // 500 in `postgres` mode while `memory` mode answers happily from
            // the evaluator. Probing validity once here keeps the two modes
            // identical: invalid ⇒ no pushdown, evaluator decides. Stored
            // geometries can't be invalid — the write path NULLs those.
            if self.geometry_is_valid(spec).unwrap_or(false) {
                if let Some(c) = crate::compile::geo::compile_geo(spec, "location", binds.len() + 1)
                {
                    wheres.push(c.sql);
                    // geo binds first, then the numeric ones — the order
                    // `compile_geo` numbered its placeholders in.
                    binds.extend(c.geo_binds.into_iter().map(Bind::Text));
                    binds.extend(c.num_binds.into_iter().map(Bind::Num));
                }
            }
        }

        // every bind up to here belongs to the WHERE clause — the count-only
        // fallback statement below reuses exactly this prefix
        let where_binds = binds.len();

        // C11 projection pushdown, only once SQL decides row membership: the
        // kept doc must only need to feed `repr::apply`, never a re-check.
        // `pick` keeps listed attrs + every non-attribute member (attribute
        // keys are expanded IRIs — `http…` — so core members never match the
        // LIKE and always survive; a non-http attr IRI merely stays
        // unprojected, which is the safe direction). `omit` drops exactly the
        // listed top-level IRIs.
        let mut select = "entity".to_owned();
        if decided {
            if let Some(keep) = f.keep_attrs {
                binds.push(Bind::TextArr(keep.to_vec()));
                select = format!(
                    "(SELECT COALESCE(jsonb_object_agg(t.k, t.v), '{{}}'::jsonb) \
                      FROM jsonb_each(entity) AS t(k, v) \
                      WHERE t.k NOT LIKE 'http%' OR t.k = ANY(${}))",
                    binds.len()
                );
            } else if let Some(drop) = f.drop_attrs {
                binds.push(Bind::TextArr(drop.to_vec()));
                select = format!(
                    "(SELECT COALESCE(jsonb_object_agg(t.k, t.v), '{{}}'::jsonb) \
                      FROM jsonb_each(entity) AS t(k, v) \
                      WHERE NOT (t.k = ANY(${})))",
                    binds.len()
                );
            }
        }

        // C11 pagination pushdown: ORDER BY id is the store's default order
        // either way; `count(*) OVER ()` rides the same statement so the
        // caller gets the pre-LIMIT total for count= and the next/prev links.
        let paged = decided && f.page.is_some();
        let sql = if let Some(p) = f.page.as_ref().filter(|_| decided) {
            binds.push(Bind::Int(p.limit));
            let lim = binds.len();
            binds.push(Bind::Int(p.offset));
            format!(
                "SELECT {select} AS entity, count(*) OVER () AS total \
                 FROM entities WHERE {} ORDER BY id LIMIT ${lim} OFFSET ${}",
                wheres.join(" AND "),
                binds.len()
            )
        } else {
            format!(
                "SELECT {select} AS entity FROM entities WHERE {} ORDER BY id",
                wheres.join(" AND ")
            )
        };
        wait(async {
            let mut tx = self.pool.begin().await?;
            crate::pg::set_tenant(&mut tx, tenant).await?;
            // sqlx 0.9 makes dynamic SQL opt-in. The assertion holds by
            // construction: `sql` is built from this function's own literals
            // plus `$n` placeholders — no caller-supplied text reaches it
            // (§16.2). The audit lives here, next to the builder.
            let mut qy = sqlx::query(sqlx::AssertSqlSafe(sql.clone()));
            for b in &binds {
                qy = match b {
                    Bind::Text(s) | Bind::Path(s) => qy.bind(s),
                    Bind::TextArr(v) => qy.bind(v),
                    Bind::Num(n) => qy.bind(n),
                    Bind::Int(n) => qy.bind(n),
                };
            }
            let rows = qy.fetch_all(&mut *tx).await?;
            let mut total = if paged {
                rows.first().map(|r| r.get::<i64, _>(1))
            } else {
                None
            };
            // an off-the-end page returns zero rows and no window total —
            // count the match set separately so links/count stay correct
            if paged && total.is_none() {
                let count_sql = format!(
                    "SELECT count(*) FROM entities WHERE {}",
                    wheres.join(" AND ")
                );
                let mut cq = sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(count_sql));
                // same wheres ⇒ same bind prefix; stop before the
                // projection/page binds, which the count statement lacks
                for b in binds.iter().take(where_binds) {
                    cq = match b {
                        Bind::Text(s) | Bind::Path(s) => cq.bind(s),
                        Bind::TextArr(v) => cq.bind(v),
                        Bind::Num(n) => cq.bind(n),
                        Bind::Int(n) => cq.bind(n),
                    };
                }
                total = Some(cq.fetch_one(&mut *tx).await?);
            }
            tx.commit().await?;
            Ok(QueryOutcome {
                rows: rows.into_iter().map(|r| r.get::<Value, _>(0)).collect(),
                decided,
                paged,
                total,
            })
        })
    }

    /// Id-ordered snapshot for one tenant (the v0 `list` shape — still the
    /// path for every non-entity kind and for callers with no filter).
    /// One cheap probe: is the client's query geometry OGC-valid? An error
    /// (unparseable GeoJSON) counts as invalid — same outcome, no pushdown.
    fn geometry_is_valid(
        &self,
        spec: &crate::compile::geo::GeoSpec<'_>,
    ) -> Result<bool, sqlx::Error> {
        let geojson = serde_json::to_string(&serde_json::json!({
            "type": spec.geometry, "coordinates": spec.coordinates
        }))
        .unwrap_or_default();
        wait(async {
            Ok(
                sqlx::query_scalar::<_, bool>("SELECT ST_IsValid(ST_GeomFromGeoJSON($1))")
                    .bind(&geojson)
                    .fetch_one(&self.pool)
                    .await
                    .unwrap_or(false),
            )
        })
    }

    pub fn list(&self, tenant: &TenantId) -> Result<Vec<Value>, sqlx::Error> {
        wait(async {
            let mut tx = self.pool.begin().await?;
            crate::pg::set_tenant(&mut tx, tenant).await?;
            let rows = sqlx::query("SELECT entity FROM entities WHERE tenant_id = $1 ORDER BY id")
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
            let before = self.outbox_on().then(|| doc.clone());
            match f(&mut doc) {
                Ok(t) => {
                    let e = extract(&doc);
                    let updated = sqlx::query(
                        "UPDATE entities SET entity = $3, types = $4, scopes = $5,
                           modified_at = $6::timestamptz, expires_at = $7::timestamptz,
                           location = CASE WHEN ST_IsValid(ST_SetSRID(ST_GeomFromGeoJSON($8), 4326))
                                           THEN ST_SetSRID(ST_GeomFromGeoJSON($8), 4326) END,
                           version = version + 1
                         WHERE tenant_id = $1 AND id = $2
                         RETURNING version, created_at::text",
                    )
                    .bind(tenant.as_str())
                    .bind(id)
                    .bind(&doc)
                    .bind(&e.types)
                    .bind(&e.scopes)
                    .bind(&e.modified)
                    .bind(&e.expires)
                    .bind(&e.location)
                    .fetch_one(&mut *tx)
                    .await?;
                    if let Some(before) = &before {
                        enqueue_change(
                            &mut tx,
                            tenant,
                            "update",
                            id,
                            &e.types,
                            Some(before),
                            Some(&doc),
                            updated.get::<i64, _>(0),
                            updated.get::<&str, _>(1),
                        )
                        .await?;
                    }
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

    /// C5 batch create: ONE multi-row INSERT for the whole batch (§4 —
    /// the jsonb elements form of UNNEST), one transaction, one commit.
    /// Returns a created-flag per input item, input order preserved.
    /// Duplicate ids within one batch are pre-deduped here: `ON CONFLICT DO
    /// NOTHING` raises "cannot affect row a second time" otherwise — the
    /// later duplicate reports `false` (5.5.11.1: the first instance wins).
    pub fn batch_create(
        &self,
        tenant: &TenantId,
        items: &[(String, Value)],
    ) -> Result<Vec<bool>, sqlx::Error> {
        let mut seen = std::collections::HashSet::new();
        let mut payload = Vec::new();
        for (id, doc) in items {
            if seen.insert(id.as_str()) {
                let e = extract(doc);
                payload.push(serde_json::json!({
                    "id": id, "doc": doc, "types": e.types, "scopes": e.scopes,
                    "created": e.created, "modified": e.modified, "expires": e.expires,
                    "location": e.location,
                }));
            }
        }
        wait(async {
            let mut tx = self.pool.begin().await?;
            crate::pg::set_tenant(&mut tx, tenant).await?;
            let rows = sqlx::query(
                "INSERT INTO entities
                   (tenant_id, id, entity, types, scopes, created_at, modified_at, expires_at,
                    location)
                 SELECT $1, e->>'id', e->'doc',
                        ARRAY(SELECT jsonb_array_elements_text(e->'types')),
                        CASE WHEN e->'scopes' = 'null'::jsonb THEN NULL
                             ELSE ARRAY(SELECT jsonb_array_elements_text(e->'scopes')) END,
                        (e->>'created')::timestamptz, (e->>'modified')::timestamptz,
                        (e->>'expires')::timestamptz,
                        CASE WHEN ST_IsValid(ST_SetSRID(ST_GeomFromGeoJSON(e->>'location'), 4326))
                             THEN ST_SetSRID(ST_GeomFromGeoJSON(e->>'location'), 4326) END
                 FROM jsonb_array_elements($2::jsonb) AS e
                 ON CONFLICT (tenant_id, id) DO NOTHING
                 RETURNING id",
            )
            .bind(tenant.as_str())
            .bind(Value::Array(payload))
            .fetch_all(&mut *tx)
            .await?;
            let created_now: std::collections::HashSet<String> =
                rows.into_iter().map(|r| r.get::<String, _>(0)).collect();
            if self.outbox_on() {
                let mut seen_ev = std::collections::HashSet::new();
                for (id, doc) in items {
                    if created_now.contains(id.as_str()) && seen_ev.insert(id.as_str()) {
                        let e = extract(doc);
                        enqueue_change(
                            &mut tx,
                            tenant,
                            "create",
                            id,
                            &e.types,
                            None,
                            Some(doc),
                            1,
                            &e.created,
                        )
                        .await?;
                    }
                }
            }
            tx.commit().await?;
            let mut created = created_now;
            // consume-once: a duplicate of a created id still reports false
            Ok(items
                .iter()
                .map(|(id, _)| created.remove(id.as_str()))
                .collect())
        })
    }

    /// C5 batch delete: ONE statement, returning each deleted row's previous
    /// document (the change-hook before-image).
    pub fn batch_delete(
        &self,
        tenant: &TenantId,
        ids: &[String],
    ) -> Result<Vec<(String, Value)>, sqlx::Error> {
        wait(async {
            let mut tx = self.pool.begin().await?;
            crate::pg::set_tenant(&mut tx, tenant).await?;
            let rows = sqlx::query(
                "DELETE FROM entities WHERE tenant_id = $1 AND id = ANY($2)
                 RETURNING id, entity, types, version, created_at::text",
            )
            .bind(tenant.as_str())
            .bind(ids)
            .fetch_all(&mut *tx)
            .await?;
            if self.outbox_on() {
                for r in &rows {
                    let id: String = r.get(0);
                    let prev: Value = r.get(1);
                    let types: Vec<String> = r.get(2);
                    enqueue_change(
                        &mut tx,
                        tenant,
                        "delete",
                        &id,
                        &types,
                        Some(&prev),
                        None,
                        r.get::<i64, _>(3),
                        r.get::<&str, _>(4),
                    )
                    .await?;
                }
            }
            tx.commit().await?;
            Ok(rows
                .into_iter()
                .map(|r| (r.get::<String, _>(0), r.get::<Value, _>(1)))
                .collect())
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
