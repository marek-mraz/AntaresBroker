//! PgStore, first slice: entity CRUD over the `entities`
//! table. Sync facade — same signatures as the in-memory `Store`, sqlx driven
//! internally via `block_in_place` + `Handle::block_on`, so the 63 existing
//! call sites in `antares-api` never change when the cutover lands.
//!
//! Extracted columns are computed in Rust at write time (no triggers):
//! `types`, `scopes`, `created_at`, `modified_at`, `expires_at` and
//! `location`, the default GeoProperty, converted by PostGIS itself from
//! bound GeoJSON text (`ST_GeomFromGeoJSON` rather than a geozero
//! dependency — the DB already owns the conversion, and the value still
//! travels as a bind).

use antares_model::{NgsiError, TenantId};
use serde_json::Value;
use sqlx::postgres::PgPool;
use sqlx::Row;

pub struct PgEntityStore {
    pool: PgPool,
    /// When on, every entity write enqueues its change event into the
    /// outbox INSIDE the write transaction (a crash between commit and
    /// publish can never lose an event). Off by default: with `bus = local`
    /// events flow through the in-process hook and undrained rows would only
    /// grow the table. The broker turns this on when `bus = nats`.
    outbox: std::sync::atomic::AtomicBool,
}

// EntityFilter/Page/QueryOutcome live in `store::filter` (pure data,
// shared with the wasm32 build); re-exported here so existing paths hold.
pub use super::filter::{EntityFilter, Page, QueryOutcome};

/// A bound value. Enumerated because the bind list is built dynamically while
/// the SQL is assembled — the alternative would be string interpolation.
enum Bind {
    Text(String),
    TextArr(Vec<String>),
    /// jsonpath; bound as text and cast with `$n::jsonpath` in the SQL
    Path(String),
    /// a distance in metres (geoquery `near`)
    Num(f64),
    /// LIMIT/OFFSET (pagination pushdown)
    Int(i64),
}

/// Run an async block from sync code without stalling a tokio worker
/// (same rationale as the redb shadow's `on_blocking`).
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

/// The internal doc's members that become extracted columns.
pub(crate) struct Extracted {
    types: Vec<String>,
    scopes: Option<Vec<String>>,
    created: String,
    modified: String,
    expires: Option<String>,
    /// The default GeoProperty as GeoJSON text, for `ST_GeomFromGeoJSON`.
    /// `None` (→ SQL NULL) whenever it cannot be represented as ONE geometry.
    location: Option<String>,
    /// True when the doc CARRIES the default GeoProperty but `location` could
    /// not be extracted (multi-instance, non-GeoJSON value). The compiled
    /// geoquery ORs on this column — index-shaped (BitmapOr), unlike the old
    /// `location IS NULL OR …` guard which forced a sequential scan — and
    /// rows without any geoproperty are excluded in SQL, which is exact.
    location_ambiguous: bool,
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
    let location = crate::compile::geo::extract_location(doc);
    let location_ambiguous =
        location.is_none() && doc.get(crate::compile::geo::LOCATION_IRI).is_some();
    Extracted {
        types: doc.get("type").map(&as_vec).unwrap_or_default(),
        scopes: doc.get("scope").map(&as_vec),
        created: ts("createdAt").unwrap_or_else(now),
        modified: ts("modifiedAt").unwrap_or_else(now),
        expires: ts("expiresAt"),
        location,
        location_ambiguous,
    }
}

