// SPDX-License-Identifier: EUPL-1.2
//! The store seam: ONE closed set of backends behind the
//! memory store's 12-method surface. An enum, not a trait — `mutate<T, E>` is
//! generic (dyn-incompatible), the backend set is closed by design
//! (exactly the implementations the product needs), and match
//! exhaustiveness forces every backend to answer every method.
//!
//! Every method returns `Result<_, NgsiError>`: the memory/file backend never
//! errors; the postgres backend maps sqlx failures to `InternalError` (500) —
//! a DB outage must be a visible 5xx, never a silent 404/409.

use antares_model::{NgsiError, TenantId};
use serde_json::Value;

#[cfg(feature = "postgres")]
use super::pg::doc::{DocKind, PgDocStore};
#[cfg(feature = "postgres")]
use super::pg::entity::PgEntityStore;
#[cfg(feature = "postgres")]
use super::pg::temporal::PgTemporalStore;
use super::{ChangeHook, Kind, Store};

/// 5.5.6: unexpected failures (database errors, timeouts) surface as
/// InternalError. The client-visible detail is deliberately generic —
/// driver internals (SQL text, constraint names, connection state) go to
/// the server log only.
#[cfg(feature = "postgres")]
pub(crate) fn db(e: sqlx::Error) -> NgsiError {
    // The store's own spec errors travel out through the same sqlx channel
    // (the signature is fixed by the callers), so recover them — by VALUE, so
    // the variant and therefore the status survive — before the generic
    // mapping turns them all into a 500.
    if let sqlx::Error::Configuration(b) = e {
        return match b.downcast::<NgsiError>() {
            Ok(n) => *n,
            Err(b) => {
                tracing::error!("database error: {b}");
                NgsiError::InternalError("database error".into())
            }
        };
    }
    // The pool handed out no connection inside its acquire timeout: the
    // broker is holding every connection it may hold and the caller waited
    // the whole wall. That is overload, not a fault — the binding answers it
    // 503 with Retry-After (`negotiate::ApiError::Overloaded`), so the detail
    // is the constant both ends name.
    if matches!(e, sqlx::Error::PoolTimedOut) {
        metrics::counter!("antares_pg_pool_timeouts_total").increment(1);
        tracing::warn!(
            "connection pool exhausted: no connection within the acquire timeout \
             (raise ANTARES_PG_POOL, or the request rate is above what this \
             database can serve)"
        );
        return NgsiError::InternalError(antares_model::error::DB_OVERLOADED.into());
    }
    // 5.5.2: "database timeouts" are InternalError. SQLSTATE 57014 is the
    // session's statement_timeout firing — named in the detail so a wall hit
    // reads differently from a broken query in the operator's log.
    if let sqlx::Error::Database(d) = &e {
        if d.code().as_deref() == Some("57014") {
            tracing::warn!("database statement timeout: {d}");
            return NgsiError::InternalError("database statement timeout".into());
        }
    }
    tracing::error!("database error: {e}");
    NgsiError::InternalError("database error".into())
}

/// 4.22: the read-boundary "now" — every entity read strips expired docs and
/// instances against this stamp (UTC Z, millisecond precision, the same form
/// the broker writes into system timestamps).
pub(crate) fn now_utc() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[cfg(feature = "postgres")]
fn doc_kind(kind: Kind) -> Result<DocKind, NgsiError> {
    match kind {
        Kind::Subscription => Ok(DocKind::Subscription),
        Kind::Registration => Ok(DocKind::Registration),
        Kind::CSourceSubscription => Ok(DocKind::CSourceSubscription),
        Kind::Snapshot => Ok(DocKind::Snapshot),
        Kind::EntityMap => Ok(DocKind::EntityMap),
        Kind::DistSub => Ok(DocKind::DistSub),
        Kind::DeadLetter => Ok(DocKind::DeadLetter),
        // Entities and Temporal Representations have their own tables and
        // never reach the document store. Every caller routes them to their
        // own match arm first; one that does not has mismatched its arms,
        // and the mistake stays inside the one request.
        Kind::Entity | Kind::Temporal => Err(NgsiError::InternalError(format!(
            "{kind:?} is not a document-store kind"
        ))),
    }
}

/// Postgres backend bundle: one pool, three table-family stores, plus the
/// change hook (the memory store emits its own; here the seam emits).
#[cfg(feature = "postgres")]
pub struct PgBackend {
    pub entities: PgEntityStore,
    pub temporal: PgTemporalStore,
    pub docs: PgDocStore,
    hook: std::sync::RwLock<Option<ChangeHook>>,
    /// Server and extension versions, read once at startup
    /// (`with_version`); `/q/health` serves this copy rather than querying
    /// the database on a polled endpoint.
    version: Value,
}

#[cfg(feature = "postgres")]
impl PgBackend {
    pub fn new(pool: sqlx::postgres::PgPool) -> Self {
        Self {
            entities: PgEntityStore::new(pool.clone()),
            temporal: PgTemporalStore::new(pool.clone()),
            docs: PgDocStore::new(pool),
            hook: std::sync::RwLock::new(None),
            version: Value::Object(serde_json::Map::new()),
        }
    }

    /// Attach what the startup probe read from the server
    /// (`pg::version_info`), so `/q/health` can answer it without a query.
    pub fn with_version(mut self, version: Value) -> Self {
        self.version = version;
        self
    }

    /// Poison recovery (`into_inner`) is deliberate, the same choice the
    /// memory arm records: the hook runs real code over attacker-shaped JSON,
    /// and a panic inside it must unwind one request, not poison this lock
    /// and panic every later entity write until the process restarts.
    fn emit(&self, tenant: &TenantId, before: Option<Value>, after: Option<Value>) {
        if let Some(h) = self
            .hook
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
        {
            h(tenant, before, after);
        }
    }
}

/// What `antares-api` sees. No core crate names redb or sqlx.
// One AnyStore exists per process — variant size difference is irrelevant.
#[allow(clippy::large_enum_variant)]
pub enum AnyStore {
    Mem(Store),
    #[cfg(feature = "postgres")]
    Pg(PgBackend),
}

/// A row's `id`, or `""` for one without: an id-less row sorts first and is
/// never skipped past, so a keyset walk over it cannot lose it. Only the
/// Postgres arm sorts rows it read whole; the memory arm's maps are already
/// keyed by id.
#[cfg(feature = "postgres")]
fn row_id(v: &Value) -> &str {
    v.get("id").and_then(Value::as_str).unwrap_or_default()
}

impl AnyStore {
    /// Readiness ping: can the store answer a trivial request
    /// RIGHT NOW? Memory/file are in-process (always ready); the Pg arm runs
    /// `SELECT 1` so a lost database (failover, network partition) flips
    /// /q/ready to 503 and the Service stops routing to this pod.
    pub fn ping(&self) -> Result<(), NgsiError> {
        match self {
            AnyStore::Mem(_) => Ok(()),
            #[cfg(feature = "postgres")]
            AnyStore::Pg(p) => super::pg::entity::wait(async {
                sqlx::query("SELECT 1")
                    .execute(p.docs.pool())
                    .await
                    .map(|_| ())
            })
            .map_err(db),
        }
    }

