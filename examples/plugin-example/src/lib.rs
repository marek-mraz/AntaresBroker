// SPDX-License-Identifier: EUPL-1.2
//! A driver plugin, written the way one from outside this repository would
//! be: nothing here is reachable from a core crate, and the broker mounts it
//! through the same four seams any deployment has — the two storage driver
//! traits, one `ApiSurface`, one `NotificationSink`, one `PolicyEngine`.
//!
//! The store is deliberately the simplest thing that is CORRECT rather than
//! the fastest: one ordered map keyed by (tenant, kind, id), no indexes, no
//! pushdown. That is allowed on purpose. `query_entities` answers
//! `decided: false` and the API re-applies every predicate itself, so a
//! driver may over-return but never drop a matching row and never cross a
//! tenant. What it may NOT skip is the contract in
//! `antares_store::contract`, which `tests/contract.rs` runs against it.
#![cfg_attr(not(test), warn(clippy::expect_used))]

use antares_model::{NgsiError, TenantId};
use antares_store::{
    context_row_visible, filter, BatchMutateFn, ChangeHook, CurrentStateDriver, Kind, MutateFn,
    TemporalDriver, TenantStats,
};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::RwLock;

mod policy;
mod sink;
mod surface;
pub use policy::{ExamplePolicy, POLICY_NAME, RULES_ENV};
pub use sink::MemorySink;
pub use surface::ExampleSurface;

/// The name this driver is selected by (`ANTARES_STORE=example`).
pub const NAME: &str = "example";

/// One document slot. The kind is a number so the whole store is ONE
/// ordered map: a tenant's documents of one kind are a contiguous range.
type Slot = (String, u8, String);

fn slot(kind: Kind) -> u8 {
    // Exhaustive on purpose: a kind added to the core seam must be placed
    // here, not silently filed under a neighbour.
    match kind {
        Kind::Entity => 0,
        Kind::Subscription => 1,
        Kind::Registration => 2,
        Kind::CSourceSubscription => 3,
        Kind::Temporal => 4,
        Kind::Snapshot => 5,
        Kind::EntityMap => 6,
        Kind::DistSub => 7,
        Kind::DeadLetter => 8,
    }
}

/// A tenant's rows of one kind, as a half-open range up to the next kind.
/// The end has to be a key no id can reach rather than the largest id anyone
/// expects: an id sorting above the bound would be stored, retrievable by id
/// and missing from every listing, query and page. `slot` never answers with
/// the top of the type, so the successor exists.
fn range(tenant: &TenantId, kind: Kind) -> std::ops::Range<Slot> {
    let k = slot(kind);
    (tenant.as_str().to_owned(), k, String::new())
        ..(tenant.as_str().to_owned(), k + 1, String::new())
}

/// Now, as the broker spells timestamps (4.11): UTC, milliseconds, `Z`.
fn now() -> String {
    antares_api::state::now_iso()
}

/// 4.22 at a WRITE: an entity past its `expiresAt` is not there, so a
/// create over it succeeds and an upsert reports a creation rather than a
/// silent replace. ONLY entities carry that stamp — a subscription past
/// its `expiresAt` stays retrievable with status `expired` (5.8.6), and a
/// registration has its own rule — so treating every kind alike here
/// answers 404 where the spec mandates a body.
fn gone(kind: Kind, doc: &Value, now: &str) -> bool {
    kind == Kind::Entity && filter::expired_at(doc, now)
}

/// 4.22 at a READ. The entity-level stamp hides the whole document, and
/// attribute instances expire independently of the entity holding them —
/// on the entity and on its history alike. Reads never serve expired
/// context, whether or not the sweep has come round yet.
fn hidden(kind: Kind, doc: &mut Value, now: &str) -> bool {
    match kind {
        Kind::Entity | Kind::Temporal => filter::strip_expired(doc, now),
        _ => false,
    }
}

/// The plugin's storage. One lock, one map, no cleverness.
#[derive(Default)]
pub struct ExampleStore {
    docs: RwLock<BTreeMap<Slot, Value>>,
    /// 5.13 `@context` documents are broker-wide, not tenant-scoped.
    contexts: RwLock<BTreeMap<String, Value>>,
    /// 5.5.10: a tenant exists from its first write and keeps existing.
    tenants: RwLock<BTreeSet<String>>,
    hook: RwLock<Option<ChangeHook>>,
}