/// The outbox row's event JSON: what the drain turns into a
/// `ChangeEvent`. Field names are the bus crate's serde names; `seq` and the
/// claim check are the drain's business.
#[allow(clippy::too_many_arguments)]
fn change_event(
    tenant: &TenantId,
    op: &str,
    id: &str,
    types: &[String],
    prev: Option<&Value>,
    next: Option<&Value>,
    version: i64,
    incarnation: &str,
) -> Value {
    // changed_attrs = top-level attribute IRIs that differ between the
    // before- and after-images (meta members excluded). Create lists every
    // attr, delete lists every prior attr.
    let meta = [
        "id",
        "type",
        "scope",
        "createdAt",
        "modifiedAt",
        "deletedAt",
        "expiresAt",
    ];
    fn keys<'a>(v: Option<&'a Value>, meta: &[&str]) -> Vec<&'a str> {
        v.and_then(Value::as_object)
            .map(|o| {
                o.keys()
                    .map(String::as_str)
                    .filter(|k| !meta.contains(k))
                    .collect()
            })
            .unwrap_or_default()
    }
    let mut changed: Vec<&str> = Vec::new();
    for k in keys(prev, &meta).into_iter().chain(keys(next, &meta)) {
        if !changed.contains(&k) && prev.and_then(|p| p.get(k)) != next.and_then(|n| n.get(k)) {
            changed.push(k);
        }
    }
    serde_json::json!({
        "tenant": tenant.as_str(),
        "entity_id": id,
        "types": types,
        "op": op,
        "changed_attrs": changed,
        "payload": next,
        "prev_payload": prev,
        "version": version,
        "incarnation": incarnation,
    })
}

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
    let ev = change_event(tenant, op, id, types, prev, next, version, incarnation);
    super::outbox::enqueue(tx, tenant, &ev).await.map(|_| ())
}

/// Rows one statement may materialize when SQL narrowed the match set but
/// did not decide it. 5.5.6 leaves the threshold to the implementation;
/// this is it for the entity query path, and it is the only bound a request
/// that carries no page (idPattern, federation, orderBy) has at all.
pub const MAX_UNDECIDED_ROWS: i64 = 10_000;

/// The LIMIT for a statement whose result the caller still has to filter.
/// OFFSET cannot be pushed there — the evaluator drops rows the SQL kept —
/// so the whole prefix up to the requested page has to come back, and
/// `offset + limit + 1` is exactly enough: one row past the page, so a
/// complete fetch also shows whether a further page exists (5.5.9.1). A
/// request without a page, or one whose offset reaches past the ceiling,
/// gets the ceiling.
fn undecided_ceiling(_page: Option<&Page>) -> i64 {
    // Deliberately NOT offset+limit+1. On this path the predicate was not
    // pushed down, so the rows are candidates: the caller's evaluator still
    // drops some of them. Fetching only one row past the requested page would
    // refuse every geoquery over a tenant larger than the page, because the
    // statement comes back full whenever candidates outnumber matches. The
    // ceiling exists to bound memory, not to bound the answer.
    MAX_UNDECIDED_ROWS
}

/// The statement, split out of `query` so its pagination shape is
/// assertable without a database. Either LIMIT/OFFSET over an exactly
/// decided set (`true`, `count(*) OVER ()` rides along for the pre-LIMIT
/// total) or the safety LIMIT of `undecided_ceiling` — never no LIMIT.
fn query_sql(
    select: &str,
    wheres: &[String],
    page: Option<&Page>,
    decided: bool,
    binds: &mut Vec<Bind>,
) -> (String, bool) {
    let wheres = wheres.join(" AND ");
    match page.filter(|_| decided) {
        Some(p) => {
            binds.push(Bind::Int(p.limit));
            let lim = binds.len();
            binds.push(Bind::Int(p.offset));
            (
                format!(
                    "SELECT {select} AS entity, count(*) OVER () AS total \
                     FROM entities WHERE {wheres} ORDER BY id LIMIT ${lim} OFFSET ${}",
                    binds.len()
                ),
                true,
            )
        }
        None => {
            binds.push(Bind::Int(undecided_ceiling(page)));
            (
                format!(
                    "SELECT {select} AS entity FROM entities WHERE {wheres} \
                     ORDER BY id LIMIT ${}",
                    binds.len()
                ),
                false,
            )
        }
    }
}

