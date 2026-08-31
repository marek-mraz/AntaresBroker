// SPDX-License-Identifier: EUPL-1.2
//! The storage seam: two driver traits every backend implements, plus the
//! backend-neutral types they speak (resource kinds, filter shapes, the
//! store-mode enum). This crate names no backend — redb, sqlx and the
//! browser's OPFS all live behind these traits in their own crates.
//!
//! Current-state and temporal storage are SEPARATE interfaces on purpose:
//! a deployment may run postgres current-state with no temporal store at
//! all, or memory current-state with a database-backed history. A driver
//! that does not support an operation answers with an error (or a benign
//! no-op for internal bookkeeping), never a panic — `NoTemporal` is the
//! canonical instance.
#![deny(missing_docs)]

#[cfg(feature = "test-kit")]
pub mod contract;
pub mod filter;

use antares_model::{NgsiError, TenantId};
use serde_json::Value;

/// A stored document as the object it is. Every document a driver holds is a
/// JSON object — `contract` asserts the shape on every backend — so a value
/// that is not one means the driver returned something the contract forbids
/// underneath a live request. That fails the one request; it never takes the
/// process down, which is what an unwrap here would do.
pub fn stored_object(doc: &mut Value) -> Result<&mut serde_json::Map<String, Value>, NgsiError> {
    doc.as_object_mut()
        .ok_or_else(|| NgsiError::InternalError("stored document is not a JSON object".into()))
}

/// Called with (tenant, before, after) on every entity write — the local-mode
/// change feed: create ⇒ (None, Some), delete ⇒ (Some, None).
pub type ChangeHook = Box<dyn Fn(&TenantId, Option<Value>, Option<Value>) + Send + Sync>;

/// Which resource family an operation touches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// 5.2.4 Entity current-state documents.
    Entity,
    /// 5.2.12 Subscription documents.
    Subscription,
    /// 5.2.9 Context Source Registration documents.
    Registration,
    /// Context Source Registration Subscription documents (5.11).
    CSourceSubscription,
    /// Temporal Representation documents (5.2.5).
    Temporal,
    /// 5.16 Snapshot status documents (+ the internal synth-tenant index).
    Snapshot,
    /// 5.14 EntityMap API documents.
    EntityMap,
    /// 5.8.1.4 distributed-subscription mappings (remote ids per CSR).
    DistSub,
    /// Notifications a delivery policy gave up on (dead letters), kept
    /// under the subscription's tenant for replay.
    DeadLetter,
}

/// The four store backends, decided ONCE at startup and threaded as a value —
/// never re-derived from strings or runtime probes, so a section gated on the
/// wrong mode is unrepresentable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum StoreMode {
    /// In-process maps; nothing survives a restart.
    #[default]
    Memory,
    /// Embedded single-file store on local disk.
    File,
    /// PostgreSQL with PostGIS.
    Postgres,
    /// PostgreSQL with PostGIS and TimescaleDB hypertables for history.
    Timescale,
}

impl StoreMode {
    /// Every backend this workspace knows, in the order a listing shows
    /// them. One source of truth: `FromStr`, the unknown-mode message and
    /// the broker's built-with shelf all read it, so a backend added to the
    /// enum cannot go missing from any of them.
    pub const ALL: [StoreMode; 4] = [
        StoreMode::Memory,
        StoreMode::File,
        StoreMode::Postgres,
        StoreMode::Timescale,
    ];

    /// The accepted mode names, `memory|file|postgres|timescale`.
    pub fn names() -> String {
        StoreMode::ALL
            .iter()
            .map(|m| m.as_str())
            .collect::<Vec<_>>()
            .join("|")
    }

    /// The mode name as accepted by `FromStr` (`memory|file|postgres|timescale`).
    pub fn as_str(self) -> &'static str {
        match self {
            StoreMode::Memory => "memory",
            StoreMode::File => "file",
            StoreMode::Postgres => "postgres",
            StoreMode::Timescale => "timescale",
        }
    }
    /// Shared-database modes — the only ones that can back multiple instances.
    pub fn is_pg(self) -> bool {
        matches!(self, StoreMode::Postgres | StoreMode::Timescale)
    }
}

impl std::str::FromStr for StoreMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        StoreMode::ALL
            .into_iter()
            .find(|m| m.as_str() == s)
            .ok_or_else(|| format!("unknown store mode {s} ({})", StoreMode::names()))
    }
}

impl std::fmt::Display for StoreMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What one tenant holds, per current-state document kind.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TenantStats {
    /// The tenant name.
    pub tenant: String,
    /// When the tenant row was created, if the backend records it.
    pub created_at: Option<String>,
    /// 5.2.4 entities.
    pub entities: u64,
    /// 5.2.12 subscriptions.
    pub subscriptions: u64,
    /// 5.2.9 context source registrations.
    pub registrations: u64,
    /// Context source registration subscriptions.
    pub csource_subscriptions: u64,
    /// 5.16 snapshots.
    pub snapshots: u64,
    /// Entity maps (5.14).
    pub entity_maps: u64,
    /// Distributed subscriptions this tenant placed at other brokers.
    pub dist_subs: u64,
}