    /// What this store runs on, for `/q/health`. The memory and file modes
    /// are one backend with two durability shapes and must not read as the
    /// same thing; the Postgres arm serves what the startup probe read.
    pub fn version_info(&self) -> Value {
        match self {
            AnyStore::Mem(s) => serde_json::json!({
                "engine": if s.shadowed() { "redb" } else { "memory" },
            }),
            #[cfg(feature = "postgres")]
            AnyStore::Pg(p) => p.version.clone(),
        }
    }

    /// (Queued writers, peak) behind the single redb committer, reported
    /// only by a store that has one: `None` for the Pg arm (Postgres has no
    /// single-writer commit queue) and for a pure in-memory store, whose
    /// lock depth is not the fsync queue this number stands for.
    pub fn commit_queue(&self) -> Option<(usize, usize)> {
        match self {
            AnyStore::Mem(s) => s.shadowed().then(|| s.commit_queue()),
            #[cfg(feature = "postgres")]
            AnyStore::Pg(_) => None,
        }
    }

    /// The last step of the drain: close the connection pool so in-flight
    /// transactions finish and the server sees a clean disconnect instead of
    /// N abandoned backends. A no-op for the memory/file arm, whose durability
    /// is already commit-before-ack — there is nothing buffered to lose.
    pub async fn close(&self) {
        match self {
            AnyStore::Mem(_) => {}
            #[cfg(feature = "postgres")]
            AnyStore::Pg(p) => p.docs.pool().close().await,
        }
    }

    pub fn set_change_hook(&self, h: ChangeHook) {
        match self {
            AnyStore::Mem(s) => s.set_change_hook(h),
            #[cfg(feature = "postgres")]
            AnyStore::Pg(p) => {
                *p.hook
                    .write()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(h)
            }
        }
    }

    /// Turn the same-tx outbox producer on (bus=nats). The memory arm has
    /// no outbox — the broker's wiring rejects bus=nats without a Pg store,
    /// so this is unreachable there by construction.
    pub fn set_outbox(
        &self,
        #[cfg_attr(not(feature = "postgres"), allow(unused_variables))] on: bool,
    ) {
        match self {
            AnyStore::Mem(_) => {}
            #[cfg(feature = "postgres")]
            AnyStore::Pg(p) => p.entities.set_outbox(on),
        }
    }

    /// Outbox drain: oldest-first page of pending rows `(seq, tenant, event)`.
    pub fn outbox_peek(
        &self,
        #[cfg_attr(not(feature = "postgres"), allow(unused_variables))] limit: i64,
    ) -> Result<Vec<(i64, String, Value)>, NgsiError> {
        match self {
            AnyStore::Mem(_) => Ok(Vec::new()),
            #[cfg(feature = "postgres")]
            AnyStore::Pg(p) => super::pg::outbox::peek(p.docs.pool(), limit).map_err(db),
        }
    }

    /// Outbox drain: delete EXACTLY the published rows (never a blanket
    /// `seq <= max`, which loses a row committing between peek and ack).
    pub fn outbox_ack(
        &self,
        #[cfg_attr(not(feature = "postgres"), allow(unused_variables))] seqs: &[i64],
    ) -> Result<u64, NgsiError> {
        match self {
            AnyStore::Mem(_) => Ok(0),
            #[cfg(feature = "postgres")]
            AnyStore::Pg(p) => super::pg::outbox::ack(p.docs.pool(), seqs).map_err(db),
        }
    }

    /// Create one document of `kind`; `false` when that id is already taken.
    pub fn create(
        &self,
        tenant: &TenantId,
        kind: Kind,
        id: &str,
        doc: Value,
    ) -> Result<bool, NgsiError> {
        match self {
            AnyStore::Mem(s) => Ok(s.create(tenant, kind, id, doc)),
            #[cfg(feature = "postgres")]
            AnyStore::Pg(p) => {
                let created = match kind {
                    Kind::Entity => p.entities.create(tenant, id, &doc).map_err(db)?,
                    Kind::Temporal => p.temporal.create(tenant, id, &doc).map_err(db)?,
                    _ => {
                        let dk = doc_kind(kind)?;
                        p.docs.create(tenant, dk, id, &doc).map_err(db)?
                    }
                };
                if created && kind == Kind::Entity {
                    p.emit(tenant, None, Some(doc));
                }
                Ok(created)
            }
        }
    }

    /// Batch create (entities only): one multi-row statement on the Pg
    /// arm, per-item loop on the memory arm. Created-flags in input order.
    pub fn batch_create(
        &self,
        tenant: &TenantId,
        items: Vec<(String, Value)>,
    ) -> Result<Vec<bool>, NgsiError> {
        match self {
            AnyStore::Mem(s) => Ok(items
                .into_iter()
                .map(|(id, doc)| s.create(tenant, Kind::Entity, &id, doc))
                .collect()),
            #[cfg(feature = "postgres")]
            AnyStore::Pg(p) => {
                let flags = p.entities.batch_create(tenant, &items).map_err(db)?;
                for ((_, doc), created) in items.into_iter().zip(&flags) {
                    if *created {
                        p.emit(tenant, None, Some(doc));
                    }
                }
                Ok(flags)
            }
        }
    }

    /// Batch delete (entities only): deleted-flags in input order; a
    /// duplicate id in the input deletes once and 404s the second time,
    /// matching the per-item loop's semantics (5.5.11.4).
    pub fn batch_delete(&self, tenant: &TenantId, ids: &[String]) -> Result<Vec<bool>, NgsiError> {
        match self {
            AnyStore::Mem(s) => Ok(ids
                .iter()
                .map(|id| s.delete(tenant, Kind::Entity, id))
                .collect()),
            #[cfg(feature = "postgres")]
            AnyStore::Pg(p) => {
                let deleted = p.entities.batch_delete(tenant, ids).map_err(db)?;
                let mut prev: std::collections::HashMap<String, Value> =
                    deleted.into_iter().collect();
                Ok(ids
                    .iter()
                    .map(|id| match prev.remove(id) {
                        Some(before) => {
                            p.emit(tenant, Some(before), None);
                            true
                        }
                        None => false,
                    })
                    .collect())
            }
        }
    }

    pub fn upsert(
        &self,
        tenant: &TenantId,
        kind: Kind,
        id: &str,
        doc: Value,
    ) -> Result<bool, NgsiError> {
        match self {
            AnyStore::Mem(s) => Ok(s.upsert(tenant, kind, id, doc)),
            #[cfg(feature = "postgres")]
            AnyStore::Pg(p) => match kind {
                Kind::Entity => {
                    // replace-or-create without a pre-read: try the replace
                    // first (captures the true before-image under the row
                    // lock), fall back to create, and on a lost create race
                    // replace after all.
                    let mut prev: Option<Value> = None;
                    let replace = |prev: &mut Option<Value>| {
                        p.entities.mutate(tenant, id, |d| {
                            *prev = Some(d.clone());
                            *d = doc.clone();
                            Ok::<(), std::convert::Infallible>(())
                        })
                    };
                    let mut existed = replace(&mut prev).map_err(db)?.is_some();
                    if !existed {
                        if p.entities.create(tenant, id, &doc).map_err(db)? {
                            existed = false;
                        } else {
                            // lost the create race — replace instead
                            existed = replace(&mut prev).map_err(db)?.is_some();
                        }
                    }
                    p.emit(tenant, prev, Some(doc));
                    Ok(existed)
                }
                Kind::Temporal => {
                    if p.temporal.create(tenant, id, &doc).map_err(db)? {
                        Ok(false)
                    } else {
                        p.temporal
                            .mutate(tenant, id, |d| {
                                *d = doc.clone();
                                Ok::<(), std::convert::Infallible>(())
                            })
                            .map_err(db)?;
                        Ok(true)
                    }
                }
                _ => {
                    let dk = doc_kind(kind)?;
                    p.docs.upsert(tenant, dk, id, &doc).map_err(db)
                }
            },
        }
    }

