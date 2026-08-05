//! The store seam (tasks.md A1/A3): ONE closed set of backends behind the
//! memory store's 12-method surface. An enum, not a trait — `mutate<T, E>` is
//! generic (dyn-incompatible), the backend set is closed by design (§8.2
//! philosophy: exactly the implementations the product needs), and match
//! exhaustiveness forces every backend to answer every method.
//!
//! Every method returns `Result<_, NgsiError>`: the memory/file backend never
//! errors; the postgres backend maps sqlx failures to `InternalError` (500) —
//! a DB outage must be a visible 5xx, never a silent 404/409.

use antares_model::{NgsiError, TenantId};
use serde_json::Value;

use super::pg_doc::{DocKind, PgDocStore};
use super::pg_entity::PgEntityStore;
use super::pg_temporal::PgTemporalStore;
use super::{ChangeHook, Kind, Store};

fn db(e: sqlx::Error) -> NgsiError {
    NgsiError::InternalError(format!("database error: {e}"))
}

fn doc_kind(kind: Kind) -> Option<DocKind> {
    match kind {
        Kind::Subscription => Some(DocKind::Subscription),
        Kind::Registration => Some(DocKind::Registration),
        Kind::CSourceSubscription => Some(DocKind::CSourceSubscription),
        Kind::Entity | Kind::Temporal => None,
    }
}

/// Postgres backend bundle: one pool, three table-family stores, plus the
/// change hook (the memory store emits its own; here the seam emits).
pub struct PgBackend {
    pub entities: PgEntityStore,
    pub temporal: PgTemporalStore,
    pub docs: PgDocStore,
    hook: std::sync::RwLock<Option<ChangeHook>>,
}

impl PgBackend {
    pub fn new(pool: sqlx::postgres::PgPool) -> Self {
        Self {
            entities: PgEntityStore::new(pool.clone()),
            temporal: PgTemporalStore::new(pool.clone()),
            docs: PgDocStore::new(pool),
            hook: std::sync::RwLock::new(None),
        }
    }

    fn emit(&self, tenant: &TenantId, before: Option<Value>, after: Option<Value>) {
        if let Some(h) = self.hook.read().expect("hook lock").as_ref() {
            h(tenant, before, after);
        }
    }
}

/// A1/A3: what `antares-api` sees. No core crate names redb or sqlx.
// One AnyStore exists per process — variant size difference is irrelevant.
#[allow(clippy::large_enum_variant)]
pub enum AnyStore {
    Mem(Store),
    Pg(PgBackend),
}

impl AnyStore {
    /// B13: (queued writers, peak) of the memory/file write-critical section;
    /// `None` for the Pg arm (Postgres has no single-writer commit queue).
    pub fn commit_queue(&self) -> Option<(usize, usize)> {
        match self {
            AnyStore::Mem(s) => Some(s.commit_queue()),
            AnyStore::Pg(_) => None,
        }
    }

    /// K1, the last step of the drain: close the connection pool so in-flight
    /// transactions finish and the server sees a clean disconnect instead of
    /// N abandoned backends. A no-op for the memory/file arm, whose durability
    /// is already commit-before-ack (B3) — there is nothing buffered to lose.
    pub async fn close(&self) {
        match self {
            AnyStore::Mem(_) => {}
            AnyStore::Pg(p) => p.docs.pool().close().await,
        }
    }

    pub fn set_change_hook(&self, h: ChangeHook) {
        match self {
            AnyStore::Mem(s) => s.set_change_hook(h),
            AnyStore::Pg(p) => *p.hook.write().expect("hook lock") = Some(h),
        }
    }

    /// F3: turn the same-tx outbox producer on (bus=nats). The memory arm has
    /// no outbox — the broker's wiring rejects bus=nats without a Pg store,
    /// so this is unreachable there by construction.
    pub fn set_outbox(&self, on: bool) {
        match self {
            AnyStore::Mem(_) => {}
            AnyStore::Pg(p) => p.entities.set_outbox(on),
        }
    }

    /// F3 drain: oldest-first page of pending outbox rows `(seq, tenant, event)`.
    pub fn outbox_peek(&self, limit: i64) -> Result<Vec<(i64, String, Value)>, NgsiError> {
        match self {
            AnyStore::Mem(_) => Ok(Vec::new()),
            AnyStore::Pg(p) => super::outbox::peek(p.docs.pool(), limit).map_err(db),
        }
    }

