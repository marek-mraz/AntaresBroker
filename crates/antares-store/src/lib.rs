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

pub mod filter;

use antares_model::{NgsiError, TenantId};
use serde_json::Value;

/// Called with (tenant, before, after) on every entity write — the local-mode
/// change feed: create ⇒ (None, Some), delete ⇒ (Some, None).
pub type ChangeHook = Box<dyn Fn(&TenantId, Option<Value>, Option<Value>) + Send + Sync>;

/// Which resource family an operation touches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Entity,
    Subscription,
    Registration,
    CSourceSubscription,
    Temporal,
    /// 5.16 Snapshot status documents (+ the internal synth-tenant index).
    Snapshot,
    /// 5.14 EntityMap API documents.
    EntityMap,
    /// 5.8.1.4 distributed-subscription mappings (remote ids per CSR).
    DistSub,
}

/// The four store backends, decided ONCE at startup and threaded as a value —
/// never re-derived from strings or runtime probes, so a section gated on the
/// wrong mode is unrepresentable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum StoreMode {
    #[default]
    Memory,
    File,
    Postgres,
    Timescale,
}

impl StoreMode {
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
        match s {
            "memory" => Ok(StoreMode::Memory),
            "file" => Ok(StoreMode::File),
            "postgres" => Ok(StoreMode::Postgres),
            "timescale" => Ok(StoreMode::Timescale),
            other => Err(format!(
                "unknown store mode {other} (memory|file|postgres|timescale)"
            )),
        }
    }
}

impl std::fmt::Display for StoreMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The read-modify-write closure as the object-safe seam carries it. The
/// typed `Result<T, E>` travels through [`CurrentStateDriverExt::mutate`]'s
/// side slot; the boxed closure only signals commit (`Ok`) vs reject
/// (`Err`) to the driver.
pub type MutateFn<'a> = Box<dyn FnOnce(&mut Value) -> Result<(), ()> + 'a>;
/// Per-id variant for batch mutation; the driver calls it once per PRESENT
/// id, in input order — the ext trait's error slot depends on that order.
pub type BatchMutateFn<'a> = Box<dyn FnMut(&str, &mut Value) -> Result<(), ()> + 'a>;

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
    fn upsert(
        &self,
        tenant: &TenantId,
        kind: Kind,
        id: &str,
        doc: Value,
    ) -> Result<bool, NgsiError>;
    fn get(&self, tenant: &TenantId, kind: Kind, id: &str) -> Result<Option<Value>, NgsiError>;
    fn delete(&self, tenant: &TenantId, kind: Kind, id: &str) -> Result<bool, NgsiError>;
    fn list(&self, tenant: &TenantId, kind: Kind) -> Result<Vec<Value>, NgsiError>;
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
    /// Reap expired docs/instances (backends with their own maintenance job
    /// return 0).
    fn sweep_expired(&self) -> usize {
        0
    }
    /// 5.5.10: the default Tenant implicitly exists; others once created.
    fn tenant_exists(&self, tenant: &TenantId) -> Result<bool, NgsiError>;
    /// Tenants that may hold interval subscriptions (the sweep's iteration
    /// domain).
    fn subscription_tenants(&self) -> Result<Vec<String>, NgsiError>;
    /// 5.13 @context documents, shared across tenants by design.
    fn context_put(&self, id: &str, doc: Value) -> Result<(), NgsiError>;
    fn context_get(&self, id: &str) -> Result<Option<Value>, NgsiError>;
    fn context_delete(&self, id: &str) -> Result<bool, NgsiError>;
    fn context_list(&self) -> Result<Vec<Value>, NgsiError>;
}

/// Typed sugar over the boxed mutate seam — call sites keep their
/// `Result<T, E>` closures; the value crosses the object boundary in a
/// side slot.
pub trait CurrentStateDriverExt {
    fn mutate<T, E>(
        &self,
        tenant: &TenantId,
        kind: Kind,
        id: &str,
        f: impl FnOnce(&mut Value) -> Result<T, E>,
    ) -> Result<Option<Result<T, E>>, NgsiError>;
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
        Ok(r.into_iter()
            .map(|slot| {
                slot.map(|flag| match flag {
                    Ok(()) => Ok(()),
                    Err(()) => Err(errs.pop_front().expect("one error per rejected id")),
                })
            })
            .collect())
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
    pub op: TemporalOp,
    pub tenant: TenantId,
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
    fn ping(&self) -> Result<(), NgsiError> {
        Ok(())
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
    fn create(&self, tenant: &TenantId, id: &str, doc: Value) -> Result<bool, NgsiError>;
    fn upsert(&self, tenant: &TenantId, id: &str, doc: Value) -> Result<bool, NgsiError>;
    fn delete(&self, tenant: &TenantId, id: &str) -> Result<bool, NgsiError>;
    fn list(&self, tenant: &TenantId) -> Result<Vec<Value>, NgsiError>;
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
        fn context_list(&self) -> Result<Vec<Value>, NgsiError> {
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