impl ExampleStore {
    /// A fresh, empty store.
    pub fn new() -> Self {
        Self::default()
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, BTreeMap<Slot, Value>> {
        self.docs
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn write(&self) -> std::sync::RwLockWriteGuard<'_, BTreeMap<Slot, Value>> {
        self.docs
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn note_tenant(&self, tenant: &TenantId) {
        self.tenants
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(tenant.as_str().to_owned());
    }

    /// The (tenant, before, after) change feed the in-process matcher runs
    /// on. Entities only — a subscription write is not an entity change.
    async fn emit(&self, tenant: &TenantId, before: Option<Value>, after: Option<Value>) {
        // cloned out of the lock first: a read guard cannot be held across
        // the await the hook now costs.
        let hook = self
            .hook
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if let Some(h) = hook {
            h(tenant, before, after).await;
        }
    }

    fn live(&self, tenant: &TenantId, kind: Kind, id: &str) -> Option<Value> {
        let now = now();
        let mut doc = self
            .read()
            .get(&(tenant.as_str().to_owned(), slot(kind), id.to_owned()))
            .cloned()?;
        (!hidden(kind, &mut doc, &now)).then_some(doc)
    }

    fn rows(&self, tenant: &TenantId, kind: Kind) -> Vec<Value> {
        let now = now();
        self.read()
            .range(range(tenant, kind))
            .map(|(_, v)| v.clone())
            .filter_map(|mut d| (!hidden(kind, &mut d, &now)).then_some(d))
            .collect()
    }
}

#[async_trait::async_trait]
impl CurrentStateDriver for ExampleStore {
    async fn ping(&self) -> Result<(), NgsiError> {
        Ok(())
    }

    async fn close(&self) {}

    fn version_info(&self) -> Value {
        json!({"engine": NAME, "storage": "in-process ordered map"})
    }

    fn set_change_hook(&self, h: ChangeHook) {
        *self
            .hook
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(h);
    }

    async fn create(
        &self,
        tenant: &TenantId,
        kind: Kind,
        id: &str,
        doc: Value,
    ) -> Result<bool, NgsiError> {
        self.note_tenant(tenant);
        let key = (tenant.as_str().to_owned(), slot(kind), id.to_owned());
        let now = now();
        {
            let mut docs = self.write();
            // An expired row is not a row: a create over one succeeds (4.22).
            if docs.get(&key).is_some_and(|d| !gone(kind, d, &now)) {
                return Ok(false);
            }
            docs.insert(key, doc.clone());
        }
        if kind == Kind::Entity {
            self.emit(tenant, None, Some(doc)).await;
        }
        Ok(true)
    }

    async fn batch_create(
        &self,
        tenant: &TenantId,
        items: Vec<(String, Value)>,
    ) -> Result<Vec<bool>, NgsiError> {
        let mut out = Vec::with_capacity(items.len());
        for (id, doc) in items {
            out.push(CurrentStateDriver::create(self, tenant, Kind::Entity, &id, doc).await?);
        }
        Ok(out)
    }

    async fn batch_delete(
        &self,
        tenant: &TenantId,
        ids: &[String],
    ) -> Result<Vec<bool>, NgsiError> {
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            out.push(CurrentStateDriver::delete(self, tenant, Kind::Entity, id).await?);
        }
        Ok(out)
    }

    async fn batch_upsert(
        &self,
        tenant: &TenantId,
        items: Vec<(String, Value)>,
    ) -> Result<Vec<bool>, NgsiError> {
        // Created-flags: the opposite polarity of `upsert`, which answers
        // whether the row was already there. The batch path splits 201 from
        // 204 (5.6.8) with these.
        let mut out = Vec::with_capacity(items.len());
        for (id, doc) in items {
            out.push(!CurrentStateDriver::upsert(self, tenant, Kind::Entity, &id, doc).await?);
        }
        Ok(out)
    }

    async fn upsert(
        &self,
        tenant: &TenantId,
        kind: Kind,
        id: &str,
        doc: Value,
    ) -> Result<bool, NgsiError> {
        self.note_tenant(tenant);
        let key = (tenant.as_str().to_owned(), slot(kind), id.to_owned());
        let now = now();
        let prev = self.write().insert(key, doc.clone());
        let existed = prev.as_ref().is_some_and(|d| !gone(kind, d, &now));
        if kind == Kind::Entity {
            self.emit(tenant, existed.then_some(prev).flatten(), Some(doc))
                .await;
        }
        Ok(existed)
    }

    async fn get(
        &self,
        tenant: &TenantId,
        kind: Kind,
        id: &str,
    ) -> Result<Option<Value>, NgsiError> {
        Ok(self.live(tenant, kind, id))
    }

    async fn delete(&self, tenant: &TenantId, kind: Kind, id: &str) -> Result<bool, NgsiError> {
        let now = now();
        let prev = self
            .write()
            .remove(&(tenant.as_str().to_owned(), slot(kind), id.to_owned()));
        let existed = prev.as_ref().is_some_and(|d| !gone(kind, d, &now));
        if kind == Kind::Entity && existed {
            self.emit(tenant, prev, None).await;
        }
        Ok(existed)
    }

    /// The read and the removal share one hold of the map's lock, so nothing
    /// can replace the Entity between the decision and the delete. A backend
    /// that reads first and deletes after keeps that window open, which is
    /// why the trait requires this method rather than defaulting it.
    async fn delete_entity_if(
        &self,
        tenant: &TenantId,
        id: &str,
        keep: &(dyn for<'v> Fn(&'v Value) -> bool + Sync),
    ) -> Result<bool, NgsiError> {
        let now = now();
        let key = (
            tenant.as_str().to_owned(),
            slot(Kind::Entity),
            id.to_owned(),
        );
        let prev = {
            let mut rows = self.write();
            match rows.get(&key) {
                Some(d) if !gone(Kind::Entity, d, &now) && keep(d) => rows.remove(&key),
                _ => None,
            }
        };
        let existed = prev.is_some();
        if existed {
            self.emit(tenant, prev, None).await;
        }
        Ok(existed)
    }