/// Did the safety LIMIT actually cut the match set? A statement that came
/// back full was truncated at a bound nobody chose, and the caller's
/// evaluator still has rows to drop, so no complete page can be built from
/// it. 5.5.6: "When a query operation is producing so many results that can
/// potentially exhaust client or server resources … implementations shall
/// raise an error of type TooManyResults." Serving the truncated prefix
/// would under-report the answer instead.
fn check_ceiling(paged: bool, rows: usize, ceiling: i64) -> Result<(), sqlx::Error> {
    if paged || (rows as i64) < ceiling {
        return Ok(());
    }
    Err(sqlx::Error::Configuration(Box::new(
        NgsiError::TooManyResults(format!(
            "query matched more than {ceiling} entities before filtering; \
             narrow it or request a smaller offset"
        )),
    )))
}

/// Recover a spec error the store raised itself from the driver error it
/// travels in (the store's only error channel is `sqlx::Error`). `None` =
/// a genuine driver failure, which stays an InternalError at the seam.
pub fn ngsi_error(e: &sqlx::Error) -> Option<&NgsiError> {
    match e {
        sqlx::Error::Configuration(b) => b.downcast_ref::<NgsiError>(),
        _ => None,
    }
}

impl PgEntityStore {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            outbox: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Outbox producer switch — the broker enables this exactly when `bus=nats`.
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
                    location, location_ambiguous)
                 VALUES ($1, $2, $3, $4, $5, $6::timestamptz, $7::timestamptz, $8::timestamptz,
                         CASE WHEN ST_IsValid(ST_SetSRID(ST_GeomFromGeoJSON($9), 4326))
                              THEN ST_SetSRID(ST_GeomFromGeoJSON($9), 4326) END,
                         $10 OR ($9 IS NOT NULL
                                 AND NOT ST_IsValid(ST_SetSRID(ST_GeomFromGeoJSON($9), 4326))))
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
            .bind(e.location_ambiguous)
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
            // 4.22: expired rows are invalid context until the sweep reaps them
            let row = sqlx::query(
                "SELECT entity FROM entities WHERE tenant_id = $1 AND id = $2
                   AND (expires_at IS NULL OR expires_at > now())",
            )
            .bind(tenant.as_str())
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?;
            tx.commit().await?;
            Ok(row.map(|r| r.get::<Value, _>(0)))
        })
    }

    /// Returns the deleted document (the before-image, captured in the same
    /// transaction as the DELETE — never re-read outside it), `None` = absent.
    pub fn delete(&self, tenant: &TenantId, id: &str) -> Result<Option<Value>, sqlx::Error> {
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
            let mut prev_out = None;
            if let Some(r) = &row {
                let prev: Value = r.get(0);
                if self.outbox_on() {
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
                prev_out = Some(prev);
            }
            tx.commit().await?;
            Ok(prev_out)
        })
    }

    /// Query pushdown. The predicates that compile EXACTLY go to
    /// Postgres; everything else is simply left out of the WHERE clause, so
    /// the result is always a superset of the answer and the caller's
    /// in-memory evaluator remains the arbiter. That is the property that
    /// makes store modes agree: SQL removes rows, it never decides them.
    ///
    /// Every value here is a bind. The only text this function
    /// concatenates is its own operators and `$n` placeholders.
    ///
    /// Every statement it builds is bounded: an exactly decided query by the
    /// caller's page, everything else by `undecided_ceiling`. A set that
    /// reaches that ceiling is refused with TooManyResults (5.5.6) rather
    /// than returned as a prefix the caller would page over as if complete.
    pub fn query(
        &self,
        tenant: &TenantId,
        f: &EntityFilter<'_>,
    ) -> Result<QueryOutcome, sqlx::Error> {
        let mut binds: Vec<Bind> = vec![Bind::Text(tenant.as_str().to_owned())];
        let mut wheres = vec![
            "tenant_id = $1".to_owned(),
            // 4.22: expired entities never enter a result (or a page/total)
            "(expires_at IS NULL OR expires_at > now())".to_owned(),
        ];
        // Exactness: ids/types/attrs translate exactly by
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

        // Projection pushdown, only once SQL decides row membership: the
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

        // Pagination pushdown: ORDER BY id is the store's default order
        // either way; `count(*) OVER ()` rides the same statement so the
        // caller gets the pre-LIMIT total for count= and the next/prev
        // links. When the page cannot be pushed the statement still carries
        // the safety LIMIT — without one, a single undecided request (any
        // `scopeQ`, any `georel`, a `q=` shape the compiler declined)
        // materializes the whole tenant into the Vec below.
        let ceiling = undecided_ceiling(f.page.as_ref());
        let (sql, paged) = query_sql(&select, &wheres, f.page.as_ref(), decided, &mut binds);
        wait(async {
            let mut tx = self.pool.begin().await?;
            crate::pg::set_tenant(&mut tx, tenant).await?;
            // sqlx 0.9 makes dynamic SQL opt-in. The assertion holds by
            // construction: `sql` is built from this function's own literals
            // plus `$n` placeholders — no caller-supplied text reaches it.
            // The audit lives here, next to the builder.
            let mut qy = sqlx::query(sqlx::AssertSqlSafe(sql.clone()));
            for b in &binds {
                qy = match b {
                    Bind::Text(s) | Bind::Path(s) => qy.bind(s),
                    Bind::TextArr(v) => qy.bind(v),
                    Bind::Num(n) => qy.bind(n),
                    Bind::Int(n) => qy.bind(n),
                };
            }
            // Stream rows, decode each to its Value and
            // drop the PgRow — the full row set never sits in memory twice.
            let mut docs: Vec<Value> = Vec::new();
            let mut total: Option<i64> = None;
            {
                use futures_util::TryStreamExt;
                let mut stream = qy.fetch(&mut *tx);
                while let Some(row) = stream.try_next().await? {
                    if paged && docs.is_empty() {
                        total = Some(row.get::<i64, _>(1));
                    }
                    docs.push(row.get::<Value, _>(0));
                }
            }
            check_ceiling(paged, docs.len(), ceiling)?;
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
                rows: docs,
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
            let mut docs: Vec<Value> = Vec::new();
            {
                use futures_util::TryStreamExt;
                let mut stream = sqlx::query(
                    "SELECT entity FROM entities WHERE tenant_id = $1
                           AND (expires_at IS NULL OR expires_at > now()) ORDER BY id",
                )
                .bind(tenant.as_str())
                .fetch(&mut *tx);
                while let Some(row) = stream.try_next().await? {
                    docs.push(row.get::<Value, _>(0));
                }
            }
            tx.commit().await?;
            Ok(docs)
        })
    }

    /// Read-modify-write: row lock via `SELECT … FOR UPDATE`, closure
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
                           location_ambiguous = $9 OR ($8 IS NOT NULL
                               AND NOT ST_IsValid(ST_SetSRID(ST_GeomFromGeoJSON($8), 4326))),
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
                    .bind(e.location_ambiguous)
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

    /// Batch create: ONE multi-row INSERT for the whole batch (the jsonb
    /// elements form of UNNEST), one transaction, one commit.
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
                    "location": e.location, "loc_ambiguous": e.location_ambiguous,
                }));
            }
        }
        wait(async {
            let mut tx = self.pool.begin().await?;
            crate::pg::set_tenant(&mut tx, tenant).await?;
            let rows = sqlx::query(
                "INSERT INTO entities
                   (tenant_id, id, entity, types, scopes, created_at, modified_at, expires_at,
                    location, location_ambiguous)
                 SELECT $1, e->>'id', e->'doc',
                        ARRAY(SELECT jsonb_array_elements_text(e->'types')),
                        CASE WHEN e->'scopes' = 'null'::jsonb THEN NULL
                             ELSE ARRAY(SELECT jsonb_array_elements_text(e->'scopes')) END,
                        (e->>'created')::timestamptz, (e->>'modified')::timestamptz,
                        (e->>'expires')::timestamptz,
                        CASE WHEN ST_IsValid(ST_SetSRID(ST_GeomFromGeoJSON(e->>'location'), 4326))
                             THEN ST_SetSRID(ST_GeomFromGeoJSON(e->>'location'), 4326) END,
                        COALESCE((e->>'loc_ambiguous')::bool, false)
                          OR (e->>'location' IS NOT NULL
                              AND NOT ST_IsValid(ST_SetSRID(ST_GeomFromGeoJSON(e->>'location'), 4326)))
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
                let mut events = Vec::new();
                for (id, doc) in items {
                    if created_now.contains(id.as_str()) && seen_ev.insert(id.as_str()) {
                        let e = extract(doc);
                        events.push(change_event(
                            tenant,
                            "create",
                            id,
                            &e.types,
                            None,
                            Some(doc),
                            1,
                            &e.created,
                        ));
                    }
                }
                super::outbox::enqueue_many(&mut tx, tenant, &events).await?;
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

    /// Batch delete: ONE statement, returning each deleted row's previous
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
                let events: Vec<Value> = rows
                    .iter()
                    .map(|r| {
                        let id: String = r.get(0);
                        let prev: Value = r.get(1);
                        let types: Vec<String> = r.get(2);
                        change_event(
                            tenant,
                            "delete",
                            &id,
                            &types,
                            Some(&prev),
                            None,
                            r.get::<i64, _>(3),
                            r.get::<&str, _>(4),
                        )
                    })
                    .collect();
                super::outbox::enqueue_many(&mut tx, tenant, &events).await?;
            }
            tx.commit().await?;
            Ok(rows
                .into_iter()
                .map(|r| (r.get::<String, _>(0), r.get::<Value, _>(1)))
                .collect())
        })
    }

    /// Batch upsert in REPLACE semantics — ONE
    /// `INSERT … ON CONFLICT DO UPDATE` for the whole batch, one transaction.
    /// Returns a created-flag per input item (input order; duplicates of one
    /// id report the first outcome). Before-images for the change events are
    /// captured by a single `FOR UPDATE` select in the same transaction.
    /// Returns per input item: (created?, before-image) — the before-image is
    /// for the caller's change hook and comes from the same-tx FOR UPDATE.
    pub fn batch_upsert_replace(
        &self,
        tenant: &TenantId,
        items: &[(String, Value)],
    ) -> Result<Vec<(bool, Option<Value>)>, sqlx::Error> {
        let mut seen = std::collections::HashSet::new();
        let mut payload = Vec::new();
        let mut ids = Vec::new();
        for (id, doc) in items {
            if seen.insert(id.as_str()) {
                let e = extract(doc);
                ids.push(id.clone());
                payload.push(serde_json::json!({
                    "id": id, "doc": doc, "types": e.types, "scopes": e.scopes,
                    "created": e.created, "modified": e.modified, "expires": e.expires,
                    "location": e.location, "loc_ambiguous": e.location_ambiguous,
                }));
            }
        }
        wait(async {
            let mut tx = self.pool.begin().await?;
            crate::pg::set_tenant(&mut tx, tenant).await?;
            // lock + before-images in one statement (ordered: stable lock order)
            let prev_rows = sqlx::query(
                "SELECT id, entity FROM entities WHERE tenant_id = $1 AND id = ANY($2)
                 ORDER BY id FOR UPDATE",
            )
            .bind(tenant.as_str())
            .bind(&ids)
            .fetch_all(&mut *tx)
            .await?;
            let prevs: std::collections::HashMap<String, Value> = prev_rows
                .into_iter()
                .map(|r| (r.get::<String, _>(0), r.get::<Value, _>(1)))
                .collect();
            let rows = sqlx::query(
                "INSERT INTO entities
                   (tenant_id, id, entity, types, scopes, created_at, modified_at, expires_at,
                    location, location_ambiguous)
                 SELECT $1, e->>'id', e->'doc',
                        ARRAY(SELECT jsonb_array_elements_text(e->'types')),
                        CASE WHEN e->'scopes' = 'null'::jsonb THEN NULL
                             ELSE ARRAY(SELECT jsonb_array_elements_text(e->'scopes')) END,
                        (e->>'created')::timestamptz, (e->>'modified')::timestamptz,
                        (e->>'expires')::timestamptz,
                        CASE WHEN ST_IsValid(ST_SetSRID(ST_GeomFromGeoJSON(e->>'location'), 4326))
                             THEN ST_SetSRID(ST_GeomFromGeoJSON(e->>'location'), 4326) END,
                        COALESCE((e->>'loc_ambiguous')::bool, false)
                          OR (e->>'location' IS NOT NULL
                              AND NOT ST_IsValid(ST_SetSRID(ST_GeomFromGeoJSON(e->>'location'), 4326)))
                 FROM jsonb_array_elements($2::jsonb) AS e
                 ON CONFLICT (tenant_id, id) DO UPDATE SET
                        entity = EXCLUDED.entity, types = EXCLUDED.types,
                        scopes = EXCLUDED.scopes, modified_at = EXCLUDED.modified_at,
                        expires_at = EXCLUDED.expires_at, location = EXCLUDED.location,
                        location_ambiguous = EXCLUDED.location_ambiguous,
                        version = entities.version + 1
                 RETURNING id, (xmax = 0) AS inserted, version, created_at::text",
            )
            .bind(tenant.as_str())
            .bind(Value::Array(payload))
            .fetch_all(&mut *tx)
            .await?;
            let mut created: std::collections::HashMap<String, bool> =
                std::collections::HashMap::new();
            let mut events = Vec::new();
            for r in &rows {
                let id: String = r.get(0);
                let inserted: bool = r.get(1);
                if self.outbox_on() {
                    let doc = items
                        .iter()
                        .find(|(i, _)| *i == id)
                        .map(|(_, d)| d)
                        .expect("row id came from items");
                    let e = extract(doc);
                    events.push(change_event(
                        tenant,
                        if inserted { "create" } else { "update" },
                        &id,
                        &e.types,
                        prevs.get(&id),
                        Some(doc),
                        r.get::<i64, _>(2),
                        r.get::<&str, _>(3),
                    ));
                }
                created.insert(id, inserted);
            }
            super::outbox::enqueue_many(&mut tx, tenant, &events).await?;
            tx.commit().await?;
            Ok(items
                .iter()
                .map(|(id, _)| {
                    (
                        created.get(id).copied().unwrap_or(false),
                        prevs.get(id).cloned(),
                    )
                })
                .collect())
        })
    }

    /// Batch read-modify-write — ONE transaction,
    /// all rows locked in a single ordered `FOR UPDATE` select, the closure
    /// applied per doc in Rust, and ONE multi-row UPDATE writeback. Per-item
    /// results align with `ids`: `None` = absent, `Some(Err)` = the closure
    /// rejected that item (its write is skipped, the rest proceed).
    pub fn batch_mutate<E>(
        &self,
        tenant: &TenantId,
        ids: &[String],
        mut f: impl FnMut(&str, &mut Value) -> Result<(), E>,
    ) -> Result<Vec<Option<Result<(), E>>>, sqlx::Error> {
        wait(async {
            let mut tx = self.pool.begin().await?;
            crate::pg::set_tenant(&mut tx, tenant).await?;
            let rows = sqlx::query(
                "SELECT id, entity FROM entities WHERE tenant_id = $1 AND id = ANY($2)
                 ORDER BY id FOR UPDATE",
            )
            .bind(tenant.as_str())
            .bind(ids)
            .fetch_all(&mut *tx)
            .await?;
            let mut docs: std::collections::HashMap<String, Value> = rows
                .into_iter()
                .map(|r| (r.get::<String, _>(0), r.get::<Value, _>(1)))
                .collect();
            let mut results: Vec<Option<Result<(), E>>> = Vec::with_capacity(ids.len());
            let mut payload = Vec::new();
            let mut changed: Vec<(String, Value, Value)> = Vec::new(); // id, before, after
            for id in ids {
                match docs.get_mut(id) {
                    None => results.push(None),
                    Some(doc) => {
                        let before = doc.clone();
                        match f(id, doc) {
                            Err(e) => {
                                *doc = before; // closure failed: discard its edits
                                results.push(Some(Err(e)));
                            }
                            Ok(()) => {
                                if *doc != before {
                                    let e = extract(doc);
                                    payload.push(serde_json::json!({
                                        "id": id, "doc": &*doc, "types": e.types,
                                        "scopes": e.scopes, "modified": e.modified,
                                        "expires": e.expires, "location": e.location,
                                        "loc_ambiguous": e.location_ambiguous,
                                    }));
                                    changed.push((id.clone(), before, doc.clone()));
                                }
                                results.push(Some(Ok(())));
                            }
                        }
                    }
                }
            }
            if !payload.is_empty() {
                let updated = sqlx::query(
                    "UPDATE entities t SET
                            entity = e->'doc',
                            types = ARRAY(SELECT jsonb_array_elements_text(e->'types')),
                            scopes = CASE WHEN e->'scopes' = 'null'::jsonb THEN NULL
                                          ELSE ARRAY(SELECT jsonb_array_elements_text(e->'scopes')) END,
                            modified_at = (e->>'modified')::timestamptz,
                            expires_at = (e->>'expires')::timestamptz,
                            location = CASE WHEN ST_IsValid(ST_SetSRID(ST_GeomFromGeoJSON(e->>'location'), 4326))
                                            THEN ST_SetSRID(ST_GeomFromGeoJSON(e->>'location'), 4326) END,
                            location_ambiguous = COALESCE((e->>'loc_ambiguous')::bool, false)
                              OR (e->>'location' IS NOT NULL
                                  AND NOT ST_IsValid(ST_SetSRID(ST_GeomFromGeoJSON(e->>'location'), 4326))),
                            version = t.version + 1
                     FROM jsonb_array_elements($2::jsonb) AS e
                     WHERE t.tenant_id = $1 AND t.id = e->>'id'
                     RETURNING t.id, t.version, t.created_at::text",
                )
                .bind(tenant.as_str())
                .bind(Value::Array(payload))
                .fetch_all(&mut *tx)
                .await?;
                if self.outbox_on() {
                    let meta: std::collections::HashMap<String, (i64, String)> = updated
                        .into_iter()
                        .map(|r| {
                            (
                                r.get::<String, _>(0),
                                (r.get::<i64, _>(1), r.get::<String, _>(2)),
                            )
                        })
                        .collect();
                    let events: Vec<Value> = changed
                        .iter()
                        .filter_map(|(id, before, after)| {
                            let e = extract(after);
                            let (version, incarnation) = meta.get(id)?;
                            Some(change_event(
                                tenant,
                                "update",
                                id,
                                &e.types,
                                Some(before),
                                Some(after),
                                *version,
                                incarnation,
                            ))
                        })
                        .collect();
                    super::outbox::enqueue_many(&mut tx, tenant, &events).await?;
                }
            }
            tx.commit().await?;
            Ok(results)
        })
    }

    /// Current row version (test hook for the version-monotonicity assertions).
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