    pub fn get(&self, tenant: &TenantId, kind: Kind, id: &str) -> Result<Option<Value>, NgsiError> {
        let doc = match self {
            AnyStore::Mem(s) => s.get(tenant, kind, id),
            #[cfg(feature = "postgres")]
            AnyStore::Pg(p) => match kind {
                Kind::Entity => p.entities.get(tenant, id).map_err(db)?,
                Kind::Temporal => p.temporal.get(tenant, id).map_err(db)?,
                _ => p.docs.get(tenant, doc_kind(kind)?, id).map_err(db)?,
            },
        };
        // 4.22: an expired entity is invalid context — a read serves it to
        // no one, whichever arm stored it (the Pg sweep lags by design).
        if kind == Kind::Entity {
            let now = now_utc();
            if let Some(mut d) = doc {
                if crate::store::filter::strip_expired(&mut d, &now) {
                    return Ok(None);
                }
                return Ok(Some(d));
            }
            return Ok(None);
        }
        Ok(doc)
    }

    pub fn delete(&self, tenant: &TenantId, kind: Kind, id: &str) -> Result<bool, NgsiError> {
        match self {
            AnyStore::Mem(s) => Ok(s.delete(tenant, kind, id)),
            #[cfg(feature = "postgres")]
            AnyStore::Pg(p) => match kind {
                Kind::Entity => {
                    // the before-image comes from the DELETE's own RETURNING —
                    // same transaction, never a separate racy read
                    let prev = p.entities.delete(tenant, id).map_err(db)?;
                    let hit = prev.is_some();
                    if hit {
                        p.emit(tenant, prev, None);
                    }
                    Ok(hit)
                }
                Kind::Temporal => p.temporal.delete(tenant, id).map_err(db),
                _ => p.docs.delete(tenant, doc_kind(kind)?, id).map_err(db),
            },
        }
    }

    /// Delete one entity only if `keep` accepts the stored document, read
    /// and removed under one lock (see the driver trait).
    pub fn delete_entity_if(
        &self,
        tenant: &TenantId,
        id: &str,
        keep: &dyn Fn(&Value) -> bool,
    ) -> Result<bool, NgsiError> {
        // 4.22 is applied to the document `keep` judges, exactly as `get`
        // applies it: an expired instance is not there to be matched on.
        let now = now_utc();
        let judge = |d: &Value| {
            let mut d = d.clone();
            !crate::store::filter::strip_expired(&mut d, &now) && keep(&d)
        };
        match self {
            AnyStore::Mem(s) => Ok(s.delete_if(tenant, id, &judge).is_some()),
            #[cfg(feature = "postgres")]
            AnyStore::Pg(p) => {
                // the before-image comes from the DELETE's own RETURNING —
                // same transaction, never a separate racy read
                let prev = p.entities.delete_if(tenant, id, &judge).map_err(db)?;
                let hit = prev.is_some();
                if hit {
                    p.emit(tenant, prev, None);
                }
                Ok(hit)
            }
        }
    }

    pub fn list(&self, tenant: &TenantId, kind: Kind) -> Result<Vec<Value>, NgsiError> {
        let mut rows = match self {
            AnyStore::Mem(s) => s.list(tenant, kind),
            #[cfg(feature = "postgres")]
            AnyStore::Pg(p) => match kind {
                Kind::Entity => p.entities.list(tenant).map_err(db)?,
                Kind::Temporal => p.temporal.list(tenant).map_err(db)?,
                _ => p.docs.list(tenant, doc_kind(kind)?).map_err(db)?,
            },
        };
        if kind == Kind::Entity {
            let now = now_utc();
            rows.retain_mut(|d| !crate::store::filter::strip_expired(d, &now));
        }
        Ok(rows)
    }