    async fn list(&self, tenant: &TenantId, kind: Kind) -> Result<Vec<Value>, NgsiError> {
        Ok(self.rows(tenant, kind))
    }

    /// One id-ordered page, ids strictly greater than `after`. This is the
    /// read the broker's mirror hydration uses, and it must never refuse a
    /// tenant for its size: a mirror filled from a short page silently stops
    /// matching that tenant's subscriptions. The map is ordered by
    /// `(tenant, kind, id)`, so the walk is a range and the cursor is a
    /// comparison on the key — no row is cloned before it is wanted.
    async fn list_page(
        &self,
        tenant: &TenantId,
        kind: Kind,
        after: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Value>, NgsiError> {
        let now = now();
        Ok(self
            .read()
            .range(range(tenant, kind))
            .filter(|(k, _)| after.is_none_or(|a| k.2.as_str() > a))
            .filter_map(|(_, v)| {
                let mut d = v.clone();
                (!hidden(kind, &mut d, &now)).then_some(d)
            })
            .take(limit)
            .collect())
    }

    /// 5.5.9.2's `limit`/`offset` window, and the size of the whole set for
    /// 6.3.10's `count`. Both come from one read so they describe the same
    /// set; taken separately they can disagree under a concurrent write.
    ///
    /// ponytail: the window is sliced from the tenant's rows, so this walks
    /// the tenant once per call. The storage here is a single in-process map
    /// that is resident anyway, which is why that is acceptable in a
    /// reference plugin and would not be in a database-backed one — copy the
    /// semantics, not the strategy: a backend with a query language pushes
    /// LIMIT/OFFSET and count(*) down and reads neither more nor less.
    async fn list_slice(
        &self,
        tenant: &TenantId,
        kind: Kind,
        offset: usize,
        limit: usize,
    ) -> Result<(Vec<Value>, usize), NgsiError> {
        let all = self.rows(tenant, kind);
        let total = all.len();
        Ok((all.into_iter().skip(offset).take(limit).collect(), total))
    }

    async fn matching_registrations(
        &self,
        tenant: &TenantId,
        _ids: Option<&[String]>,
        _types: Option<&[String]>,
    ) -> Result<Vec<Value>, NgsiError> {
        // Narrowing is a backend's optimisation, never its obligation:
        // the whole tenant list is always a correct answer.
        Ok(self.rows(tenant, Kind::Registration))
    }

    async fn query_entities(
        &self,
        tenant: &TenantId,
        _f: &filter::EntityFilter<'_>,
    ) -> Result<filter::QueryOutcome, NgsiError> {
        // `decided: false` hands every predicate back to the API, which
        // re-applies all of them. Over-returning is allowed; dropping a
        // matching row or crossing a tenant is not.
        Ok(filter::QueryOutcome {
            rows: self.rows(tenant, Kind::Entity),
            decided: false,
            paged: false,
            total: None,
        })
    }

    async fn mutate_boxed<'a>(
        &self,
        tenant: &TenantId,
        kind: Kind,
        id: &str,
        f: MutateFn<'a>,
    ) -> Result<Option<Result<(), ()>>, NgsiError> {
        let key = (tenant.as_str().to_owned(), slot(kind), id.to_owned());
        let now = now();
        let (before, next, verdict) = {
            let mut docs = self.write();
            // ADR-0005 / ETSI 047_06: read-modify-write under ONE lock, and a
            // missing row is None — never an insert. A get-then-upsert here
            // would let a bookkeeping writeback resurrect a deleted row.
            let Some(current) = docs.get(&key).filter(|d| !gone(kind, d, &now)) else {
                return Ok(None);
            };
            let before = current.clone();
            let mut next = current.clone();
            let verdict = f(&mut next);
            if verdict.is_ok() {
                docs.insert(key, next.clone());
            }
            (before, next, verdict)
        };
        if verdict.is_ok() && kind == Kind::Entity {
            self.emit(tenant, Some(before), Some(next)).await;
        }
        Ok(Some(verdict))
    }

