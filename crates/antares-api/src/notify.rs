// SPDX-License-Identifier: EUPL-1.2
//! Subscription matching + HTTP notification delivery (5.8.6, 5.3.1).
//!
//! Change detection: the store's change hook feeds every entity write here as
//! a (before, after) pair; attribute-level changes are derived by diffing —
//! one hook point instead of one call per write handler.
//!
//! Candidate lookup is index-shaped. `SubMirror` keeps inverted
//! (tenant, type) and (tenant, watched-attr) maps next to the docs, so one
//! change evaluates only the subscriptions that could possibly fire — never
//! a scan over all of a tenant's subscriptions. Subscriptions the index
//! cannot classify exactly (4.17 type-selection expressions) fall into a
//! `broad` bucket that is always evaluated: the index may over-select,
//! never under-select. Full evaluation (selector/q/geo/scope/triggers)
//! stays the truth for every candidate.

use crate::negotiate::{inject_context, link_header_value};
use crate::state::{now_iso, AppState};
use antares_jsonld::Context;
use antares_model::{dt_key, TenantId};
use antares_store::CurrentStateDriverExt;
use antares_store::Kind;
use antares_store::{TemporalEvent, TemporalOp};
use serde_json::{json, Map, Value};
use std::sync::Arc;

const DEFAULT_TRIGGERS: &[&str] = &["attributeCreated", "attributeUpdated"];
/// Depth of the change→matcher queue, the same ring size the local bus uses.
const CHANGE_QUEUE: usize = 1024;
static CHANGES_DROPPED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static TASK_PANICS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// Hand a batch to the matcher queue: counted as pending on acceptance,
/// counted as dropped when the queue is full. Not the durable outbox
/// (`antares_sql::store::pg::outbox::enqueue`), which is a row inside the
/// caller's transaction: this ring lives in the process and a full one
/// drops, which is why the drop is counted.
fn queue_for_matching(
    tx: &tokio::sync::mpsc::Sender<Vec<Change>>,
    pending: &std::sync::atomic::AtomicUsize,
    changes: Vec<Change>,
) {
    if tx.try_send(changes).is_ok() {
        pending.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    } else {
        note_drop();
    }
}

/// Changes dropped because the matcher queue was full, since process start.
pub fn changes_dropped() -> u64 {
    CHANGES_DROPPED.load(std::sync::atomic::Ordering::Relaxed)
}

/// Panics absorbed at the notification-task boundary, since process start.
pub fn task_panics() -> u64 {
    TASK_PANICS.load(std::sync::atomic::Ordering::Relaxed)
}

/// Entity-level members that are NOT Attributes. Table 5.2.12-1 scopes
/// watchedAttributes to "Properties or Relationships", so the entity's own
/// system members (including 4.22 `expiresAt` and `deletedAt`) must never be
/// diffed as attribute-level changes.
#[derive(Clone, Copy, PartialEq, Debug)]
enum ChangeClass {
    Created,
    Updated,
    Deleted,
}

/// A per-instance tenant-keyed document mirror (bus=nats). One
/// instance holds subscriptions (fed by the KV watcher), another holds
/// registrations (fed by `ANTARES_REGISTRY` deltas). Postgres stays the
/// system of record — this map is a cache with exactly one writer (the
/// watcher task); readers only snapshot.
#[derive(Default)]
pub struct DocMirror {
    map: std::sync::RwLock<
        std::collections::HashMap<String, std::collections::HashMap<String, Value>>,
    >,
}

/// Both mirror flavours accept `{tenant, id, doc|null}` deltas — the seam
/// wiring's hydrate/watch loops program against.
pub trait Mirror: Send + Sync {
    fn apply(&self, tenant: &str, id: &str, doc: Option<Value>);

    /// A Context Source Registration Subscription was written somewhere.
    /// Mirrors that serve no interval sweep have nothing to do.
    fn csub_written(&self) {}
}

impl Mirror for DocMirror {
    fn apply(&self, tenant: &str, id: &str, doc: Option<Value>) {
        DocMirror::apply(self, tenant, id, doc);
    }
}

/// The subscription mirror — docs plus the inverted candidate index.
///
/// Bucketing per subscription (conservative, union-of-buckets = candidates):
/// - has an `entities` selector whose every entry names a plain expanded
///   type IRI → `by_type[iri]` (idPattern/watchedAttributes narrow FURTHER,
///   so type is the widest exact key);
/// - no selector but `watchedAttributes` → `by_attr[iri]` (such a sub can
///   only fire when a watched attribute changed — 5.8.6);
/// - anything else (4.17 selection expressions, shapes the index cannot
///   prove) → `broad`, evaluated on every change.
///
/// The mirror also carries the interval sweep's clocks: the earliest instant
/// at which a periodic subscription it holds can be due (5.8.6 sends that
/// Notification "when the time interval (in seconds) specified in such value
/// field is reached"), so ticks that cannot fire anything never read the
/// store. Per instance, not global — one broker process, one clock pair.
#[derive(Default)]
pub struct SubMirror {
    map: std::sync::RwLock<std::collections::HashMap<String, TenantIndex>>,
    /// Epoch millis; `0` = sweep at the next tick. Set by every sweep from
    /// the subscriptions it saw, and zeroed again by `apply` whenever a
    /// periodic subscription is written, so a new one is never waited out.
    next_sub_sweep_ms: std::sync::atomic::AtomicI64,
    /// The same clock for Context Source Registration Subscriptions
    /// (5.11.7). They are not mirrored as documents — the sweep reads them
    /// from the store — so a write signals this clock through
    /// [`SubMirror::csub_written`] instead, and a sweep falls back to
    /// `CSUB_SWEEP_BACKSTOP_MS` in case a signal was lost.
    next_csub_sweep_ms: std::sync::atomic::AtomicI64,
}

/// The longest a sweep parks the Context Source Registration Subscription
/// half when nothing it saw is due.
///
/// A write clears the clock, so this is not the path a newly created
/// periodic subscription waits on: it is the repair time for a lost signal,
/// which on the bus is a KV put that exhausted its retries. Between sweeps
/// the half costs one `list` per tenant, and the tenant target is 10 000, so
/// polling it at the tick rate is the broker's whole idle cost with nothing
/// periodic configured — which is the state most deployments are in.
///
/// Table 5.2.12-1 bounds `timeInterval` only by "greater than 0", so a
/// sub-second interval is legal; the tick period bounds how closely any of
/// them can be served, signal or no signal.
// ponytail: one `list` per tenant per backstop, cross-tenant enumeration of
// the periodic rows would replace it — that needs the RLS service escape
// (`antares.service`) extended to the two subscription tables.
const CSUB_SWEEP_BACKSTOP_MS: i64 = 60_000;

#[derive(Default)]
struct TenantIndex {
    docs: std::collections::HashMap<String, Value>,
    by_type: std::collections::HashMap<String, std::collections::HashSet<String>>,
    by_attr: std::collections::HashMap<String, std::collections::HashSet<String>>,
    broad: std::collections::HashSet<String>,
}

/// Which index bucket(s) one stored subscription doc belongs in.
enum Keys {
    Types(Vec<String>),
    Attrs(Vec<String>),
    Broad,
}

fn index_keys(doc: &Value) -> Keys {
    if let Some(entities) = doc.get("entities").and_then(Value::as_array) {
        let mut types = Vec::new();
        for e in entities {
            match e.get("type").and_then(Value::as_str) {
                // 4.17 selection expressions (and any wildcard) are evaluated
                // at match time — the index cannot prove them, so the whole
                // sub goes broad (entries are OR-ed: one opaque entry taints
                // the union).
                Some(t) if !t.contains(['|', ',', ';', '(', ')', '*']) => {
                    types.push(t.to_owned());
                }
                _ => return Keys::Broad,
            }
        }
        if types.is_empty() {
            return Keys::Broad;
        }
        return Keys::Types(types);
    }
    if let Some(watched) = doc.get("watchedAttributes").and_then(Value::as_array) {
        let attrs: Vec<String> = watched
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        if attrs.len() == watched.len() && !attrs.is_empty() {
            return Keys::Attrs(attrs);
        }
    }
    Keys::Broad
}

impl SubMirror {
    /// Apply one KV delta: `None` doc = deleted. Rekeys the index from the
    /// old doc before inserting the new one.
    pub fn apply(&self, tenant: &str, id: &str, doc: Option<Value>) {
        let mut map = self
            .map
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let t = map.entry(tenant.to_owned()).or_default();
        if let Some(old) = t.docs.remove(id) {
            match index_keys(&old) {
                Keys::Types(ts) => {
                    for ty in ts {
                        if let Some(s) = t.by_type.get_mut(&ty) {
                            s.remove(id);
                            if s.is_empty() {
                                t.by_type.remove(&ty);
                            }
                        }
                    }
                }
                Keys::Attrs(ats) => {
                    for a in ats {
                        if let Some(s) = t.by_attr.get_mut(&a) {
                            s.remove(id);
                            if s.is_empty() {
                                t.by_attr.remove(&a);
                            }
                        }
                    }
                }
                Keys::Broad => {
                    t.broad.remove(id);
                }
            }
        }
        if let Some(d) = doc {
            if d.get("timeInterval").is_some() {
                // A periodic subscription just appeared or changed its
                // anchor: the sweep clock computed without it must not hold
                // it back (5.8.6).
                self.next_sub_sweep_ms
                    .store(0, std::sync::atomic::Ordering::Relaxed);
            }
            match index_keys(&d) {
                Keys::Types(ts) => {
                    for ty in ts {
                        t.by_type.entry(ty).or_default().insert(id.to_owned());
                    }
                }
                Keys::Attrs(ats) => {
                    for a in ats {
                        t.by_attr.entry(a).or_default().insert(id.to_owned());
                    }
                }
                Keys::Broad => {
                    t.broad.insert(id.to_owned());
                }
            }
            t.docs.insert(id.to_owned(), d);
        }
        if map.get(tenant).is_some_and(|t| t.docs.is_empty()) {
            map.remove(tenant);
        }
    }

    /// 5.11.7: a Context Source Registration Subscription was written. Only
    /// the clock moves — these are matched against registrations rather than
    /// entities, so they are not carried in the candidate index, and the
    /// sweep reads the tenant's rows from the store once it wakes.
    pub fn csub_written(&self) {
        self.next_csub_sweep_ms
            .store(0, std::sync::atomic::Ordering::Relaxed);
    }

    /// The hot path: subscriptions that could possibly fire for a change
    /// touching these entity types and these changed attributes. Union of
    /// the type hits, the attr hits and the broad bucket — a superset of
    /// the firing set, never a subset.
    pub fn candidates(&self, tenant: &str, types: &[&str], changed_attrs: &[&str]) -> Vec<Value> {
        let map = self
            .map
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(t) = map.get(tenant) else {
            return Vec::new();
        };
        let mut ids: std::collections::HashSet<&str> = t.broad.iter().map(String::as_str).collect();
        for ty in types {
            if let Some(s) = t.by_type.get(*ty) {
                ids.extend(s.iter().map(String::as_str));
            }
        }
        for a in changed_attrs {
            if let Some(s) = t.by_attr.get(*a) {
                ids.extend(s.iter().map(String::as_str));
            }
        }
        ids.iter()
            .filter_map(|id| t.docs.get(*id).cloned())
            .collect()
    }

    pub fn docs(&self, tenant: &str) -> Vec<Value> {
        self.map
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(tenant)
            .map(|t| t.docs.values().cloned().collect())
            .unwrap_or_default()
    }