    /// One id-ordered page of documents (see `CurrentStateDriver::list_page`).
    pub fn list_page(
        &self,
        tenant: &TenantId,
        kind: Kind,
        after: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Value>, NgsiError> {
        match self {
            AnyStore::Mem(s) => Ok(s.list_page(tenant, kind, after, limit)),
            #[cfg(feature = "postgres")]
            AnyStore::Pg(p) => match kind {
                // Not doc kinds: they live in their own tables, behind their
                // own readers, and `doc_kind` refuses them.
                Kind::Entity => {
                    let mut rows = p
                        .entities
                        .list_page(tenant, after, i64::try_from(limit).unwrap_or(i64::MAX))
                        .map_err(db)?;
                    // 4.22: the statement excluded expired ENTITIES, so no
                    // document leaves this page and its length still means
                    // what the caller reads it as; expired INSTANCES are
                    // stripped here, at the read boundary, as everywhere else.
                    let now = now_utc();
                    rows.retain_mut(|d| !crate::store::filter::strip_expired(d, &now));
                    Ok(rows)
                }
                // ponytail: temporal is sliced from the whole list, so a
                // tenant large enough still refuses this read for volume; the
                // upgrade is a keyset reader over the instance rows.
                // Temporal has no keyset reader of its own: its documents are
                // reconstructed from the instance rows, where the volume that
                // needs bounding is instances and not entities — the one kind
                // that does not yet honour `list_page`'s contract, and the
                // reason nothing internal walks temporal state.
                Kind::Temporal => {
                    let mut rows = AnyStore::list(self, tenant, kind)?;
                    rows.sort_by(|a, b| row_id(a).cmp(row_id(b)));
                    Ok(rows
                        .into_iter()
                        .skip_while(|r| after.is_some_and(|a| row_id(r) <= a))
                        .take(limit)
                        .collect())
                }
                _ => p
                    .docs
                    .list_page(
                        tenant,
                        doc_kind(kind)?,
                        after,
                        i64::try_from(limit).unwrap_or(i64::MAX),
                    )
                    .map_err(db),
            },
        }
    }

    /// One id-ordered window of documents and the total (see
    /// `CurrentStateDriver::list_slice`).
    pub fn list_slice(
        &self,
        tenant: &TenantId,
        kind: Kind,
        offset: usize,
        limit: usize,
    ) -> Result<(Vec<Value>, usize), NgsiError> {
        match self {
            AnyStore::Mem(s) => Ok(s.list_slice(tenant, kind, offset, limit)),
            #[cfg(feature = "postgres")]
            AnyStore::Pg(p) => match kind {
                // Not doc kinds; sliced from the whole list, like `list_page`.
                Kind::Entity | Kind::Temporal => {
                    let mut rows = AnyStore::list(self, tenant, kind)?;
                    rows.sort_by(|a, b| row_id(a).cmp(row_id(b)));
                    let total = rows.len();
                    Ok((rows.into_iter().skip(offset).take(limit).collect(), total))
                }
                _ => {
                    let (page, total) = p
                        .docs
                        .list_slice(
                            tenant,
                            doc_kind(kind)?,
                            i64::try_from(offset).unwrap_or(i64::MAX),
                            i64::try_from(limit).unwrap_or(i64::MAX),
                        )
                        .map_err(db)?;
                    Ok((page, usize::try_from(total).unwrap_or(usize::MAX)))
                }
            },
        }
    }

    /// 5.12 registration candidates for these entity ids / types. The Pg arm
    /// reads the `csource_index` rows (an indexed narrowing); `memory`/`file`
    /// have nothing to push into and return the same snapshot `list` does.
    /// Either way the result is a SUPERSET — the caller's matcher decides
    /// every 5.12 condition, this only avoids reading registrations that
    /// cannot match on id or type. `types` must be expanded plain type IRIs
    /// (what the registration write stored); a caller holding terms or a 4.17
    /// selection expression passes `None` and narrows on ids alone.
    pub fn matching_registrations(
        &self,
        tenant: &TenantId,
        #[cfg_attr(not(feature = "postgres"), allow(unused_variables))] ids: Option<&[String]>,
        #[cfg_attr(not(feature = "postgres"), allow(unused_variables))] types: Option<&[String]>,
    ) -> Result<Vec<Value>, NgsiError> {
        match self {
            AnyStore::Mem(s) => Ok(s.list(tenant, Kind::Registration)),
            #[cfg(feature = "postgres")]
            AnyStore::Pg(p) => p
                .docs
                .matching_registrations(tenant, ids, types)
                .map_err(db),
        }
    }

    /// Query Entities with the filter pushed down where the backend
    /// can take it. `memory`/`file` have nothing to push into — their
    /// entities are already in RAM — so they return the same snapshot `list`
    /// does (never `decided`, never `paged`). Either way the caller applies
    /// the exact filter afterwards unless the outcome says SQL already did.
    pub fn query_entities(
        &self,
        tenant: &TenantId,
        #[cfg_attr(not(feature = "postgres"), allow(unused_variables))]
        f: &crate::store::filter::EntityFilter<'_>,
    ) -> Result<crate::store::filter::QueryOutcome, NgsiError> {
        let mut outcome = match self {
            AnyStore::Mem(s) => crate::store::filter::QueryOutcome {
                rows: s.list(tenant, Kind::Entity),
                decided: false,
                paged: false,
                total: None,
            },
            #[cfg(feature = "postgres")]
            AnyStore::Pg(p) => p.entities.query(tenant, f).map_err(db)?,
        };
        // 4.22: the Pg arm already excludes expired ENTITIES in SQL (so
        // paging/totals stay exact); instance stripping — and the whole job
        // on the memory arm — happens here at the read boundary.
        let now = now_utc();
        outcome
            .rows
            .retain_mut(|d| !crate::store::filter::strip_expired(d, &now));
        Ok(outcome)
    }

    /// Query Temporal Evolution with entity narrowing, instance-window
    /// pruning AND entity paging pushed down. Same
    /// contract: the API's window() is the arbiter; the memory arm returns
    /// the full snapshot, never paged.
    pub fn query_temporal(
        &self,
        tenant: &TenantId,
        #[cfg_attr(not(feature = "postgres"), allow(unused_variables))]
        f: &crate::store::filter::TemporalFilter<'_>,
    ) -> Result<crate::store::filter::TemporalOutcome, NgsiError> {
        let mut outcome = match self {
            AnyStore::Mem(s) => crate::store::filter::TemporalOutcome {
                rows: s.list(tenant, Kind::Temporal),
                paged: false,
                total: None,
                aggregated: false,
            },
            #[cfg(feature = "postgres")]
            AnyStore::Pg(p) => p.temporal.query(tenant, f).map_err(db)?,
        };
        // 4.22 on temporal reads: the Pg arm already dropped expired ENTITIES in
        // SQL (paging exact); this strips expired attribute INSTANCES, and does
        // the whole job on the memory arm (no SQL to push into).
        let now = now_utc();
        outcome
            .rows
            .retain_mut(|d| !crate::store::filter::strip_expired(d, &now));
        Ok(outcome)
    }

    /// Auto-recording fast path: append instances to an
    /// entity's temporal evolution, creating the meta shell on first touch.
    /// Pg: pure multi-row INSERT — no history read, no doc rewrite. Memory:
    /// the create-or-extend the mirror always did, under the store lock.
    /// `shell` carries the meta members; `additions` maps attr IRI →
    /// instance array (instanceIds already stamped by the caller).
    ///
    /// Both arms record only for an entity that still exists: 5.6.6 deletes
    /// the entity and then the temporal evolution recorded for it, so an
    /// append overlapping the delete must not recreate history nothing will
    /// ever clean again. An instance marked `temporal_only` never holds the
    /// entities (they live in another backend) and records unconditionally;
    /// there the delete overlap stays a window the temporal delete closes.
    pub fn temporal_append(
        &self,
        tenant: &TenantId,
        id: &str,
        shell: &Value,
        additions: &Value,
    ) -> Result<(), NgsiError> {
        match self {
            AnyStore::Mem(s) => {
                if !s.temporal_only && s.get(tenant, Kind::Entity, id).is_none() {
                    return Ok(());
                }
                if s.get(tenant, Kind::Temporal, id).is_none() {
                    // loser of a concurrent create race just extends below
                    let _ = s.create(tenant, Kind::Temporal, id, shell.clone());
                }
                s.mutate(tenant, Kind::Temporal, id, |doc| {
                    let target = doc.as_object_mut().ok_or(())?;
                    if let Some(adds) = additions.as_object() {
                        for (k, v) in adds {
                            let incoming: Vec<Value> = v.as_array().cloned().unwrap_or_default();
                            match target.get_mut(k).and_then(Value::as_array_mut) {
                                // same instanceId = the same instance corrected
                                // (the pg arm's ON CONFLICT DO UPDATE)
                                Some(cur) => {
                                    for inst in incoming {
                                        let iid = inst.get("instanceId");
                                        match cur.iter_mut().find(|c| c.get("instanceId") == iid) {
                                            Some(slot) => *slot = inst,
                                            None => cur.push(inst),
                                        }
                                    }
                                }
                                None => {
                                    target.insert(k.clone(), Value::Array(incoming));
                                }
                            }
                        }
                    }
                    Ok::<(), ()>(())
                });
                Ok(())
            }
            #[cfg(feature = "postgres")]
            AnyStore::Pg(p) => p.temporal.append(tenant, id, shell, additions).map_err(db),
        }
    }

    /// Mark this instance as the temporal half of a mixed deployment: the
    /// entities live in another backend, so appends never look for them here.
    pub fn temporal_only(mut self) -> Self {
        match &mut self {
            AnyStore::Mem(s) => s.temporal_only = true,
            #[cfg(feature = "postgres")]
            AnyStore::Pg(p) => p.temporal.temporal_only = true,
        }
        self
    }

    /// Retrieve Temporal Evolution with the same instance pruning.
    pub fn get_temporal(
        &self,
        tenant: &TenantId,
        id: &str,
        #[cfg_attr(not(feature = "postgres"), allow(unused_variables))]
        f: &crate::store::filter::TemporalFilter<'_>,
    ) -> Result<Option<Value>, NgsiError> {
        let mut doc = match self {
            AnyStore::Mem(s) => s.get(tenant, Kind::Temporal, id),
            #[cfg(feature = "postgres")]
            AnyStore::Pg(p) => p.temporal.get_range(tenant, id, f).map_err(db)?,
        };
        // 4.22: expired entity → None (Pg already did this in SQL); otherwise
        // strip expired instances.
        if let Some(d) = &mut doc {
            let now = now_utc();
            if crate::store::filter::strip_expired(d, &now) {
                return Ok(None);
            }
        }
        Ok(doc)
    }

    pub fn mutate<T, E>(
        &self,
        tenant: &TenantId,
        kind: Kind,
        id: &str,
        f: impl FnOnce(&mut Value) -> Result<T, E>,
    ) -> Result<Option<Result<T, E>>, NgsiError> {
        match self {
            AnyStore::Mem(s) => Ok(s.mutate(tenant, kind, id, f)),
            #[cfg(feature = "postgres")]
            AnyStore::Pg(p) => match kind {
                Kind::Entity => {
                    // before/after captured for the change hook's
                    // prev_payload INSIDE the row lock — a before-image read
                    // in its own transaction can belong to a different version
                    // than the one the lock serialized on.
                    let mut before: Option<Value> = None;
                    let mut after: Option<Value> = None;
                    let r = p
                        .entities
                        .mutate(tenant, id, |d| {
                            before = Some(d.clone());
                            let r = f(d);
                            if r.is_ok() {
                                after = Some(d.clone());
                            }
                            r
                        })
                        .map_err(db)?;
                    if let (Some(Ok(_)), Some(a)) = (&r, after) {
                        if before.as_ref() != Some(&a) {
                            p.emit(tenant, before, Some(a));
                        }
                    }
                    Ok(r)
                }
                Kind::Temporal => p.temporal.mutate(tenant, id, f).map_err(db),
                _ => {
                    // FOR UPDATE + UPDATE in one tx: a bookkeeping writeback
                    // racing a DELETE must never resurrect the row (047_06).
                    let dk = doc_kind(kind)?;
                    match p.docs.mutate(tenant, dk, id, f).map_err(db)? {
                        Some(r) => Ok(Some(r)),
                        None => Ok(None),
                    }
                }
            },
        }
    }

    /// Batch upsert with REPLACE semantics for entities:
    /// one statement + one transaction on the Pg arm, per-item loop on the
    /// memory arm. Created-flags in input order.
    pub fn batch_upsert(
        &self,
        tenant: &TenantId,
        items: Vec<(String, Value)>,
    ) -> Result<Vec<bool>, NgsiError> {
        match self {
            AnyStore::Mem(s) => Ok(items
                .into_iter()
                .map(|(id, doc)| !s.upsert(tenant, Kind::Entity, &id, doc))
                .collect()),
            #[cfg(feature = "postgres")]
            AnyStore::Pg(p) => {
                let out = p
                    .entities
                    .batch_upsert_replace(tenant, &items)
                    .map_err(db)?;
                for ((_, doc), (_, prev)) in items.iter().zip(&out) {
                    p.emit(tenant, prev.clone(), Some(doc.clone()));
                }
                Ok(out.into_iter().map(|(created, _)| created).collect())
            }
        }
    }

    /// Batch read-modify-write for entities: one transaction + one ordered
    /// lock set + one multi-row writeback on the Pg arm; per-item mutate on
    /// the memory arm. Results align with `ids` (`None` = absent).
    pub fn batch_mutate<E>(
        &self,
        tenant: &TenantId,
        ids: &[String],
        mut f: impl FnMut(&str, &mut Value) -> Result<(), E>,
    ) -> Result<Vec<Option<Result<(), E>>>, NgsiError> {
        match self {
            AnyStore::Mem(s) => Ok(ids
                .iter()
                .map(|id| s.mutate(tenant, Kind::Entity, id, |d| f(id, d)))
                .collect()),
            #[cfg(feature = "postgres")]
            AnyStore::Pg(p) => {
                // hook images captured inside the lock, same as single mutate
                let mut images: Vec<(Value, Value)> = Vec::new();
                let r = p
                    .entities
                    .batch_mutate(tenant, ids, |id, d| {
                        let before = d.clone();
                        let r = f(id, d);
                        if r.is_ok() && *d != before {
                            images.push((before, d.clone()));
                        }
                        r
                    })
                    .map_err(db)?;
                for (before, after) in images {
                    p.emit(tenant, Some(before), Some(after));
                }
                Ok(r)
            }
        }
    }

    /// 4.22 GC for the memory/file arm (the Pg arm's sweep lives in the
    /// maintenance job, mode-switched in the broker). Returns reaped count.
    pub fn sweep_expired(&self) -> usize {
        match self {
            AnyStore::Mem(s) => s.sweep_expired(&now_utc()),
            // 4.22 reaping on the Pg arm is `pg::maintenance`'s job
            // (`reap_expired_entities` / `reap_expired_instances`), which
            // deletes in one indexed statement instead of walking the store;
            // reads refuse an expired document either way.
            #[cfg(feature = "postgres")]
            AnyStore::Pg(_) => 0,
        }
    }

    /// 5.5.10: does the Tenant exist? The default Tenant "implicitly exists";
    /// others exist once implicitly created by a create operation.
    pub fn tenant_exists(&self, tenant: &TenantId) -> Result<bool, NgsiError> {
        if tenant.as_str() == TenantId::DEFAULT {
            return Ok(true);
        }
        match self {
            AnyStore::Mem(s) => Ok(s.tenant_exists(tenant)),
            #[cfg(feature = "postgres")]
            AnyStore::Pg(p) => super::pg::entity::wait(async {
                let row =
                    sqlx::query_scalar::<_, i32>("SELECT 1 FROM tenants WHERE tenant_id = $1")
                        .bind(tenant.as_str())
                        .fetch_optional(p.docs.pool())
                        .await
                        .map_err(db)?;
                Ok(row.is_some())
            }),
        }
    }

    /// Every tenant name the backend knows, sorted. One cheap query: at the
    /// 10 000-tenant target (ADR-0001) an inventory that counted every kind
    /// of every tenant would be 7 counts per tenant on one transaction, so
    /// the counts live in `tenant_stats_one` and are paid per lookup.
    pub fn tenant_ids(&self) -> Result<Vec<String>, NgsiError> {
        match self {
            AnyStore::Mem(s) => Ok(s.tenant_ids()),
            #[cfg(feature = "postgres")]
            AnyStore::Pg(p) => super::pg::entity::wait(async {
                let mut rows: Vec<String> =
                    sqlx::query_scalar("SELECT tenant_id FROM tenants ORDER BY 1")
                        .fetch_all(p.docs.pool())
                        .await
                        .map_err(db)?;
                // 5.5.10: the default Tenant implicitly exists, whether or
                // not a row was ever written for it.
                if !rows.iter().any(|t| t == TenantId::DEFAULT) {
                    rows.push(TenantId::DEFAULT.to_owned());
                    rows.sort();
                }
                Ok(rows)
            }),
        }
    }

    /// What one tenant holds; `None` when it does not exist (5.5.10 keeps
    /// the default Tenant existing even when empty).
    pub fn tenant_stats_one(
        &self,
        tenant: &TenantId,
    ) -> Result<Option<antares_store::TenantStats>, NgsiError> {
        if !self.tenant_exists(tenant)? {
            return Ok(None);
        }
        match self {
            AnyStore::Mem(s) => Ok(Some(s.tenant_stats_one(tenant))),
            #[cfg(feature = "postgres")]
            AnyStore::Pg(p) => super::pg::entity::wait(async {
                let mut tx = p.docs.pool().begin().await.map_err(db)?;
                // the tenant setting is what the RLS-guarded counts see
                crate::store::pg::set_tenant(&mut tx, tenant)
                    .await
                    .map_err(db)?;
                let created_at: Option<String> =
                    sqlx::query_scalar("SELECT created_at::text FROM tenants WHERE tenant_id = $1")
                        .bind(tenant.as_str())
                        .fetch_optional(&mut *tx)
                        .await
                        .map_err(db)?;
                let c: (i64, i64, i64, i64, i64, i64, i64) = sqlx::query_as(
                    "SELECT (SELECT count(*) FROM entities WHERE tenant_id = $1),
                            (SELECT count(*) FROM subscriptions WHERE tenant_id = $1),
                            (SELECT count(*) FROM csource_registrations WHERE tenant_id = $1),
                            (SELECT count(*) FROM csource_subscriptions WHERE tenant_id = $1),
                            (SELECT count(*) FROM snapshots WHERE tenant_id = $1),
                            (SELECT count(*) FROM entity_map_docs WHERE tenant_id = $1),
                            (SELECT count(*) FROM dist_subs WHERE tenant_id = $1)",
                )
                .bind(tenant.as_str())
                .fetch_one(&mut *tx)
                .await
                .map_err(db)?;
                tx.commit().await.map_err(db)?;
                Ok(Some(antares_store::TenantStats {
                    tenant: tenant.as_str().to_owned(),
                    created_at,
                    entities: c.0 as u64,
                    subscriptions: c.1 as u64,
                    registrations: c.2 as u64,
                    csource_subscriptions: c.3 as u64,
                    snapshots: c.4 as u64,
                    entity_maps: c.5 as u64,
                    dist_subs: c.6 as u64,
                }))
            }),
        }
    }

    /// Purge the current-state half of one tenant in one transaction;
    /// `false` when the tenant did not exist. The default tenant's row stays.
    pub fn purge_tenant(&self, tenant: &TenantId) -> Result<bool, NgsiError> {
        match self {
            AnyStore::Mem(s) => Ok(s.purge_tenant(tenant)),
            #[cfg(feature = "postgres")]
            AnyStore::Pg(p) => super::pg::entity::wait(async {
                let mut tx = p.docs.pool().begin().await.map_err(db)?;
                crate::store::pg::set_tenant(&mut tx, tenant)
                    .await
                    .map_err(db)?;
                let known = sqlx::query_scalar::<_, i32>(
                    "SELECT 1 FROM tenants WHERE tenant_id = $1 FOR UPDATE",
                )
                .bind(tenant.as_str())
                .fetch_optional(&mut *tx)
                .await
                .map_err(db)?
                .is_some();
                if !known {
                    return Ok(false);
                }
                for table in [
                    "entities",
                    "subscriptions",
                    "csource_subscriptions",
                    "csource_registrations",
                    "csource_index",
                    "outbox",
                    "snapshots",
                    "entity_map_docs",
                    "dist_subs",
                    "dead_letters",
                ] {
                    sqlx::query(sqlx::AssertSqlSafe(format!(
                        "DELETE FROM {table} WHERE tenant_id = $1"
                    )))
                    .bind(tenant.as_str())
                    .execute(&mut *tx)
                    .await
                    .map_err(db)?;
                }
                if tenant.as_str() != TenantId::DEFAULT {
                    sqlx::query("DELETE FROM tenants WHERE tenant_id = $1")
                        .bind(tenant.as_str())
                        .execute(&mut *tx)
                        .await
                        .map_err(db)?;
                }
                tx.commit().await.map_err(db)?;
                Ok(true)
            }),
        }
    }

    pub fn subscription_tenants(&self) -> Result<Vec<String>, NgsiError> {
        match self {
            AnyStore::Mem(s) => Ok(s.subscription_tenants()),
            // Pg answers the superset the contract allows: every known
            // tenant. Narrowing it would need a cross-tenant read of
            // `subscriptions`, and that table is deliberately outside the
            // `antares.service` escape — a subscription document carries
            // endpoint.receiverInfo, i.e. the credentials the notification
            // is sent with. The callers list per tenant under set_tenant
            // afterwards, so the extra names cost one empty list each.
            #[cfg(feature = "postgres")]
            AnyStore::Pg(p) => super::pg::entity::wait(async {
                let rows =
                    sqlx::query_scalar::<_, String>("SELECT tenant_id FROM tenants ORDER BY 1")
                        .fetch_all(p.docs.pool())
                        .await
                        .map_err(db)?;
                Ok(rows)
            }),
        }
    }

    pub fn context_put(&self, id: &str, doc: Value) -> Result<(), NgsiError> {
        match self {
            AnyStore::Mem(s) => {
                let _: () = s.context_put(id, doc);
                Ok(())
            }
            #[cfg(feature = "postgres")]
            AnyStore::Pg(p) => {
                let kind = doc
                    .get("kind")
                    .and_then(Value::as_str)
                    .unwrap_or("Cached")
                    .to_owned();
                p.docs.context_put(id, &doc, &kind).map_err(db)
            }
        }
    }

    pub fn context_get(&self, id: &str) -> Result<Option<Value>, NgsiError> {
        match self {
            AnyStore::Mem(s) => Ok(s.context_get(id)),
            #[cfg(feature = "postgres")]
            AnyStore::Pg(p) => p.docs.context_get(id).map_err(db),
        }
    }

    pub fn context_delete(&self, id: &str) -> Result<bool, NgsiError> {
        match self {
            AnyStore::Mem(s) => Ok(s.context_delete(id)),
            #[cfg(feature = "postgres")]
            AnyStore::Pg(p) => p.docs.context_delete(id).map_err(db),
        }
    }

    pub fn context_list_meta(&self) -> Result<Vec<Value>, NgsiError> {
        match self {
            AnyStore::Mem(s) => Ok(s.context_list_meta()),
            #[cfg(feature = "postgres")]
            AnyStore::Pg(p) => p.docs.context_list_meta().map_err(db),
        }
    }
}

#[cfg(all(test, feature = "postgres"))]
mod db_error_tests {
    #[allow(unused_imports)]
    use antares_model::NgsiError;