    async fn batch_mutate_boxed<'a>(
        &self,
        tenant: &TenantId,
        ids: &[String],
        mut f: BatchMutateFn<'a>,
    ) -> Result<Vec<Option<Result<(), ()>>>, NgsiError> {
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            out.push(
                CurrentStateDriver::mutate_boxed(
                    self,
                    tenant,
                    Kind::Entity,
                    id,
                    Box::new(|v| f(id, v)),
                )
                .await?,
            );
        }
        Ok(out)
    }

    async fn sweep_expired(&self) -> usize {
        // 4.22: the reads above already hide expired context; this is the
        // memory the sweep gets back — whole entities past their stamp, and
        // the expired instances still sitting inside live documents.
        let now = now();
        let entity = slot(Kind::Entity);
        let mut docs = self.write();
        let dead: Vec<Slot> = docs
            .iter()
            .filter(|((_, k, _), d)| *k == entity && filter::expired_at(d, &now))
            .map(|(k, _)| k.clone())
            .collect();
        for k in &dead {
            docs.remove(k);
        }
        let temporal = slot(Kind::Temporal);
        let mut pruned = 0;
        for ((_, k, _), doc) in docs.iter_mut() {
            if *k != entity && *k != temporal {
                continue;
            }
            let before = doc.clone();
            filter::strip_expired(doc, &now);
            if *doc != before {
                pruned += 1;
            }
        }
        dead.len() + pruned
    }

    async fn tenant_exists(&self, tenant: &TenantId) -> Result<bool, NgsiError> {
        Ok(tenant.as_str() == TenantId::DEFAULT
            || self
                .tenants
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .contains(tenant.as_str()))
    }

    /// The iteration domain of the interval sweep and of every mirror
    /// hydration. All three kinds a mirror is built from, not just
    /// subscriptions: the registration mirror hydrates from this domain too,
    /// and the federation path reads that mirror alone once it is installed,
    /// so a tenant missing here forwards to no Context Source and never
    /// fires a periodic notification — with no error anywhere. A superset is
    /// allowed and a subset is a silent outage, so when in doubt a backend
    /// returns more.
    async fn subscription_tenants(&self) -> Result<Vec<String>, NgsiError> {
        let kinds = [
            slot(Kind::Subscription),
            slot(Kind::CSourceSubscription),
            slot(Kind::Registration),
        ];
        let out: BTreeSet<String> = self
            .read()
            .keys()
            .filter(|(_, kind, _)| kinds.contains(kind))
            .map(|(t, _, _)| t.clone())
            .collect();
        Ok(out.into_iter().collect())
    }

    async fn tenant_ids(&self) -> Result<Vec<String>, NgsiError> {
        let mut out: BTreeSet<String> = self
            .tenants
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        // 5.5.10: the default tenant exists whether or not it was written to.
        out.insert(TenantId::DEFAULT.to_owned());
        Ok(out.into_iter().collect())
    }

    async fn tenant_stats_one(&self, tenant: &TenantId) -> Result<Option<TenantStats>, NgsiError> {
        if !self.tenant_exists(tenant).await? {
            return Ok(None);
        }
        let count = |k: Kind| self.rows(tenant, k).len() as u64;
        Ok(Some(TenantStats {
            tenant: tenant.as_str().to_owned(),
            created_at: None,
            entities: count(Kind::Entity),
            subscriptions: count(Kind::Subscription),
            csource_subscriptions: count(Kind::CSourceSubscription),
            registrations: count(Kind::Registration),
            snapshots: count(Kind::Snapshot),
            entity_maps: count(Kind::EntityMap),
            dist_subs: count(Kind::DistSub),
        }))
    }

    async fn purge_tenant(&self, tenant: &TenantId) -> Result<bool, NgsiError> {
        if !self.tenant_exists(tenant).await? {
            return Ok(false);
        }
        // One hold of the lock: collecting the keys under a read guard and
        // removing them under a write guard leaves a window in which a row
        // written for this tenant survives the purge.
        self.write().retain(|(t, _, _), _| t != tenant.as_str());
        // The default tenant is emptied and keeps existing (5.5.10).
        if tenant.as_str() != TenantId::DEFAULT {
            self.tenants
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(tenant.as_str());
        }
        Ok(true)
    }

    /// ADR-0021: a `Cached` row belongs to no Tenant and every caller reaches
    /// it; every other kind belongs to the Tenant its `owner` member names.
    /// `context_row_visible` is the broker's own statement of that rule, so a
    /// plugin store enforces exactly what the built-in ones do — an
    /// implementation that ignored `tenant` here would hand one Tenant's term
    /// mappings to another.
    async fn context_put(
        &self,
        tenant: Option<&TenantId>,
        id: &str,
        doc: Value,
    ) -> Result<(), NgsiError> {
        let mut rows = self
            .contexts
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !rows
            .get(id)
            .is_none_or(|held| context_row_visible(held, tenant))
            || !context_row_visible(&doc, tenant)
        {
            return Err(NgsiError::InternalError(
                "@context belongs to another tenant".into(),
            ));
        }
        rows.insert(id.to_owned(), doc);
        Ok(())
    }

    async fn context_get(
        &self,
        tenant: Option<&TenantId>,
        id: &str,
    ) -> Result<Option<Value>, NgsiError> {
        Ok(self
            .contexts
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(id)
            .filter(|row| context_row_visible(row, tenant))
            .cloned())
    }

    async fn context_delete(&self, tenant: Option<&TenantId>, id: &str) -> Result<bool, NgsiError> {
        let mut rows = self
            .contexts
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !rows
            .get(id)
            .is_some_and(|row| context_row_visible(row, tenant))
        {
            return Ok(false);
        }
        Ok(rows.remove(id).is_some())
    }

    /// Rows without their `body`: a body may be 5 MiB and only the `Cached`
    /// rows are capped in number, so a plugin that returned whole rows here
    /// would put gigabytes on the boot path.
    async fn context_list_meta(&self, tenant: Option<&TenantId>) -> Result<Vec<Value>, NgsiError> {
        Ok(self
            .contexts
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values()
            .filter(|v| context_row_visible(v, tenant))
            .map(|v| {
                let mut row = v.clone();
                if let Some(o) = row.as_object_mut() {
                    o.remove("body");
                }
                row
            })
            .collect())
    }
}