    /// The interval sweep's whole input: the tenant's periodic (5.2.12
    /// `timeInterval`) subscriptions. The walk is over every subscription the
    /// tenant holds, but only the periodic ones are cloned — the sweep clocks
    /// keep a tick with nothing due from reaching this at all.
    fn periodic_docs(&self, tenant: &str) -> Vec<Value> {
        self.map
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(tenant)
            .map(|t| {
                t.docs
                    .values()
                    .filter(|d| d.get("timeInterval").is_some())
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn tenants(&self) -> Vec<String> {
        self.map
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .keys()
            .cloned()
            .collect()
    }
}

impl Mirror for SubMirror {
    fn apply(&self, tenant: &str, id: &str, doc: Option<Value>) {
        SubMirror::apply(self, tenant, id, doc);
    }

    fn csub_written(&self) {
        SubMirror::csub_written(self);
    }
}

impl DocMirror {
    /// Apply one KV delta: `None` doc = deleted.
    pub fn apply(&self, tenant: &str, id: &str, doc: Option<Value>) {
        let mut map = self
            .map
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match doc {
            Some(d) => {
                map.entry(tenant.to_owned())
                    .or_default()
                    .insert(id.to_owned(), d);
            }
            None => {
                if let Some(t) = map.get_mut(tenant) {
                    t.remove(id);
                    if t.is_empty() {
                        map.remove(tenant);
                    }
                }
            }
        }
    }

    pub fn docs(&self, tenant: &str) -> Vec<Value> {
        self.map
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(tenant)
            .map(|t| t.values().cloned().collect())
            .unwrap_or_default()
    }

    pub fn tenants(&self) -> Vec<String> {
        self.map
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .keys()
            .cloned()
            .collect()
    }
}

/// Where the matcher reads candidates from: the indexed mirror (both bus
/// modes wire one), with the store scan only as the never-wired
/// fallback so a missing mirror degrades to correct-but-slow.
fn subs_for(st: &AppState, tenant: &TenantId, types: &[&str], changed: &[&str]) -> Vec<Value> {
    match &st.sub_mirror {
        Some(m) => m.candidates(tenant.as_str(), types, changed),
        // The scan is the fallback, so its own failure may not be read as
        // "this tenant has no subscriptions" either: that is the silence
        // the mirror seed refuses to install.
        None => match st.store.list(tenant, Kind::Subscription) {
            Ok(subs) => subs,
            Err(e) => {
                tracing::error!(
                    "subscription scan failed for tenant {}: {e}; no candidate matched this change",
                    tenant.as_str()
                );
                Vec::new()
            }
        },
    }
}

/// Fill a mirror from the store, or say why it could not be filled.
///
/// Every document of every tenant has to be in the mirror before it is
/// installed. `CurrentStateDriver::subscription_tenants` states the rule for
/// the data path — "A SUBSET is a silent outage: a tenant missing here never
/// fires a periodic notification and never reaches the mirror" — and an
/// error absorbed into an empty list is the same subset by another route.
/// A connection failure at startup refuses it, so this is reachable rather
/// than theoretical.
///
/// Paged, not `list`: 5.5.6 licenses TooManyResults for "a query operation
/// … producing so many results that can potentially exhaust client or
/// server resources", and the seed is not one — it must see every
/// document of every tenant or it is the silent outage above. Reading it
/// through the ceiling `list` carries for client queries made one tenant's
/// stored volume decide whether OTHER tenants are matched at all.
///
/// One function for every mirror and both bus modes. `bus=local` seeds the
/// subscription mirror here; `bus=nats` seeds a subscription mirror and a
/// registration mirror, and re-seeds the registration one after a consumer
/// gap. Those were a second copy of this walk that kept the ceiling and
/// swallowed the error into an empty list, which is how a rule fixed in one
/// place stayed broken in the others.
///
/// The domain is `subscription_tenants`, whose contract covers every kind a
/// mirror is built from. A tenant holding nothing of this kind costs one
/// empty page.
pub fn seed_mirror(
    store: &dyn antares_store::CurrentStateDriver,
    mirror: &dyn Mirror,
    kind: Kind,
) -> Result<(), antares_model::NgsiError> {
    for tenant_str in store.subscription_tenants()? {
        let tenant = TenantId::new(&tenant_str)?;
        let mut after: Option<String> = None;
        loop {
            let page = store.list_page(&tenant, kind, after.as_deref(), SEED_PAGE)?;
            let short = page.len() < SEED_PAGE;
            let before = after.clone();
            for doc in page {
                if let Some(id) = doc.get("id").and_then(Value::as_str) {
                    let id = id.to_owned();
                    after = Some(id.clone());
                    mirror.apply(&tenant_str, &id, Some(doc));
                }
            }
            // A short page is the end. A cursor that did not move is also the
            // end, and it is the load-bearing half: only a document carrying
            // an `id` can advance it, so a full page without one would
            // otherwise re-read the same page forever. No write path stores
            // such a document — which is exactly why the loop may not depend
            // on that staying true.
            if short || after == before {
                break;
            }
        }
    }
    Ok(())
}

/// Documents per mirror-seed page: the peak transient allocation of the
/// walk, paid once per tenant at startup.
const SEED_PAGE: usize = 1_000;

/// Wire the store hook and background tasks. Call once at startup.
pub fn wire(state: &mut AppState) {
    // bus=local: the same indexed mirror the nats wiring builds, fed
    // synchronously by the CUD hook — the matcher never rescans the store.
    let mirror = Arc::new(SubMirror::default());
    match seed_mirror(state.store.as_ref(), mirror.as_ref(), Kind::Subscription) {
        Ok(()) => state.sub_mirror = Some(mirror.clone()),
        // Not installed, so `subs_for` takes the store scan it documents as
        // the missing-mirror fallback. Installing what the seed managed to
        // read would be the one outcome that is neither correct nor slow:
        // the matcher reads candidates from the mirror alone, so a
        // subscription absent from it never fires again in this process.
        Err(e) => tracing::error!(
            "subscription mirror seed failed ({e}); \
             matching falls back to a store scan per change"
        ),
    }
    let m = mirror.clone();
    state.sub_sync = Some(Arc::new(
        move |tenant: &TenantId, kind: Kind, id: &str, doc: Option<&Value>| match kind {
            Kind::CSourceSubscription => m.csub_written(),
            _ => m.apply(tenant.as_str(), id, doc.cloned()),
        },
    ));

    // The queue carries whole before+after payloads and is drained one
    // inline delivery at a time, so behind one slow subscriber an unbounded
    // queue grows until the process dies. Bounded instead: a full queue drops
    // the change and counts it.
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Vec<Change>>(CHANGE_QUEUE);
    let flush_tx = tx.clone();
    let flush_pending = state.pending_changes.clone();
    state.change_flush = Some(Arc::new(move |changes: Vec<Change>| {
        queue_for_matching(&flush_tx, &flush_pending, changes)
    }));
    // Temporal auto-recording runs SYNCHRONOUSLY on the hook (read-your-writes:
    // the ETSI suite queries history immediately after a write); the matcher
    // work is handed to the async task below. One choke point for every write.
    let st_rec = state.clone();
    let hook_pending = state.pending_changes.clone();
    state
        .store
        .set_change_hook(Box::new(move |tenant, before, after| {
            record_temporal_change(&st_rec, tenant, before.as_ref(), after.as_ref());
            // inside a request the change rides the request's buffer and
            // reaches the matcher with the rest of that request's changes
            let Some(change) =
                crate::history::buffer_change((tenant.as_str().to_owned(), before, after))
            else {
                return;
            };
            queue_for_matching(&tx, &hook_pending, vec![change]);
        }));
    let st = state.clone();
    let pending = state.pending_changes.clone();
    crate::spawn_loop(async move {
        while let Some(mut batch) = rx.recv().await {
            let mut taken = 1;
            // everything already queued behind it rides the same pass
            while batch.len() < CHANGE_BATCH {
                match rx.try_recv() {
                    Ok(c) => {
                        batch.extend(c);
                        taken += 1;
                    }
                    Err(_) => break,
                }
            }
            let st = st.clone();
            guarded(async move { process_changes(&st, batch).await }).await;
            pending.fetch_sub(taken, std::sync::atomic::Ordering::SeqCst);
        }
    });
    let st = state.clone();
    crate::spawn_loop(async move {
        loop {
            // Tokio's timer natively; the browser's own timer on wasm32
            // (tokio time never fires without a reactor there).
            #[cfg(not(target_arch = "wasm32"))]
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            #[cfg(target_arch = "wasm32")]
            gloo_timers::future::TimeoutFuture::new(500).await;
            let st = st.clone();
            guarded(async move { interval_tick(&st).await }).await;
        }
    });
}

/// Run one pipeline step on its own task so a panic inside it cannot end
/// notification delivery for the whole process: the task boundary absorbs
/// the panic, it is counted and logged, and the caller keeps consuming — a
/// later matching change still notifies (5.8.6). The step is awaited, so
/// delivery stays as serial as it was.
#[cfg(not(target_arch = "wasm32"))]
async fn guarded<F>(fut: F)
where
    F: std::future::Future<Output = ()>,
{
    use futures_util::FutureExt as _;
    // Caught here rather than on a spawned task: a task boundary would demand
    // Send + 'static of the step, and the interval step holds state that is
    // not Sync. Unwinding in place absorbs the panic just as well and keeps
    // the step running on this task, so delivery stays exactly as serial.
    if std::panic::AssertUnwindSafe(fut)
        .catch_unwind()
        .await
        .is_err()
    {
        note_panic();
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn note_panic() {
    TASK_PANICS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    metrics::counter!("antares_notification_task_panics_total").increment(1);
    tracing::error!("notification pipeline task panicked; this change is lost");
}

/// Wasm32 has no task boundary to catch with (single-threaded executor, and
/// the browser profile aborts on panic); the step runs inline.
#[cfg(target_arch = "wasm32")]
async fn guarded<F>(fut: F)
where
    F: std::future::Future<Output = ()>,
{
    fut.await;
}

/// Strip volatile members before comparing attribute instance arrays.
fn stable(v: &Value) -> Value {
    match v {
        Value::Array(a) => Value::Array(a.iter().map(stable).collect()),
        Value::Object(o) => Value::Object(
            o.iter()
                .filter(|(k, _)| !matches!(k.as_str(), "createdAt" | "modifiedAt" | "instanceId"))
                .map(|(k, v)| (k.clone(), stable(v)))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn attr_keys(doc: &Value) -> Vec<String> {
    doc.as_object()
        .map(|o| {
            o.keys()
                .filter(|k| !crate::repr::ENTITY_META.contains(&k.as_str()))
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

/// Per-attribute change classification between two internal docs.
fn diff(before: Option<&Value>, after: Option<&Value>) -> Vec<(String, ChangeClass)> {
    let empty = Value::Object(Map::new());
    let b = before.unwrap_or(&empty);
    let a = after.unwrap_or(&empty);
    let mut keys = attr_keys(b);
    for k in attr_keys(a) {
        if !keys.contains(&k) {
            keys.push(k);
        }
    }
    let mut out = Vec::new();
    for k in keys {
        let bv = b.get(&k);
        let av = a.get(&k);
        match (bv, av) {
            (None, Some(_)) => out.push((k, ChangeClass::Created)),
            (Some(_), None) => out.push((k, ChangeClass::Deleted)),
            (Some(x), Some(y)) => {
                // instance-level deletion (by datasetId) counts as
                // attributeDeleted even when other instances survive (5.8.6)
                let bx: Vec<&Value> = x.as_array().map(|a| a.iter().collect()).unwrap_or_default();
                let by: Vec<&Value> = y.as_array().map(|a| a.iter().collect()).unwrap_or_default();
                let removed = bx
                    .iter()
                    .any(|bi| !by.iter().any(|ai| instance_ds(ai) == instance_ds(bi)));
                if removed {
                    out.push((k.clone(), ChangeClass::Deleted));
                    let survivors_changed = by.iter().any(|ai| {
                        match bx.iter().find(|bi| instance_ds(bi) == instance_ds(ai)) {
                            Some(bi) => stable(bi) != stable(ai),
                            None => true,
                        }
                    });
                    if survivors_changed {
                        out.push((k, ChangeClass::Updated));
                    }
                } else if stable(x) != stable(y) {
                    out.push((k, ChangeClass::Updated));
                }
            }
            _ => {}
        }
    }
    out
}

/// Auto-record a current-state change into the temporal representation
/// (5.6.11). Driven by the store's SYNCHRONOUS change hook, which fires inside
/// every entity write (create, update, partial update, merge, replace, batch)
/// for both the memory and postgres stores — so a new write path records
/// without the handler having to remember. The per-handler `mirror_record`
/// this replaced was the forgettable trap that left Partial Attribute Update
/// (5.6.4) and Replace Attribute (5.6.19) silently unrecorded.
///
/// Append-only, instance-precise: only the instances that are new or changed
/// (by datasetId, ignoring volatile members) are appended, so a multi-instance
/// attribute does not re-record unchanged datasets. Entity and attribute
/// DELETIONS keep their dedicated typed-null mirrors (`mirror_delete_entity` /
/// `mirror_delete_attr`), which the delete handlers still call — their deletion
/// shape is not derivable from a plain append.
pub fn record_temporal_change(
    st: &AppState,
    tenant: &TenantId,
    before: Option<&Value>,
    after: Option<&Value>,
) {
    if !st.record_locally() {
        return;
    }
    let Some(after) = after else {
        return; // entity deletion — handled by mirror_delete_entity
    };
    let Some(id) = after.get("id").and_then(Value::as_str) else {
        return;
    };
    let mut shell = Map::new();
    for k in ["id", "type", "createdAt", "modifiedAt", "scope"] {
        if let Some(v) = after.get(k) {
            shell.insert(k.to_string(), v.clone());
        }
    }
    let shell = Value::Object(shell);
    let event = |op, attr: &str, instance| TemporalEvent {
        op,
        tenant: tenant.clone(),
        entity_id: id.to_owned(),
        shell: shell.clone(),
        attr: attr.to_owned(),
        instance,
    };
    // 4.5.6: the Scope of a Temporal Evolution is represented as a temporal
    // Property whose only sub-properties are the non-reified createdAt,
    // modifiedAt, deletedAt and observedAt; when it "is updated as the result
    // of a change from the Core API, the observedAt sub-Property should be
    // set as a copy of the modifiedAt sub-Property".
    if before.is_some_and(|b| b.get("scope") != after.get("scope")) {
        if let Some(scope) = after.get("scope") {
            let ts = after
                .get("modifiedAt")
                .and_then(Value::as_str)
                .map(String::from)
                .unwrap_or_else(now_iso);
            let inst = json!({
                "type": "Property",
                "value": scope.clone(),
                "instanceId": format!("urn:ngsi-ld:Instance:{}", uuid::Uuid::new_v4()),
                "createdAt": ts, "modifiedAt": ts, "observedAt": ts,
            });
            crate::history::push(st, event(TemporalOp::ScopeChanged, "scope", inst));
        }
    }
    for (k, class) in diff(before, Some(after)) {
        let op = match class {
            ChangeClass::Created => TemporalOp::AttrCreated,
            ChangeClass::Updated => TemporalOp::AttrModified,
            ChangeClass::Deleted => continue, // handled by mirror_delete_attr
        };
        let Some(av) = after.get(&k) else { continue };
        // gate 1, value-change: an unchanged instance produces no event
        for mut inst in changed_instances(before.and_then(|b| b.get(&k)), av) {
            if let Some(o) = inst.as_object_mut() {
                let iid = instance_id(id, &k, &*o);
                o.entry("instanceId".to_owned())
                    .or_insert_with(|| Value::String(iid));
            }
            crate::history::push(st, event(op, &k, inst));
        }
    }
}

/// 4.5.7: an instance is the Attribute "at a particular point in time",
/// recorded as its observedAt. The id of an observed instance is therefore
/// derived from (entity, attribute, datasetId, observedAt), so a re-send for
/// the same instant lands on the same row — the temporal store's upsert key —
/// and corrects it instead of appending a duplicate. Without observedAt there
/// is no instant to key on: a fresh random id, append-only.
///
/// The instant is keyed through `dt_key`, not through the stamp as written.
/// 4.6.3 leaves the seconds fraction optional and accepts a comma separator
/// in requests, and the broker stores a DateTime exactly as the client wrote
/// it, so one instant arrives under several spellings; keying the raw text
/// gave each spelling its own instance and left the correction's target in
/// place beside it — the failure 4.5.7 calls severe "in the case of
/// modification or deletion requests for legal reasons".
fn instance_id(entity: &str, attr: &str, inst: &serde_json::Map<String, Value>) -> String {
    let u = match inst.get("observedAt").and_then(Value::as_str) {
        Some(at) => {
            let ds = inst
                .get("datasetId")
                .and_then(Value::as_str)
                .unwrap_or("@none");
            uuid::Uuid::new_v5(
                &uuid::Uuid::NAMESPACE_URL,
                format!("{entity}\n{attr}\n{ds}\n{}", dt_key(at)).as_bytes(),
            )
        }
        None => uuid::Uuid::new_v4(),
    };
    format!("urn:ngsi-ld:Instance:{u}")
}

/// The instances in `after` that are new or changed vs `before` — matched by
/// datasetId, compared via `stable` (volatile members ignored). A newly
/// created attribute (`before` None) contributes every instance.
fn changed_instances(before: Option<&Value>, after: &Value) -> Vec<Value> {
    let before_arr: Vec<&Value> = before
        .and_then(Value::as_array)
        .map(|a| a.iter().collect())
        .unwrap_or_default();
    after
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|ai| {
            match before_arr
                .iter()
                .find(|bi| instance_ds(bi) == instance_ds(ai))
            {
                None => true,
                Some(bi) => stable(bi) != stable(ai),
            }
        })
        .collect()
}

/// One string-valued member of a stored Subscription, absent or of another
/// JSON type reading the same as absent.
fn sub_str<'a>(sub: &'a Value, key: &str) -> Option<&'a str> {
    sub.get(key).and_then(Value::as_str)
}

/// 4.9 EXAMPLE 13/14: linked-entity q terms (`attr{path}`) resolve through
/// the local store, same tenant.
pub(crate) fn store_lookup<'a>(
    st: &'a AppState,
    tenant: &'a TenantId,
) -> impl Fn(&str) -> Option<Value> + 'a {
    move |uri: &str| st.store.get(tenant, Kind::Entity, uri).ok().flatten()
}

pub(crate) use antares_matcher::{
    conditions_match, geo_params, is_active, selector_match, throttled,
};

/// The @context governing a subscription's notifications (5.8.6): the
/// jsonldContext member if set, else the @context of the creating request.
pub(crate) async fn sub_context(st: &AppState, tenant: &TenantId, sub: &Value) -> Arc<Context> {
    let source = sub
        .get("jsonldContext")
        .cloned()
        .or_else(|| sub.get("__context").cloned());
    match source {
        // 5.5.10: the Subscription belongs to one Tenant, so the @context it
        // names resolves within that Tenant — a Hosted @context another Tenant
        // stored (5.13.1) is not in scope here and falls back to the core
        // context rather than compacting this Notification against it.
        Some(v) if !v.is_null() => st
            .loader
            .resolve_quiet_for(tenant, &v)
            .await
            .unwrap_or_else(|_| st.loader.core()),
        _ => st.loader.core(),
    }
}

/// Per-type NGSI-LD-null member and its showChanges previous-member (5.8.6).
fn null_members(atype: &str) -> (&'static str, Value, &'static str) {
    let null = Value::String("urn:ngsi-ld:null".into());
    match atype {
        "Relationship" => ("object", null, "previousObject"),
        "LanguageProperty" => (
            "languageMap",
            json!({"@none": "urn:ngsi-ld:null"}),
            "previousLanguageMap",
        ),
        "JsonProperty" => ("json", null, "previousJson"),
        "VocabProperty" => ("vocab", null, "previousVocab"),
        _ => ("value", null, "previousValue"),
    }
}

fn current_member(atype: &str) -> &'static str {
    null_members(atype).0
}

/// The deletion tombstone for one former attribute instance (5.8.6 payload
/// forms: typed null + optional datasetId / sysAttrs stamps / previous value).
fn tombstone(before_inst: &Value, sys: bool, show: bool, now: &str) -> Value {
    let atype = before_inst
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("Property");
    let (member, null_val, prev_member) = null_members(atype);
    let mut m = Map::new();
    m.insert("type".into(), Value::String(atype.to_owned()));
    m.insert(member.into(), null_val);
    if let Some(ds) = before_inst.get("datasetId") {
        m.insert("datasetId".into(), ds.clone());
    }
    if sys {
        for k in ["createdAt", "modifiedAt"] {
            if let Some(v) = before_inst.get(k) {
                m.insert(k.into(), v.clone());
            }
        }
        m.insert("deletedAt".into(), Value::String(now.to_owned()));
    }
    if show {
        if let Some(prev) = before_inst.get(member) {
            m.insert(prev_member.into(), prev.clone());
        }
    }
    Value::Object(m)
}

fn instance_ds(inst: &Value) -> Option<&str> {
    inst.get("datasetId").and_then(Value::as_str)
}

/// Instances of `before[attr]` that no longer exist in `after[attr]`
/// (matched by datasetId — instance-level deletions count, 046_22_06).
fn deleted_instances<'a>(before: &'a Value, after: Option<&Value>, attr: &str) -> Vec<&'a Value> {
    let b: Vec<&Value> = before
        .get(attr)
        .and_then(Value::as_array)
        .map(|a| a.iter().collect())
        .unwrap_or_default();
    let a: Vec<&Value> = after
        .and_then(|d| d.get(attr))
        .and_then(Value::as_array)
        .map(|x| x.iter().collect())
        .unwrap_or_default();
    b.into_iter()
        .filter(|bi| !a.iter().any(|ai| instance_ds(ai) == instance_ds(bi)))
        .collect()
}

pub(crate) struct NotifShape {
    pub(crate) repr: crate::repr::Repr,
    pub(crate) show_changes: bool,
    pub(crate) join: Option<(String, usize)>,
}

pub(crate) fn notif_shape(sub: &Value, ctx: &Context) -> NotifShape {
    let n = sub.get("notification").and_then(Value::as_object);
    let format = n
        .and_then(|n| n.get("format"))
        .and_then(Value::as_str)
        .unwrap_or("normalized");
    let mut repr = crate::repr::Repr {
        sys_attrs: n
            .and_then(|n| n.get("sysAttrs"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
        key_values: matches!(format, "keyValues" | "simplified"),
        concise: format == "concise",
        ..Default::default()
    };
    let names = |key: &str| -> Option<Vec<String>> {
        n.and_then(|n| n.get(key))
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
    };
    if let Some(attrs) = names("attributes") {
        repr.attrs = Some(attrs.iter().map(|a| ctx.expand_key(a)).collect());
    }
    // 4.21 NGSI-LD Attribute Projection Language: pick/omit values are
    // projection language strings, which Table 5.2.14.1-1 requires for the
    // notification members too ("a valid attribute projection language string
    // as per clause 4.21"). Each term may carry a LinkedEntityTerm
    // (`ProjectionTerm = AttrName *1(LinkedEntityTerm)`), which is what
    // constrains an Attribute inside a Linked Entity retrieved by join. Building
    // the nodes by hand here degraded `refDevice{type}` to a literal Attribute
    // name matching nothing, so the term was dropped instead of applied.
    // A term that fails to parse is dropped rather than kept flat: for pick that
    // withholds the Attribute, which is the safe direction.
    let nodes = |list: Vec<String>| -> Vec<crate::repr::ProjNode> {
        list.iter()
            .filter_map(|term| crate::repr::parse_projection(term, ctx).ok())
            .flatten()
            .collect()
    };
    if let Some(pick) = names("pick") {
        repr.pick = Some(nodes(pick));
    }
    if let Some(omit) = names("omit") {
        repr.omit = Some(nodes(omit));
    }
    if let Some(ds) = sub.get("datasetId").and_then(Value::as_array) {
        repr.dataset_id = Some(
            ds.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
        );
    }
    let join = n
        .and_then(|n| n.get("join"))
        .and_then(Value::as_str)
        .filter(|j| *j == "inline" || *j == "flat")
        .map(|j| {
            // the stored member is bounded at creation, but a Subscription
            // written before that bound existed is still on disk, so the
            // traversal takes the ceiling from here too
            let level = (n
                .and_then(|n| n.get("joinLevel"))
                .and_then(Value::as_u64)
                .unwrap_or(1) as usize)
                .min(crate::bounds::MAX_JOIN_LEVEL);
            (j.to_owned(), level)
        });
    NotifShape {
        repr,
        show_changes: n
            .and_then(|n| n.get("showChanges"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
        join,
    }
}

/// Build the notification `data` array for one change and one subscription.
#[allow(clippy::too_many_arguments)] // one param per 5.8.6 payload input
fn build_data(
    st: &AppState,
    tenant: &TenantId,
    sub: &Value,
    ctx: &Context,
    before: Option<&Value>,
    after: Option<&Value>,
    relevant_deleted_attrs: &[String],
    entity_deleted_fired: bool,
    now: &str,
) -> Vec<Value> {
    let shape = notif_shape(sub, ctx);
    let sys = shape.repr.sys_attrs;
    let show = shape.show_changes;
    let internal = match after {
        Some(a) => {
            let mut doc = a.clone();
            if show {
                // previous* on changed instances (046_31..33). An entity off
                // the change feed is an object; one that is not carries
                // nothing to decorate and travels on unchanged.
                if let (Some(b), Some(obj)) = (before, doc.as_object_mut()) {
                    for (k, v) in obj.iter_mut() {
                        if crate::repr::ENTITY_META.contains(&k.as_str()) {
                            continue;
                        }
                        let Some(arr) = v.as_array_mut() else {
                            continue;
                        };
                        for inst in arr {
                            let Some(bi) = b.get(k).and_then(Value::as_array).and_then(|ba| {
                                ba.iter().find(|x| instance_ds(x) == instance_ds(inst))
                            }) else {
                                continue;
                            };
                            let atype = inst
                                .get("type")
                                .and_then(Value::as_str)
                                .unwrap_or("Property");
                            let member = current_member(atype);
                            let (_, _, prev_member) = null_members(atype);
                            if bi.get(member) != inst.get(member) {
                                if let (Some(pv), Some(io)) =
                                    (bi.get(member).cloned(), inst.as_object_mut())
                                {
                                    io.insert(prev_member.into(), pv);
                                }
                            }
                        }
                    }
                }
            }
            // deletion tombstones appended beside surviving instances
            if let Some(b) = before {
                for attr in relevant_deleted_attrs {
                    let gone = deleted_instances(b, Some(&doc), attr);
                    if gone.is_empty() {
                        continue;
                    }
                    let attr_absent = doc.get(attr).is_none();
                    let tss: Vec<Value> = if attr_absent {
                        // whole attribute deleted at once: ONE tombstone,
                        // no datasetId (046_22_08)
                        let base = gone
                            .iter()
                            .find(|i| instance_ds(i).is_none())
                            .unwrap_or(&gone[0]);
                        let mut ts = tombstone(base, sys, show, now);
                        if let Some(o) = ts.as_object_mut() {
                            o.remove("datasetId");
                        }
                        vec![ts]
                    } else {
                        gone.iter()
                            .map(|di| tombstone(di, sys, show, now))
                            .collect()
                    };
                    let Some(target) = doc.as_object_mut() else {
                        continue;
                    };
                    if let Some(arr) = target
                        .entry(attr.clone())
                        .or_insert_with(|| Value::Array(vec![]))
                        .as_array_mut()
                    {
                        arr.extend(tss);
                    }
                }
            }
            doc
        }
        None => {
            // entity deleted: tombstone entity (046_21) + per-trigger attrs.
            // The caller returns early unless one of before/after is there,
            // so this arm has a before; without one there is nothing to
            // describe and nothing to notify about.
            let Some(b) = before else {
                return Vec::new();
            };
            let mut m = Map::new();
            for k in ["id", "type"] {
                if let Some(v) = b.get(k) {
                    m.insert(k.into(), v.clone());
                }
            }
            if sys {
                for k in ["createdAt", "modifiedAt"] {
                    if let Some(v) = b.get(k) {
                        m.insert(k.into(), v.clone());
                    }
                }
            }
            m.insert("deletedAt".into(), Value::String(now.to_owned()));
            let attrs: Vec<String> = if entity_deleted_fired && show {
                attr_keys(b) // showChanges: every attribute, tombstoned (046_37)
            } else {
                relevant_deleted_attrs.to_vec()
            };
            for attr in attrs {
                let insts: Vec<Value> = b
                    .get(&attr)
                    .and_then(Value::as_array)
                    .map(|a| a.iter().map(|i| tombstone(i, sys, show, now)).collect())
                    .unwrap_or_default();
                if !insts.is_empty() {
                    m.insert(attr, Value::Array(insts));
                }
            }
            Value::Object(m)
        }
    };
    let shaped = crate::repr::apply(&internal, &shape.repr);
    let mut main = crate::entities::compact_for(&shape.repr, &shaped, ctx);
    let mut data = Vec::new();
    match &shape.join {
        Some((mode, level)) if mode == "inline" => {
            crate::entities::inline_join(st, tenant, ctx, &shape.repr, &mut main, *level);
            data.push(main);
        }
        Some((mode, level)) if mode == "flat" => {
            let main_id = internal.get("id").and_then(Value::as_str).unwrap_or("");
            let mut linked = std::collections::BTreeMap::new();
            crate::entities::collect_flat(st, tenant, &shape.repr, &internal, *level, &mut linked);
            data.push(main);
            for (id, (ldoc, lrepr)) in linked {
                if id != main_id {
                    data.push(crate::entities::compact_for(
                        &lrepr,
                        &crate::repr::apply(&ldoc, &lrepr),
                        ctx,
                    ));
                }
            }
        }
        _ => data.push(main),
    }
    data
}

/// The notification triggers of one subscription, in the form the matcher
/// compares against (Table 5.2.12-1): absent means the default combination
/// `"attributeCreated"` + `"attributeUpdated"`, and `"entityUpdated"` "is
/// equivalent to the combination `"attributeCreated"`, `"attributeUpdated"`
/// and `"attributeDeleted"`" — so it is expanded here, at the single point
/// the list is read, rather than at each comparison.
fn triggers_of(sub: &Value) -> Vec<String> {
    let mut triggers: Vec<String> = sub
        .get("notificationTrigger")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_else(|| DEFAULT_TRIGGERS.iter().map(|s| s.to_string()).collect());
    if triggers.iter().any(|t| t == "entityUpdated") {
        for t in DEFAULT_TRIGGERS.iter().copied().chain(["attributeDeleted"]) {
            if !triggers.iter().any(|s| s == t) {
                triggers.push(t.to_owned());
            }
        }
    }
    triggers
}

/// One change queue event: tenant, before-image, after-image.
pub type Change = (String, Option<Value>, Option<Value>);

/// Changes one drain of the queue folds into one delivery pass — a batch
/// request's N writes arrive as N events back to back and leave as ONE
/// notification per matching subscription. Bounded so a flood cannot hold
/// the first notification back indefinitely.
const CHANGE_BATCH: usize = 256;

/// One matched (subscription, entity) pair before delivery.
struct Matched {
    tenant: TenantId,
    sub: Value,
    ctx: Arc<Context>,
    data: Vec<Value>,
}

pub async fn process_change(
    st: &AppState,
    tenant_str: &str,
    before: Option<Value>,
    after: Option<Value>,
) {
    process_changes(st, vec![(tenant_str.to_owned(), before, after)]).await;
}

/// 5.8.6: "the Notification ... data ... shall contain the Entities that
/// match" — every change of one drain that matches the same subscription
/// travels in one notification, so a batch of N entities is one POST with N
/// data entries (and timesSent moves by one), never N POSTs.
pub async fn process_changes(st: &AppState, changes: Vec<Change>) {
    let mut groups: Vec<Matched> = Vec::new();
    // (tenant, subscription id) → its group. A scan for the group would be
    // linear in the subscriptions already matched, and a drain of
    // CHANGE_BATCH changes over S matching subscriptions walks it S times per
    // change: quadratic in the one dimension this broker is built to grow
    // (100 000 subscriptions), on the notification hot path.
    let mut index: std::collections::HashMap<(String, String), usize> =
        std::collections::HashMap::new();
    for (tenant_str, before, after) in changes {
        for m in matches_for(st, &tenant_str, before, after).await {
            let key = (
                m.tenant.as_str().to_owned(),
                m.sub
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            );
            match index.get(&key) {
                Some(&i) => groups[i].data.extend(m.data),
                None => {
                    index.insert(key, groups.len());
                    groups.push(m);
                }
            }
        }
    }
    // Groups are distinct subscriptions, so they leave concurrently; a
    // subscription's changes in this drain are already one group, and the
    // next drain starts only after this one, so per-subscription order holds.
    // Each group is its own task: the store bridge blocks the calling
    // thread (block_in_place) for every bookkeeping read/write, and inside
    // one task's for_each_concurrent that stalled all 64 deliveries behind
    // each Postgres round-trip (~320 notifications/s on 32 idle cores).
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut set = tokio::task::JoinSet::new();
        for g in groups {
            let st = st.clone();
            set.spawn(async move {
                let _permit = DELIVERY_SLOTS.acquire().await;
                deliver(&st, &g.tenant, &g.sub, g.data, &g.ctx).await;
            });
        }
        while let Some(joined) = set.join_next().await {
            if joined.is_err() {
                note_panic();
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        use futures_util::StreamExt;
        futures_util::stream::iter(groups)
            .for_each_concurrent(DELIVERY_WIDTH, |g| async move {
                deliver(st, &g.tenant, &g.sub, g.data, &g.ctx).await;
            })
            .await;
    }
}

/// A change the full queue refused: counted, and said once per thousand so
/// a 40 % delivery gap is visible in the log and not only on the counter.
fn note_drop() {
    let n = CHANGES_DROPPED.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    metrics::counter!("antares_notification_changes_dropped_total").increment(1);
    if n == 1 || n.is_multiple_of(1000) {
        tracing::warn!("notification change queue full: {n} changes dropped so far (delivery slower than the write rate)");
    }
}

/// Notifications in flight at once per drain: one serial POST at a time
/// capped a 9-subscription fan-out at ~600 POST/s and overflowed the queue.
const DELIVERY_WIDTH: usize = 64;
#[cfg(not(target_arch = "wasm32"))]
static DELIVERY_SLOTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(DELIVERY_WIDTH);

async fn matches_for(
    st: &AppState,
    tenant_str: &str,
    before: Option<Value>,
    after: Option<Value>,
) -> Vec<Matched> {
    let mut out = Vec::new();
    let Ok(tenant) = TenantId::new(tenant_str) else {
        return out;
    };
    let changes = diff(before.as_ref(), after.as_ref());
    let entity_trigger = match (&before, &after) {
        (None, Some(_)) => "entityCreated",
        (Some(_), None) => "entityDeleted",
        _ => "entityUpdated",
    };
    let eval_doc = after.as_ref().or(before.as_ref());
    let Some(eval_doc) = eval_doc else { return out };
    // Candidate lookup by the entity's types and the changed attribute
    // IRIs — no linear scan over all subscriptions.
    let types: Vec<&str> = eval_doc
        .get("type")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let changed_keys: Vec<&str> = changes.iter().map(|(k, _)| k.as_str()).collect();
    let subs = subs_for(st, &tenant, &types, &changed_keys);
    for sub in subs {
        if !is_active(&sub) || sub.get("timeInterval").is_some() {
            continue;
        }
        let triggers = triggers_of(&sub);
        // which attribute-level changes this sub cares about
        let watched: Option<Vec<&str>> = sub
            .get("watchedAttributes")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).collect());
        let relevant: Vec<&(String, ChangeClass)> = changes
            .iter()
            .filter(|(k, _)| watched.as_ref().is_none_or(|w| w.contains(&k.as_str())))
            .collect();
        let attr_trigger_fired = relevant.iter().any(|(_, c)| {
            let t = match c {
                ChangeClass::Created => "attributeCreated",
                ChangeClass::Updated => "attributeUpdated",
                ChangeClass::Deleted => "attributeDeleted",
            };
            triggers.iter().any(|s| s == t)
        });
        let entity_trigger_fired = triggers.iter().any(|s| s == entity_trigger)
            && (watched.is_none() || !relevant.is_empty());
        if !attr_trigger_fired && !entity_trigger_fired {
            continue;
        }
        let ctx = sub_context(st, &tenant, &sub).await;
        if !selector_match(&sub, eval_doc, &ctx) {
            continue;
        }
        if !conditions_match(&sub, eval_doc, &ctx, &store_lookup(st, &tenant)) {
            continue;
        }
        if throttled(&sub) {
            continue;
        }
        let deleted: Vec<String> = if triggers.iter().any(|t| t == "attributeDeleted") {
            relevant
                .iter()
                .filter(|(_, c)| *c == ChangeClass::Deleted)
                .map(|(k, _)| k.clone())
                .collect()
        } else {
            Vec::new()
        };
        let entity_deleted_fired = after.is_none() && triggers.iter().any(|t| t == "entityDeleted");
        let now = now_iso();
        let data = build_data(
            st,
            &tenant,
            &sub,
            &ctx,
            before.as_ref(),
            after.as_ref(),
            &deleted,
            entity_deleted_fired,
            &now,
        );
        out.push(Matched {
            tenant: tenant.clone(),
            sub,
            ctx,
            data,
        });
    }
    out
}

/// When an interval subscription is next due, in epoch millis: one
/// `timeInterval` after the last Notification it sent (Table 5.2.14.2-1
/// `lastNotification`), or after its creation while it has sent none. Without
/// either anchor it is due immediately.
fn due_at_ms(sub: &Value, interval: f64) -> i64 {
    let anchor = sub
        .get("notification")
        .and_then(|n| n.get("lastNotification"))
        .and_then(Value::as_str)
        .or_else(|| sub.get("createdAt").and_then(Value::as_str));
    match anchor.and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok()) {
        Some(last) => last
            .timestamp_millis()
            .saturating_add(interval_offset_ms(interval)),
        None => i64::MIN,
    }
}

/// One period of a periodic Subscription (5.2.12 `timeInterval`) in
/// milliseconds.
///
/// Table 5.2.12-1 bounds the member only as greater than 0, so the seconds a
/// client names can exceed what epoch milliseconds hold. The cast saturates,
/// and every caller adds it with `saturating_add`, which puts an interval the
/// broker cannot schedule at the end of representable time. Adding it plainly
/// wraps the sum negative, and a negative firing instant reads as permanently
/// due: the subscription then fires its whole query on every tick, and the
/// same value poisons the process-wide sweep clock those minima feed.
fn interval_offset_ms(interval: f64) -> i64 {
    (interval * 1000.0) as i64
}

/// The exact Entity ids a subscription's `entities` selector pins down, or
/// `None` when it leaves any of them open. Table 5.2.33-1: `id` is a String
/// or a String[] and "id takes precedence over idPattern", so an entry
/// carrying an id constrains the read exactly — while one entry without an id
/// (a bare type, or an idPattern no store column can answer) admits every id
/// and forfeits the narrowing for the whole OR-ed selector.
fn selector_ids(sub: &Value) -> Option<Vec<String>> {
    let sel = sub.get("entities").and_then(Value::as_array)?;
    let mut ids = Vec::new();
    for e in sel {
        match e.get("id") {
            Some(Value::String(i)) => ids.push(i.clone()),
            Some(Value::Array(a)) => {
                for v in a {
                    ids.push(v.as_str()?.to_owned());
                }
            }
            _ => return None,
        }
    }
    (!ids.is_empty()).then_some(ids)
}

/// A store read on a delivery path has no caller to fail: the sweep is a
/// timer, the fan-out is spawned, and both answer `()`. Silence is what makes
/// a failure dangerous here — a subscription that stops firing because the
/// store could not be read looks exactly like one with nothing to send — so
/// the failure is named and the path continues on the empty set it would
/// have continued on anyway.
fn read_or_warn<T>(res: Result<Vec<T>, antares_model::NgsiError>, what: &str) -> Vec<T> {
    res.unwrap_or_else(|e| {
        tracing::warn!("notification path: reading {what} failed: {e}");
        Vec::new()
    })
}

/// timeInterval subscriptions: fire when due, with all matching entities.
/// Multi-instance: claim one interval firing under the subscription row
/// lock — N matcher pods race, exactly one wins (single-winner by
/// lock, no leader election). The due-check reruns INSIDE the lock; the
/// winner stamps `lastNotification` as its claim, losers see not-due and
/// roll back. Only engaged in bus=nats mode — single-process behaviour (and
/// its 046_12 bookkeeping ordering) is untouched.
///
/// `None` = the firing is not this pod's. `Some(prev)` = claimed, carrying
/// the `lastNotification` the claim overwrote: a firing that turns out to
/// have nothing to send gives it back through [`release_interval`], because
/// Table 5.2.14.2-1 stamps the instant a notification was SENT and 5.8.6
/// sends none when nothing matches.
fn claim_interval(
    st: &AppState,
    tenant: &TenantId,
    kind: Kind,
    sub: &Value,
    interval: f64,
) -> Option<Option<Value>> {
    let id = sub.get("id").and_then(Value::as_str)?;
    let mut prev: Option<Value> = None;
    let res = st.store.mutate(tenant, kind, id, |doc| {
        if chrono::Utc::now().timestamp_millis() < due_at_ms(doc, interval) {
            return Err(());
        }
        let Some(sub_doc) = doc.as_object_mut() else {
            // a stored Subscription is an object; one that is not carries no
            // notification member to stamp, and Err(()) is the same "nothing
            // written" the not-due branch above returns
            return Err(());
        };
        if let Some(n) = sub_doc
            .entry("notification")
            .or_insert_with(|| json!({}))
            .as_object_mut()
        {
            prev = n.insert("lastNotification".into(), Value::String(now_iso()));
        }
        Ok(())
    });
    matches!(res, Ok(Some(Ok(())))).then_some(prev)
}

/// Give a claimed firing back (5.8.6: nothing matched, so nothing was sent).
/// The stamp returns to what [`claim_interval`] found, which both keeps
/// `lastNotification` truthful and leaves the subscription due, exactly as
/// the single-process path does.
fn release_interval(st: &AppState, tenant: &TenantId, kind: Kind, id: &str, prev: Option<Value>) {
    let res = st.store.mutate::<(), ()>(tenant, kind, id, |doc| {
        if let Some(n) = doc
            .as_object_mut()
            .and_then(|o| o.get_mut("notification"))
            .and_then(Value::as_object_mut)
        {
            match &prev {
                Some(v) => n.insert("lastNotification".into(), v.clone()),
                None => n.remove("lastNotification"),
            };
        }
        Ok(())
    });
    if let Err(e) = res {
        tracing::warn!("releasing the interval claim for {id} failed: {e}");
    }
}

/// One sweep of the interval subscriptions (5.8.6, 5.11.7): "If a
/// Subscription defines a timeInterval member, a Notification shall be sent
/// periodically, when the time interval (in seconds) specified in such value
/// field is reached, regardless of Attribute changes."
///
/// The sweep runs on a fixed tick, so its idle cost is what has to stay
/// small. Two things keep it off the store. A tick that cannot fire anything
/// returns before enumerating tenants: each sweep records the earliest instant
/// a subscription it saw can next be due, and a write zeroes that clock —
/// through the mirror for Subscriptions, through `csub_written` for the
/// Context Source Registration Subscription half, which is mirrored by clock
/// rather than by document. A lost signal is repaired within
/// `CSUB_SWEEP_BACKSTOP_MS`. And a due subscription reads only the Entities
/// its own selector can match instead of its tenant's entity set.
pub async fn interval_tick(st: &AppState) {
    use std::sync::atomic::Ordering::Relaxed;
    let now_ms = chrono::Utc::now().timestamp_millis();
    let clocks = st.sub_mirror.as_ref().map(|m| {
        (
            m.next_sub_sweep_ms.load(Relaxed),
            m.next_csub_sweep_ms.load(Relaxed),
        )
    });
    let (sweep_subs, sweep_csubs) = match clocks {
        Some((sub_clock, csub_clock)) => (now_ms >= sub_clock, now_ms >= csub_clock),
        // Never-wired fallback: no clock to keep, so every tick sweeps.
        None => (true, true),
    };
    if !sweep_subs && !sweep_csubs {
        return;
    }
    // Earliest next-due instant seen by this sweep, per half.
    let mut next_sub = i64::MAX;
    let mut next_csub = i64::MAX;
    // One sweep visits every tenant, and a delivery costs up to the
    // endpoint's whole timeout (Table 5.2.15-1, 30 s at the ceiling). Awaited
    // in turn, one unresponsive endpoint becomes the deadline of every other
    // subscriber's periodic notification. The deliveries of a tick therefore
    // run together, under the same width the change path uses, and the tick
    // still does not return until they have settled: ticks never overlap, so
    // a subscription cannot be fired twice for one period.
    #[cfg(not(target_arch = "wasm32"))]
    let mut sending = tokio::task::JoinSet::new();
    for tenant_str in read_or_warn(
        st.store.subscription_tenants(),
        "the tenants with subscriptions",
    ) {
        let Ok(tenant) = TenantId::new(&tenant_str) else {
            continue;
        };
        // Same source the matcher reads: the indexed mirror, with the store
        // list only as the never-wired fallback.
        let subs = match (&st.sub_mirror, sweep_subs) {
            (_, false) => Vec::new(),
            (Some(m), _) => m.periodic_docs(tenant.as_str()),
            (None, _) => read_or_warn(
                st.store.list(&tenant, Kind::Subscription),
                "the periodic Subscriptions",
            ),
        };
        for sub in subs {
            let Some(interval) = sub.get("timeInterval").and_then(Value::as_f64) else {
                continue;
            };
            if !is_active(&sub) {
                continue;
            }
            let due_at = due_at_ms(&sub, interval);
            if now_ms < due_at {
                next_sub = next_sub.min(due_at);
                continue;
            }
            // Due: the following firing is one interval away. Recorded before
            // the claim, so a pod that LOSES the race (another one is firing
            // this subscription right now) keeps sweeping on the interval
            // instead of parking on an anchor only the winner advanced.
            next_sub = next_sub.min(now_ms.saturating_add(interval_offset_ms(interval)));
            let claim = if st.nats {
                match claim_interval(st, &tenant, Kind::Subscription, &sub, interval) {
                    Some(prev) => Some(prev),
                    None => continue,
                }
            } else {
                None
            };
            let ctx = sub_context(st, &tenant, &sub).await;
            let now = now_iso();
            // 5.8.6: the periodic Notification "shall include all the
            // subscribed Entities that match the query, geoquery and Scope
            // query conditions" — so the read is exactly this subscription's
            // own selector (5.2.33) and conditions, never the tenant's entity
            // set. Only predicates a store reproduces without hiding a
            // candidate are offered (ids when every selector entry names one,
            // types when the index proves them plain, q/scopeQ/geoQ under the
            // store's own rule that SQL removes rows and never decides them);
            // the selector_match/conditions_match pair below stays the
            // arbiter, exactly as on the query path.
            let type_groups: Vec<Vec<String>> = match index_keys(&sub) {
                Keys::Types(ts) => ts.into_iter().map(|t| vec![t]).collect(),
                _ => Vec::new(),
            };
            let ids = selector_ids(&sub);
            // The filter borrows a term expander that is not Sync, so it lives
            // and dies inside this block: held across the delivery await it
            // would make the whole interval task non-Send.
            let rows = {
                let expand = |t: &str| ctx.expand_key(t);
                let id_refs: Vec<&str> = ids.iter().flatten().map(String::as_str).collect();
                // q values in subscription bodies may be percent-encoded (4.9);
                // parses shared per distinct expression text, as in
                // conditions_match — the sweep re-runs per due subscription
                let q_ast = sub_str(&sub, "q").and_then(|q| {
                    crate::regexcache::q_node(&crate::negotiate::percent_decode(q.as_bytes()))
                });
                let geo = sub.get("geoQ").and_then(Value::as_object).and_then(|g| {
                    let key = serde_json::to_string(g).unwrap_or_default();
                    crate::regexcache::geo_query(&key, || {
                        crate::geo::GeoQuery::from_params(&geo_params(g))
                            .ok()
                            .flatten()
                    })
                });
                let geo_spec = geo.as_ref().and_then(|g| g.to_sql_spec(&ctx));
                let filter = antares_store::filter::EntityFilter {
                    ids: ids.as_ref().map(|_| id_refs.as_slice()),
                    types: (!type_groups.is_empty()).then_some(type_groups.as_slice()),
                    q: q_ast.as_deref(),
                    scope_q: sub_str(&sub, "scopeQ"),
                    geo: geo_spec.as_ref(),
                    expand: &expand,
                    ..Default::default()
                };
                read_or_warn(
                    st.store.query_entities(&tenant, &filter).map(|o| o.rows),
                    "the Entities a periodic Subscription notifies about",
                )
            };
            let matching: Vec<Value> = rows
                .into_iter()
                .filter(|d| {
                    selector_match(&sub, d, &ctx)
                        && conditions_match(&sub, d, &ctx, &store_lookup(st, &tenant))
                })
                .flat_map(|d| build_data(st, &tenant, &sub, &ctx, None, Some(&d), &[], false, &now))
                .collect();
            if matching.is_empty() {
                // 5.8.6: "If there are no matching Entities, no Notification
                // is sent" — lastNotification stays untouched, so this
                // subscription is still due and every following tick
                // re-checks it. A claim taken to win the firing is given back
                // here, or the multi-pod path would stamp an instant nothing
                // was sent at and park the subscription for a whole interval.
                if let (Some(prev), Some(id)) = (claim, sub.get("id").and_then(Value::as_str)) {
                    release_interval(st, &tenant, Kind::Subscription, id, prev);
                }
                next_sub = next_sub.min(due_at);
                continue;
            }
            #[cfg(not(target_arch = "wasm32"))]
            {
                let (st, tenant, ctx) = (st.clone(), tenant.clone(), Arc::clone(&ctx));
                sending.spawn(async move {
                    let _permit = DELIVERY_SLOTS.acquire().await;
                    deliver(&st, &tenant, &sub, matching, &ctx).await;
                });
            }
            #[cfg(target_arch = "wasm32")]
            deliver(st, &tenant, &sub, matching, &ctx).await;
        }
        if !sweep_csubs {
            continue;
        }
        // csource timeInterval subs: periodic CSourceNotification with all
        // matching registrations, independent of changes (5.11.7)
        for sub in read_or_warn(
            st.store.list(&tenant, Kind::CSourceSubscription),
            "the periodic Context Source Registration Subscriptions",
        ) {
            let Some(interval) = sub.get("timeInterval").and_then(Value::as_f64) else {
                continue;
            };
            if !is_active(&sub) {
                continue;
            }
            let due_at = due_at_ms(&sub, interval);
            if now_ms < due_at {
                next_csub = next_csub.min(due_at);
                continue;
            }
            next_csub = next_csub.min(now_ms.saturating_add(interval_offset_ms(interval)));
            // 5.11.7 sends the periodic CSourceNotification whatever the
            // matching set is, so this claim is never given back.
            if st.nats
                && claim_interval(st, &tenant, Kind::CSourceSubscription, &sub, interval).is_none()
            {
                continue;
            }
            let ctx = sub_context(st, &tenant, &sub).await;
            let spec = crate::csource::spec_for_subscription(&sub);
            let data: Vec<Value> = read_or_warn(
                st.store.list(&tenant, Kind::Registration),
                "the registrations a periodic Context Source Notification carries",
            )
            .into_iter()
            .filter(|r| crate::csource::csr_matches_subscription(&sub, r, &ctx))
            .map(|r| {
                let mut p =
                    crate::csource::present_registration(&filter_csr(&spec, &r, &ctx), &ctx, false);
                arrayify_entity_types(&mut p);
                p
            })
            .collect();
            #[cfg(not(target_arch = "wasm32"))]
            {
                let (st, tenant, ctx) = (st.clone(), tenant.clone(), Arc::clone(&ctx));
                sending.spawn(async move {
                    let _permit = DELIVERY_SLOTS.acquire().await;
                    deliver_csource(&st, &tenant, &sub, data, &ctx, "newlyMatching").await;
                });
            }
            #[cfg(target_arch = "wasm32")]
            deliver_csource(st, &tenant, &sub, data, &ctx, "newlyMatching").await;
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    while let Some(joined) = sending.join_next().await {
        if joined.is_err() {
            note_panic();
        }
    }
    if let (Some(m), Some((sub_clock, csub_clock))) = (&st.sub_mirror, clocks) {
        // A periodic subscription written DURING the sweep has already zeroed
        // the clock: the exchange then fails, the zero stands and the next
        // tick sweeps rather than waiting out an interval computed without it.
        if sweep_subs {
            let _ = m
                .next_sub_sweep_ms
                .compare_exchange(sub_clock, next_sub, Relaxed, Relaxed);
        }
        if sweep_csubs {
            let _ = m.next_csub_sweep_ms.compare_exchange(
                csub_clock,
                next_csub.min(now_ms + CSUB_SWEEP_BACKSTOP_MS),
                Relaxed,
                Relaxed,
            );
        }
    }
}

/// POST the Notification (5.3.1) and write the 5.2.14.2 bookkeeping back.
pub(crate) async fn deliver(
    st: &AppState,
    tenant: &TenantId,
    sub: &Value,
    data: Vec<Value>,
    ctx: &Context,
) {
    // a notification body is bounded the way an inbound body is (6.3.4 wall,
    // MAX_BODY_BYTES): a grouped delivery over the cap leaves as several
    // notifications, each whole entities, never one unbounded POST
    for chunk in chunk_by_bytes(data, *crate::bounds::MAX_BODY_BYTES) {
        deliver_as(
            st,
            tenant,
            Kind::Subscription,
            sub,
            "Notification",
            chunk,
            ctx,
            None,
        )
        .await;
    }
}

/// Split `data` into runs whose serialized sizes stay under `cap`, cutting
/// only at whole items; one item alone over the cap still travels alone.
fn chunk_by_bytes(data: Vec<Value>, cap: usize) -> Vec<Vec<Value>> {
    let mut out: Vec<Vec<Value>> = Vec::new();
    let mut size = 0usize;
    for item in data {
        let n = serde_json::to_vec(&item).map(|b| b.len()).unwrap_or(0);
        match out.last_mut() {
            Some(run) if size + n <= cap => {
                run.push(item);
                size += n;
            }
            _ => {
                out.push(vec![item]);
                size = n;
            }
        }
    }
    out
}

/// 5.11.7: which csource subs care about a registration change, and why.
fn csource_trigger(
    sub: &Value,
    before: Option<&Value>,
    after: Option<&Value>,
    ctx: &Context,
) -> Option<&'static str> {
    let m = |d: Option<&Value>| {
        d.is_some_and(|d| crate::csource::csr_matches_subscription(sub, d, ctx))
    };
    match (m(before), m(after)) {
        (false, true) => Some("newlyMatching"),
        (true, true) => Some("updated"),
        (true, false) => Some("noLongerMatching"),
        (false, false) => None,
    }
}

/// The notification validator reads EntityInfo.type as an array
/// (`entities[0]["type"][0]`) — normalize to array form in notification data.
fn arrayify_entity_types(reg: &mut Value) {
    let Some(infos) = reg.get_mut("information").and_then(Value::as_array_mut) else {
        return;
    };
    for info in infos {
        let Some(es) = info.get_mut("entities").and_then(Value::as_array_mut) else {
            continue;
        };
        for e in es {
            if let Some(t) = e.get("type").filter(|t| t.is_string()).cloned() {
                if let Some(o) = e.as_object_mut() {
                    o.insert("type".into(), Value::Array(vec![t]));
                }
            }
        }
    }
}

/// One prepared CSource notification, ready to send.
pub struct CsourceJob {
    sub: Value,
    presented: Value,
    ctx: std::sync::Arc<antares_jsonld::Context>,
    reason: &'static str,
}

/// Registration create/update/delete → CSourceNotification fan-out (5.11.7),
/// in two phases: `prepare_csource_jobs` runs IN the request path (store
/// reads + matching + payload build — so job order is the handlers' commit
/// order even on a slower store), and the caller spawns `send_csource_jobs`
/// (network only — the ack must not block on the receiver: the ETSI mock
/// replies only when the robot side wakes).
pub async fn prepare_csource_jobs(
    st: &AppState,
    tenant: &TenantId,
    before: Option<Value>,
    after: Option<Value>,
) -> Vec<CsourceJob> {
    let mut jobs = Vec::new();
    // 5.8.1.4: the Registration Subscriptions the distributed half owns are
    // not client resources, so they live under Kind::DistSub beside the
    // mapping documents — which carry no `type`. A document under the
    // client kind in the internal id namespace is a leftover of a release
    // that stored the internal ones there, and drives nothing.
    let client = read_or_warn(
        st.store.list(tenant, Kind::CSourceSubscription),
        "the Context Source Registration Subscriptions of a changed registration",
    )
    .into_iter()
    .filter(|d| {
        sub_str(d, "id").is_some_and(|id| crate::distsub::csr_kind(id) == Kind::CSourceSubscription)
    });
    let internal = read_or_warn(
        st.store.list(tenant, Kind::DistSub),
        "the internal Registration Subscriptions of a changed registration",
    )
    .into_iter()
    .filter(|d| d.get("type").and_then(Value::as_str) == Some("Subscription"));
    for sub in client.chain(internal) {
        if !is_active(&sub) || sub.get("timeInterval").is_some() {
            continue;
        }
        let ctx = sub_context(st, tenant, &sub).await;
        let Some(reason) = csource_trigger(&sub, before.as_ref(), after.as_ref(), &ctx) else {
            continue;
        };
        let spec = crate::csource::spec_for_subscription(&sub);
        let source = if reason == "noLongerMatching" {
            &before
        } else {
            &after
        };
        let Some(reg) = source.as_ref().or(before.as_ref()) else {
            continue;
        };
        let filtered = filter_csr(&spec, reg, &ctx);
        let mut presented = crate::csource::present_registration(&filtered, &ctx, false);
        arrayify_entity_types(&mut presented);
        jobs.push(CsourceJob {
            sub,
            presented,
            ctx,
            reason,
        });
    }
    jobs
}

pub async fn send_csource_jobs(st: &AppState, tenant: &TenantId, jobs: Vec<CsourceJob>) {
    for job in jobs {
        // 5.11.7: re-check the subscription still exists right
        // before the send — a deleted subscription must never notify.
        let sub_id = job
            .sub
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let kind = crate::distsub::csr_kind(sub_id);
        if !matches!(st.store.get(tenant, kind, sub_id), Ok(Some(_))) {
            continue;
        }
        deliver_as(
            st,
            tenant,
            kind,
            &job.sub,
            "ContextSourceNotification",
            vec![job.presented],
            &job.ctx,
            Some(job.reason),
        )
        .await;
    }
}

/// Registration writes prepare one job per CSource subscription of the
/// tenant (5.11.7) and every subscription with localOnly != true owns one,
/// so a registration stream against many subscriptions queues
/// subscriptions × registrations jobs faster than the sources drain them —
/// at 10 000 × 100 the queued jobs were a 3 GB broker peak. The permit is
/// taken in the request path: a write waits for a fan-out slot instead of
/// stacking jobs, and the prepare order (the handlers' commit order) holds.
const CSOURCE_FANOUT_WIDTH: usize = 64;
static CSOURCE_FANOUT: tokio::sync::Semaphore =
    tokio::sync::Semaphore::const_new(CSOURCE_FANOUT_WIDTH);

/// Registration create/update/delete → prepare in the request path, send
/// spawned (the ack must not block on the receiver), bounded as above.
pub async fn csource_fanout(
    st: &AppState,
    tenant: &TenantId,
    before: Option<Value>,
    after: Option<Value>,
) {
    // never closed, so acquire only fails if it were — then send unbounded
    let permit = CSOURCE_FANOUT.acquire().await.ok();
    let jobs = prepare_csource_jobs(st, tenant, before, after).await;
    let (st2, t2) = (st.clone(), tenant.clone());
    crate::spawn(async move {
        send_csource_jobs(&st2, &t2, jobs).await;
        drop(permit);
    });
}

/// POST a CSourceNotification (5.3.2) under the same body bound as every
/// other one: 5.11.2.4 sends "all matching Context Source Registrations",
/// and a broker holding 100 000 of them must not turn that into one
/// unbounded request. Over the cap the set leaves as several notifications,
/// each carrying whole registrations — the trade [`deliver`] already makes
/// for entity Notifications.
async fn deliver_csource(
    st: &AppState,
    tenant: &TenantId,
    sub: &Value,
    data: Vec<Value>,
    ctx: &Context,
    reason: &str,
) {
    let kind = crate::distsub::csr_kind(sub_str(sub, "id").unwrap_or_default());
    for chunk in chunk_by_bytes(data, *crate::bounds::MAX_BODY_BYTES) {
        deliver_as(
            st,
            tenant,
            kind,
            sub,
            "ContextSourceNotification",
            chunk,
            ctx,
            Some(reason),
        )
        .await;
    }
}

/// Initial / post-update CSourceNotification with all currently matching
/// registrations (5.11.2.4 / 5.11.3.4).
pub async fn csource_initial(st: &AppState, tenant: &TenantId, sub_id: &str) {
    let Some(sub) = st
        .store
        .get(tenant, crate::distsub::csr_kind(sub_id), sub_id)
        .ok()
        .flatten()
    else {
        return;
    };
    if !is_active(&sub) {
        return;
    }
    let ctx = sub_context(st, tenant, &sub).await;
    let spec = crate::csource::spec_for_subscription(&sub);
    let data: Vec<Value> = read_or_warn(
        st.store.list(tenant, Kind::Registration),
        "the registrations an initial Context Source Notification carries",
    )
    .into_iter()
    .filter(|r| crate::csource::csr_matches_subscription(&sub, r, &ctx))
    .map(|r| {
        let mut p = crate::csource::present_registration(&filter_csr(&spec, &r, &ctx), &ctx, false);
        arrayify_entity_types(&mut p);
        p
    })
    .collect();
    if data.is_empty() {
        return; // nothing currently matching ⇒ no initial notification
    }
    deliver_csource(st, tenant, &sub, data, &ctx, "newlyMatching").await;
}

/// Registration copy reduced to the matching RegistrationInfo elements
/// (5.10.2.5 / 5.11.7 "filtered Context Source Registrations").
fn filter_csr(spec: &crate::csource::CsrSpec, reg: &Value, ctx: &Context) -> Value {
    let mut out = reg.clone();
    let matching: Vec<Value> = crate::csource::matching_infos(spec, reg, ctx)
        .into_iter()
        .cloned()
        .collect();
    if !matching.is_empty() {
        if let Some(o) = out.as_object_mut() {
            o.insert("information".into(), Value::Array(matching));
        }
    }
    out
}

/// Table 5.2.15-1 `timeout`: per-endpoint delivery deadline in milliseconds.
/// "The NGSI-LD system can override this value" — clamped to [100 ms, 30 s]
/// so one subscription cannot park a delivery task for minutes. Default 5 s
/// (the previous hard-coded deadline). HTTP only: the clause scopes it to
/// bindings that "always return a response".
fn endpoint_timeout_ms(ep: &serde_json::Map<String, Value>) -> u32 {
    ep.get("timeout")
        .and_then(Value::as_f64)
        .filter(|t| *t > 0.0)
        .map(|t| (t as u32).clamp(100, 30_000))
        .unwrap_or(5_000)
}

/// Table 5.2.15-1 `cooldown`: "Once a failure has occurred, minimum period of
/// time in milliseconds which shall elapse before attempting to make a
/// subsequent notification to the same endpoint after failure. If requests
/// are received before the cooldown period has expired, no notification is
/// sent." — i.e. matches inside the window are DROPPED, not queued.
fn in_cooldown(sub: &Value, now: chrono::DateTime<chrono::Utc>) -> bool {
    let n = sub.get("notification");
    let Some(cd) = n
        .and_then(|n| n.get("endpoint"))
        .and_then(|e| e.get("cooldown"))
        .and_then(Value::as_f64)
        .filter(|c| *c > 0.0)
    else {
        return false;
    };
    // the gate exists only "once a failure has occurred" and only until a
    // success clears it — notification.status tracks exactly that (5.2.14.2)
    if n.and_then(|n| n.get("status")).and_then(Value::as_str) != Some("failed") {
        return false;
    }
    let Some(lf) = n.and_then(|n| n.get("lastFailure")).and_then(Value::as_str) else {
        return false;
    };
    let Ok(t) = chrono::DateTime::parse_from_rfc3339(lf) else {
        return false;
    };
    let elapsed = now
        .signed_duration_since(t.with_timezone(&chrono::Utc))
        .num_milliseconds();
    (elapsed as f64) < cd
}

#[allow(clippy::too_many_arguments)] // one param per notification dimension
async fn deliver_as(
    st: &AppState,
    tenant: &TenantId,
    kind: Kind,
    sub: &Value,
    ntype: &str,
    data: Vec<Value>,
    ctx: &Context,
    trigger_reason: Option<&str>,
) {
    // 5.2.12: a paused subscription (isActive false) and an expired one send
    // nothing. Every caller that assembles data locally checks this first;
    // the ones that arrive with data already assembled — the 5.8.6 inbound
    // notification among them — did not, so the check belongs here too.
    if !is_active(sub) {
        return;
    }
    let sub_id = sub_str(sub, "id").unwrap_or_default().to_owned();
    let Some(ep) = sub
        .get("notification")
        .and_then(|n| n.get("endpoint"))
        .and_then(Value::as_object)
    else {
        return;
    };
    let Some(uri) = ep.get("uri").and_then(Value::as_str) else {
        return;
    };
    // 5.8.1.4 consumer half: the internal CSR subscription's notifications
    // are handled in-process (urn:antares:distsub:{tenant}\n{own sub id})
    if let Some(own) = uri.strip_prefix("urn:antares:distsub:") {
        if let Some((_, own_id)) = own.split_once('\n') {
            crate::distsub::on_csource_notification(st, tenant, own_id, trigger_reason, &data)
                .await;
        }
        return;
    }
    // 6.3.8: the binding comes from the registry and nowhere else. Creation
    // rejects an endpoint whose scheme no sink serves, so a stored row that
    // still names one was hand-edited — it is dropped, never delivered
    // through some other binding.
    if st.sinks.sink_for_uri(uri).is_none() {
        tracing::warn!(
            "subscription {sub_id} endpoint {} has no registered binding",
            redact_userinfo(uri)
        );
        return;
    }
    // endpoint.cooldown — drop (never queue) while the window is open.
    // Before any bookkeeping: a suppressed notification was never sent, so
    // timesSent/lastNotification must not move.
    if in_cooldown(sub, chrono::Utc::now()) {
        tracing::debug!("subscription {sub_id} in cooldown; notification suppressed (5.2.15)");
        return;
    }
    // An open circuit is the same class of self-inflicted suppression: no
    // request leaves the process, so Table 5.2.14.2-1 timesSent ("number of
    // times that the notification has been sent") and lastNotification ("the
    // instant when the last notification has been sent") must not move
    // either. `is_open` returning false IS the half-open probe, so the
    // check stays exactly once per attempt.
    // A binding that opens no socket has no destination for the policy or
    // the breaker to judge; every network binding is policed below.
    let policed = st.sinks.sink_for_uri(uri).is_some_and(|s| s.network());
    if policed && st.egress.is_open(tenant.as_str(), uri) {
        tracing::debug!(
            "notification to {} short-circuited (breaker open)",
            redact_userinfo(uri)
        );
        return;
    }
    let accept = ep
        .get("accept")
        .and_then(Value::as_str)
        .unwrap_or("application/json");
    let now = now_iso();
    let mut body = json!({
        "id": format!("urn:ngsi-ld:{ntype}:{}", uuid::Uuid::new_v4()),
        "type": ntype,
        "subscriptionId": sub_id,
        "notifiedAt": now,
        "data": data,
    });
    if let Some(r) = trigger_reason {
        body["triggerReason"] = Value::String(r.into());
    }
    // 5.8.6: a subscription's ngsildConformance pins the notification format —
    // amend the data entities per the 4.3.6.8 fallbacks.
    if let Some(ver) = sub_str(sub, "ngsildConformance").and_then(crate::conformance::parse_version)
    {
        if let Some(d) = body.get_mut("data") {
            crate::conformance::amend_payload(d, ver);
        }
    }
    if accept == "application/ld+json" {
        // JSON-LD notifications carry the @context inside each data entity
        // (046_14: data[0] must contain @context; no Link header) — same rule
        // over MQTT: with ld+json the @context travels in the body (7.2).
        if let Some(arr) = body.get_mut("data").and_then(Value::as_array_mut) {
            for e in arr.iter_mut() {
                *e = inject_context(e.clone(), ctx);
            }
        }
    }
    if accept == "application/geo+json" {
        // Table 5.3.1-1: with endpoint.accept application/geo+json, data is
        // a FeatureCollection (5.2.30); if receiverInfo carries
        // Prefer=body=json the FeatureCollection carries no @context.
        let prefer_body_json = ep
            .get("receiverInfo")
            .and_then(Value::as_array)
            .is_some_and(|ri| {
                ri.iter().any(|kv| {
                    kv.get("key").and_then(Value::as_str) == Some("Prefer")
                        && kv.get("value").and_then(Value::as_str) == Some("body=json")
                })
            });
        let entities = body["data"].as_array().cloned().unwrap_or_default();
        let mut fc = crate::entities::to_geojson_collection(entities, None);
        if !prefer_body_json {
            fc["@context"] = crate::negotiate::served_context(ctx);
        }
        body["data"] = fc;
    }
    let receiver_info = kv_pairs(ep.get("receiverInfo"));

    // 6.3.22: a subscription living under a snapshot's synthetic tenant
    // notifies with the NGSILD-Snapshot header and the OWNER tenant — the
    // internal "snap-…" tenant never leaks.
    let (hdr_tenant, snapshot_id) = match crate::snapshots::snapshot_of_synth(st, tenant.as_str()) {
        Some((owner, sid)) => (owner, Some(sid)),
        None => (tenant.clone(), None),
    };

    // Prepared BEFORE the bookkeeping writeback so the optimistic stamp
    // covers only the in-flight attempt (046_12_01 race). The parts are
    // transport-neutral: the sink registered for the endpoint's scheme turns
    // them into HTTP headers (6.3.8) or an MQTT metadata object (Table
    // 7.2-2).
    let mut info = receiver_info;
    strip_reserved_markers(&mut info);
    if hdr_tenant.as_str() != "default" {
        info.push(("NGSILD-Tenant".into(), hdr_tenant.as_str().to_owned()));
    }
    if let Some(sid) = &snapshot_id {
        info.push(("NGSILD-Snapshot".into(), sid.clone()));
    }
    let outbound = Outbound {
        body,
        accept: accept.to_owned(),
        link: link_header_value(ctx),
        receiver_info: info,
        notifier_info: kv_pairs(ep.get("notifierInfo")),
    };
    // Bookkeeping BEFORE the send (5.8.6/5.2.14.2: lastNotification is the
    // instant the notification is sent). The ETSI mock unblocks the test the
    // moment the request ARRIVES, so a post-response-only writeback races the
    // test's immediate Retrieve Subscription (CI flake on 046_12_01).
    // Optimistic ok; a failed attempt is corrected right below — the transient
    // window is the in-flight attempt itself, and the failure TPs wait for the
    // attempt to resolve before asserting.
    // One store call: the stamp is a fixed mutation, so a backend can write
    // it as a single statement instead of locking the row across a round
    // trip. At fan-out that lock is what serializes delivery.
    let booked = st
        .store
        .record_delivery(tenant, kind, &sub_id, &now)
        .unwrap_or_else(|e| {
            tracing::warn!("bookkeeping writeback failed: {e}");
            None
        });
    // 5.8.6: notifications are sent for the subscriptions the broker holds.
    // No row to book against means the subscription was deleted (or the
    // store failed) between matching and delivery — nothing may be sent.
    let Some(booked) = booked else {
        return;
    };
    let mut prev_success = booked.prev_success;
    mirror_bookkeeping(st, tenant, kind, &sub_id, Some(booked.doc));
    // The notification endpoint is an egress destination like any other
    // — policy check once, breaker consulted before the attempt.
    // A refusal is a delivery failure for bookkeeping (status "failed",
    // lastSuccess rolled back below) but never breaker state: the policy
    // verdict says nothing about the endpoint's health.
    let refused = policed
        && match st.egress.check_destination(uri).await {
            Ok(()) => false,
            Err(e) => {
                tracing::warn!(
                    "notification endpoint {} refused by egress policy: {e}",
                    redact_userinfo(uri)
                );
                true
            }
        };
    // (delivered, timed_out): only a TIMEOUT-class failure feeds the breaker
    // — that protects against peers that eat the deadline. An endpoint
    // that ANSWERS (any status) is alive, costs only its own response time,
    // and 6.3.8 says the notification shall be sent — suppressing sends to a
    // responding host:port starves unrelated subscriptions sharing it.
    let timeout_ms = endpoint_timeout_ms(ep);
    let first = if refused {
        Err((false, "refused by egress policy".to_owned()))
    } else {
        send_outbound(st, uri, timeout_ms, &outbound).await
    };
    let (ok, timed_out) = match &first {
        Ok(()) => (true, false),
        Err((t, _)) => (false, *t),
    };
    if policed && !refused {
        if ok {
            st.egress.record_success(tenant.as_str(), uri);
        } else if timed_out {
            st.egress.record_failure(tenant.as_str(), uri);
        } else {
            // the destination responded (or refused fast): alive — clear
            // any stale consecutive-timeout state
            st.egress.record_success(tenant.as_str(), uri);
        }
    }
    // Delivery counters by binding (facade — no-op without the broker's
    // recorder). The label is the sink's first scheme, so the two members of
    // a family share one series.
    let scheme = st
        .sinks
        .sink_for_uri(uri)
        .and_then(|s| s.schemes().first().copied())
        .unwrap_or("unknown");
    if ok {
        metrics::counter!("antares_notifications_sent_total", "scheme" => scheme).increment(1);
    } else {
        metrics::counter!("antares_notifications_failed_total", "scheme" => scheme).increment(1);
    }
    if !ok {
        // 5.8.6 / 5.11.7: subscription status → "failed" on delivery failure;
        // roll back the optimistic lastSuccess stamp.
        let ts = now_iso();
        let mut failed_doc: Option<Value> = None;
        st.store
            .mutate(tenant, kind, &sub_id, |doc| {
                if let Some(o) = doc.as_object_mut() {
                    o.insert("status".into(), Value::String("failed".into()));
                }
                if let Some(n) = doc
                    .as_object_mut()
                    .and_then(|o| o.get_mut("notification"))
                    .and_then(Value::as_object_mut)
                {
                    match prev_success.take() {
                        Some(v) => n.insert("lastSuccess".into(), v),
                        None => n.remove("lastSuccess"),
                    };
                    n.insert("lastFailure".into(), Value::String(ts.clone()));
                    // Table 5.2.14.2-1 timesFailed: "Number of times an
                    // unsuccessful response (or timeout) has been received
                    // when delivering the notification" — an output-only
                    // member implementations shall generate.
                    let failed = n.get("timesFailed").and_then(Value::as_i64).unwrap_or(0);
                    n.insert("timesFailed".into(), json!(failed + 1));
                    n.insert("status".into(), Value::String("failed".into()));
                }
                failed_doc = Some(doc.clone());
                Ok::<(), antares_model::NgsiError>(())
            })
            .unwrap_or_else(|e| {
                tracing::warn!("failure-status writeback failed: {e}");
                None
            });
        mirror_bookkeeping(st, tenant, kind, &sub_id, failed_doc);
        // Retries are transport, not new notifications: they run on their
        // own task (never on the request path, never delaying another
        // subscription's delivery) and book only the final outcome — a
        // success sets lastSuccess/status ok without touching timesSent;
        // an exhausted policy leaves a dead letter.
        #[cfg(not(target_arch = "wasm32"))]
        if !refused && st.delivery.attempts > 1 {
            let first_err = first.err().map(|(_, e)| e).unwrap_or_default();
            let (st, tenant, uri) = (st.clone(), tenant.clone(), uri.to_owned());
            crate::spawn(async move {
                retry_and_settle(
                    &st, &tenant, kind, &sub_id, &uri, timeout_ms, outbound, first_err,
                )
                .await;
            });
        }
    }
}

/// A `KeyValuePair[]` member of `endpoint` (Table 5.2.15-1) as owned pairs.
/// A member that is not an array of well-formed pairs contributes nothing:
/// 5.2.12 validation at creation already refused a malformed one.
fn kv_pairs(v: Option<&Value>) -> Vec<(String, String)> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|kv| {
                    Some((
                        kv.get("key")?.as_str()?.to_owned(),
                        kv.get("value")?.as_str()?.to_owned(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 6.3.22 / 6.3.8: `NGSILD-Tenant` and `NGSILD-Snapshot` on a notification
/// are the broker's own statement of where the data came from, appended to
/// the `receiverInfo` pairs. The HTTP binding appends every pair it is
/// handed, so a subscriber naming one of the two in `receiverInfo` would
/// put a second value of it on the wire beside the broker's, and a receiver
/// reading "the" tenant of a notification could not tell which one the
/// broker meant. Ordinary custom headers are untouched.
fn strip_reserved_markers(info: &mut Vec<(String, String)>) {
    info.retain(|(k, _)| {
        !k.eq_ignore_ascii_case("NGSILD-Tenant") && !k.eq_ignore_ascii_case("NGSILD-Snapshot")
    });
}

/// One attempt on the wire, through the binding the registry holds for the
/// endpoint's scheme (6.3.8). `Err((timed_out, why))`: only a timeout-class
/// failure feeds the breaker — an endpoint that answers is alive.
async fn send_outbound(
    st: &AppState,
    uri: &str,
    timeout_ms: u32,
    outbound: &Outbound,
) -> Result<(), (bool, String)> {
    let Some(sink) = st.sinks.sink_for_uri(uri) else {
        return Err((
            false,
            format!(
                "no notification binding registered for {}",
                redact_userinfo(uri)
            ),
        ));
    };
    sink.deliver(
        uri,
        outbound,
        std::time::Duration::from_millis(u64::from(timeout_ms)),
    )
    .await
    .map_err(|e| (e.timed_out, e.message))
}

static DEAD_LETTERS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Dead letters written by this process since start (`/q/health`
/// deadLetters); the letters themselves live in the store.
pub fn dead_letters_written() -> u64 {
    DEAD_LETTERS.load(std::sync::atomic::Ordering::Relaxed)
}

/// The retries the delivery policy grants after a failed first attempt,
/// then the settlement: lastSuccess/status ok on success, a dead letter
/// when the policy is exhausted.
#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::too_many_arguments)] // one param per piece of the attempt's state
async fn retry_and_settle(
    st: &AppState,
    tenant: &TenantId,
    kind: Kind,
    sub_id: &str,
    uri: &str,
    timeout_ms: u32,
    outbound: Outbound,
    first_err: String,
) {
    let policy = st.delivery;
    let started = std::time::Instant::now();
    let first_at = now_iso();
    let mut made = 1u32;
    let mut last_err = first_err.clone();
    while let Some(delay) = policy.next_delay(made, started.elapsed()) {
        tokio::time::sleep(delay).await;
        // the subscription may have gone, or its endpoint may have tripped
        // the breaker meanwhile — a retry is still one more attempt
        if st.store.get(tenant, kind, sub_id).ok().flatten().is_none() {
            return;
        }
        made += 1;
        match send_outbound(st, uri, timeout_ms, &outbound).await {
            Ok(()) => {
                st.egress.record_success(tenant.as_str(), uri);
                metrics::counter!("antares_notifications_retried_total", "outcome" => "ok")
                    .increment(1);
                let ts = now_iso();
                let mut retried_doc: Option<Value> = None;
                st.store
                    .mutate(tenant, kind, sub_id, |doc| {
                        if let Some(o) = doc.as_object_mut() {
                            o.remove("status");
                        }
                        if let Some(n) = doc
                            .as_object_mut()
                            .and_then(|o| o.get_mut("notification"))
                            .and_then(Value::as_object_mut)
                        {
                            n.insert("lastSuccess".into(), Value::String(ts.clone()));
                            n.insert("status".into(), Value::String("ok".into()));
                        }
                        retried_doc = Some(doc.clone());
                        Ok::<(), antares_model::NgsiError>(())
                    })
                    .unwrap_or_else(|e| {
                        tracing::warn!("retry bookkeeping writeback failed: {e}");
                        None
                    });
                mirror_bookkeeping(st, tenant, kind, sub_id, retried_doc);
                return;
            }
            Err((timed_out, e)) => {
                if timed_out {
                    st.egress.record_failure(tenant.as_str(), uri);
                } else {
                    st.egress.record_success(tenant.as_str(), uri);
                }
                last_err = e;
            }
        }
    }
    metrics::counter!("antares_notifications_retried_total", "outcome" => "dead").increment(1);
    let letter = dead_letter(
        sub_id, uri, timeout_ms, &outbound, made, &first_err, &last_err, &first_at,
    );
    let id = letter["id"].as_str().unwrap_or_default().to_owned();
    match st.store.create(tenant, Kind::DeadLetter, &id, letter) {
        Ok(_) => {
            DEAD_LETTERS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            tracing::warn!(
                "notification for {sub_id} to {} dead-lettered after {made} attempts: {last_err}",
                redact_userinfo(uri)
            );
        }
        Err(e) => tracing::error!("dead letter for {sub_id} could not be stored: {e}"),
    }
}

/// The dead-letter document: everything a replay needs to send the very
/// same request again, plus the attempt history.
#[allow(clippy::too_many_arguments)] // one param per stored letter member
fn dead_letter(
    sub_id: &str,
    uri: &str,
    timeout_ms: u32,
    outbound: &Outbound,
    attempts: u32,
    first_err: &str,
    last_err: &str,
    first_at: &str,
) -> Value {
    let mut doc = json!({
        "id": format!("urn:ngsi-ld:DeadLetter:{}", uuid::Uuid::new_v4()),
        "type": "DeadLetter",
        "subscriptionId": sub_id,
        "uri": uri,
        "timeoutMs": timeout_ms,
        "attempts": attempts,
        "firstError": first_err,
        "lastError": last_err,
        "firstAt": first_at,
        "lastAt": now_iso(),
    });
    doc["binding"] = json!(antares_notifier::SinkRegistry::scheme_of(uri));
    doc["payload"] = outbound.body.clone();
    doc["accept"] = json!(outbound.accept);
    doc["link"] = json!(outbound.link);
    doc["receiverInfo"] = json!(outbound.receiver_info);
    doc["notifierInfo"] = json!(outbound.notifier_info);
    doc
}

/// Replay one dead letter through the same binding, once. `Ok` = delivered
/// (the caller deletes the letter); `Err` carries the failure text.
pub(crate) async fn replay_dead_letter(st: &AppState, letter: &Value) -> Result<(), String> {
    let uri = letter["uri"].as_str().ok_or("dead letter without uri")?;
    let timeout_ms = letter["timeoutMs"].as_u64().unwrap_or(5_000) as u32;
    let outbound = Outbound::from_dead_letter(letter)?;
    // the egress policy of the moment applies, exactly as for a fresh send
    if st.sinks.sink_for_uri(uri).is_some_and(|s| s.network()) {
        st.egress
            .check_destination(uri)
            .await
            .map_err(|e| e.to_string())?;
    }
    send_outbound(st, uri, timeout_ms, &outbound)
        .await
        .map_err(|(_, e)| e)
}

pub(crate) use antares_notifier::{redact_userinfo, Outbound};

/// The matcher reads subscriptions from the
/// SubMirror, so every notification bookkeeping writeback must be applied
/// there too — otherwise the mirror copy never gains
/// `notification.lastNotification` and 5.2.12 `throttling` suppresses
/// nothing. In-process apply only: a KV write per notification would not
/// scale to the 100k-sub target, so in bus=nats multi-pod deployments the
/// throttling window is per-pod approximate.
/// Known ceiling: exact distributed throttling = per-notification KV sync or a
/// store read in `throttled()`; add if a deployment needs the strict window.
/// The mirror learns the counters from the document the writeback just
/// committed under the row lock; `None` (no row) leaves it untouched.
fn mirror_bookkeeping(
    st: &AppState,
    tenant: &TenantId,
    kind: Kind,
    sub_id: &str,
    doc: Option<Value>,
) {
    if kind != Kind::Subscription {
        return;
    }
    if let (Some(m), Some(doc)) = (&st.sub_mirror, doc) {
        m.apply(tenant.as_str(), sub_id, Some(doc));
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod deleted_subscription_delivery {
    use super::*;

    /// 5.8.6: notifications are sent for the subscriptions a Context Broker
    /// holds — a subscription deleted between matching and delivery no
    /// longer exists, so its endpoint must receive nothing.
    #[tokio::test(flavor = "multi_thread")]
    async fn deleted_subscription_receives_no_notification() {
        crate::allow_private();
        let st = AppState::new("antares-deleted-sub-test".into());
        let count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let c = count.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let app = axum::Router::new().route(
            "/notify",
            axum::routing::post(move || {
                let c = c.clone();
                async move {
                    c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    axum::http::StatusCode::OK
                }
            }),
        );
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });
        let tenant = TenantId::new("default").expect("tenant");
        let ctx = antares_jsonld::Loader::new().core();
        // the sub doc is a snapshot whose row is NOT in the store — the
        // deleted-concurrently case
        let sub = json!({
            "id": "urn:ngsi-ld:Subscription:ghost",
            "type": "Subscription",
            "entities": [{"type": "Vehicle"}],
            "notification": {"endpoint": {"uri": format!("http://{addr}/notify")}},
        });
        let data = vec![json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle"})];
        deliver_as(
            &st,
            &tenant,
            Kind::Subscription,
            &sub,
            "Notification",
            data,
            &ctx,
            None,
        )
        .await;
        assert_eq!(
            count.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "a deleted subscription's endpoint must receive NO notification"
        );
    }
}

#[cfg(test)]
mod interval_tests {
    use super::*;
    use serde_json::json;

    /// 5.2.12 Table 5.2.12-1 bounds `Subscription.timeInterval` only as
    /// "greater than 0", so a client may name an interval whose milliseconds
    /// do not fit in the epoch arithmetic. A firing instant the broker cannot
    /// represent must read as far in the future, never as far in the past: a
    /// wrapped anchor is a NEGATIVE instant, which reports the subscription
    /// as permanently due and fires it on every tick, each firing running the
    /// subscription's whole query.
    #[test]
    fn an_interval_too_large_to_schedule_is_never_due_rather_than_always() {
        let sub = json!({
            "id": "urn:ngsi-ld:Subscription:huge",
            "type": "Subscription",
            "createdAt": "2026-01-01T00:00:00Z",
        });
        let now = chrono::Utc::now().timestamp_millis();
        for interval in [1e18, 1e30, f64::MAX] {
            let due = due_at_ms(&sub, interval);
            assert!(
                due > now,
                "timeInterval {interval} is due at {due}, which is not after {now}"
            );
        }
        // and an interval that DOES fit still schedules exactly one interval
        // past the anchor, so the guard costs the ordinary case nothing
        let anchor = chrono::DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .expect("anchor")
            .timestamp_millis();
        assert_eq!(due_at_ms(&sub, 30.0), anchor + 30_000);
    }

    /// A subscriber cannot put its own value of a marker the broker sets on
    /// the wire: the pair is dropped before the broker appends its own, and
    /// the drop is by ASCII case-insensitive name, since a header name is
    /// case-insensitive (IETF RFC 9110 clause 5.1) and the pair would
    /// otherwise slip past under a different spelling.
    #[test]
    fn a_subscriber_cannot_add_its_own_notification_markers() {
        let mut info = vec![
            ("Authorization".to_owned(), "Bearer t".to_owned()),
            ("ngsild-tenant".to_owned(), "victim".to_owned()),
            ("NGSILD-Tenant".to_owned(), "victim".to_owned()),
            (
                "NGSILD-SNAPSHOT".to_owned(),
                "urn:ngsi-ld:Snapshot:x".to_owned(),
            ),
            ("X-NGSILD-Tenant".to_owned(), "kept".to_owned()),
        ];
        strip_reserved_markers(&mut info);
        assert_eq!(
            info,
            [
                ("Authorization".to_owned(), "Bearer t".to_owned()),
                ("X-NGSILD-Tenant".to_owned(), "kept".to_owned()),
            ]
        );
    }

    /// The sweep's own clock takes the same offset, so the same overflow
    /// would park (or un-park) every tenant's sweep, not just this
    /// subscription: `next_sub`/`next_csub` are process-wide minima.
    #[test]
    fn the_sweep_clock_offset_survives_an_unschedulable_interval() {
        let now_ms = chrono::Utc::now().timestamp_millis();
        for interval in [1e18, f64::MAX] {
            let next = now_ms.saturating_add(interval_offset_ms(interval));
            assert!(next > now_ms, "sweep clock went backwards for {interval}");
        }
        assert_eq!(interval_offset_ms(1.5), 1500);
    }
}

#[cfg(test)]
mod endpoint_tests {
    use super::*;
    use serde_json::json;

    fn ep(v: Value) -> serde_json::Map<String, Value> {
        v.as_object().expect("map").clone()
    }

    /// Clause 4.21 + Table 5.2.14.1-1: notification `pick`/`omit` are
    /// "a valid attribute projection language string as per clause 4.21", so a
    /// LinkedEntityTerm (`ProjectionTerm = AttrName *1(LinkedEntityTerm)`) must
    /// constrain the Linked Entity, exactly as it does on the query path.
    #[test]
    fn notification_projection_parses_linked_entity_terms() {
        let ctx = antares_jsonld::Loader::new().core();
        let sub = json!({
            "notification": { "pick": ["id", "type", "refDevice{type}"] }
        });
        let shape = notif_shape(&sub, &ctx);
        let pick = shape.repr.pick.expect("pick parsed");

        let linked = pick.iter().find(|n| n.raw == "refDevice").expect(
            "refDevice must survive as its own term, not as the literal \"refDevice{type}\"",
        );
        let children = linked
            .children
            .as_ref()
            .expect("the {…} term must become children so the Linked Entity is constrained");
        assert!(
            children.iter().any(|c| c.raw == "type"),
            "refDevice{{type}} must select `type` inside the Linked Entity"
        );
    }

    /// Table 5.2.15-1 `timeout`: honored, clamped, defaulted.
    #[test]
    fn endpoint_timeout_is_honored_clamped_and_defaulted() {
        assert_eq!(endpoint_timeout_ms(&ep(json!({"timeout": 1500}))), 1500);
        assert_eq!(endpoint_timeout_ms(&ep(json!({}))), 5_000, "default");
        // "The NGSI-LD system can override this value" — the clamp is that
        // override, keeping delivery tasks bounded
        assert_eq!(endpoint_timeout_ms(&ep(json!({"timeout": 600000}))), 30_000);
        assert_eq!(endpoint_timeout_ms(&ep(json!({"timeout": 1}))), 100);
        // creation rejects <=0, but a hand-edited row must not panic
        assert_eq!(endpoint_timeout_ms(&ep(json!({"timeout": -5}))), 5_000);
    }

    /// Table 5.2.15-1 `cooldown`: gate opens only after a
    /// failure and closes once the window elapses or a success lands.
    #[test]
    fn cooldown_gates_only_failed_subscriptions_within_the_window() {
        let now = chrono::Utc::now();
        let recent = (now - chrono::Duration::milliseconds(500)).to_rfc3339();
        let old = (now - chrono::Duration::milliseconds(5_000)).to_rfc3339();
        let sub = |status: &str, last_failure: &str| {
            json!({
                "notification": {
                    "status": status,
                    "lastFailure": last_failure,
                    "endpoint": {"uri": "http://x/n", "cooldown": 2000}
                }
            })
        };
        assert!(
            in_cooldown(&sub("failed", &recent), now),
            "failed 0.5s ago, 2s cooldown ⇒ suppressed"
        );
        assert!(
            !in_cooldown(&sub("failed", &old), now),
            "failure outside the window ⇒ delivered"
        );
        assert!(
            !in_cooldown(&sub("ok", &recent), now),
            "a success clears the gate — status is not \"failed\""
        );
        let no_cooldown = json!({
            "notification": {
                "status": "failed", "lastFailure": recent,
                "endpoint": {"uri": "http://x/n"}
            }
        });
        assert!(
            !in_cooldown(&no_cooldown, now),
            "no cooldown member ⇒ no gate"
        );
    }
}

#[cfg(test)]
mod clause_5_8_1 {
    use super::*;

    /// 5.8.1.4: "the status of the Subscription changes automatically to
    /// \"expired\", so that notifications will no longer be sent" — an
    /// expiresAt spelled without a seconds fraction must count as expired
    /// the moment now (spelled with milliseconds) passes it. A raw
    /// lexicographic compare ranks 'Z' above '.' and keeps the
    /// subscription alive for the whole boundary second.
    #[test]
    fn expiry_compare_survives_fraction_spellings() {
        let now = crate::state::now_iso();
        let secs = &now[..19];
        std::thread::sleep(std::time::Duration::from_millis(5));
        let sub = serde_json::json!({ "expiresAt": format!("{secs}Z") });
        assert!(
            !is_active(&sub),
            "expiresAt {secs}Z lies in the past and must expire the subscription"
        );
        // a genuinely future expiry stays active
        let sub = serde_json::json!({ "expiresAt": "2999-01-01T00:00:00Z" });
        assert!(is_active(&sub));
    }
}

#[cfg(test)]
mod clause_5_3_3 {
    use super::*;
    use serde_json::json;

    /// 5.3.2 triggerReason + 5.3.3 TriggerReasonEnumeration: newlyMatching
    /// (did not match -> matches), updated (matched -> still matches),
    /// noLongerMatching (matched -> no longer / deleted); no notification
    /// when neither side matches.
    #[test]
    fn trigger_reason_enumeration() {
        let ctx = antares_jsonld::Loader::new().core();
        let sub = json!({"entities": [
            {"type": "https://uri.etsi.org/ngsi-ld/default-context/Building"}]});
        let reg = |t: &str| {
            json!({"id": "urn:csr:1", "type": "ContextSourceRegistration",
                "endpoint": "http://peer:9090",
                "information": [{"entities": [
                    {"type": format!("https://uri.etsi.org/ngsi-ld/default-context/{t}")}]}]})
        };
        let hit = reg("Building");
        let miss = reg("Vehicle");
        assert_eq!(
            csource_trigger(&sub, None, Some(&hit), &ctx),
            Some("newlyMatching")
        );
        assert_eq!(
            csource_trigger(&sub, Some(&miss), Some(&hit), &ctx),
            Some("newlyMatching"),
            "an update that STARTS matching is newlyMatching"
        );
        assert_eq!(
            csource_trigger(&sub, Some(&hit), Some(&hit), &ctx),
            Some("updated")
        );
        assert_eq!(
            csource_trigger(&sub, Some(&hit), None, &ctx),
            Some("noLongerMatching"),
            "deletion of a matching registration"
        );
        assert_eq!(
            csource_trigger(&sub, Some(&hit), Some(&miss), &ctx),
            Some("noLongerMatching"),
            "an update that STOPS matching"
        );
        assert_eq!(
            csource_trigger(&sub, Some(&miss), Some(&miss), &ctx),
            None,
            "never-matching changes produce no notification"
        );
    }
}

#[cfg(test)]
mod clause_5_2_33 {
    use super::*;
    use serde_json::json;

    /// Table 5.2.33-1: id is String or String[]; "id takes precedence over
    /// idPattern" when a selector carries both.
    #[test]
    fn selector_id_array_and_precedence() {
        let ctx = antares_jsonld::Loader::new().core();
        let doc = json!({"id": "urn:x:A", "type": ["T"]});
        let sub = |e: serde_json::Value| json!({"entities": [e]});
        assert!(!selector_match(
            &sub(json!({"type": "T", "idPattern": "^urn:x:B"})),
            &doc,
            &ctx
        ));
        assert!(
            selector_match(
                &sub(json!({"type": "T", "id": "urn:x:A", "idPattern": "^urn:x:B"})),
                &doc,
                &ctx
            ),
            "id takes precedence over idPattern"
        );
        assert!(selector_match(
            &sub(json!({"type": "T", "id": ["urn:x:A", "urn:x:C"]})),
            &doc,
            &ctx
        ));
        assert!(
            !selector_match(&sub(json!({"type": "T", "id": ["urn:x:B"]})), &doc, &ctx),
            "an id array not containing the entity id must not match"
        );
    }
}

/// Availability of the change→notification pipeline itself: the consumer
/// task must survive a panic, and its queue must stay bounded.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod change_pipeline {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tower::ServiceExt;

    async fn post(st: &AppState, uri: &str, body: Value) -> u16 {
        let body = body.to_string();
        crate::router(st.clone())
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri(uri)
                    .header("Content-Type", "application/json")
                    .header("Content-Length", body.len())
                    .body(axum::body::Body::from(body))
                    .expect("request"),
            )
            .await
            .expect("response")
            .status()
            .as_u16()
    }

    /// An endpoint that answers 200 and counts the notifications it got.
    async fn counting_endpoint() -> (String, Arc<AtomicUsize>) {
        let hits: Arc<AtomicUsize> = Arc::default();
        let seen = hits.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let app = axum::Router::new().route(
            "/notify",
            axum::routing::post(move || {
                let seen = seen.clone();
                async move {
                    seen.fetch_add(1, Ordering::SeqCst);
                    axum::http::StatusCode::OK
                }
            }),
        );
        tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });
        (format!("http://{addr}/notify"), hits)
    }

    async fn subscribe(st: &AppState, id: &str, uri: &str, timeout_ms: u32) {
        let status = post(
            st,
            "/ngsi-ld/v1/subscriptions",
            json!({
                "id": format!("urn:ngsi-ld:Subscription:{id}"),
                "type": "Subscription",
                "entities": [{"type": "Vehicle"}],
                "notification": {"endpoint": {"uri": uri, "timeout": timeout_ms}},
            }),
        )
        .await;
        assert_eq!(status, 201, "subscription created");
    }

    async fn create_vehicle(st: &AppState, n: usize) -> u16 {
        post(
            st,
            "/ngsi-ld/v1/entities",
            json!({
                "id": format!("urn:ngsi-ld:Vehicle:pipe{n}"),
                "type": "Vehicle",
                "speed": {"type": "Property", "value": n},
            }),
        )
        .await
    }

    /// Table 5.2.14.2-1 timesSent / lastNotification: the mirror the matcher
    /// reads carries the counters the delivery writeback committed, and the
    /// store row agrees — one writeback, no second read.
    #[tokio::test(flavor = "multi_thread")]
    async fn mirror_carries_the_booked_counters() {
        crate::allow_private();
        let (uri, hits) = counting_endpoint().await;
        let mut st = AppState::new("antares-mirror-booked".into());
        wire(&mut st);
        subscribe(&st, "booked", &uri, 30_000).await;
        assert_eq!(create_vehicle(&st, 7).await, 201);
        let deadline = std::time::Instant::now()
            + std::time::Duration::from_secs(10 * crate::state::slow_factor());
        while hits.load(Ordering::SeqCst) < 1 && std::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        assert_eq!(hits.load(Ordering::SeqCst), 1, "one notification delivered");
        let id = "urn:ngsi-ld:Subscription:booked";
        let mirrored = st
            .sub_mirror
            .as_ref()
            .expect("mirror wired")
            .docs("default")
            .into_iter()
            .find(|d| d["id"] == id)
            .expect("subscription mirrored");
        assert_eq!(mirrored["notification"]["timesSent"], json!(1));
        assert!(mirrored["notification"]["lastNotification"].is_string());
        let tenant = TenantId::new("default").expect("tenant");
        let stored = st
            .store
            .get(&tenant, Kind::Subscription, id)
            .expect("store")
            .expect("row");
        assert_eq!(stored["notification"]["timesSent"], json!(1));
        assert_eq!(
            stored["notification"]["lastNotification"],
            mirrored["notification"]["lastNotification"]
        );
    }

    /// 5.8.6: a matching change notifies, and a panic around ONE change must
    /// not end notification delivery for the process — the NEXT change still
    /// notifies. The lock poisoning here used to make the matcher itself
    /// panic; since the mirrors recover from poisoning, the first change may
    /// legitimately deliver too. The contract under test is therefore that
    /// the SECOND change's notification arrives — not a count, which only
    /// measured the race between the two deliveries and failed either way on
    /// slow machines.
    #[tokio::test(flavor = "multi_thread")]
    async fn panicking_change_does_not_stop_the_next_notification() {
        crate::allow_private();
        let (uri, hits) = counting_endpoint().await;
        let mut st = AppState::new("antares-panic-guard".into());
        wire(&mut st);
        subscribe(&st, "guard", &uri, 2_000).await;
        // Poison the mirror lock while a change is in flight: whatever the
        // matcher does with that (recover, or panic into the supervision
        // boundary), the pipeline must keep delivering afterwards.
        let mirror = st.sub_mirror.clone().expect("mirror");
        let m = mirror.clone();
        let _ = std::thread::spawn(move || {
            let _held = m
                .map
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            panic!("poison the mirror");
        })
        .join();
        assert_eq!(create_vehicle(&st, 1).await, 201);
        assert_eq!(create_vehicle(&st, 2).await, 201);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while hits.load(Ordering::SeqCst) < 1 && std::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        let got = hits.load(Ordering::SeqCst);
        assert!(
            (1..=2).contains(&got),
            "delivery must survive the poisoned change: expected 1 or 2 notifications, got {got}"
        );
        // and the pipeline is still alive for a THIRD change after the dust
        // settles — the actual supervision contract
        let before = hits.load(Ordering::SeqCst);
        assert_eq!(create_vehicle(&st, 3).await, 201);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while hits.load(Ordering::SeqCst) <= before && std::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        assert!(
            hits.load(Ordering::SeqCst) > before,
            "a change created after the poisoned one must still notify"
        );
    }

    /// The matcher queue is bounded: behind a stalled subscriber the excess
    /// changes are dropped and counted instead of growing without limit.
    #[tokio::test(flavor = "multi_thread")]
    async fn overflowing_change_queue_drops_and_counts() {
        // stages a race between producer and a parked consumer; under a
        // sanitizer's slowdown the producer never outruns the queue
        // (same for the file store: fsync per create is slower than the drain)
        if std::env::var_os("ANTARES_TEST_SANITIZER").is_some()
            || std::env::var("ANTARES_TEST_STORE").is_ok_and(|s| s == "file")
        {
            return;
        }
        crate::allow_private();
        // accepts, reads nothing, never answers — the serial consumer parks
        // on the first delivery for the endpoint's whole timeout
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        std::thread::spawn(move || {
            let mut held = Vec::new();
            for s in listener.incoming().flatten() {
                held.push(s);
            }
        });
        let mut st = AppState::new("antares-queue-bound".into());
        wire(&mut st);
        // the stall is bounded by the outbound client's own 5 s timeout, not
        // by endpoint.timeout: after each timeout the consumer frees another
        // CHANGE_BATCH slots, and on a slow runner a single-queue-depth loop
        // fits inside that window and never overflows (seen in CI). Produce
        // several queue depths and stop at the first counted drop — the
        // producer only has to outpace the drain, not beat one window.
        subscribe(&st, "staller", &format!("http://{addr}/notify"), 30_000).await;
        let before = changes_dropped();
        for n in 0..(4 * CHANGE_QUEUE) {
            assert_eq!(create_vehicle(&st, 1_000 + n).await, 201);
            if changes_dropped() > before {
                break;
            }
        }
        assert!(
            changes_dropped() > before,
            "a full matcher queue must drop and count, not grow (dropped {} → {})",
            before,
            changes_dropped()
        );
    }

    /// Every accepted change is counted until its pass has run, so a drain
    /// that waits for zero closes the pool only after the last delivery.
    #[tokio::test(flavor = "multi_thread")]
    async fn pending_changes_return_to_zero_once_delivered() {
        crate::allow_private();
        let (uri, hits) = counting_endpoint().await;
        let mut st = AppState::new("antares-pending-changes".into());
        wire(&mut st);
        subscribe(&st, "counter", &uri, 30_000).await;
        for n in 0..20 {
            assert_eq!(create_vehicle(&st, 5_000 + n).await, 201);
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        // one pass folds queued changes into one notification, so the hit
        // count is at least one, not twenty
        let pending = || st.pending_changes.load(Ordering::SeqCst);
        while (hits.load(Ordering::SeqCst) == 0 || pending() > 0)
            && std::time::Instant::now() < deadline
        {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert!(hits.load(Ordering::SeqCst) > 0, "nothing delivered");
        assert_eq!(
            pending(),
            0,
            "pending must fall back to zero after delivery"
        );
    }

    /// Table 5.2.12-1: "\"entityUpdated\" is equivalent to the combination
    /// \"attributeCreated\", \"attributeUpdated\" and \"attributeDeleted\"",
    /// so such a subscription notifies on a creation exactly like the
    /// spelled-out list does — while "entityDeleted" alone does not.
    #[tokio::test(flavor = "multi_thread")]
    async fn entity_updated_trigger_notifies_on_entity_creation() {
        // under a sanitizer 370 concurrent tests starve the endpoint past the
        // outbound client's 5 s cap (seen twice in strict); triggers, not latency
        if std::env::var_os("ANTARES_TEST_SANITIZER").is_some() {
            return;
        }
        crate::allow_private();
        let (uri, hits) = counting_endpoint().await;
        let (quiet_uri, quiet_hits) = counting_endpoint().await;
        let mut st = AppState::new("antares-trigger-equivalence".into());
        wire(&mut st);
        let sub = |id: &str, trigger: &str, uri: &str| {
            json!({
                "id": format!("urn:ngsi-ld:Subscription:{id}"),
                "type": "Subscription",
                "entities": [{"type": "Vehicle"}],
                "notificationTrigger": [trigger],
                // 30 s: a sanitizer runner with 370 concurrent tests took
                // 8 s to deliver once; this test is about triggers, not latency
                "notification": {"endpoint": {"uri": uri, "timeout": 30_000}},
            })
        };
        for body in [
            sub("eu", "entityUpdated", &uri),
            sub("ed", "entityDeleted", &quiet_uri),
        ] {
            assert_eq!(post(&st, "/ngsi-ld/v1/subscriptions", body).await, 201);
        }
        assert_eq!(create_vehicle(&st, 7).await, 201);
        // a wall-clock bound, not an iteration count: a sanitizer build
        // delivers the same notification an order of magnitude slower
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while hits.load(Ordering::SeqCst) == 0 && std::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        // the receiver's count alone cannot say WHY nothing arrived: the
        // subscription's own bookkeeping and the pipeline counters can
        let diagnosis = || {
            let sub = st
                .store
                .get(
                    &TenantId::default(),
                    Kind::Subscription,
                    "urn:ngsi-ld:Subscription:eu",
                )
                .ok()
                .flatten()
                .map(|s| s["notification"].to_string())
                .unwrap_or_else(|| "subscription missing".into());
            format!(
                "notification={sub} changes_dropped={} task_panics={} dead_letters={}",
                changes_dropped(),
                task_panics(),
                DEAD_LETTERS.load(Ordering::Relaxed)
            )
        };
        assert_eq!(
            hits.load(Ordering::SeqCst),
            1,
            "entityUpdated implies attributeCreated, so a creation notifies; {}",
            diagnosis()
        );
        assert_eq!(
            quiet_hits.load(Ordering::SeqCst),
            0,
            "entityDeleted carries no equivalence and must not fire on a creation"
        );
    }
}

#[cfg(test)]
mod clause_5_2_12_triggers {
    use super::*;
    use serde_json::json;

    /// Table 5.2.12-1 notificationTrigger: "If not present, the default is
    /// the combination \"attributeCreated\" and \"attributeUpdated\".
    /// \"entityUpdated\" is equivalent to the combination
    /// \"attributeCreated\", \"attributeUpdated\" and \"attributeDeleted\"."
    #[test]
    fn entity_updated_expands_to_its_equivalent_attribute_triggers() {
        let has = |v: &[String], t: &str| v.iter().any(|s| s == t);

        let default = triggers_of(&json!({}));
        assert_eq!(default, vec!["attributeCreated", "attributeUpdated"]);
        assert!(
            !has(&default, "attributeDeleted"),
            "the default combination is two triggers, not three"
        );

        let expanded = triggers_of(&json!({"notificationTrigger": ["entityUpdated"]}));
        for t in ["attributeCreated", "attributeUpdated", "attributeDeleted"] {
            assert!(has(&expanded, t), "entityUpdated must imply {t}");
        }
        assert!(
            has(&expanded, "entityUpdated"),
            "the declared trigger itself survives the expansion"
        );

        // Only entityUpdated carries the equivalence: the other two entity
        // triggers must NOT gain attribute triggers.
        for t in ["entityCreated", "entityDeleted"] {
            let only = triggers_of(&json!({ "notificationTrigger": [t] }));
            assert_eq!(only, vec![t.to_owned()], "{t} is not an equivalence");
        }

        // Idempotent: an explicit list that already spells the combination
        // out gains nothing, and the equivalent forms agree.
        let literal = triggers_of(&json!({"notificationTrigger":
            ["entityUpdated", "attributeCreated", "attributeUpdated", "attributeDeleted"]}));
        assert_eq!(literal.len(), 4, "no duplicates are appended");
        let mut a = expanded.clone();
        let mut b = literal.clone();
        a.sort();
        b.sort();
        assert_eq!(a, b, "[\"entityUpdated\"] == the spelled-out combination");
    }

    /// Table 5.2.12-1 scopes watchedAttributes to "Watched Attributes
    /// (Properties or Relationships)", so a write that only moves the
    /// entity's own system members is no attribute-level change and must
    /// raise no attributeCreated/attributeUpdated trigger.
    #[test]
    fn entity_system_members_are_not_attribute_changes() {
        let before = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": ["Vehicle"],
            "expiresAt": "2030-01-01T00:00:00Z",
        });
        let after = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": ["Vehicle"],
            "expiresAt": "2031-01-01T00:00:00Z",
            "deletedAt": "2031-01-01T00:00:00Z",
            "modifiedAt": "2026-01-01T00:00:00Z",
            "scope": "/a",
        });
        assert!(
            diff(Some(&before), Some(&after)).is_empty(),
            "entity-level system members are not Attributes"
        );
        // Positive control: a real Property change is still reported.
        let mut with_attr = after.clone();
        with_attr["speed"] = json!([{"type": "Property", "value": 1}]);
        assert_eq!(
            diff(Some(&before), Some(&with_attr)),
            vec![("speed".to_owned(), ChangeClass::Created)]
        );
    }
}

#[cfg(test)]
mod clause_4_5_7_instance_identity {
    use super::*;
    use serde_json::json;

    fn id_of(observed: Option<&str>, dataset: Option<&str>) -> String {
        let mut inst = Map::new();
        if let Some(o) = observed {
            inst.insert("observedAt".into(), json!(o));
        }
        if let Some(d) = dataset {
            inst.insert("datasetId".into(), json!(d));
        }
        instance_id("urn:ngsi-ld:Vehicle:1", "https://a/speed", &inst)
    }

    /// 4.5.7: an instance is the Property "at a particular point in time,
    /// which is recorded as a Temporal Property of the instance (typically
    /// observedAt)" — so the instant, not the way a client spelled it,
    /// decides which instance a record belongs to. 4.6.3 leaves the seconds
    /// fraction optional and accepts a comma separator in requests, and the
    /// broker stores the stamp as written, so one instant reaches this
    /// function under several spellings.
    ///
    /// The consequence is the one the clause names: "Without such an
    /// instanceId, it is not possible to selectively modify or delete
    /// temporal information via the NGSI-LD API. The consequences of this
    /// may be severe in the case of modification or deletion requests for
    /// legal reasons". A correction re-sent with a different spelling landed
    /// on a second row and left the value it was correcting in place.
    #[test]
    fn one_instant_is_one_instance_however_it_is_spelled() {
        let spellings = [
            "2020-01-01T00:00:00Z",
            "2020-01-01T00:00:00.0Z",
            "2020-01-01T00:00:00.000Z",
            "2020-01-01T00:00:00.000000Z",
            "2020-01-01T00:00:00,000Z",
        ];
        let first = id_of(Some(spellings[0]), None);
        for s in &spellings[1..] {
            assert_eq!(id_of(Some(s), None), first, "{s} is the same instant");
        }
        // and with a datasetId, which is part of the same identity
        let first = id_of(Some(spellings[0]), Some("urn:ds:1"));
        for s in &spellings[1..] {
            assert_eq!(
                id_of(Some(s), Some("urn:ds:1")),
                first,
                "{s} is the same instant"
            );
        }
    }

    /// The identity still separates what the clause separates: a different
    /// instant, a different dataset and a different fractional value are
    /// different instances.
    #[test]
    fn different_instants_and_datasets_stay_different_instances() {
        let base = id_of(Some("2020-01-01T00:00:00Z"), None);
        for other in [
            id_of(Some("2020-01-01T00:00:00.500Z"), None),
            id_of(Some("2020-01-01T00:00:01Z"), None),
            id_of(Some("2020-01-02T00:00:00Z"), None),
            id_of(Some("2020-01-01T00:00:00Z"), Some("urn:ds:1")),
        ] {
            assert_ne!(other, base);
        }
        assert_ne!(
            id_of(Some("2020-01-01T00:00:00.500Z"), Some("urn:ds:1")),
            id_of(Some("2020-01-01T00:00:00.500Z"), Some("urn:ds:2"))
        );
        // "Without observedAt there is no instant to key on": a fresh id
        // every time, so two unobserved records never collide.
        assert_ne!(id_of(None, None), id_of(None, None));
    }
}

#[cfg(test)]
mod clause_5_8_6_deletion_payload {
    use super::*;
    use serde_json::json;

    const DC: &str = "https://uri.etsi.org/ngsi-ld/default-context";

    fn ctx() -> Arc<Context> {
        antares_jsonld::Loader::new().core()
    }

    /// Instances of one attribute in a notification data entry, whether the
    /// representation collapsed a single instance to an object or kept an
    /// array.
    fn insts<'a>(entry: &'a Value, name: &str) -> Vec<&'a Value> {
        match entry.get(name) {
            Some(Value::Array(a)) => a.iter().collect(),
            Some(v) => vec![v],
            None => Vec::new(),
        }
    }

    fn build(
        sub: &Value,
        before: &Value,
        after: Option<&Value>,
        deleted: &[String],
        entity_deleted: bool,
    ) -> Value {
        let st = AppState::new("antares-tombstone-test".into());
        let tenant = TenantId::new("default").expect("tenant");
        let ctx = ctx();
        let data = build_data(
            &st,
            &tenant,
            sub,
            &ctx,
            Some(before),
            after,
            deleted,
            entity_deleted,
            "2026-01-01T00:00:00Z",
        );
        assert_eq!(data.len(), 1, "one changed entity ⇒ one data entry");
        data.into_iter().next().expect("entry")
    }

    /// 5.8.6: a deleted Attribute is notified as the NGSI-LD null value of
    /// its own type — `object` for a Relationship, the `{"@none": …}`
    /// languageMap form for a LanguageProperty, `json`, `vocab`, `value`.
    #[test]
    fn typed_null_member_per_attribute_type() {
        let cases = [
            ("Property", "value", json!("urn:ngsi-ld:null")),
            ("Relationship", "object", json!("urn:ngsi-ld:null")),
            (
                "LanguageProperty",
                "languageMap",
                json!({"@none": "urn:ngsi-ld:null"}),
            ),
            ("JsonProperty", "json", json!("urn:ngsi-ld:null")),
            ("VocabProperty", "vocab", json!("urn:ngsi-ld:null")),
        ];
        for (atype, member, null_value) in cases {
            let before = json!({
                "id": "urn:ngsi-ld:Vehicle:tomb",
                "type": [format!("{DC}/Vehicle")],
                format!("{DC}/gone"): [{"type": atype, member: json!("previous")}],
            });
            let after = json!({
                "id": "urn:ngsi-ld:Vehicle:tomb",
                "type": [format!("{DC}/Vehicle")],
            });
            let entry = build(
                &json!({"notification": {}}),
                &before,
                Some(&after),
                &[format!("{DC}/gone")],
                false,
            );
            let got = insts(&entry, "gone");
            assert_eq!(got.len(), 1, "{atype}: one tombstone");
            assert_eq!(got[0].get("type"), Some(&json!(atype)));
            assert_eq!(
                got[0].get(member),
                Some(&null_value),
                "{atype} tombstones through its own {member} member"
            );
            // showChanges false and sysAttrs false ⇒ neither stamp appears
            assert!(
                got[0].get("deletedAt").is_none(),
                "{atype}: no deletedAt without sysAttrs"
            );
            assert!(
                got[0]
                    .as_object()
                    .is_some_and(|o| !o.keys().any(|k| k.starts_with("previous"))),
                "{atype}: no previous* member without showChanges"
            );
        }
    }

    /// 5.8.6: a whole Attribute deleted at once is ONE tombstone with no
    /// datasetId, while losing individual instances tombstones each lost
    /// datasetId and leaves the survivors untouched.
    #[test]
    fn whole_attribute_versus_per_instance_deletion() {
        let three = json!([
            {"type": "Property", "value": 1},
            {"type": "Property", "value": 2, "datasetId": "urn:ds:a"},
            {"type": "Property", "value": 3, "datasetId": "urn:ds:b"},
        ]);
        let before = json!({
            "id": "urn:ngsi-ld:Vehicle:multi",
            "type": [format!("{DC}/Vehicle")],
            format!("{DC}/speed"): three,
        });
        let attr = vec![format!("{DC}/speed")];

        // whole attribute gone
        let after = json!({
            "id": "urn:ngsi-ld:Vehicle:multi",
            "type": [format!("{DC}/Vehicle")],
        });
        let entry = build(
            &json!({"notification": {}}),
            &before,
            Some(&after),
            &attr,
            false,
        );
        let got = insts(&entry, "speed");
        assert_eq!(got.len(), 1, "a whole-attribute deletion is one tombstone");
        assert!(
            got[0].get("datasetId").is_none(),
            "the single tombstone carries no datasetId"
        );

        // one instance gone, two survive
        let after = json!({
            "id": "urn:ngsi-ld:Vehicle:multi",
            "type": [format!("{DC}/Vehicle")],
            format!("{DC}/speed"): [
                {"type": "Property", "value": 1},
                {"type": "Property", "value": 3, "datasetId": "urn:ds:b"},
            ],
        });
        let entry = build(
            &json!({"notification": {}}),
            &before,
            Some(&after),
            &attr,
            false,
        );
        let got = insts(&entry, "speed");
        assert_eq!(got.len(), 3, "two survivors plus one tombstone");
        let tombs: Vec<&&Value> = got
            .iter()
            .filter(|i| i.get("value") == Some(&json!("urn:ngsi-ld:null")))
            .collect();
        assert_eq!(tombs.len(), 1, "exactly the lost instance is tombstoned");
        assert_eq!(tombs[0].get("datasetId"), Some(&json!("urn:ds:a")));
        for surviving in got
            .iter()
            .filter(|i| i.get("value") != Some(&json!("urn:ngsi-ld:null")))
        {
            assert!(
                surviving.get("deletedAt").is_none(),
                "a surviving instance is not marked deleted"
            );
        }
    }

    /// 5.8.6: "If an Attribute has been deleted, only the name of the
    /// attribute as key and the URI `urn:ngsi-ld:null` as value shall be
    /// provided, unless more information is required. The latter is the case,
    /// if: a datasetId needs to be provided; the notification.sysAttrs is set
    /// to true …; notification.showChanges is set to true …. In all such
    /// cases, a JSON object with all the required information is provided,
    /// where the value or the object is set to the URI `urn:ngsi-ld:null`
    /// respectively or, in case of a LanguageProperty, the languageMap is set
    /// to `{"@none": "urn:ngsi-ld:null"}`."
    ///
    /// The bare-key form is the one an interoperability campaign reads, and
    /// it is reached only through the concise collapse — the tombstone itself
    /// is always built as an object, so nothing below this level can tell the
    /// two apart. 5.5.4 confines the bare form to concise: normalized keeps
    /// the object, and a first-level `urn:ngsi-ld:null` is BadRequestData
    /// everywhere else.
    #[test]
    fn a_deleted_attribute_is_bare_unless_it_has_more_to_say() {
        let speed = format!("{DC}/speed");
        let label = format!("{DC}/label");
        let before = json!({
            "id": "urn:ngsi-ld:Vehicle:c",
            "type": [format!("{DC}/Vehicle")],
            speed.clone(): [{"type": "Property", "value": 1}],
            label.clone(): [{"type": "LanguageProperty", "languageMap": {"en": "hi"}}],
        });
        let after = json!({
            "id": "urn:ngsi-ld:Vehicle:c",
            "type": [format!("{DC}/Vehicle")],
        });
        let deleted = [speed.clone(), label.clone()];
        let entry = |n: Value| {
            build(
                &json!({"notification": n}),
                &before,
                Some(&after),
                &deleted,
                false,
            )
        };

        // Nothing more is required: the attribute IS the URI.
        for fmt in ["concise", "simplified"] {
            let e = entry(json!({"format": fmt}));
            assert_eq!(
                e["speed"],
                json!("urn:ngsi-ld:null"),
                "{fmt}: a plain deletion is the bare URI, not an object"
            );
            assert_eq!(
                e["label"],
                json!({"languageMap": {"@none": "urn:ngsi-ld:null"}}),
                "{fmt}: a LanguageProperty deletion is the @none map"
            );
        }

        // Normalized is not the bare form (5.5.4 confines that to concise).
        let e = entry(json!({"format": "normalized"}));
        assert_eq!(
            e["speed"],
            json!({"type": "Property", "value": "urn:ngsi-ld:null"}),
            "normalized keeps the typed object"
        );

        // sysAttrs: the system-generated sub-attributes have to be provided,
        // so the deletion becomes an object carrying them.
        let e = entry(json!({"format": "concise", "sysAttrs": true}));
        assert_eq!(e["speed"]["value"], json!("urn:ngsi-ld:null"));
        assert_eq!(e["speed"]["deletedAt"], json!("2026-01-01T00:00:00Z"));

        // showChanges: a previous value has to be provided.
        let e = entry(json!({"format": "concise", "showChanges": true}));
        assert_eq!(e["speed"]["value"], json!("urn:ngsi-ld:null"));
        assert_eq!(e["speed"]["previousValue"], json!(1));
        assert_eq!(
            e["label"]["previousLanguageMap"],
            json!({"en": "hi"}),
            "a LanguageProperty reports previousLanguageMap"
        );

        // A datasetId needs to be provided: one instance of two goes, so the
        // tombstone must name which — and stays an object to do it.
        let before_ds = json!({
            "id": "urn:ngsi-ld:Vehicle:c",
            "type": [format!("{DC}/Vehicle")],
            speed.clone(): [{"type": "Property", "value": 1, "datasetId": "urn:ds:a"},
                    {"type": "Property", "value": 2, "datasetId": "urn:ds:b"}],
        });
        let after_ds = json!({
            "id": "urn:ngsi-ld:Vehicle:c",
            "type": [format!("{DC}/Vehicle")],
            speed.clone(): [{"type": "Property", "value": 2, "datasetId": "urn:ds:b"}],
        });
        let e = build(
            &json!({"notification": {"format": "concise"}}),
            &before_ds,
            Some(&after_ds),
            &[speed],
            false,
        );
        let gone = e["speed"]
            .as_array()
            .and_then(|a| a.iter().find(|i| i["value"] == json!("urn:ngsi-ld:null")))
            .expect("the lost instance is tombstoned");
        assert_eq!(gone["datasetId"], json!("urn:ds:a"));
        assert!(
            e["speed"]
                .as_array()
                .is_some_and(|a| a.iter().any(|i| i["value"] == json!(2))),
            "the surviving instance is still reported: {e}"
        );
    }

    /// 5.8.6 with sysAttrs and showChanges: the tombstone carries deletedAt
    /// and the previous value of its own typed member.
    #[test]
    fn sys_attrs_and_show_changes_stamp_the_tombstone() {
        let before = json!({
            "id": "urn:ngsi-ld:Vehicle:stamp",
            "type": [format!("{DC}/Vehicle")],
            format!("{DC}/where"): [{"type": "Relationship", "object": "urn:ngsi-ld:P:1",
                                     "createdAt": "2025-01-01T00:00:00Z"}],
        });
        let after = json!({
            "id": "urn:ngsi-ld:Vehicle:stamp",
            "type": [format!("{DC}/Vehicle")],
        });
        let sub = json!({"notification": {"sysAttrs": true, "showChanges": true}});
        let entry = build(&sub, &before, Some(&after), &[format!("{DC}/where")], false);
        let got = insts(&entry, "where");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].get("object"), Some(&json!("urn:ngsi-ld:null")));
        assert_eq!(
            got[0].get("deletedAt"),
            Some(&json!("2026-01-01T00:00:00Z"))
        );
        assert_eq!(
            got[0].get("previousObject"),
            Some(&json!("urn:ngsi-ld:P:1")),
            "showChanges reports the previous object, not previousValue"
        );
        assert!(
            got[0].get("previousValue").is_none(),
            "a Relationship never reports previousValue"
        );
    }
}

#[cfg(test)]
mod candidate_index {
    use super::*;
    use serde_json::json;

    const DC: &str = "https://uri.etsi.org/ngsi-ld/default-context";

    /// The index may over-select, never under-select: every subscription
    /// `selector_match` accepts for a change must come back from
    /// `candidates()` for that change's types and changed attributes —
    /// otherwise it silently stops firing (5.8.6).
    #[test]
    fn candidates_never_under_select_for_any_selector_shape() {
        let ctx = antares_jsonld::Loader::new().core();
        let doc = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": [format!("{DC}/Vehicle")],
            format!("{DC}/speed"): [{"type": "Property", "value": 1}],
        });
        let shapes: Vec<(&str, Value)> = vec![
            (
                "plain type",
                json!({"entities": [{"type": format!("{DC}/Vehicle")}]}),
            ),
            (
                "multiple types",
                json!({"entities": [{"type": format!("{DC}/Vehicle")},
                                    {"type": format!("{DC}/Building")}]}),
            ),
            (
                "id only",
                json!({"entities": [{"id": "urn:ngsi-ld:Vehicle:1"}]}),
            ),
            (
                "idPattern only",
                json!({"entities": [{"idPattern": "^urn:ngsi-ld:Vehicle:"}]}),
            ),
            (
                "type selection expression",
                json!({"entities": [{"type": format!("{DC}/Vehicle|{DC}/Building")}]}),
            ),
            (
                "watchedAttributes only",
                json!({"watchedAttributes": [format!("{DC}/speed")]}),
            ),
            (
                "watchedAttributes with entities",
                json!({"entities": [{"type": format!("{DC}/Vehicle")}],
                       "watchedAttributes": [format!("{DC}/speed")]}),
            ),
            (
                "type with idPattern",
                json!({"entities": [{"type": format!("{DC}/Vehicle"),
                                     "idPattern": "^urn:ngsi-ld:Vehicle:"}]}),
            ),
        ];
        let mirror = SubMirror::default();
        for (i, (_, doc)) in shapes.iter().enumerate() {
            let mut sub = doc.clone();
            sub["id"] = json!(format!("urn:ngsi-ld:Subscription:{i}"));
            mirror.apply(
                "default",
                &format!("urn:ngsi-ld:Subscription:{i}"),
                Some(sub),
            );
        }
        let types = [format!("{DC}/Vehicle")];
        let changed = [format!("{DC}/speed")];
        let got = mirror.candidates(
            "default",
            &types.iter().map(String::as_str).collect::<Vec<_>>(),
            &changed.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        for (i, (name, shape)) in shapes.iter().enumerate() {
            if !selector_match(shape, &doc, &ctx) {
                continue;
            }
            let id = format!("urn:ngsi-ld:Subscription:{i}");
            assert!(
                got.iter()
                    .any(|c| c.get("id").and_then(Value::as_str) == Some(id.as_str())),
                "{name}: selector_match accepts it, so candidates() must return it"
            );
        }
        // A change touching neither the type nor the watched attribute still
        // yields the shapes the index cannot classify — over-selection is
        // allowed — but never the exactly-classified plain-type subscription.
        let other = mirror.candidates("default", &[&format!("{DC}/Device")], &[]);
        assert!(
            !other
                .iter()
                .any(|c| c.get("id").and_then(Value::as_str) == Some("urn:ngsi-ld:Subscription:0")),
            "an exactly classified type subscription is not evaluated for other types"
        );
    }

    /// The two classification sites must stay in the superset relation:
    /// every character `selector_match` reads as a 4.17 type-selection
    /// expression has to send the subscription to the broad bucket, or the
    /// index would under-select.
    #[test]
    fn type_selection_expressions_are_always_broad() {
        for c in ['|', ',', ';', '('] {
            let sub = json!({"entities": [{"type": format!("{DC}/A{c}{DC}/B")}]});
            assert!(
                matches!(index_keys(&sub), Keys::Broad),
                "a type containing {c:?} is a selection expression for selector_match, \
                 so the index must not classify it as a plain type"
            );
        }
        // Table 5.2.33-1's "*" is the other member of that relation: it is
        // stored raw and `selector_match` reads it as every type, so an index
        // that classified it as the literal type "*" would look up a key no
        // change carries and the subscription would never be a candidate.
        assert!(
            matches!(
                index_keys(&json!({"entities": [{"type": "*"}]})),
                Keys::Broad
            ),
            "a \"*\" selector matches every type, so the index must go broad"
        );
        assert!(matches!(
            index_keys(&json!({"entities": [{"type": format!("{DC}/Vehicle")}]})),
            Keys::Types(_)
        ));
    }
}

/// Table 5.2.14.2-1 bookkeeping around a delivery that fails or is never
/// attempted at all.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod clause_5_2_14_2_bookkeeping {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const SUB_ID: &str = "urn:ngsi-ld:Subscription:book";