    /// An acquire timeout is overload, not a fault, and the HTTP binding
    /// recognises it by this exact detail. If the two ends ever spell it
    /// differently the broker answers 500 to a condition a client could
    /// have retried, so the constant is asserted rather than the words.
    #[test]
    fn a_pool_timeout_is_marked_as_overload() {
        let pd = super::db(sqlx::Error::PoolTimedOut).to_problem_details();
        assert_eq!(pd.detail, antares_model::error::DB_OVERLOADED);
        assert_eq!(pd.title, "InternalError");
        // and no other sqlx error borrows the mark
        assert_ne!(
            super::db(sqlx::Error::RowNotFound)
                .to_problem_details()
                .detail,
            antares_model::error::DB_OVERLOADED
        );
    }

    /// 5.5.6 InternalError: the RFC 7807 `detail` a client sees must be
    /// generic — driver internals (SQL text, row counts, connection
    /// strings) belong in the server log, never in the response body.
    #[test]
    fn db_error_detail_is_generic() {
        let pd = super::db(sqlx::Error::RowNotFound).to_problem_details();
        assert_eq!(pd.detail, "database error");
        assert_eq!(pd.title, "InternalError");
        assert!(
            !pd.detail.contains("no rows"),
            "sqlx internals leaked into the client-visible detail: {}",
            pd.detail
        );
        // a configuration error that is NOT one of ours stays generic too
        let pd = super::db(sqlx::Error::Configuration("boom".into())).to_problem_details();
        assert_eq!(pd.detail, "database error");
        assert!(!pd.detail.contains("boom"), "{}", pd.detail);
    }