#[async_trait::async_trait]
impl TemporalDriver for ExampleStore {
    async fn close(&self) {}

    fn version_info(&self) -> Value {
        json!({"engine": NAME, "storage": "in-process ordered map"})
    }

    async fn temporal_append(
        &self,
        tenant: &TenantId,
        id: &str,
        shell: &Value,
        additions: &Value,
    ) -> Result<(), NgsiError> {
        // 5.6.6: history belongs to an entity that still exists — an append
        // racing a delete must not recreate it.
        if CurrentStateDriver::get(self, tenant, Kind::Entity, id)
            .await?
            .is_none()
        {
            return Ok(());
        }
        if CurrentStateDriver::get(self, tenant, Kind::Temporal, id)
            .await?
            .is_none()
        {
            // The loser of a concurrent create race just extends below.
            let _ =
                CurrentStateDriver::create(self, tenant, Kind::Temporal, id, shell.clone()).await?;
        }
        CurrentStateDriver::mutate_boxed(
            self,
            tenant,
            Kind::Temporal,
            id,
            Box::new(|doc| {
                let Some(target) = doc.as_object_mut() else {
                    return Err(());
                };
                let Some(adds) = additions.as_object() else {
                    return Ok(());
                };
                for (attr, instances) in adds {
                    let incoming: Vec<Value> = instances.as_array().cloned().unwrap_or_default();
                    match target.get_mut(attr).and_then(Value::as_array_mut) {
                        // Same instanceId = the same instance corrected.
                        Some(kept) => {
                            for inst in incoming {
                                let iid = inst.get("instanceId");
                                match kept.iter_mut().find(|k| k.get("instanceId") == iid) {
                                    Some(slot) => *slot = inst,
                                    None => kept.push(inst),
                                }
                            }
                        }
                        None => {
                            target.insert(attr.clone(), Value::Array(incoming));
                        }
                    }
                }
                Ok(())
            }),
        )
        .await?;
        Ok(())
    }