    /// An endpoint answering `status`, counting the requests that reach it.
    async fn endpoint(status: axum::http::StatusCode) -> (String, Arc<AtomicUsize>) {
        let hits: Arc<AtomicUsize> = Arc::default();
        let seen = hits.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let app = axum::Router::new().route(
            "/notify",
            axum::routing::post(move || {
                let seen = seen.clone();
                async move {
                    seen.fetch_add(1, Ordering::SeqCst);
                    status
                }
            }),
        );
        tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });
        (format!("http://{addr}/notify"), hits)
    }

    fn stored_notification(st: &AppState, tenant: &TenantId) -> Value {
        st.store
            .get(tenant, Kind::Subscription, SUB_ID)
            .expect("store read")
            .expect("subscription row")
            .get("notification")
            .cloned()
            .expect("notification member")
    }

    async fn send(st: &AppState, tenant: &TenantId, sub: &Value) {
        let ctx = antares_jsonld::Loader::new().core();
        deliver_as(
            st,
            tenant,
            Kind::Subscription,
            sub,
            "Notification",
            vec![json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle"})],
            &ctx,
            None,
        )
        .await;
    }

    fn subscribe(st: &AppState, tenant: &TenantId, uri: &str) -> Value {
        let sub = json!({
            "id": SUB_ID,
            "type": "Subscription",
            "entities": [{"type": "Vehicle"}],
            "notification": {"endpoint": {"uri": uri}},
        });
        st.store
            .create(tenant, Kind::Subscription, SUB_ID, sub.clone())
            .expect("subscription row");
        sub
    }

    /// Table 5.2.14.2-1 timesFailed: "Number of times an unsuccessful
    /// response (or timeout) has been received when delivering the
    /// notification" — an output-only member implementations shall generate.
    #[tokio::test(flavor = "multi_thread")]
    async fn failed_delivery_generates_and_increments_times_failed() {
        crate::allow_private();
        let (uri, hits) = endpoint(axum::http::StatusCode::INTERNAL_SERVER_ERROR).await;
        let st = AppState::new("antares-times-failed".into());
        let tenant = TenantId::new("default").expect("tenant");
        let sub = subscribe(&st, &tenant, &uri);

        send(&st, &tenant, &sub).await;
        let n = stored_notification(&st, &tenant);
        assert_eq!(hits.load(Ordering::SeqCst), 1, "the endpoint was tried");
        assert_eq!(n.get("timesFailed"), Some(&json!(1)));
        assert_eq!(n.get("timesSent"), Some(&json!(1)), "the attempt was sent");
        assert_eq!(n.get("status"), Some(&json!("failed")));
        assert!(
            n.get("lastSuccess").is_none(),
            "a failure rolls the optimistic lastSuccess back"
        );

        send(&st, &tenant, &sub).await;
        let n = stored_notification(&st, &tenant);
        assert_eq!(
            n.get("timesFailed"),
            Some(&json!(2)),
            "timesFailed counts every unsuccessful response"
        );
    }

    /// Table 5.2.14.2-1 timesSent = "Number of times that the notification
    /// has been sent" and lastNotification = "the instant when the last
    /// notification has been sent": a change suppressed by the open circuit
    /// never reaches the wire, so neither member may move.
    #[tokio::test(flavor = "multi_thread")]
    async fn breaker_suppressed_delivery_does_not_move_times_sent() {
        crate::allow_private();
        let (uri, hits) = endpoint(axum::http::StatusCode::OK).await;
        let st = AppState::new("antares-breaker-bookkeeping".into());
        let tenant = TenantId::new("default").expect("tenant");
        let sub = subscribe(&st, &tenant, &uri);
        for _ in 0..crate::egress::TRIP_AFTER {
            st.egress.record_failure(tenant.as_str(), &uri);
        }
        assert!(
            st.egress.is_open(tenant.as_str(), &uri),
            "the destination is open-circuit"
        );

        send(&st, &tenant, &sub).await;
        let n = stored_notification(&st, &tenant);
        assert_eq!(hits.load(Ordering::SeqCst), 0, "nothing left the process");
        assert!(
            n.get("timesSent").is_none(),
            "a suppressed notification was never sent"
        );
        assert!(
            n.get("lastNotification").is_none(),
            "lastNotification is the instant a notification was sent"
        );

        // Positive control: with the circuit closed the same call delivers,
        // so the assertions above cannot pass vacuously.
        st.egress.record_success(tenant.as_str(), &uri);
        send(&st, &tenant, &sub).await;
        let n = stored_notification(&st, &tenant);
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        assert_eq!(n.get("timesSent"), Some(&json!(1)));
        assert!(n.get("lastNotification").is_some());
    }
}