#[cfg(test)]
mod tests {
    use super::*;

    fn wheres() -> Vec<String> {
        vec!["tenant_id = $1".to_owned()]
    }

    fn page(offset: i64, limit: i64) -> Page {
        Page { offset, limit }
    }

    /// 5.5.9.1: a query SQL decided exactly keeps the pushed page — LIMIT
    /// and OFFSET, with the window total that feeds count= and the
    /// next/prev pointers.
    #[test]
    fn a_decided_page_pushes_limit_and_offset() {
        let mut binds = vec![Bind::Text("t".to_owned())];
        let (sql, paged) = query_sql("entity", &wheres(), Some(&page(20, 10)), true, &mut binds);
        assert!(paged);
        assert!(sql.contains("LIMIT $2 OFFSET $3"), "{sql}");
        assert!(sql.contains("count(*) OVER ()"), "{sql}");
        assert!(matches!(binds[1], Bind::Int(10)));
        assert!(matches!(binds[2], Bind::Int(20)));
    }

    /// The undecided path must never leave the statement unbounded: SQL only
    /// narrowed the set (`scopeQ`, `georel`, a `q=` the compiler declined),
    /// so the rows come back to be filtered — under a LIMIT, and without an
    /// OFFSET, which would page over the wrong set.
    #[test]
    fn an_undecided_query_carries_a_safety_limit_and_no_offset() {
        let mut binds = vec![Bind::Text("t".to_owned())];
        let (sql, paged) = query_sql("entity", &wheres(), Some(&page(20, 10)), false, &mut binds);
        assert!(!paged);
        assert!(sql.contains("LIMIT $2"), "{sql}");
        assert!(!sql.contains("OFFSET"), "offset cannot be pushed: {sql}");
        assert!(!sql.contains("count(*) OVER ()"), "{sql}");
        // the candidate ceiling, not the page: the caller's evaluator still
        // filters these rows, so a page-sized fetch would refuse legitimate
        // queries whenever candidates outnumber matches
        assert!(matches!(binds[1], Bind::Int(MAX_UNDECIDED_ROWS)));
    }