    /// F3 drain: delete everything published up to and including `seq`.
    pub fn outbox_ack(&self, seq: i64) -> Result<u64, NgsiError> {
        match self {
            AnyStore::Mem(_) => Ok(0),
            AnyStore::Pg(p) => super::outbox::ack(p.docs.pool(), seq).map_err(db),
        }
    }

    /// §3.1.4/6.3.14 implicit tenant creation on Pg write paths.
    fn ensure_tenant(p: &PgBackend, tenant: &TenantId) -> Result<(), NgsiError> {
        super::pg_entity::wait(async {
            crate::pg::ensure_tenant(p.docs.pool(), tenant)
                .await
                .map_err(db)
        })
    }

    pub fn create(
        &self,
        tenant: &TenantId,
        kind: Kind,
        id: &str,
        doc: Value,
    ) -> Result<bool, NgsiError> {
        match self {
            AnyStore::Mem(s) => Ok(s.create(tenant, kind, id, doc)),
            AnyStore::Pg(p) => {
                Self::ensure_tenant(p, tenant)?;
                let created = match kind {
                    Kind::Entity => p.entities.create(tenant, id, &doc).map_err(db)?,
                    Kind::Temporal => p.temporal.create(tenant, id, &doc).map_err(db)?,
                    _ => {
                        let dk = doc_kind(kind).expect("doc kind");
                        if p.docs.get(tenant, dk, id).map_err(db)?.is_some() {
                            false
                        } else {
                            p.docs.upsert(tenant, dk, id, &doc).map_err(db)?;
                            true
                        }
                    }
                };
                if created && kind == Kind::Entity {
                    p.emit(tenant, None, Some(doc));
                }
                Ok(created)
            }
        }
    }