/// The read-modify-write closure as the object-safe seam carries it. The
/// typed `Result<T, E>` travels through [`CurrentStateDriverExt::mutate`]'s
/// side slot; the boxed closure only signals commit (`Ok`) vs reject
/// (`Err`) to the driver.
pub type MutateFn<'a> = Box<dyn FnOnce(&mut Value) -> Result<(), ()> + 'a>;
/// Per-id variant for batch mutation; the driver calls it once per PRESENT
/// id, in input order — the ext trait's error slot depends on that order.
pub type BatchMutateFn<'a> = Box<dyn FnMut(&str, &mut Value) -> Result<(), ()> + 'a>;
/// The delivery stamp expressed as a `mutate`. This is the rule itself —
/// what `timesSent`, `lastNotification`, `lastSuccess` and `status` become
/// after one attempt (5.2.14.2) — so a backend that reimplements
/// [`CurrentStateDriver::record_delivery`] in its own query language is
/// reimplementing THIS, and the two must agree.
pub fn record_delivery_via_mutate(
    d: &(impl CurrentStateDriver + ?Sized),
    tenant: &TenantId,
    kind: Kind,
    id: &str,
    now: &str,
) -> Result<Option<Delivery>, NgsiError> {
    let mut out: Option<Delivery> = None;
    d.mutate::<(), ()>(tenant, kind, id, |doc| {
        let mut prev_success = None;
        if let Some(o) = doc.as_object_mut() {
            o.remove("status");
        }
        if let Some(n) = doc
            .as_object_mut()
            .and_then(|o| o.get_mut("notification"))
            .and_then(Value::as_object_mut)
        {
            let sent = n.get("timesSent").and_then(Value::as_i64).unwrap_or(0);
            n.insert("timesSent".into(), serde_json::json!(sent + 1));
            n.insert("lastNotification".into(), Value::String(now.to_owned()));
            prev_success = n.insert("lastSuccess".into(), Value::String(now.to_owned()));
            n.insert("status".into(), Value::String("ok".into()));
        }
        out = Some(Delivery {
            doc: doc.clone(),
            prev_success,
        });
        Ok(())
    })?;
    Ok(out)
}

/// What one delivery attempt wrote: the stored subscription as it now
/// stands (the mirror is fed from it) and the `lastSuccess` that was there
/// before, which a failed attempt puts back.
#[derive(Debug, Clone)]
pub struct Delivery {
    /// The subscription as it now stands, the shape the mirror is fed.
    pub doc: Value,
    /// The `notification.lastSuccess` this attempt overwrote, absent when
    /// the subscription had never succeeded.
    pub prev_success: Option<Value>,
}