    /// A caller with no page (idPattern, federation, orderBy) hands the store
    /// no bound at all — the ceiling is then the only one.
    #[test]
    fn a_query_without_a_page_falls_back_to_the_ceiling() {
        let mut binds = vec![Bind::Text("t".to_owned())];
        let (sql, paged) = query_sql("entity", &wheres(), None, false, &mut binds);
        assert!(!paged);
        assert!(sql.contains("LIMIT $2"), "{sql}");
        assert!(matches!(binds[1], Bind::Int(MAX_UNDECIDED_ROWS)));
        // an exactly decided query with no page is materialized whole too
        let mut binds = vec![Bind::Text("t".to_owned())];
        let (sql, _) = query_sql("entity", &wheres(), None, true, &mut binds);
        assert!(sql.contains("LIMIT $2"), "{sql}");
    }

    /// Deep paging must not turn the bound into the unbounded read it
    /// replaced: `offset + limit + 1` is capped, and an absurd offset cannot
    /// overflow it into a negative LIMIT.
    #[test]
    fn the_ceiling_caps_a_deep_page() {
        // The requested page never narrows the candidate fetch, however deep
        // or shallow it is — only the memory ceiling applies.
        for p in [page(0, 10), page(MAX_UNDECIDED_ROWS, 10), page(i64::MAX, i64::MAX), page(0, 0)] {
            assert_eq!(undecided_ceiling(Some(&p)), MAX_UNDECIDED_ROWS);
        }
        assert_eq!(undecided_ceiling(None), MAX_UNDECIDED_ROWS);
    }