    async fn query_temporal(
        &self,
        tenant: &TenantId,
        _f: &filter::TemporalFilter<'_>,
    ) -> Result<filter::TemporalOutcome, NgsiError> {
        // Same bargain as query_entities: hand back the tenant's history and
        // let the API window and page it. 4.22 is `rows`' job, not the
        // caller's — an expired instance is not history, it is gone.
        Ok(filter::TemporalOutcome {
            rows: self.rows(tenant, Kind::Temporal),
            paged: false,
            total: None,
            aggregated: false,
        })
    }

    async fn get_temporal(
        &self,
        tenant: &TenantId,
        id: &str,
        _f: &filter::TemporalFilter<'_>,
    ) -> Result<Option<Value>, NgsiError> {
        // `live` already applied 4.22 to the temporal document.
        Ok(self.live(tenant, Kind::Temporal, id))
    }

    async fn get(&self, tenant: &TenantId, id: &str) -> Result<Option<Value>, NgsiError> {
        CurrentStateDriver::get(self, tenant, Kind::Temporal, id).await
    }

    async fn create(&self, tenant: &TenantId, id: &str, doc: Value) -> Result<bool, NgsiError> {
        CurrentStateDriver::create(self, tenant, Kind::Temporal, id, doc).await
    }

    async fn upsert(&self, tenant: &TenantId, id: &str, doc: Value) -> Result<bool, NgsiError> {
        CurrentStateDriver::upsert(self, tenant, Kind::Temporal, id, doc).await
    }

    async fn delete(&self, tenant: &TenantId, id: &str) -> Result<bool, NgsiError> {
        CurrentStateDriver::delete(self, tenant, Kind::Temporal, id).await
    }

    async fn list(&self, tenant: &TenantId) -> Result<Vec<Value>, NgsiError> {
        CurrentStateDriver::list(self, tenant, Kind::Temporal).await
    }

    async fn mutate_boxed<'a>(
        &self,
        tenant: &TenantId,
        id: &str,
        f: MutateFn<'a>,
    ) -> Result<Option<Result<(), ()>>, NgsiError> {
        CurrentStateDriver::mutate_boxed(self, tenant, Kind::Temporal, id, f).await
    }

    async fn attr_instance_count(&self, tenant: &TenantId) -> Result<u64, NgsiError> {
        Ok(self
            .rows(tenant, Kind::Temporal)
            .iter()
            .filter_map(Value::as_object)
            .map(count_instances)
            .sum())
    }

    async fn purge_tenant(&self, tenant: &TenantId) -> Result<(), NgsiError> {
        // The current-state purge already drops every kind, this seam
        // included; nothing is left to do here.
        let _ = tenant;
        Ok(())
    }
}

/// Attribute instances of one temporal document: every member that is an
/// array of instance objects (`id`, `type` and `@context` are not).
fn count_instances(doc: &Map<String, Value>) -> u64 {
    doc.iter()
        .filter(|(k, _)| !matches!(k.as_str(), "id" | "type" | "@context"))
        .filter_map(|(_, v)| v.as_array())
        .map(|a| a.len() as u64)
        .sum()
}