    /// A spec error the store raised itself travels out through the driver
    /// error channel. Rebuilding it as a fixed variant forces every one of
    /// them to that variant's status — a 400 BadRequestData would reach the
    /// client as a 403.
    #[test]
    fn a_store_raised_spec_error_keeps_its_own_status() {
        for (err, kind, status) in [
            (NgsiError::BadRequestData("x".into()), "BadRequestData", 400),
            (NgsiError::TooManyResults("x".into()), "TooManyResults", 403),
            (NgsiError::AlreadyExists("x".into()), "AlreadyExists", 409),
        ] {
            let out = super::db(sqlx::Error::Configuration(Box::new(err)));
            assert_eq!(out.kind(), kind);
            assert_eq!(out.status(), status, "{kind} lost its status");
        }
    }
}

// The driver seam: `AnyStore` carries both driver interfaces, delegating to
// the inherent methods above. New backends implement the traits directly —
// this enum stays an implementation detail of the built-in backends, no
// longer the API surface.
impl antares_store::CurrentStateDriver for AnyStore {
    fn ping(&self) -> Result<(), NgsiError> {
        AnyStore::ping(self)
    }
    fn commit_queue(&self) -> Option<(usize, usize)> {
        AnyStore::commit_queue(self)
    }
    fn version_info(&self) -> Value {
        AnyStore::version_info(self)
    }
    fn close<'a>(&'a self) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
        Box::pin(AnyStore::close(self))
    }
    fn set_change_hook(&self, h: super::ChangeHook) {
        AnyStore::set_change_hook(self, h);
    }
    fn set_outbox(&self, on: bool) {
        AnyStore::set_outbox(self, on);
    }
    fn outbox_peek(&self, limit: i64) -> Result<Vec<(i64, String, Value)>, NgsiError> {
        AnyStore::outbox_peek(self, limit)
    }
    fn outbox_ack(&self, seqs: &[i64]) -> Result<u64, NgsiError> {
        AnyStore::outbox_ack(self, seqs)
    }
    fn create(
        &self,
        tenant: &TenantId,
        kind: Kind,
        id: &str,
        doc: Value,
    ) -> Result<bool, NgsiError> {
        AnyStore::create(self, tenant, kind, id, doc)
    }
    fn batch_create(
        &self,
        tenant: &TenantId,
        items: Vec<(String, Value)>,
    ) -> Result<Vec<bool>, NgsiError> {
        AnyStore::batch_create(self, tenant, items)
    }
    fn batch_delete(&self, tenant: &TenantId, ids: &[String]) -> Result<Vec<bool>, NgsiError> {
        AnyStore::batch_delete(self, tenant, ids)
    }
    fn batch_upsert(
        &self,
        tenant: &TenantId,
        items: Vec<(String, Value)>,
    ) -> Result<Vec<bool>, NgsiError> {
        AnyStore::batch_upsert(self, tenant, items)
    }
    fn upsert(
        &self,
        tenant: &TenantId,
        kind: Kind,
        id: &str,
        doc: Value,
    ) -> Result<bool, NgsiError> {
        AnyStore::upsert(self, tenant, kind, id, doc)
    }
    fn get(&self, tenant: &TenantId, kind: Kind, id: &str) -> Result<Option<Value>, NgsiError> {
        AnyStore::get(self, tenant, kind, id)
    }
    fn delete(&self, tenant: &TenantId, kind: Kind, id: &str) -> Result<bool, NgsiError> {
        AnyStore::delete(self, tenant, kind, id)
    }
    fn delete_entity_if(
        &self,
        tenant: &TenantId,
        id: &str,
        keep: &dyn Fn(&Value) -> bool,
    ) -> Result<bool, NgsiError> {
        AnyStore::delete_entity_if(self, tenant, id, keep)
    }
    fn list(&self, tenant: &TenantId, kind: Kind) -> Result<Vec<Value>, NgsiError> {
        AnyStore::list(self, tenant, kind)
    }
    fn list_page(
        &self,
        tenant: &TenantId,
        kind: Kind,
        after: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Value>, NgsiError> {
        AnyStore::list_page(self, tenant, kind, after, limit)
    }
    fn list_slice(
        &self,
        tenant: &TenantId,
        kind: Kind,
        offset: usize,
        limit: usize,
    ) -> Result<(Vec<Value>, usize), NgsiError> {
        AnyStore::list_slice(self, tenant, kind, offset, limit)
    }
    fn matching_registrations(
        &self,
        tenant: &TenantId,
        ids: Option<&[String]>,
        types: Option<&[String]>,
    ) -> Result<Vec<Value>, NgsiError> {
        AnyStore::matching_registrations(self, tenant, ids, types)
    }
    fn query_entities(
        &self,
        tenant: &TenantId,
        f: &crate::store::filter::EntityFilter<'_>,
    ) -> Result<crate::store::filter::QueryOutcome, NgsiError> {
        AnyStore::query_entities(self, tenant, f)
    }
    fn mutate_boxed<'a>(
        &self,
        tenant: &TenantId,
        kind: Kind,
        id: &str,
        f: antares_store::MutateFn<'a>,
    ) -> Result<Option<Result<(), ()>>, NgsiError> {
        AnyStore::mutate(self, tenant, kind, id, f)
    }
    fn batch_mutate_boxed<'a>(
        &self,
        tenant: &TenantId,
        ids: &[String],
        mut f: antares_store::BatchMutateFn<'a>,
    ) -> Result<Vec<Option<Result<(), ()>>>, NgsiError> {
        AnyStore::batch_mutate(self, tenant, ids, |id, v| f(id, v))
    }
    /// The Postgres arm answers this in ONE statement (see
    /// `pg::doc::record_delivery`); every other arm runs the shared rule as a
    /// `mutate`, which is what the one statement is a translation of.
    fn record_delivery(
        &self,
        tenant: &TenantId,
        kind: Kind,
        id: &str,
        now: &str,
    ) -> Result<Option<antares_store::Delivery>, NgsiError> {
        // `doc_kind` exists only with the Postgres arm, so its call lives
        // inside the gate with it: outside the feature this function is the
        // shared rule and nothing else.
        #[cfg(feature = "postgres")]
        {
            if let (AnyStore::Pg(p), Ok(dk)) = (self, doc_kind(kind)) {
                let found = p.docs.record_delivery(tenant, dk, id, now).map_err(db)?;
                return Ok(
                    found.map(|(doc, prev_success)| antares_store::Delivery { doc, prev_success })
                );
            }
        }
        antares_store::record_delivery_via_mutate(self, tenant, kind, id, now)
    }
    fn sweep_expired(&self) -> usize {
        AnyStore::sweep_expired(self)
    }
    fn tenant_exists(&self, tenant: &TenantId) -> Result<bool, NgsiError> {
        AnyStore::tenant_exists(self, tenant)
    }
    fn subscription_tenants(&self) -> Result<Vec<String>, NgsiError> {
        AnyStore::subscription_tenants(self)
    }
    fn tenant_ids(&self) -> Result<Vec<String>, NgsiError> {
        AnyStore::tenant_ids(self)
    }
    fn tenant_stats_one(
        &self,
        tenant: &TenantId,
    ) -> Result<Option<antares_store::TenantStats>, NgsiError> {
        AnyStore::tenant_stats_one(self, tenant)
    }
    fn purge_tenant(&self, tenant: &TenantId) -> Result<bool, NgsiError> {
        AnyStore::purge_tenant(self, tenant)
    }
    fn context_put(&self, id: &str, doc: Value) -> Result<(), NgsiError> {
        AnyStore::context_put(self, id, doc)
    }
    fn context_get(&self, id: &str) -> Result<Option<Value>, NgsiError> {
        AnyStore::context_get(self, id)
    }
    fn context_delete(&self, id: &str) -> Result<bool, NgsiError> {
        AnyStore::context_delete(self, id)
    }
    fn context_list_meta(&self) -> Result<Vec<Value>, NgsiError> {
        AnyStore::context_list_meta(self)
    }
}