/// Current-state storage: everything except the temporal evolution.
///
/// Contract carried over from the enum seam it replaces: every mutate is
/// one transaction under the row lock, and a missing row is `None`, never
/// an insert (a bookkeeping writeback racing a DELETE must not resurrect
/// the row). Backends map their internal failures to
/// `NgsiError::InternalError` with a GENERIC client-visible detail.
pub trait CurrentStateDriver: Send + Sync {
    /// Readiness RIGHT NOW — a lost database flips /q/ready to 503.
    fn ping(&self) -> Result<(), NgsiError>;
    /// (Queued writers, peak) of a single-writer commit section, if the
    /// backend has one.
    fn commit_queue(&self) -> Option<(usize, usize)> {
        None
    }
    /// Drain: finish in-flight work and disconnect cleanly.
    fn close<'a>(&'a self) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>>;
    /// What this driver runs on, for `/q/health`: engine, server version,
    /// extensions — whatever an operator needs to tell two deployments of
    /// the same backend name apart. Read from state captured at startup,
    /// never by querying on the request: health is polled. An empty object
    /// (the default) means the driver has nothing to add to its name.
    fn version_info(&self) -> Value {
        Value::Object(serde_json::Map::new())
    }
    /// Installs the (tenant, before, after) hook called on every entity write.
    fn set_change_hook(&self, h: ChangeHook);
    /// Turn the same-transaction outbox producer on (bus=nats).
    fn set_outbox(&self, on: bool) {
        let _ = on;
    }
    /// Outbox drain: oldest-first page of pending rows `(seq, tenant, event)`.
    fn outbox_peek(&self, limit: i64) -> Result<Vec<(i64, String, Value)>, NgsiError> {
        let _ = limit;
        Ok(Vec::new())
    }
    /// Outbox drain: delete EXACTLY the published rows.
    fn outbox_ack(&self, seqs: &[i64]) -> Result<u64, NgsiError> {
        let _ = seqs;
        Ok(0)
    }

    /// Insert a document; `false` if the id already exists (nothing written).
    fn create(
        &self,
        tenant: &TenantId,
        kind: Kind,
        id: &str,
        doc: Value,
    ) -> Result<bool, NgsiError>;
    /// Batch create (entities only); created-flags in input order.
    fn batch_create(
        &self,
        tenant: &TenantId,
        items: Vec<(String, Value)>,
    ) -> Result<Vec<bool>, NgsiError>;
    /// Batch delete (entities only); deleted-flags in input order, a
    /// duplicate id deletes once and reads absent the second time.
    fn batch_delete(&self, tenant: &TenantId, ids: &[String]) -> Result<Vec<bool>, NgsiError>;
    /// Batch upsert with REPLACE semantics (entities only); created-flags in
    /// input order.
    fn batch_upsert(
        &self,
        tenant: &TenantId,
        items: Vec<(String, Value)>,
    ) -> Result<Vec<bool>, NgsiError>;
    /// Insert or replace a document. `true` means a document was ALREADY
    /// there and this call replaced it; `false` means this call created it.
    /// The polarity is the opposite of [`Self::batch_upsert`], which answers
    /// created-flags — the batch path needs them to split 201 from 204
    /// (5.6.8) while the single path ignores the value.
    fn upsert(
        &self,
        tenant: &TenantId,
        kind: Kind,
        id: &str,
        doc: Value,
    ) -> Result<bool, NgsiError>;
    /// Read one document; `None` if absent.
    fn get(&self, tenant: &TenantId, kind: Kind, id: &str) -> Result<Option<Value>, NgsiError>;
    /// Delete one document; `false` if it was absent.
    fn delete(&self, tenant: &TenantId, kind: Kind, id: &str) -> Result<bool, NgsiError>;
    /// Every document of this kind in the tenant.
    ///
    /// A backend may refuse a tenant that holds too many to materialize
    /// (5.5.6 TooManyResults). That is right for a client query and wrong
    /// for a reader that must see ALL of them — use [`Self::list_page`].
    fn list(&self, tenant: &TenantId, kind: Kind) -> Result<Vec<Value>, NgsiError>;
    /// One id-ordered page of documents, for the internal readers that must
    /// see every one of them and so cannot be refused: ids strictly greater
    /// than `after`, at most `limit`. A short page means the end.
    ///
    /// Keyset, not offset: the caller walks a tenant that is being written
    /// to underneath it, where OFFSET skips and repeats rows. The peak cost
    /// is one page, not the whole tenant, which is what the row ceiling on
    /// `list` was protecting — so this carries no ceiling of its own, and a
    /// backend may not refuse it for volume.
    ///
    /// Required, deliberately: the obvious default is `list` sliced, and
    /// `list` is the read that may refuse. A backend inheriting that would
    /// silently reacquire the outage this method exists to prevent, with no
    /// compile error to say so.
    fn list_page(
        &self,
        tenant: &TenantId,
        kind: Kind,
        after: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Value>, NgsiError>;
    /// One id-ordered window of documents and the size of the whole set:
    /// elements `offset..offset + limit`, plus the count 6.3.10's `count`
    /// parameter reports. `limit` 0 is a legal request for the count alone.
    ///
    /// 5.5.9.1: "the query resolution mechanisms of the NGSI-LD System shall
    /// ensure that only up to a maximum of L NGSI-LD Elements are RETRIEVED
    /// and returned to the NGSI-LD client". Reading a whole tenant to serve
    /// one page of it is what that sentence rules out, and it is why this
    /// carries no row ceiling: the window bounds the result by construction,
    /// so the only thing a ceiling could refuse is a page the client is
    /// entitled to.
    ///
    /// Offset, not the keyset of [`Self::list_page`], because this serves
    /// 5.5.9.2's `limit`/`offset`, which lets a client "jump to a desired
    /// set of elements". A cursor cannot answer that; the two reads exist
    /// for different callers and neither replaces the other.
    ///
    /// Required for the same reason `list_page` is: the obvious default
    /// slices `list`, and `list` is the read that may refuse.
    fn list_slice(
        &self,
        tenant: &TenantId,
        kind: Kind,
        offset: usize,
        limit: usize,
    ) -> Result<(Vec<Value>, usize), NgsiError>;
    /// Registrations that can match these ids/types (a backend may narrow;
    /// returning the full tenant list is always correct).
    fn matching_registrations(
        &self,
        tenant: &TenantId,
        ids: Option<&[String]>,
        types: Option<&[String]>,
    ) -> Result<Vec<Value>, NgsiError>;
    /// Query Entities with the filter pushed down where the backend can
    /// take it; the caller re-checks unless the outcome says `decided`.
    fn query_entities(
        &self,
        tenant: &TenantId,
        f: &filter::EntityFilter<'_>,
    ) -> Result<filter::QueryOutcome, NgsiError>;
    /// Read-modify-write under the row lock; `None` = absent (never an
    /// insert), `Some(Err)` = the closure rejected, nothing committed.
    fn mutate_boxed<'a>(
        &self,
        tenant: &TenantId,
        kind: Kind,
        id: &str,
        f: MutateFn<'a>,
    ) -> Result<Option<Result<(), ()>>, NgsiError>;
    /// Batch read-modify-write (entities only); results align with `ids`.
    fn batch_mutate_boxed<'a>(
        &self,
        tenant: &TenantId,
        ids: &[String],
        f: BatchMutateFn<'a>,
    ) -> Result<Vec<Option<Result<(), ()>>>, NgsiError>;
    /// 5.2.14.2 delivery bookkeeping: stamp one delivery attempt on a
    /// subscription and hand back the stored document. `timesSent` moves by
    /// one, `lastNotification` and `lastSuccess` take `now`, and the previous
    /// `lastSuccess` comes back so a failed attempt can roll it back.
    /// `None` means the row is gone — the subscription was deleted between
    /// matching and delivery, and nothing may be sent (5.8.6).
    ///
    /// The default expresses it as a `mutate`, which is correct everywhere.
    /// A backend whose `mutate` locks the row across a network round trip
    /// should override it with one statement: at fan-out every delivery on
    /// one subscription contends for that row, so the lock hold time — not
    /// the statement count — is what serializes delivery.
    fn record_delivery(
        &self,
        tenant: &TenantId,
        kind: Kind,
        id: &str,
        now: &str,
    ) -> Result<Option<Delivery>, NgsiError> {
        record_delivery_via_mutate(self, tenant, kind, id, now)
    }
    /// Reap expired docs/instances (backends with their own maintenance job
    /// return 0).
    fn sweep_expired(&self) -> usize {
        0
    }
    /// 5.5.10: the default Tenant implicitly exists; others once created.
    fn tenant_exists(&self, tenant: &TenantId) -> Result<bool, NgsiError>;
    /// The iteration domain of the interval sweep and of every mirror
    /// hydration: a tenant holding a Subscription, a Context Source
    /// Registration Subscription OR a Registration SHALL appear. A superset
    /// is allowed — the callers list per tenant afterwards and an empty list
    /// costs nothing — so a backend that cannot narrow the set cheaply may
    /// return every tenant it knows. A SUBSET is a silent outage: a tenant
    /// missing here never fires a periodic notification and never reaches
    /// the mirror.
    ///
    /// Registrations are in the domain because one of the hydrations fills
    /// the REGISTRATION mirror, and the federation path reads that mirror
    /// alone whenever it is installed. A domain that stopped at
    /// subscription-holding tenants left a tenant with registrations and no
    /// subscription forwarding to no Context Source at all.
    fn subscription_tenants(&self) -> Result<Vec<String>, NgsiError>;
    /// Every tenant the backend knows, sorted. The default Tenant is listed
    /// even when empty (5.5.10: it always exists). Names only: at the
    /// 10 000-tenant target (ADR-0001) an inventory carrying per-kind counts
    /// would cost a count per kind per tenant, so the counts are paid per
    /// lookup in `tenant_stats_one`.
    fn tenant_ids(&self) -> Result<Vec<String>, NgsiError> {
        Err(NgsiError::OperationNotSupported("tenant inventory".into()))
    }
    /// What one tenant holds; `None` when it does not exist.
    fn tenant_stats_one(&self, tenant: &TenantId) -> Result<Option<TenantStats>, NgsiError> {
        let _ = tenant;
        Err(NgsiError::OperationNotSupported("tenant inventory".into()))
    }
    /// Remove every current-state document of one tenant; `false` when the
    /// tenant did not exist. The default Tenant is emptied but keeps
    /// existing.
    fn purge_tenant(&self, tenant: &TenantId) -> Result<bool, NgsiError> {
        let _ = tenant;
        Err(NgsiError::OperationNotSupported("tenant purge".into()))
    }
    /// 5.13 @context documents, shared across tenants by design.
    fn context_put(&self, id: &str, doc: Value) -> Result<(), NgsiError>;
    /// Read a stored @context document by id; `None` if absent.
    fn context_get(&self, id: &str) -> Result<Option<Value>, NgsiError>;
    /// Delete a stored @context document; `false` if it was absent.
    fn context_delete(&self, id: &str) -> Result<bool, NgsiError>;
    /// Every stored @context document.
    /// Every stored `@context` row WITHOUT its `body` member — the url,
    /// localId, kind, owner and usage counters, and not the document.
    ///
    /// There is deliberately no read that returns every row WITH its body.
    /// A body is accepted up to `MAX_CONTEXT_BYTES` (5 MiB) and only the
    /// `Cached` rows are capped in number, so one such read materializes
    /// gigabytes — on the boot path, where it decides whether the broker
    /// starts at all. The callers that need one body ask for it by id
    /// ([`Self::context_get`]); nothing needs them all at once.
    fn context_list_meta(&self) -> Result<Vec<Value>, NgsiError>;
}