    /// 5.5.6 / Table 6.3.2-1: reaching the ceiling means the statement was
    /// truncated, so the page built from it would silently under-report —
    /// the operation is refused with TooManyResults (403) instead.
    #[test]
    fn a_ceiling_hit_is_too_many_results_not_a_short_page() {
        assert!(check_ceiling(false, 30, 31).is_ok(), "under the ceiling");
        let err = check_ceiling(false, 31, 31).expect_err("ceiling reached");
        let ngsi = ngsi_error(&err).expect("the spec error travels in the driver error");
        assert_eq!(ngsi.kind(), "TooManyResults");
        assert_eq!(ngsi.status(), 403);
        // a pushed page is exact by construction: a full page is a page, not
        // a truncation
        assert!(check_ceiling(true, 1000, 1000).is_ok());
    }

    /// A real driver failure must stay a driver failure — only the store's
    /// own spec errors are recovered, or every 500 would become a 403.
    #[test]
    fn a_driver_error_is_not_mistaken_for_a_spec_error() {
        assert!(ngsi_error(&sqlx::Error::RowNotFound).is_none());
        assert!(ngsi_error(&sqlx::Error::PoolTimedOut).is_none());
        assert!(ngsi_error(&sqlx::Error::Configuration("boom".into())).is_none());
    }
}