impl antares_store::TemporalDriver for AnyStore {
    fn close<'a>(&'a self) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
        Box::pin(AnyStore::close(self))
    }
    fn version_info(&self) -> Value {
        AnyStore::version_info(self)
    }
    fn attr_instance_count(&self, tenant: &TenantId) -> Result<u64, NgsiError> {
        match self {
            AnyStore::Mem(s) => Ok(s.attr_instance_count(tenant)),
            #[cfg(feature = "postgres")]
            AnyStore::Pg(p) => super::pg::entity::wait(async {
                let mut tx = p.docs.pool().begin().await.map_err(db)?;
                crate::store::pg::set_tenant(&mut tx, tenant)
                    .await
                    .map_err(db)?;
                let n: i64 =
                    sqlx::query_scalar("SELECT count(*) FROM attr_instances WHERE tenant_id = $1")
                        .bind(tenant.as_str())
                        .fetch_one(&mut *tx)
                        .await
                        .map_err(db)?;
                Ok(n as u64)
            }),
        }
    }
    fn purge_tenant(&self, tenant: &TenantId) -> Result<(), NgsiError> {
        match self {
            AnyStore::Mem(s) => {
                s.purge_kinds(tenant, &[Kind::Temporal]);
                Ok(())
            }
            #[cfg(feature = "postgres")]
            AnyStore::Pg(p) => super::pg::entity::wait(async {
                let mut tx = p.docs.pool().begin().await.map_err(db)?;
                crate::store::pg::set_tenant(&mut tx, tenant)
                    .await
                    .map_err(db)?;
                for table in ["attr_instances", "temporal_entities"] {
                    sqlx::query(sqlx::AssertSqlSafe(format!(
                        "DELETE FROM {table} WHERE tenant_id = $1"
                    )))
                    .bind(tenant.as_str())
                    .execute(&mut *tx)
                    .await
                    .map_err(db)?;
                }
                tx.commit().await.map_err(db)
            }),
        }
    }
    fn temporal_append(
        &self,
        tenant: &TenantId,
        id: &str,
        shell: &Value,
        additions: &Value,
    ) -> Result<(), NgsiError> {
        AnyStore::temporal_append(self, tenant, id, shell, additions)
    }
    /// Only the Postgres arm pages in SQL, and only an exact prefilter
    /// makes that page the evaluator's page; the memory arm answers
    /// `paged: false` and the caller pages.
    fn q_pushdown_exact(
        &self,
        q: &antares_ql::QNode,
        range: Option<&crate::store::filter::InstanceRange<'_>>,
        expand: &dyn Fn(&str) -> String,
    ) -> bool {
        match self {
            // the memory arm pages nothing, so the filter is not its business
            AnyStore::Mem(_) => {
                let _ = (q, range, expand);
                false
            }
            #[cfg(feature = "postgres")]
            AnyStore::Pg(_) => crate::compile::qprefilter::prefilter_exact(q, range, expand),
        }
    }
    fn query_temporal(
        &self,
        tenant: &TenantId,
        f: &crate::store::filter::TemporalFilter<'_>,
    ) -> Result<crate::store::filter::TemporalOutcome, NgsiError> {
        AnyStore::query_temporal(self, tenant, f)
    }
    fn get_temporal(
        &self,
        tenant: &TenantId,
        id: &str,
        f: &crate::store::filter::TemporalFilter<'_>,
    ) -> Result<Option<Value>, NgsiError> {
        AnyStore::get_temporal(self, tenant, id, f)
    }
    fn get(&self, tenant: &TenantId, id: &str) -> Result<Option<Value>, NgsiError> {
        AnyStore::get(self, tenant, Kind::Temporal, id)
    }
    fn create(&self, tenant: &TenantId, id: &str, doc: Value) -> Result<bool, NgsiError> {
        AnyStore::create(self, tenant, Kind::Temporal, id, doc)
    }
    fn upsert(&self, tenant: &TenantId, id: &str, doc: Value) -> Result<bool, NgsiError> {
        AnyStore::upsert(self, tenant, Kind::Temporal, id, doc)
    }
    fn delete(&self, tenant: &TenantId, id: &str) -> Result<bool, NgsiError> {
        AnyStore::delete(self, tenant, Kind::Temporal, id)
    }
    fn list(&self, tenant: &TenantId) -> Result<Vec<Value>, NgsiError> {
        AnyStore::list(self, tenant, Kind::Temporal)
    }
    fn mutate_boxed<'a>(
        &self,
        tenant: &TenantId,
        id: &str,
        f: antares_store::MutateFn<'a>,
    ) -> Result<Option<Result<(), ()>>, NgsiError> {
        AnyStore::mutate(self, tenant, Kind::Temporal, id, f)
    }
}

#[cfg(test)]
mod temporal_only_tests {
    use super::*;
    use antares_model::TenantId;

    fn append(store: &AnyStore) -> Option<Value> {
        let t = TenantId::new("combo").expect("tenant");
        let shell = serde_json::json!({"id": "urn:a", "type": ["T"]});
        let adds = serde_json::json!({"speed": [{"instanceId": "urn:i:1", "value": 1}]});
        store
            .temporal_append(&t, "urn:a", &shell, &adds)
            .expect("append");
        store.get(&t, Kind::Temporal, "urn:a").expect("get")
    }

    /// A shared instance records only for an entity it holds (5.6.6 delete
    /// overlap); the temporal half of a mixed deployment records
    /// unconditionally, since the entities live elsewhere.
    #[test]
    fn a_temporal_only_instance_records_without_the_entity() {
        assert!(append(&AnyStore::Mem(Store::default())).is_none());
        let doc = append(&AnyStore::Mem(Store::default()).temporal_only()).expect("recorded");
        assert_eq!(doc["speed"][0]["value"], 1);
    }
}