/// Typed sugar over the boxed mutate seam — call sites keep their
/// `Result<T, E>` closures; the value crosses the object boundary in a
/// side slot.
pub trait CurrentStateDriverExt {
    /// Typed read-modify-write: `None` = absent, `Some(Err(e))` = the closure
    /// rejected and nothing was committed.
    fn mutate<T, E>(
        &self,
        tenant: &TenantId,
        kind: Kind,
        id: &str,
        f: impl FnOnce(&mut Value) -> Result<T, E>,
    ) -> Result<Option<Result<T, E>>, NgsiError>;
    /// Typed batch read-modify-write (entities only); results align with `ids`.
    fn batch_mutate<E>(
        &self,
        tenant: &TenantId,
        ids: &[String],
        f: impl FnMut(&str, &mut Value) -> Result<(), E>,
    ) -> Result<Vec<Option<Result<(), E>>>, NgsiError>;
}

impl<S: CurrentStateDriver + ?Sized> CurrentStateDriverExt for S {
    fn mutate<T, E>(
        &self,
        tenant: &TenantId,
        kind: Kind,
        id: &str,
        f: impl FnOnce(&mut Value) -> Result<T, E>,
    ) -> Result<Option<Result<T, E>>, NgsiError> {
        let slot = std::cell::RefCell::new(None);
        let r = self.mutate_boxed(
            tenant,
            kind,
            id,
            Box::new(|v| {
                let r = f(v);
                let flag = if r.is_ok() { Ok(()) } else { Err(()) };
                *slot.borrow_mut() = Some(r);
                flag
            }),
        )?;
        Ok(r.and_then(|_| slot.into_inner()))
    }

    fn batch_mutate<E>(
        &self,
        tenant: &TenantId,
        ids: &[String],
        mut f: impl FnMut(&str, &mut Value) -> Result<(), E>,
    ) -> Result<Vec<Option<Result<(), E>>>, NgsiError> {
        // Errors land in the queue in closure-call order, which the trait
        // contract fixes to input order over present ids.
        let errs = std::cell::RefCell::new(std::collections::VecDeque::new());
        let r = self.batch_mutate_boxed(
            tenant,
            ids,
            Box::new(|id, v| match f(id, v) {
                Ok(()) => Ok(()),
                Err(e) => {
                    errs.borrow_mut().push_back(e);
                    Err(())
                }
            }),
        )?;
        let mut errs = errs.into_inner();
        let mut out = Vec::with_capacity(r.len());
        for slot in r {
            out.push(match slot {
                None => None,
                Some(Ok(())) => Some(Ok(())),
                // one queued error per rejected id is the trait contract; a
                // driver that reports more rejections than the closure raised
                // has broken it, and the batch fails rather than inventing an
                // error for the caller to read.
                Some(Err(())) => Some(Err(errs.pop_front().ok_or_else(|| {
                    NgsiError::InternalError(
                        "batch driver reported more rejections than were raised".into(),
                    )
                })?)),
            });
        }
        Ok(out)
    }
}

/// What a temporal event records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TemporalOp {
    /// An Attribute the entity did not carry before (one event per instance).
    AttrCreated,
    /// A changed instance of an existing Attribute (4.5.6 append).
    AttrModified,
    /// 4.5.6: the Scope changed through the Core API — recorded as a scope
    /// instance whose observedAt copies modifiedAt.
    ScopeChanged,
}