    /// C5 batch create (entities only): one multi-row statement on the Pg
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
            AnyStore::Pg(p) => {
                Self::ensure_tenant(p, tenant)?;
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

    /// C5 batch delete (entities only): deleted-flags in input order; a
    /// duplicate id in the input deletes once and 404s the second time,
    /// matching the per-item loop's semantics (5.5.11.4).
    pub fn batch_delete(&self, tenant: &TenantId, ids: &[String]) -> Result<Vec<bool>, NgsiError> {
        match self {
            AnyStore::Mem(s) => Ok(ids
                .iter()
                .map(|id| s.delete(tenant, Kind::Entity, id))
                .collect()),
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
            AnyStore::Pg(p) => match kind {
                _ if Self::ensure_tenant(p, tenant).is_err() => Err(NgsiError::InternalError(
                    "tenant provisioning failed".into(),
                )),
                Kind::Entity => {
                    let prev = p.entities.get(tenant, id).map_err(db)?;
                    let existed = prev.is_some();
                    if existed {
                        let r = p
                            .entities
                            .mutate(tenant, id, |d| {
                                *d = doc.clone();
                                Ok::<(), std::convert::Infallible>(())
                            })
                            .map_err(db)?;
                        debug_assert!(r.is_some());
                    } else if !p.entities.create(tenant, id, &doc).map_err(db)? {
                        // lost the create race — replace instead
                        p.entities
                            .mutate(tenant, id, |d| {
                                *d = doc.clone();
                                Ok::<(), std::convert::Infallible>(())
                            })
                            .map_err(db)?;
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
                    let dk = doc_kind(kind).expect("doc kind");
                    p.docs.upsert(tenant, dk, id, &doc).map_err(db)
                }
            },
        }
    }

    pub fn get(&self, tenant: &TenantId, kind: Kind, id: &str) -> Result<Option<Value>, NgsiError> {
        match self {
            AnyStore::Mem(s) => Ok(s.get(tenant, kind, id)),
            AnyStore::Pg(p) => match kind {
                Kind::Entity => p.entities.get(tenant, id).map_err(db),
                Kind::Temporal => p.temporal.get(tenant, id).map_err(db),
                _ => p
                    .docs
                    .get(tenant, doc_kind(kind).expect("doc kind"), id)
                    .map_err(db),
            },
        }
    }

    pub fn delete(&self, tenant: &TenantId, kind: Kind, id: &str) -> Result<bool, NgsiError> {
        match self {
            AnyStore::Mem(s) => Ok(s.delete(tenant, kind, id)),
            AnyStore::Pg(p) => match kind {
                Kind::Entity => {
                    let prev = p.entities.get(tenant, id).map_err(db)?;
                    let hit = p.entities.delete(tenant, id).map_err(db)?;
                    if hit {
                        p.emit(tenant, prev, None);
                    }
                    Ok(hit)
                }
                Kind::Temporal => p.temporal.delete(tenant, id).map_err(db),
                _ => p
                    .docs
                    .delete(tenant, doc_kind(kind).expect("doc kind"), id)
                    .map_err(db),
            },
        }
    }

    pub fn list(&self, tenant: &TenantId, kind: Kind) -> Result<Vec<Value>, NgsiError> {
        match self {
            AnyStore::Mem(s) => Ok(s.list(tenant, kind)),
            AnyStore::Pg(p) => match kind {
                Kind::Entity => p.entities.list(tenant).map_err(db),
                Kind::Temporal => p.temporal.list(tenant).map_err(db),
                _ => p
                    .docs
                    .list(tenant, doc_kind(kind).expect("doc kind"))
                    .map_err(db),
            },
        }
    }

    /// C10/C11: Query Entities with the filter pushed down where the backend
    /// can take it. `memory`/`file` have nothing to push into — their
    /// entities are already in RAM — so they return the same snapshot `list`
    /// does (never `decided`, never `paged`). Either way the caller applies
    /// the exact filter afterwards unless the outcome says SQL already did.
    pub fn query_entities(
        &self,
        tenant: &TenantId,
        f: &crate::store::pg_entity::EntityFilter<'_>,
    ) -> Result<crate::store::pg_entity::QueryOutcome, NgsiError> {
        match self {
            AnyStore::Mem(s) => Ok(crate::store::pg_entity::QueryOutcome {
                rows: s.list(tenant, Kind::Entity),
                decided: false,
                paged: false,
                total: None,
            }),
            AnyStore::Pg(p) => p.entities.query(tenant, f).map_err(db),
        }
    }

    /// C11: Query Temporal Evolution with entity narrowing + instance-window
    /// pruning pushed down. Same contract: the API's window() is the arbiter;
    /// the memory arm returns the full snapshot.
    pub fn query_temporal(
        &self,
        tenant: &TenantId,
        f: &crate::store::pg_temporal::TemporalFilter<'_>,
    ) -> Result<Vec<Value>, NgsiError> {
        match self {
            AnyStore::Mem(s) => Ok(s.list(tenant, Kind::Temporal)),
            AnyStore::Pg(p) => p.temporal.query(tenant, f).map_err(db),
        }
    }

    /// C11: Retrieve Temporal Evolution with the same instance pruning.
    pub fn get_temporal(
        &self,
        tenant: &TenantId,
        id: &str,
        f: &crate::store::pg_temporal::TemporalFilter<'_>,
    ) -> Result<Option<Value>, NgsiError> {
        match self {
            AnyStore::Mem(s) => Ok(s.get(tenant, Kind::Temporal, id)),
            AnyStore::Pg(p) => p.temporal.get_range(tenant, id, f).map_err(db),
        }
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
            AnyStore::Pg(p) => match kind {
                Kind::Entity => {
                    // before/after captured for the change hook (§7 prev_payload).
                    let before = p.entities.get(tenant, id).map_err(db)?;
                    let mut after: Option<Value> = None;
                    let r = p
                        .entities
                        .mutate(tenant, id, |d| {
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
                    let dk = doc_kind(kind).expect("doc kind");
                    match p.docs.mutate(tenant, dk, id, f).map_err(db)? {
                        Some(r) => Ok(Some(r)),
                        None => Ok(None),
                    }
                }
            },
        }
    }

    pub fn subscription_tenants(&self) -> Result<Vec<String>, NgsiError> {
        match self {
            AnyStore::Mem(s) => Ok(s.subscription_tenants()),
            // Pg: all known tenants (tenants table carries no RLS); the
            // interval scan lists per tenant under set_tenant anyway.
            AnyStore::Pg(p) => super::pg_entity::wait(async {
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
            AnyStore::Pg(p) => p.docs.context_get(id).map_err(db),
        }
    }

    pub fn context_delete(&self, id: &str) -> Result<bool, NgsiError> {
        match self {
            AnyStore::Mem(s) => Ok(s.context_delete(id)),
            AnyStore::Pg(p) => p.docs.context_delete(id).map_err(db),
        }
    }

    pub fn context_list(&self) -> Result<Vec<Value>, NgsiError> {
        match self {
            AnyStore::Mem(s) => Ok(s.context_list()),
            AnyStore::Pg(p) => p.docs.context_list().map_err(db),
        }
    }
}