/// Delivery policy: retries are transport under one notification (5.8.6
/// books the notification once), a success by retry sets lastSuccess and
/// status ok, an exhausted policy leaves exactly one dead letter, and the
/// default policy is the single attempt the clause describes.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod delivery_policy_tests {
    use super::*;
    use antares_notifier::DeliveryPolicy;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    const SUB_ID: &str = "urn:ngsi-ld:Subscription:policy";

    fn policy(attempts: u32, backoff_ms: u64) -> DeliveryPolicy {
        DeliveryPolicy {
            attempts,
            backoff: Duration::from_millis(backoff_ms),
            jitter: 0.0,
            max_age: Duration::from_secs(60),
        }
    }

    /// Answers 500 to the first `fail_first` requests, 200 afterwards.
    async fn flaky_endpoint(fail_first: usize) -> (String, Arc<AtomicUsize>) {
        let hits: Arc<AtomicUsize> = Arc::default();
        let seen = hits.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let app = axum::Router::new().route(
            "/notify",
            axum::routing::post(move || {
                let seen = seen.clone();
                async move {
                    let n = seen.fetch_add(1, Ordering::SeqCst);
                    if n < fail_first {
                        axum::http::StatusCode::INTERNAL_SERVER_ERROR
                    } else {
                        axum::http::StatusCode::OK
                    }
                }
            }),
        );
        tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });
        (format!("http://{addr}/notify"), hits)
    }

    fn state(p: DeliveryPolicy) -> (AppState, TenantId) {
        crate::allow_private();
        let mut st = AppState::new("antares-policy".into());
        st.delivery = p;
        (st, TenantId::new("default").expect("tenant"))
    }

    fn subscribe(st: &AppState, tenant: &TenantId, id: &str, uri: &str) -> Value {
        let sub = json!({
            "id": id,
            "type": "Subscription",
            "entities": [{"type": "Vehicle"}],
            "notification": {"endpoint": {"uri": uri}},
        });
        st.store
            .create(tenant, Kind::Subscription, id, sub.clone())
            .expect("subscription row");
        sub
    }

    async fn send(st: &AppState, tenant: &TenantId, sub: &Value) {
        let ctx = antares_jsonld::Loader::new().core();
        deliver_as(
            st,
            tenant,
            Kind::Subscription,
            sub,
            "Notification",
            vec![json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle"})],
            &ctx,
            None,
        )
        .await;
    }

    fn notification(st: &AppState, tenant: &TenantId, id: &str) -> Value {
        st.store
            .get(tenant, Kind::Subscription, id)
            .expect("store read")
            .expect("subscription row")["notification"]
            .clone()
    }

    fn letters(st: &AppState, tenant: &TenantId) -> Vec<Value> {
        st.store.list(tenant, Kind::DeadLetter).expect("list")
    }

    async fn wait_until(mut cond: impl FnMut() -> bool, what: &str) {
        for _ in 0..100 {
            if cond() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("timed out waiting for {what}");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_retry_that_succeeds_is_one_notification() {
        let (st, t) = state(policy(3, 50));
        let (uri, hits) = flaky_endpoint(2).await;
        let sub = subscribe(&st, &t, SUB_ID, &uri);
        send(&st, &t, &sub).await;
        // the first attempt is booked at once, as 5.8.6 says
        let n = notification(&st, &t, SUB_ID);
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        assert_eq!(n["timesSent"], json!(1));
        assert_eq!(n["status"], json!("failed"));
        assert!(n.get("lastSuccess").is_none());
        wait_until(
            || notification(&st, &t, SUB_ID)["status"] == json!("ok"),
            "retry success",
        )
        .await;
        let n = notification(&st, &t, SUB_ID);
        assert_eq!(hits.load(Ordering::SeqCst), 3, "two retries were made");
        assert_eq!(
            n["timesSent"],
            json!(1),
            "retries never count as a second notification"
        );
        assert_eq!(
            n["timesFailed"],
            json!(1),
            "the failed first attempt was booked once"
        );
        assert!(n.get("lastSuccess").is_some());
        assert!(
            n.get("lastFailure").is_some(),
            "the earlier failure stays recorded"
        );
        assert!(
            letters(&st, &t).is_empty(),
            "a delivered notification is no dead letter"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn an_exhausted_policy_leaves_exactly_one_dead_letter() {
        let (st, t) = state(policy(2, 50));
        let (uri, hits) = flaky_endpoint(usize::MAX).await;
        let sub = subscribe(&st, &t, SUB_ID, &uri);
        let before = dead_letters_written();
        send(&st, &t, &sub).await;
        wait_until(|| !letters(&st, &t).is_empty(), "dead letter").await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        let l = letters(&st, &t);
        assert_eq!(l.len(), 1, "{l:?}");
        assert_eq!(hits.load(Ordering::SeqCst), 2, "attempts = policy.attempts");
        let l = &l[0];
        assert_eq!(l["subscriptionId"], json!(SUB_ID));
        assert_eq!(l["attempts"], json!(2));
        assert_eq!(l["binding"], json!("http"));
        assert_eq!(l["uri"], json!(uri));
        assert_eq!(l["lastError"], json!("HTTP 500"));
        assert_eq!(l["payload"]["type"], json!("Notification"));
        assert_eq!(l["payload"]["subscriptionId"], json!(SUB_ID));
        assert!(l["id"]
            .as_str()
            .is_some_and(|i| i.starts_with("urn:ngsi-ld:DeadLetter:")));
        // the letter carries the endpoint members the binding renders from,
        // so a replay produces the identical request
        assert_eq!(l["accept"], json!("application/json"));
        assert!(l["link"].as_str().is_some_and(|v| v.contains("rel=")));
        assert!(l["receiverInfo"].is_array());
        assert!(dead_letters_written() > before);
        let n = notification(&st, &t, SUB_ID);
        assert_eq!(n["timesSent"], json!(1));
        assert_eq!(n["timesFailed"], json!(1));
        assert_eq!(n["status"], json!("failed"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_default_policy_never_retries_and_never_dead_letters() {
        let (st, t) = state(DeliveryPolicy::default());
        let (uri, hits) = flaky_endpoint(usize::MAX).await;
        let sub = subscribe(&st, &t, SUB_ID, &uri);
        send(&st, &t, &sub).await;
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        assert!(letters(&st, &t).is_empty());
        assert_eq!(notification(&st, &t, SUB_ID)["status"], json!("failed"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_backoff_on_one_subscription_does_not_delay_another() {
        let (st, t) = state(policy(2, 3_000));
        let (dead, _) = flaky_endpoint(usize::MAX).await;
        let (live, live_hits) = flaky_endpoint(0).await;
        let a = subscribe(&st, &t, "urn:ngsi-ld:Subscription:a", &dead);
        let b = subscribe(&st, &t, "urn:ngsi-ld:Subscription:b", &live);
        let started = std::time::Instant::now();
        send(&st, &t, &a).await;
        send(&st, &t, &b).await;
        assert_eq!(live_hits.load(Ordering::SeqCst), 1, "B delivered");
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "A's 3 s backoff must not sit on the delivery path: {:?}",
            started.elapsed()
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn retries_stop_when_the_subscription_is_deleted() {
        let (st, t) = state(policy(4, 100));
        let (uri, hits) = flaky_endpoint(usize::MAX).await;
        let sub = subscribe(&st, &t, SUB_ID, &uri);
        send(&st, &t, &sub).await;
        st.store
            .delete(&t, Kind::Subscription, SUB_ID)
            .expect("delete");
        tokio::time::sleep(Duration::from_millis(800)).await;
        assert_eq!(
            hits.load(Ordering::SeqCst),
            1,
            "no retry for a gone subscription"
        );
        assert!(
            letters(&st, &t).is_empty(),
            "no dead letter for a gone subscription"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn an_egress_refusal_is_never_retried() {
        let (mut st, t) = state(policy(3, 50));
        // the deny policy is built directly: the environment is shared by
        // every test thread in this process, and a state constructed while
        // the variable read "false" would refuse its loopback endpoint for
        // the rest of its life
        st.egress = Arc::new(crate::egress::Egress::new(antares_jsonld::EgressPolicy {
            allow_private: false,
        }));
        let (uri, hits) = flaky_endpoint(0).await;
        let sub = subscribe(&st, &t, SUB_ID, &uri);
        send(&st, &t, &sub).await;
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert_eq!(
            hits.load(Ordering::SeqCst),
            0,
            "policy refusal: nothing leaves"
        );
        assert!(
            letters(&st, &t).is_empty(),
            "a policy verdict is not a transport failure"
        );
        assert_eq!(notification(&st, &t, SUB_ID)["status"], json!("failed"));
    }
}

/// 5.8.6 periodic notifications: what a due `timeInterval` subscription
/// reads, what it sends, and which ticks it costs nothing at all.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod clause_5_8_6_periodic_sweep {
    use super::*;
    use serde_json::json;

    const DC: &str = "https://uri.etsi.org/ngsi-ld/default-context";

    fn entity(id: &str, ty: &str) -> Value {
        json!({"id": id, "type": [format!("{DC}/{ty}")]})
    }

    /// 5.8.6 narrows the periodic read to "all the subscribed Entities" — and
    /// a narrowing may only ever over-select: every Entity the arbiter
    /// (`selector_match`) accepts has to survive both predicates the sweep
    /// hands the store, or that subscription silently stops reporting it.
    #[test]
    fn periodic_read_narrowing_never_under_selects() {
        let ctx = antares_jsonld::Loader::new().core();
        let docs = [
            entity("urn:ngsi-ld:Vehicle:1", "Vehicle"),
            entity("urn:ngsi-ld:Vehicle:2", "Vehicle"),
            entity("urn:ngsi-ld:Building:1", "Building"),
        ];
        let subs: Vec<(&str, Value)> = vec![
            (
                "plain type",
                json!({"entities": [{"type": format!("{DC}/Vehicle")}]}),
            ),
            (
                "one id with its type",
                json!({"entities": [{"id": "urn:ngsi-ld:Vehicle:2",
                                     "type": format!("{DC}/Vehicle")}]}),
            ),
            (
                "id array",
                json!({"entities": [{"id": ["urn:ngsi-ld:Vehicle:1",
                                            "urn:ngsi-ld:Building:1"]}]}),
            ),
            (
                "one id plus a bare-type entry",
                json!({"entities": [{"id": "urn:ngsi-ld:Vehicle:1"},
                                    {"type": format!("{DC}/Building")}]}),
            ),
            (
                "idPattern only",
                json!({"entities": [{"idPattern": "^urn:ngsi-ld:Vehicle:"}]}),
            ),
            (
                "id overriding a contradicting idPattern",
                json!({"entities": [{"id": "urn:ngsi-ld:Vehicle:1",
                                     "idPattern": "^urn:ngsi-ld:Building:"}]}),
            ),
            (
                "type selection expression",
                json!({"entities": [{"type": format!("{DC}/Vehicle|{DC}/Building")}]}),
            ),
            (
                "no entities selector at all",
                json!({"watchedAttributes": [format!("{DC}/speed")]}),
            ),
        ];
        for (name, sub) in &subs {
            let ids = selector_ids(sub);
            let type_groups: Vec<Vec<String>> = match index_keys(sub) {
                Keys::Types(ts) => ts.into_iter().map(|t| vec![t]).collect(),
                _ => Vec::new(),
            };
            for doc in &docs {
                if !selector_match(sub, doc, &ctx) {
                    continue;
                }
                let id = doc["id"].as_str().expect("id");
                if let Some(ids) = &ids {
                    assert!(
                        ids.iter().any(|i| i == id),
                        "{name}: selector_match accepts {id}, so the id narrowing must keep it"
                    );
                }
                if !type_groups.is_empty() {
                    let types: Vec<&str> = doc["type"]
                        .as_array()
                        .expect("type array")
                        .iter()
                        .filter_map(Value::as_str)
                        .collect();
                    assert!(
                        type_groups
                            .iter()
                            .any(|g| g.iter().all(|t| types.contains(&t.as_str()))),
                        "{name}: selector_match accepts {id}, so the type narrowing must keep it"
                    );
                }
            }
        }
    }

    /// Table 5.2.33-1: `id` is a String or a String[] and takes precedence
    /// over `idPattern`, so it pins the read exactly — while any entry
    /// leaving the id open (a bare type, an idPattern, no selector at all)
    /// must yield NO id predicate, since the OR-ed selector then admits
    /// Entities no listed id names.
    #[test]
    fn periodic_read_narrows_by_id_only_when_every_selector_entry_pins_one() {
        assert_eq!(
            selector_ids(&json!({"entities": [{"id": "urn:x:A", "type": "T"}]})),
            Some(vec!["urn:x:A".to_owned()])
        );
        assert_eq!(
            selector_ids(&json!({"entities": [{"id": ["urn:x:A", "urn:x:B"]},
                                              {"id": "urn:x:C"}]})),
            Some(vec![
                "urn:x:A".to_owned(),
                "urn:x:B".to_owned(),
                "urn:x:C".to_owned()
            ])
        );
        for open in [
            json!({"entities": [{"id": "urn:x:A"}, {"type": "T"}]}),
            json!({"entities": [{"idPattern": "^urn:x:"}]}),
            json!({"entities": [{"type": "T"}]}),
            json!({"entities": []}),
            json!({"watchedAttributes": ["a"]}),
            json!({}),
            json!({"entities": [{"id": {"not": "a string"}}]}),
        ] {
            assert_eq!(
                selector_ids(&open),
                None,
                "{open} leaves the id open — narrowing by id would drop matching Entities"
            );
        }
    }

    /// An endpoint that keeps every notification body it receives.
    async fn recording_endpoint() -> (String, Arc<std::sync::Mutex<Vec<Value>>>) {
        let seen: Arc<std::sync::Mutex<Vec<Value>>> = Arc::default();
        let sink = seen.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let app = axum::Router::new().route(
            "/notify",
            axum::routing::post(move |body: String| {
                let sink = sink.clone();
                async move {
                    if let Ok(v) = serde_json::from_str::<Value>(&body) {
                        sink.lock().expect("recorded bodies").push(v);
                    }
                    axum::http::StatusCode::OK
                }
            }),
        );
        tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });
        (format!("http://{addr}/notify"), seen)
    }

    /// A periodic subscription created `age_s` seconds ago with a one-second
    /// interval — due on the next sweep.
    fn periodic(id: &str, uri: &str, entities: Value, age_s: i64) -> Value {
        let created = chrono::Utc::now() - chrono::Duration::seconds(age_s);
        json!({
            "id": id,
            "type": "Subscription",
            "timeInterval": 1,
            "createdAt": created.to_rfc3339(),
            "entities": entities,
            "notification": {"endpoint": {"uri": uri, "accept": "application/json"}},
        })
    }

    fn install(st: &AppState, tenant: &TenantId, sub: &Value) {
        let id = sub["id"].as_str().expect("sub id");
        st.store
            .create(tenant, Kind::Subscription, id, sub.clone())
            .expect("subscription row");
        if let Some(m) = &st.sub_mirror {
            m.apply(tenant.as_str(), id, Some(sub.clone()));
        }
    }

    fn state_with_mirror(alias: &str) -> (AppState, Arc<SubMirror>, TenantId) {
        let mut st = AppState::new(alias.into());
        let mirror = Arc::new(SubMirror::default());
        st.sub_mirror = Some(mirror.clone());
        let tenant = TenantId::new("default").expect("tenant");
        (st, mirror, tenant)
    }

    /// 5.8.6: the periodic Notification carries "all the subscribed Entities
    /// that match the query, geoquery and Scope query conditions" — the one
    /// Entity this selector names out of a populated tenant, and nothing
    /// else. And for the subscription whose selector matches nothing: "If
    /// there are no matching Entities, no Notification is sent."
    #[tokio::test(flavor = "multi_thread")]
    async fn periodic_sweep_notifies_only_the_subscribed_entities() {
        crate::allow_private();
        let (uri, seen) = recording_endpoint().await;
        let (st, _mirror, tenant) = state_with_mirror("antares-periodic-narrowing");
        for (id, ty) in [
            ("urn:ngsi-ld:Vehicle:1", "Vehicle"),
            ("urn:ngsi-ld:Vehicle:2", "Vehicle"),
            ("urn:ngsi-ld:Vehicle:3", "Vehicle"),
            ("urn:ngsi-ld:Building:1", "Building"),
        ] {
            st.store
                .create(&tenant, Kind::Entity, id, entity(id, ty))
                .expect("entity row");
        }
        install(
            &st,
            &tenant,
            &periodic(
                "urn:ngsi-ld:Subscription:one",
                &uri,
                json!([{"id": "urn:ngsi-ld:Vehicle:2", "type": format!("{DC}/Vehicle")}]),
                10,
            ),
        );
        install(
            &st,
            &tenant,
            &periodic(
                "urn:ngsi-ld:Subscription:none",
                &uri,
                json!([{"id": "urn:ngsi-ld:Vehicle:404", "type": format!("{DC}/Vehicle")}]),
                10,
            ),
        );

        interval_tick(&st).await;

        let bodies = seen.lock().expect("recorded bodies").clone();
        assert_eq!(bodies.len(), 1, "exactly one subscription had a match");
        let body = &bodies[0];
        assert_eq!(
            body["subscriptionId"], "urn:ngsi-ld:Subscription:one",
            "a subscription matching no Entity sends no Notification"
        );
        let data = body["data"].as_array().expect("data array");
        assert_eq!(data.len(), 1, "only the subscribed Entity is included");
        assert_eq!(data[0]["id"], "urn:ngsi-ld:Vehicle:2");
        for other in [
            "urn:ngsi-ld:Vehicle:1",
            "urn:ngsi-ld:Vehicle:3",
            "urn:ngsi-ld:Building:1",
        ] {
            assert!(
                !body.to_string().contains(other),
                "{other} is not subscribed and must not appear in the notification"
            );
        }
    }

    /// The sweep clock: 5.8.6 sends the periodic Notification "when the time
    /// interval (in seconds) specified in such value field is reached", so a
    /// tick before the earliest such instant cannot fire and must not sweep —
    /// while writing a periodic subscription clears the clock, because a
    /// subscription the previous sweep never saw may be due sooner.
    #[tokio::test(flavor = "multi_thread")]
    async fn armed_sweep_clock_skips_the_tick_until_a_subscription_write_clears_it() {
        use std::sync::atomic::Ordering::Relaxed;
        crate::allow_private();
        let (uri, seen) = recording_endpoint().await;
        let (st, mirror, tenant) = state_with_mirror("antares-periodic-clock");
        st.store
            .create(
                &tenant,
                Kind::Entity,
                "urn:ngsi-ld:Vehicle:1",
                entity("urn:ngsi-ld:Vehicle:1", "Vehicle"),
            )
            .expect("entity row");
        let sub = periodic(
            "urn:ngsi-ld:Subscription:clock",
            &uri,
            json!([{"type": format!("{DC}/Vehicle")}]),
            10,
        );
        install(&st, &tenant, &sub);
        let armed = chrono::Utc::now().timestamp_millis() + 60_000;
        mirror.next_sub_sweep_ms.store(armed, Relaxed);

        interval_tick(&st).await;
        assert!(
            seen.lock().expect("recorded bodies").is_empty(),
            "a tick before the clock must not fire, due subscription or not"
        );
        assert_eq!(
            mirror.next_sub_sweep_ms.load(Relaxed),
            armed,
            "a skipped tick leaves the clock alone"
        );

        // what a subscription write does
        mirror.apply(
            tenant.as_str(),
            sub["id"].as_str().expect("sub id"),
            Some(sub.clone()),
        );
        assert_eq!(
            mirror.next_sub_sweep_ms.load(Relaxed),
            0,
            "writing a periodic subscription clears the sweep clock"
        );

        interval_tick(&st).await;
        assert_eq!(
            seen.lock().expect("recorded bodies").len(),
            1,
            "with the clock cleared the due subscription fires"
        );
        assert!(
            mirror.next_sub_sweep_ms.load(Relaxed) > chrono::Utc::now().timestamp_millis(),
            "after firing, the clock points at the next due instant"
        );
    }

    /// 5.11.7: a Context Source Registration Subscription with `timeInterval`
    /// fires periodically. Its writes reach the sweep as a signal rather than
    /// as a mirrored document, so a write must clear the clock the way a
    /// Subscription write does — otherwise a new one waits out a clock
    /// computed before it existed.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_written_csource_subscription_clears_the_csub_sweep_clock() {
        use std::sync::atomic::Ordering::Relaxed;
        let (_st, mirror, _tenant) = state_with_mirror("antares-csub-clock");
        let armed = chrono::Utc::now().timestamp_millis() + 600_000;
        mirror.next_csub_sweep_ms.store(armed, Relaxed);
        assert_eq!(
            mirror.next_csub_sweep_ms.load(Relaxed),
            armed,
            "the clock stands until something writes"
        );

        mirror.csub_written();

        assert_eq!(
            mirror.next_csub_sweep_ms.load(Relaxed),
            0,
            "writing a Context Source Registration Subscription clears the sweep clock"
        );
    }

    /// With nothing periodic to serve, the sweep must park past the next few
    /// ticks instead of re-listing every tenant's Context Source Registration
    /// Subscriptions every second: at the tenant target that poll is the
    /// broker's whole idle cost.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_sweep_that_finds_nothing_periodic_parks_the_csub_clock() {
        use std::sync::atomic::Ordering::Relaxed;
        let (st, mirror, _tenant) = state_with_mirror("antares-csub-park");
        mirror.next_csub_sweep_ms.store(0, Relaxed);

        interval_tick(&st).await;

        let parked = mirror.next_csub_sweep_ms.load(Relaxed);
        let now = chrono::Utc::now().timestamp_millis();
        assert!(
            parked > now + 1_000,
            "an idle sweep parked until {parked} ({} ms out) — the poll is still the fast path",
            parked - now
        );
    }
}

#[cfg(test)]
mod clause_5_8_6_grouped_delivery {
    use super::*;
    use serde_json::json;
    use tower::ServiceExt as _;

    async fn post(st: &AppState, uri: &str, body: Value) -> u16 {
        let body = body.to_string();
        crate::router(st.clone())
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri(uri)
                    .header("Content-Type", "application/json")
                    .header("Content-Length", body.len())
                    .body(axum::body::Body::from(body))
                    .expect("request"),
            )
            .await
            .expect("response")
            .status()
            .as_u16()
    }

    async fn recording_endpoint() -> (String, Arc<std::sync::Mutex<Vec<Value>>>) {
        let seen: Arc<std::sync::Mutex<Vec<Value>>> = Arc::default();
        let sink = seen.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let app = axum::Router::new().route(
            "/notify",
            axum::routing::post(move |body: String| {
                let sink = sink.clone();
                async move {
                    if let Ok(v) = serde_json::from_str::<Value>(&body) {
                        sink.lock().expect("recorded bodies").push(v);
                    }
                    axum::http::StatusCode::OK
                }
            }),
        );
        tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });
        (format!("http://{addr}/notify"), seen)
    }

    /// 5.8.6: one batch request writing N matching entities is ONE
    /// notification whose `data` carries the N entities — not N
    /// notifications — and Table 5.2.14.2-1 `timesSent` moves by one.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_batch_of_matching_entities_is_one_notification() {
        crate::allow_private();
        let (uri, seen) = recording_endpoint().await;
        let mut st = AppState::new("antares-grouped-delivery".into());
        wire(&mut st);
        assert_eq!(
            post(
                &st,
                "/ngsi-ld/v1/subscriptions",
                json!({
                    "id": "urn:ngsi-ld:Subscription:grouped",
                    "type": "Subscription",
                    "entities": [{"type": "Vehicle"}],
                    "notification": {"endpoint": {"uri": uri, "accept": "application/json"}},
                }),
            )
            .await,
            201
        );
        let batch: Vec<Value> = (1..=3)
            .map(|i| {
                json!({"id": format!("urn:ngsi-ld:Vehicle:{i}"), "type": "Vehicle",
                       "speed": {"type": "Property", "value": i}})
            })
            .collect();
        assert_eq!(
            post(&st, "/ngsi-ld/v1/entityOperations/create", json!(batch)).await,
            201
        );
        for _ in 0..600 {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            if !seen.lock().expect("bodies").is_empty() {
                break;
            }
        }
        // settle: a second POST, if the broker were still splitting, lands here
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let bodies = seen.lock().expect("bodies").clone();
        assert_eq!(bodies.len(), 1, "one POST for the batch, got {bodies:?}");
        let data = bodies[0]["data"].as_array().expect("data array");
        let mut ids: Vec<&str> = data.iter().filter_map(|e| e["id"].as_str()).collect();
        ids.sort_unstable();
        assert_eq!(
            ids,
            [
                "urn:ngsi-ld:Vehicle:1",
                "urn:ngsi-ld:Vehicle:2",
                "urn:ngsi-ld:Vehicle:3"
            ]
        );
        let tenant = TenantId::new("default").expect("tenant");
        let sub = st
            .store
            .get(
                &tenant,
                Kind::Subscription,
                "urn:ngsi-ld:Subscription:grouped",
            )
            .expect("store")
            .expect("row");
        assert_eq!(sub["notification"]["timesSent"], json!(1));
    }
}

#[cfg(test)]
mod notification_body_bound {
    use super::*;
    use serde_json::json;

    /// A grouped notification is cut into whole-entity runs under the byte
    /// cap; nothing is split mid-entity and an oversize single entity still
    /// travels (alone) instead of being dropped.
    #[test]
    fn chunks_cut_at_whole_items_under_the_cap() {
        let item = |i: usize| json!({"id": format!("urn:ngsi-ld:V:{i}"), "v": "x".repeat(40)});
        let one = serde_json::to_vec(&item(0)).expect("json").len();
        let runs = chunk_by_bytes((0..5).map(item).collect(), one * 2);
        assert_eq!(runs.iter().map(Vec::len).collect::<Vec<_>>(), [2, 2, 1]);
        assert_eq!(runs[2][0]["id"], json!("urn:ngsi-ld:V:4"));
        let runs = chunk_by_bytes(vec![item(0), item(1)], one / 2);
        assert_eq!(runs.len(), 2, "an over-cap item is its own run, never lost");
        assert!(chunk_by_bytes(Vec::new(), 1).is_empty());
    }
}

#[cfg(test)]
mod interval_claim {
    use super::*;
    use serde_json::json;

    fn seed(st: &AppState, tenant: &TenantId) {
        st.store
            .create(
                tenant,
                Kind::Subscription,
                "urn:ngsi-ld:Subscription:tick",
                json!({
                    "id": "urn:ngsi-ld:Subscription:tick",
                    "type": "Subscription",
                    "entities": [{"type": "https://uri.etsi.org/ngsi-ld/default-context/Vehicle"}],
                    "timeInterval": 1,
                    "status": "active",
                    "createdAt": "2020-01-01T00:00:00Z",
                    "notification": {"endpoint": {"uri": "http://127.0.0.1:9/notify"}},
                }),
            )
            .expect("seed");
    }

    fn stamp(st: &AppState, tenant: &TenantId) -> Option<String> {
        st.store
            .get(tenant, Kind::Subscription, "urn:ngsi-ld:Subscription:tick")
            .expect("store")
            .expect("row")["notification"]["lastNotification"]
            .as_str()
            .map(str::to_owned)
    }

    /// 5.8.6: "If there are no matching Entities, no Notification is sent",
    /// and Table 5.2.14.2-1 makes `lastNotification` "the timestamp
    /// corresponding to the instant when the last notification was sent".
    /// The multi-pod claim stamps that member to win the firing, so a due
    /// subscription that then matches nothing must have the stamp put back:
    /// otherwise a client reads a notification instant for a notification
    /// that never happened, and the subscription — still owing its firing —
    /// waits out a whole interval that the single-process path does not.
    #[tokio::test]
    async fn a_claimed_firing_that_matches_nothing_leaves_no_notification_instant() {
        let tenant = TenantId::new("default").expect("tenant");
        for nats in [false, true] {
            let mut st = AppState::new("me".into());
            st.nats = nats;
            seed(&st, &tenant);
            interval_tick(&st).await;
            assert_eq!(
                stamp(&st, &tenant),
                None,
                "nats={nats}: nothing matched, so nothing was sent"
            );
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod change_grouping {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc as StdArc;

    /// 5.8.6: "the Notification … data … shall contain the Entities that
    /// match" — every change of one drain that matches the same Subscription
    /// travels in ONE notification. Grouping is what makes a batch of N
    /// writes one POST with N data entries instead of N POSTs, and it has to
    /// hold when many Subscriptions match the same change: each of them gets
    /// exactly one notification carrying every entity, and no entity lands on
    /// the wrong Subscription.
    #[tokio::test(flavor = "multi_thread")]
    async fn every_change_of_a_drain_reaches_each_matching_subscription_once() {
        crate::allow_private();
        let mut st = AppState::new("antares-grouping-test".into());
        wire(&mut st);
        let tenant = TenantId::new("default").expect("tenant");
        let posts: StdArc<AtomicUsize> = StdArc::default();
        let entities: StdArc<std::sync::Mutex<Vec<usize>>> = StdArc::default();
        let (p, e) = (posts.clone(), entities.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let app = axum::Router::new().route(
            "/notify",
            axum::routing::post(move |body: String| {
                let (p, e) = (p.clone(), e.clone());
                async move {
                    p.fetch_add(1, Ordering::SeqCst);
                    let v: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
                    e.lock()
                        .expect("seen")
                        .push(v["data"].as_array().map(Vec::len).unwrap_or(0));
                    axum::http::StatusCode::OK
                }
            }),
        );
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });

        const SUBS: usize = 8;
        const CHANGES: usize = 5;
        for i in 0..SUBS {
            let sub = json!({
                "id": format!("urn:ngsi-ld:Subscription:group-{i}"),
                "type": "Subscription",
                "status": "active",
                "entities": [{"type": "https://uri.etsi.org/ngsi-ld/default-context/Vehicle"}],
                "notification": {"endpoint": {"uri": format!("http://{addr}/notify")}},
            });
            st.store
                .create(
                    &tenant,
                    Kind::Subscription,
                    &format!("urn:ngsi-ld:Subscription:group-{i}"),
                    sub.clone(),
                )
                .expect("seed subscription");
            if let Some(m) = &st.sub_mirror {
                m.apply(
                    tenant.as_str(),
                    &format!("urn:ngsi-ld:Subscription:group-{i}"),
                    Some(sub),
                );
            }
        }
        let changes: Vec<Change> = (0..CHANGES)
            .map(|i| {
                (
                    tenant.as_str().to_owned(),
                    None,
                    Some(json!({
                        "id": format!("urn:ngsi-ld:Vehicle:{i}"),
                        "type": ["https://uri.etsi.org/ngsi-ld/default-context/Vehicle"],
                        "https://uri.etsi.org/ngsi-ld/default-context/speed": [
                            {"type": "Property", "value": i}
                        ],
                    })),
                )
            })
            .collect();
        process_changes(&st, changes).await;
        assert_eq!(
            posts.load(Ordering::SeqCst),
            SUBS,
            "one notification per matching subscription, never one per change"
        );
        let sizes = entities.lock().expect("seen").clone();
        assert_eq!(
            sizes,
            vec![CHANGES; SUBS],
            "each notification carries every entity of the drain"
        );
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod interval_sweep_concurrency {
    use super::*;

    /// 5.8.6 sends a periodic Notification "when the time interval … is
    /// reached". One sweep visits every tenant and every due Subscription, so
    /// whatever it does per Subscription it does 10 000 tenants' worth of —
    /// and a notification endpoint may take its whole `endpoint.timeout` to
    /// answer (Table 5.2.15-1). Awaiting each delivery in turn makes one
    /// unresponsive endpoint the deadline of every other subscriber's
    /// periodic notification, on a broker whose targets are 10 000 tenants
    /// and 100 000 subscriptions.
    #[tokio::test(flavor = "multi_thread")]
    async fn one_unresponsive_endpoint_does_not_hold_up_the_other_subscriptions() {
        crate::allow_private();
        // a listener that accepts and never answers: every delivery to it
        // costs exactly its endpoint timeout
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((s, _)) = listener.accept().await {
                held.push(s);
            }
        });
        // no mirror: the sweep reads the subscriptions from the store, which
        // is where this test seeds them
        let st = AppState::new("antares-sweep-test".into());
        let tenant = TenantId::new("default").expect("tenant");
        st.store
            .create(
                &tenant,
                Kind::Entity,
                "urn:ngsi-ld:Vehicle:sweep",
                json!({
                    "id": "urn:ngsi-ld:Vehicle:sweep",
                    "type": ["https://uri.etsi.org/ngsi-ld/default-context/Vehicle"],
                }),
            )
            .expect("seed entity");
        const SUBS: u32 = 4;
        const TIMEOUT_MS: u64 = 300;
        for i in 0..SUBS {
            let id = format!("urn:ngsi-ld:Subscription:sweep-{i}");
            st.store
                .create(
                    &tenant,
                    Kind::Subscription,
                    &id,
                    json!({
                        "id": id,
                        "type": "Subscription",
                        "status": "active",
                        "timeInterval": 1,
                        "createdAt": "2020-01-01T00:00:00Z",
                        "entities": [{
                            "type": "https://uri.etsi.org/ngsi-ld/default-context/Vehicle"
                        }],
                        "notification": {"endpoint": {
                            "uri": format!("http://{addr}/notify"),
                            "timeout": TIMEOUT_MS,
                        }},
                    }),
                )
                .expect("seed subscription");
        }
        let started = std::time::Instant::now();
        interval_tick(&st).await;
        let elapsed = started.elapsed().as_millis() as u64;
        let serial = TIMEOUT_MS * u64::from(SUBS);
        assert!(
            elapsed < serial * crate::state::slow_factor(),
            "the sweep took {elapsed} ms — {SUBS} deliveries of {TIMEOUT_MS} ms ran one after \
             the other instead of together"
        );
    }
}