/// One change the write path hands to the temporal seam. Events are
/// produced per attribute INSTANCE (the gate chain and a columnar writer
/// both work per instance) and drained per request, in order.
#[derive(Clone, Debug)]
pub struct TemporalEvent {
    /// What the event records.
    pub op: TemporalOp,
    /// Owning tenant.
    pub tenant: TenantId,
    /// Id of the entity whose history this instance belongs to.
    pub entity_id: String,
    /// The entity's meta shell (id, type, createdAt, modifiedAt, scope) as
    /// it stood after the write — what `temporal_append` creates on first
    /// touch.
    pub shell: Value,
    /// Expanded Attribute name, or `scope`.
    pub attr: String,
    /// The instance snapshot: value, datasetId, observedAt, instanceId…
    pub instance: Value,
}

/// Temporal storage: the entity history (Temporal Evolution) plus the raw
/// temporal documents the 5.6.13-5.6.16 edit paths operate on.
///
/// Internal bookkeeping (snapshot copies, delete cascades) degrades to
/// benign no-ops on a driver without temporal support; the CLIENT-facing
/// operations answer `OperationNotSupported` (422 per CIM 009
/// Table 6.3.2-1) instead.
pub trait TemporalDriver: Send + Sync {
    /// The drain: one call carries a whole request's events, in production
    /// order. The default folds consecutive events of one entity into a
    /// single `temporal_append` (scope changes go through `mutate`, as
    /// 4.5.6 shapes them); a bulk writer overrides this and sees the batch.
    fn event_list(&self, evs: &[TemporalEvent]) -> Result<(), NgsiError> {
        let mut i = 0;
        while i < evs.len() {
            let (tenant, id) = (&evs[i].tenant, evs[i].entity_id.as_str());
            let mut additions = serde_json::Map::new();
            let mut shell = &evs[i].shell;
            let mut j = i;
            while j < evs.len() && evs[j].tenant == *tenant && evs[j].entity_id == id {
                let ev = &evs[j];
                shell = &ev.shell;
                if ev.op == TemporalOp::ScopeChanged {
                    let inst = ev.instance.clone();
                    self.mutate(tenant, id, |doc| {
                        let target = doc.as_object_mut().ok_or(())?;
                        match target.get_mut("scope").and_then(Value::as_array_mut) {
                            Some(arr) if arr.first().is_some_and(Value::is_object) => {
                                arr.push(inst);
                            }
                            _ => {
                                target.insert("scope".into(), Value::Array(vec![inst]));
                            }
                        }
                        Ok::<(), ()>(())
                    })?;
                } else {
                    if let Some(arr) = additions
                        .entry(ev.attr.clone())
                        .or_insert_with(|| Value::Array(Vec::new()))
                        .as_array_mut()
                    {
                        arr.push(ev.instance.clone());
                    }
                }
                j += 1;
            }
            if !additions.is_empty() {
                self.temporal_append(tenant, id, shell, &Value::Object(additions))?;
            }
            i = j;
        }
        Ok(())
    }
    /// `false` = this deployment records no history (`NoTemporal`); the
    /// write path skips recording entirely.
    fn supported(&self) -> bool {
        true
    }
    /// Stored attribute instances of one tenant (inventory); a driver
    /// without history reports 0.
    fn attr_instance_count(&self, tenant: &TenantId) -> Result<u64, NgsiError> {
        let _ = tenant;
        Ok(0)
    }
    /// Remove the whole history of one tenant; nothing to do without history.
    fn purge_tenant(&self, tenant: &TenantId) -> Result<(), NgsiError> {
        let _ = tenant;
        Ok(())
    }
    /// Readiness of the temporal backend; the default is always ready.
    fn ping(&self) -> Result<(), NgsiError> {
        Ok(())
    }
    /// What this temporal driver runs on, for `/q/health`; the same
    /// contract as [`CurrentStateDriver::version_info`].
    fn version_info(&self) -> Value {
        Value::Object(serde_json::Map::new())
    }
    /// Drain: finish in-flight work and disconnect cleanly. A temporal
    /// driver configured as a backend of its own holds its own pool, and the
    /// shutdown path closes it here. May be called more than once — when one
    /// instance serves both seams it is closed through each of them — so an
    /// implementation makes it idempotent. The default has nothing to close.
    fn close<'a>(&'a self) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
        Box::pin(async {})
    }
    /// Auto-recording fast path: append instances, creating the meta shell
    /// on first touch — and only for an entity that still exists (5.6.6
    /// deletes history; an append overlapping the delete must not recreate
    /// it).
    fn temporal_append(
        &self,
        tenant: &TenantId,
        id: &str,
        shell: &Value,
        additions: &Value,
    ) -> Result<(), NgsiError>;
    /// Query Temporal Evolution (5.7.4) with pushdown where possible.
    fn query_temporal(
        &self,
        tenant: &TenantId,
        f: &filter::TemporalFilter<'_>,
    ) -> Result<filter::TemporalOutcome, NgsiError>;
    /// Retrieve Temporal Evolution (5.7.3) with instance pruning.
    fn get_temporal(
        &self,
        tenant: &TenantId,
        id: &str,
        f: &filter::TemporalFilter<'_>,
    ) -> Result<Option<Value>, NgsiError>;
    /// Raw temporal document access (the 5.6.11-5.6.16 edit/delete paths
    /// and internal copies).
    fn get(&self, tenant: &TenantId, id: &str) -> Result<Option<Value>, NgsiError>;
    /// Insert a temporal document; `false` if the id already exists.
    fn create(&self, tenant: &TenantId, id: &str, doc: Value) -> Result<bool, NgsiError>;
    /// Insert or replace a temporal document. `true` means it was ALREADY
    /// there and this call replaced it; `false` means this call created it,
    /// the same polarity as the current-state seam.
    fn upsert(&self, tenant: &TenantId, id: &str, doc: Value) -> Result<bool, NgsiError>;
    /// Delete an entity's whole history; `false` if it had none.
    fn delete(&self, tenant: &TenantId, id: &str) -> Result<bool, NgsiError>;
    /// Every temporal document in the tenant.
    fn list(&self, tenant: &TenantId) -> Result<Vec<Value>, NgsiError>;
    /// Read-modify-write of one temporal document under its row lock;
    /// `None` = absent (never an insert), `Some(Err)` = rejected, not committed.
    fn mutate_boxed<'a>(
        &self,
        tenant: &TenantId,
        id: &str,
        f: MutateFn<'a>,
    ) -> Result<Option<Result<(), ()>>, NgsiError>;
}

/// Typed mutate sugar for the temporal seam, same slot trick as
/// [`CurrentStateDriverExt`].
pub trait TemporalDriverExt {
    /// Typed read-modify-write of one temporal document: `None` = absent,
    /// `Some(Err(e))` = the closure rejected and nothing was committed.
    fn mutate<T, E>(
        &self,
        tenant: &TenantId,
        id: &str,
        f: impl FnOnce(&mut Value) -> Result<T, E>,
    ) -> Result<Option<Result<T, E>>, NgsiError>;
}

impl<S: TemporalDriver + ?Sized> TemporalDriverExt for S {
    fn mutate<T, E>(
        &self,
        tenant: &TenantId,
        id: &str,
        f: impl FnOnce(&mut Value) -> Result<T, E>,
    ) -> Result<Option<Result<T, E>>, NgsiError> {
        let slot = std::cell::RefCell::new(None);
        let r = self.mutate_boxed(
            tenant,
            id,
            Box::new(|v| {
                let r = f(v);
                let flag = if r.is_ok() { Ok(()) } else { Err(()) };
                *slot.borrow_mut() = Some(r);
                flag
            }),
        )?;
        Ok(r.and_then(|_| slot.into_inner()))
    }
}

/// The no-history driver: temporal OFF as a driver choice. Client-facing
/// reads answer `OperationNotSupported`; the recorder and the internal
/// bookkeeping paths degrade to no-ops.
pub struct NoTemporal;

fn unsupported() -> NgsiError {
    NgsiError::OperationNotSupported("no temporal store is configured".into())
}

impl TemporalDriver for NoTemporal {
    fn supported(&self) -> bool {
        false
    }
    fn temporal_append(
        &self,
        _tenant: &TenantId,
        _id: &str,
        _shell: &Value,
        _additions: &Value,
    ) -> Result<(), NgsiError> {
        Ok(())
    }
    fn query_temporal(
        &self,
        _tenant: &TenantId,
        _f: &filter::TemporalFilter<'_>,
    ) -> Result<filter::TemporalOutcome, NgsiError> {
        Err(unsupported())
    }
    fn get_temporal(
        &self,
        _tenant: &TenantId,
        _id: &str,
        _f: &filter::TemporalFilter<'_>,
    ) -> Result<Option<Value>, NgsiError> {
        Err(unsupported())
    }
    fn get(&self, _tenant: &TenantId, _id: &str) -> Result<Option<Value>, NgsiError> {
        Ok(None)
    }
    fn create(&self, _tenant: &TenantId, _id: &str, _doc: Value) -> Result<bool, NgsiError> {
        Ok(false)
    }
    fn upsert(&self, _tenant: &TenantId, _id: &str, _doc: Value) -> Result<bool, NgsiError> {
        Ok(false)
    }
    fn delete(&self, _tenant: &TenantId, _id: &str) -> Result<bool, NgsiError> {
        Ok(false)
    }
    fn list(&self, _tenant: &TenantId) -> Result<Vec<Value>, NgsiError> {
        Ok(Vec::new())
    }
    fn mutate_boxed<'a>(
        &self,
        _tenant: &TenantId,
        _id: &str,
        _f: MutateFn<'a>,
    ) -> Result<Option<Result<(), ()>>, NgsiError> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The mode list is one source of truth: every variant round-trips
    /// through its name, and the message for an unknown name lists all of
    /// them — a backend added to the enum cannot be missing from either.
    #[test]
    fn every_store_mode_round_trips_and_the_error_lists_them_all() {
        for m in StoreMode::ALL {
            let back = m
                .as_str()
                .parse::<StoreMode>()
                .unwrap_or_else(|e| panic!("{m} must parse back: {e}"));
            assert_eq!(back, m);
        }
        let err = "mongo".parse::<StoreMode>().expect_err("unknown mode");
        assert!(err.contains("mongo"), "{err}");
        for m in StoreMode::ALL {
            assert!(err.contains(m.as_str()), "the message must name {m}: {err}");
        }
    }

    /// A minimal driver proving the boxed seam round-trips typed results:
    /// present row → the closure's T and E cross intact; absent → None.
    struct OneDoc(std::sync::Mutex<Option<Value>>);
    impl CurrentStateDriver for OneDoc {
        fn ping(&self) -> Result<(), NgsiError> {
            Ok(())
        }
        fn close<'a>(
            &'a self,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
            Box::pin(async {})
        }
        fn set_change_hook(&self, _h: ChangeHook) {}
        fn create(
            &self,
            _t: &TenantId,
            _k: Kind,
            _id: &str,
            doc: Value,
        ) -> Result<bool, NgsiError> {
            *self.0.lock().expect("lock") = Some(doc);
            Ok(true)
        }
        fn batch_create(
            &self,
            _t: &TenantId,
            _items: Vec<(String, Value)>,
        ) -> Result<Vec<bool>, NgsiError> {
            unimplemented!()
        }
        fn list_page(
            &self,
            _t: &TenantId,
            _k: Kind,
            _after: Option<&str>,
            _limit: usize,
        ) -> Result<Vec<Value>, NgsiError> {
            unimplemented!()
        }
        fn list_slice(
            &self,
            _t: &TenantId,
            _k: Kind,
            _offset: usize,
            _limit: usize,
        ) -> Result<(Vec<Value>, usize), NgsiError> {
            unimplemented!()
        }
        fn batch_delete(&self, _t: &TenantId, _ids: &[String]) -> Result<Vec<bool>, NgsiError> {
            unimplemented!()
        }
        fn batch_upsert(
            &self,
            _t: &TenantId,
            _items: Vec<(String, Value)>,
        ) -> Result<Vec<bool>, NgsiError> {
            unimplemented!()
        }
        fn upsert(
            &self,
            _t: &TenantId,
            _k: Kind,
            _id: &str,
            _doc: Value,
        ) -> Result<bool, NgsiError> {
            unimplemented!()
        }
        fn get(&self, _t: &TenantId, _k: Kind, _id: &str) -> Result<Option<Value>, NgsiError> {
            Ok(self.0.lock().expect("lock").clone())
        }
        fn delete(&self, _t: &TenantId, _k: Kind, _id: &str) -> Result<bool, NgsiError> {
            unimplemented!()
        }
        fn list(&self, _t: &TenantId, _k: Kind) -> Result<Vec<Value>, NgsiError> {
            unimplemented!()
        }
        fn matching_registrations(
            &self,
            _t: &TenantId,
            _ids: Option<&[String]>,
            _types: Option<&[String]>,
        ) -> Result<Vec<Value>, NgsiError> {
            unimplemented!()
        }
        fn query_entities(
            &self,
            _t: &TenantId,
            _f: &filter::EntityFilter<'_>,
        ) -> Result<filter::QueryOutcome, NgsiError> {
            unimplemented!()
        }
        fn mutate_boxed<'a>(
            &self,
            _t: &TenantId,
            _k: Kind,
            _id: &str,
            f: MutateFn<'a>,
        ) -> Result<Option<Result<(), ()>>, NgsiError> {
            let mut guard = self.0.lock().expect("lock");
            match guard.as_mut() {
                None => Ok(None),
                Some(v) => {
                    let mut copy = v.clone();
                    match f(&mut copy) {
                        Ok(()) => {
                            *v = copy;
                            Ok(Some(Ok(())))
                        }
                        Err(()) => Ok(Some(Err(()))),
                    }
                }
            }
        }
        fn batch_mutate_boxed<'a>(
            &self,
            _t: &TenantId,
            _ids: &[String],
            _f: BatchMutateFn<'a>,
        ) -> Result<Vec<Option<Result<(), ()>>>, NgsiError> {
            unimplemented!()
        }
        fn tenant_exists(&self, _t: &TenantId) -> Result<bool, NgsiError> {
            Ok(true)
        }
        fn subscription_tenants(&self) -> Result<Vec<String>, NgsiError> {
            Ok(Vec::new())
        }
        fn context_put(&self, _id: &str, _doc: Value) -> Result<(), NgsiError> {
            Ok(())
        }
        fn context_get(&self, _id: &str) -> Result<Option<Value>, NgsiError> {
            Ok(None)
        }
        fn context_delete(&self, _id: &str) -> Result<bool, NgsiError> {
            Ok(false)
        }
        fn context_list_meta(&self) -> Result<Vec<Value>, NgsiError> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn typed_mutate_round_trips_through_the_boxed_seam() {
        let t = TenantId::new("t").expect("tenant");
        let d: std::sync::Arc<dyn CurrentStateDriver> =
            std::sync::Arc::new(OneDoc(std::sync::Mutex::new(None)));
        // absent → None, closure never runs
        let r = d
            .mutate::<u32, &str>(&t, Kind::Entity, "x", |_| panic!("must not run"))
            .expect("driver ok");
        assert!(r.is_none());
        d.create(&t, Kind::Entity, "x", serde_json::json!({"n": 1}))
            .expect("create");
        // typed success crosses the boundary AND the write commits
        let r = d
            .mutate::<u32, &str>(&t, Kind::Entity, "x", |v| {
                v["n"] = serde_json::json!(2);
                Ok(7)
            })
            .expect("driver ok");
        assert_eq!(r, Some(Ok(7)));
        assert_eq!(
            d.get(&t, Kind::Entity, "x").expect("get").expect("doc")["n"],
            2
        );
        // typed error crosses the boundary AND the write is discarded
        let r = d
            .mutate::<u32, &str>(&t, Kind::Entity, "x", |v| {
                v["n"] = serde_json::json!(99);
                Err("rejected")
            })
            .expect("driver ok");
        assert_eq!(r, Some(Err("rejected")));
        assert_eq!(
            d.get(&t, Kind::Entity, "x").expect("get").expect("doc")["n"],
            2,
            "a rejecting closure must not commit"
        );
    }

    /// A temporal driver that only records what the seam hands it: appends
    /// as (id, additions) in call order, scope mutations on a held doc.
    #[derive(Default)]
    struct Recorder {
        appends: std::sync::Mutex<Vec<(String, Value)>>,
        doc: std::sync::Mutex<Option<Value>>,
    }
    impl TemporalDriver for Recorder {
        fn temporal_append(
            &self,
            _t: &TenantId,
            id: &str,
            _shell: &Value,
            additions: &Value,
        ) -> Result<(), NgsiError> {
            self.appends
                .lock()
                .expect("lock")
                .push((id.to_owned(), additions.clone()));
            Ok(())
        }
        fn query_temporal(
            &self,
            _t: &TenantId,
            _f: &filter::TemporalFilter<'_>,
        ) -> Result<filter::TemporalOutcome, NgsiError> {
            unimplemented!()
        }
        fn get_temporal(
            &self,
            _t: &TenantId,
            _id: &str,
            _f: &filter::TemporalFilter<'_>,
        ) -> Result<Option<Value>, NgsiError> {
            unimplemented!()
        }
        fn get(&self, _t: &TenantId, _id: &str) -> Result<Option<Value>, NgsiError> {
            Ok(self.doc.lock().expect("lock").clone())
        }
        fn create(&self, _t: &TenantId, _id: &str, doc: Value) -> Result<bool, NgsiError> {
            *self.doc.lock().expect("lock") = Some(doc);
            Ok(true)
        }
        fn upsert(&self, _t: &TenantId, _id: &str, _doc: Value) -> Result<bool, NgsiError> {
            unimplemented!()
        }
        fn delete(&self, _t: &TenantId, _id: &str) -> Result<bool, NgsiError> {
            unimplemented!()
        }
        fn list(&self, _t: &TenantId) -> Result<Vec<Value>, NgsiError> {
            unimplemented!()
        }
        fn mutate_boxed<'a>(
            &self,
            _t: &TenantId,
            _id: &str,
            f: MutateFn<'a>,
        ) -> Result<Option<Result<(), ()>>, NgsiError> {
            let mut guard = self.doc.lock().expect("lock");
            match guard.as_mut() {
                None => Ok(None),
                Some(v) => Ok(Some(f(v))),
            }
        }
    }

    fn ev(op: TemporalOp, id: &str, attr: &str, n: u32) -> TemporalEvent {
        TemporalEvent {
            op,
            tenant: TenantId::new("t").expect("tenant"),
            entity_id: id.into(),
            shell: serde_json::json!({"id": id, "type": ["T"]}),
            attr: attr.into(),
            instance: serde_json::json!({"type": "Property", "value": n}),
        }
    }

    /// The drain folds one request's events into ONE append per entity run
    /// — a 2-attribute entity is one call carrying both, not two — and
    /// keeps production order across entities.
    #[test]
    fn event_list_folds_a_request_into_one_append_per_entity_run() {
        let d = Recorder::default();
        d.event_list(&[
            ev(TemporalOp::AttrCreated, "urn:a", "speed", 1),
            ev(TemporalOp::AttrCreated, "urn:a", "speed", 2),
            ev(TemporalOp::AttrModified, "urn:a", "heading", 3),
            ev(TemporalOp::AttrModified, "urn:b", "speed", 4),
            ev(TemporalOp::AttrModified, "urn:a", "speed", 5),
        ])
        .expect("drain ok");
        let appends = d.appends.lock().expect("lock").clone();
        let ids: Vec<&str> = appends.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(
            ids,
            ["urn:a", "urn:b", "urn:a"],
            "one append per entity run, in order"
        );
        let first = &appends[0].1;
        assert_eq!(first["speed"].as_array().map(Vec::len), Some(2), "{first}");
        assert_eq!(
            first["heading"].as_array().map(Vec::len),
            Some(1),
            "{first}"
        );
        assert!(
            first.get("value").is_none(),
            "instances live under their attribute: {first}"
        );
        assert_eq!(appends[2].1["speed"][0]["value"], 5);
    }

    /// 4.5.6: a scope change becomes a scope instance on the held temporal
    /// doc (array-of-instances form), not an attribute append.
    #[test]
    fn event_list_records_scope_changes_as_scope_instances() {
        let t = TenantId::new("t").expect("tenant");
        let d = Recorder::default();
        d.create(
            &t,
            "urn:a",
            serde_json::json!({"id": "urn:a", "scope": "/old"}),
        )
        .expect("create");
        let mut scope = ev(TemporalOp::ScopeChanged, "urn:a", "scope", 0);
        scope.instance = serde_json::json!({"type": "Property", "value": "/new",
                                            "observedAt": "2026-01-01T00:00:00Z"});
        d.event_list(&[scope]).expect("drain ok");
        assert!(
            d.appends.lock().expect("lock").is_empty(),
            "no attribute append for a scope change"
        );
        let doc = d.get(&t, "urn:a").expect("get").expect("doc");
        assert_eq!(doc["scope"][0]["value"], "/new", "{doc}");
        assert_eq!(
            doc["scope"].as_array().map(Vec::len),
            Some(1),
            "the plain scope became an instance array: {doc}"
        );
    }

    #[test]
    fn no_temporal_degrades_without_panicking() {
        let t = TenantId::new("t").expect("tenant");
        let d: std::sync::Arc<dyn TemporalDriver> = std::sync::Arc::new(NoTemporal);
        assert!(!d.supported());
        // client-facing reads: the spec error, not a panic
        let e = match d.query_temporal(&t, &filter::TemporalFilter::default()) {
            Err(e) => e,
            Ok(_) => panic!("query_temporal on NoTemporal must be unsupported"),
        };
        assert_eq!(e.status(), 422);
        assert_eq!(e.kind(), "OperationNotSupported");
        // internal bookkeeping: benign no-ops
        assert!(d
            .temporal_append(&t, "x", &Value::Null, &Value::Null)
            .is_ok());
        assert!(!d.delete(&t, "x").expect("ok"));
        assert!(d.list(&t).expect("ok").is_empty());
        assert!(d
            .mutate::<(), ()>(&t, "x", |_| Ok(()))
            .expect("ok")
            .is_none());
    }
}
