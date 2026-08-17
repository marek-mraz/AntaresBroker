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
use antares_model::TenantId;
use antares_sql::store::Kind;
use serde_json::{json, Map, Value};
use std::sync::Arc;

const DEFAULT_TRIGGERS: &[&str] = &["attributeCreated", "attributeUpdated"];
/// Depth of the change→matcher queue, the same ring size the local bus uses.
const CHANGE_QUEUE: usize = 1024;
static CHANGES_DROPPED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static TASK_PANICS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

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
const ENTITY_META: &[&str] = &[
    "id",
    "type",
    "scope",
    "createdAt",
    "modifiedAt",
    "deletedAt",
    "expiresAt",
    "@context",
];

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
    /// (5.11.7). Their writes reach no mirror, so a sweep never trusts this
    /// one for longer than `CSUB_SWEEP_HORIZON_MS`.
    next_csub_sweep_ms: std::sync::atomic::AtomicI64,
}

/// How long a sweep may skip the Context Source Registration Subscription
/// half of the tick. A newly created one cannot be due sooner than its own
/// `timeInterval` after creation, and the smallest interval the API defines
/// is one second (`Subscription.Periodic.timeInterval`, minimum 1), so a
/// second of blindness cannot delay a firing. Table 5.2.12-1 only bounds
/// `timeInterval` by "greater than 0": for a sub-second one created between
/// sweeps, this is the worst-case delay of its FIRST firing.
const CSUB_SWEEP_HORIZON_MS: i64 = 1000;

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
        let mut map = self.map.write().expect("sub mirror lock");
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

    /// The hot path: subscriptions that could possibly fire for a change
    /// touching these entity types and these changed attributes. Union of
    /// the type hits, the attr hits and the broad bucket — a superset of
    /// the firing set, never a subset.
    pub fn candidates(&self, tenant: &str, types: &[&str], changed_attrs: &[&str]) -> Vec<Value> {
        let map = self.map.read().expect("sub mirror lock");
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
            .expect("sub mirror lock")
            .get(tenant)
            .map(|t| t.docs.values().cloned().collect())
            .unwrap_or_default()
    }

    /// The interval sweep's whole input: the tenant's periodic (5.2.12
    /// `timeInterval`) subscriptions. `docs` clones every subscription of the
    /// tenant, which at 100 000 subscriptions is the sweep's dominant cost
    /// even on a tick where nothing is due.
    fn periodic_docs(&self, tenant: &str) -> Vec<Value> {
        self.map
            .read()
            .expect("sub mirror lock")
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
            .expect("sub mirror lock")
            .keys()
            .cloned()
            .collect()
    }
}

impl Mirror for SubMirror {
    fn apply(&self, tenant: &str, id: &str, doc: Option<Value>) {
        SubMirror::apply(self, tenant, id, doc);
    }
}

impl DocMirror {
    /// Apply one KV delta: `None` doc = deleted.
    pub fn apply(&self, tenant: &str, id: &str, doc: Option<Value>) {
        let mut map = self.map.write().expect("sub mirror lock");
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
            .expect("sub mirror lock")
            .get(tenant)
            .map(|t| t.values().cloned().collect())
            .unwrap_or_default()
    }

    pub fn tenants(&self) -> Vec<String> {
        self.map
            .read()
            .expect("sub mirror lock")
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
        None => st
            .store
            .list(tenant, Kind::Subscription)
            .unwrap_or_default(),
    }
}

/// Wire the store hook and background tasks. Call once at startup.
pub fn wire(state: &mut AppState) {
    // bus=local: the same indexed mirror the nats wiring builds, fed
    // synchronously by the CUD hook — the matcher never rescans the store.
    let mirror = Arc::new(SubMirror::default());
    for tenant_str in state.store.subscription_tenants().unwrap_or_default() {
        if let Ok(tenant) = TenantId::new(&tenant_str) {
            for doc in state
                .store
                .list(&tenant, Kind::Subscription)
                .unwrap_or_default()
            {
                if let Some(id) = doc.get("id").and_then(Value::as_str) {
                    let id = id.to_owned();
                    mirror.apply(&tenant_str, &id, Some(doc));
                }
            }
        }
    }
    state.sub_mirror = Some(mirror.clone());
    let m = mirror.clone();
    state.sub_sync = Some(Arc::new(move |tenant: &TenantId, id: &str, doc| {
        m.apply(tenant.as_str(), id, doc.cloned());
    }));

    // The queue carries whole before+after payloads and is drained one
    // inline delivery at a time, so behind one slow subscriber an unbounded
    // queue grows until the process dies. Bounded instead: a full queue drops
    // the change and counts it.
    let (tx, mut rx) =
        tokio::sync::mpsc::channel::<(String, Option<Value>, Option<Value>)>(CHANGE_QUEUE);
    // Temporal auto-recording runs SYNCHRONOUSLY on the hook (read-your-writes:
    // the ETSI suite queries history immediately after a write); the matcher
    // work is handed to the async task below. One choke point for every write.
    let st_rec = state.clone();
    state
        .store
        .set_change_hook(Box::new(move |tenant, before, after| {
            record_temporal_change(&st_rec, tenant, before.as_ref(), after.as_ref());
            if tx
                .try_send((tenant.as_str().to_owned(), before, after))
                .is_err()
            {
                CHANGES_DROPPED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                metrics::counter!("antares_notification_changes_dropped_total").increment(1);
            }
        }));
    let st = state.clone();
    crate::spawn(async move {
        while let Some((tenant, before, after)) = rx.recv().await {
            let st = st.clone();
            guarded(async move { process_change(&st, &tenant, before, after).await }).await;
        }
    });
    let st = state.clone();
    crate::spawn(async move {
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
        TASK_PANICS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        metrics::counter!("antares_notification_task_panics_total").increment(1);
        tracing::error!("notification pipeline task panicked; this change is lost");
    }
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
                .filter(|k| !ENTITY_META.contains(&k.as_str()))
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
    if !st.record_locally {
        return;
    }
    let Some(after) = after else {
        return; // entity deletion — handled by mirror_delete_entity
    };
    let Some(id) = after.get("id").and_then(Value::as_str) else {
        return;
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
            let r = st.store.mutate(tenant, Kind::Temporal, id, |doc| {
                let target = doc.as_object_mut().ok_or(())?;
                match target.get_mut("scope").and_then(Value::as_array_mut) {
                    Some(arr) if arr.first().is_some_and(Value::is_object) => {
                        arr.push(inst.clone());
                    }
                    _ => {
                        target.insert("scope".into(), Value::Array(vec![inst.clone()]));
                    }
                }
                Ok::<(), ()>(())
            });
            if let Err(e) = r {
                tracing::warn!("temporal scope mirror failed: {e}");
            }
        }
    }
    let mut additions = Map::new();
    for (k, class) in diff(before, Some(after)) {
        if !matches!(class, ChangeClass::Created | ChangeClass::Updated) {
            continue; // attribute deletion — handled by mirror_delete_attr
        }
        let Some(av) = after.get(&k) else { continue };
        let mut incoming = changed_instances(before.and_then(|b| b.get(&k)), av);
        if incoming.is_empty() {
            continue;
        }
        for inst in &mut incoming {
            if let Some(o) = inst.as_object_mut() {
                o.entry("instanceId".to_owned()).or_insert_with(|| {
                    Value::String(format!("urn:ngsi-ld:Instance:{}", uuid::Uuid::new_v4()))
                });
            }
        }
        additions.insert(k, Value::Array(incoming));
    }
    if additions.is_empty() {
        return;
    }
    let mut shell = Map::new();
    for k in ["id", "type", "createdAt", "modifiedAt", "scope"] {
        if let Some(v) = after.get(k) {
            shell.insert(k.to_string(), v.clone());
        }
    }
    if let Err(e) =
        st.store
            .temporal_append(tenant, id, &Value::Object(shell), &Value::Object(additions))
    {
        tracing::warn!("temporal auto-record failed: {e}");
    }
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

fn sub_str<'a>(sub: &'a Value, key: &str) -> Option<&'a str> {
    sub.get(key).and_then(Value::as_str)
}

fn is_active(sub: &Value) -> bool {
    if sub.get("isActive") == Some(&Value::Bool(false)) {
        return false;
    }
    // 5.8.1.4 auto-expiry; dt_key so fraction spellings cannot misorder
    // around the boundary second (4.11)
    !sub.get("expiresAt")
        .and_then(Value::as_str)
        .is_some_and(|e| crate::temporal::dt_key(e) < crate::temporal::dt_key(&now_iso()))
}

/// entities selector (5.2.33) against an internal entity doc.
pub(crate) fn selector_match(sub: &Value, doc: &Value, ctx: &Context) -> bool {
    let Some(sel) = sub.get("entities").and_then(Value::as_array) else {
        return true; // watchedAttributes-only subscription
    };
    let types: Vec<&str> = doc
        .get("type")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let id = doc.get("id").and_then(Value::as_str).unwrap_or("");
    sel.iter().any(|e| {
        let t_ok = e.get("type").and_then(Value::as_str).is_none_or(|t| {
            if t.contains(['|', ',', ';', '(']) {
                crate::entities::type_selection_matches(t, &types, ctx)
            } else {
                types.contains(&t)
            }
        });
        // Table 5.2.33-1: id is String or String[]; "id takes precedence
        // over idPattern" — a selector carrying id ignores its idPattern.
        let id_ok = match e.get("id") {
            None => true,
            Some(Value::String(i)) => i == id,
            Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).any(|i| i == id),
            Some(_) => false,
        };
        let pat_ok = e.get("id").is_some()
            || e.get("idPattern").and_then(Value::as_str).is_none_or(|p| {
                crate::regexcache::compile(p).is_ok_and(|re| re.find(id).is_some())
            });
        t_ok && id_ok && pat_ok
    })
}

/// A subscription's `geoQ` (Table 5.2.13-1) in the parameter shape the 4.10
/// GeoQuery parser takes.
fn geo_params(g: &Map<String, Value>) -> std::collections::HashMap<String, String> {
    let mut params: std::collections::HashMap<String, String> = Default::default();
    for k in ["georel", "geometry", "geoproperty"] {
        if let Some(s) = g.get(k).and_then(Value::as_str) {
            params.insert(k.into(), s.to_owned());
        }
    }
    if let Some(c) = g.get("coordinates") {
        params.insert(
            "coordinates".into(),
            match c {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            },
        );
    }
    params
}

/// q / scopeQ / geoQ conditions against an internal entity doc.
pub(crate) fn conditions_match(
    st: &AppState,
    tenant: &TenantId,
    sub: &Value,
    doc: &Value,
    ctx: &Context,
) -> bool {
    if let Some(q) = sub_str(sub, "q") {
        // q values in subscription bodies may be percent-encoded (4.9, 046_05)
        let q = crate::negotiate::percent_decode(q.as_bytes());
        // parsed once per distinct q text, not once per event per candidate
        match crate::regexcache::q_node(&q) {
            Some(node) => {
                // 4.9 EXAMPLE 13/14: linked-entity q terms (attr{path})
                // resolve through the local store, same tenant
                let lookup = |uri: &str| st.store.get(tenant, Kind::Entity, uri).ok().flatten();
                if !crate::qeval::eval_q(&node, doc, ctx, &lookup) {
                    return false;
                }
            }
            None => return false,
        }
    }
    if let Some(sq) = sub_str(sub, "scopeQ") {
        if !crate::scope_matches(sq, doc) {
            return false;
        }
    }
    if let Some(g) = sub.get("geoQ").and_then(Value::as_object) {
        // the geometry parse is shared per distinct geoQ member; the
        // serialization of the stored member is the key
        let key = serde_json::to_string(g).unwrap_or_default();
        let gq = crate::regexcache::geo_query(&key, || {
            crate::geo::GeoQuery::from_params(&geo_params(g))
                .ok()
                .flatten()
        });
        match gq {
            Some(gq) => {
                if !gq.matches(doc, ctx) {
                    return false;
                }
            }
            None => return false,
        }
    }
    true
}

fn throttled(sub: &Value) -> bool {
    let Some(secs) = sub.get("throttling").and_then(Value::as_f64) else {
        return false;
    };
    let Some(last) = sub
        .get("notification")
        .and_then(|n| n.get("lastNotification"))
        .and_then(Value::as_str)
    else {
        return false;
    };
    chrono::DateTime::parse_from_rfc3339(last).is_ok_and(|t| {
        (chrono::Utc::now() - t.with_timezone(&chrono::Utc)).num_milliseconds()
            < (secs * 1000.0) as i64
    })
}

/// The @context governing a subscription's notifications (5.8.6): the
/// jsonldContext member if set, else the @context of the creating request.
pub(crate) async fn sub_context(st: &AppState, sub: &Value) -> Arc<Context> {
    let source = sub
        .get("jsonldContext")
        .cloned()
        .or_else(|| sub.get("__context").cloned());
    match source {
        Some(v) if !v.is_null() => st
            .loader
            .resolve_quiet(&v)
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
    let nodes = |list: Vec<String>| -> Vec<crate::repr::ProjNode> {
        list.into_iter()
            .map(|raw| crate::repr::ProjNode {
                iri: ctx.expand_key(&raw),
                raw,
                children: None,
            })
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
            let level = n
                .and_then(|n| n.get("joinLevel"))
                .and_then(Value::as_u64)
                .unwrap_or(1) as usize;
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
#[allow(clippy::too_many_arguments)]
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
            let obj = doc.as_object_mut().expect("entity object");
            if show {
                // previous* on changed instances (046_31..33)
                if let Some(b) = before {
                    for (k, v) in obj.iter_mut() {
                        if ENTITY_META.contains(&k.as_str()) {
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
                    if let Some(arr) = doc
                        .as_object_mut()
                        .expect("entity object")
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
            // entity deleted: tombstone entity (046_21) + per-trigger attrs
            let b = before.expect("deletion carries before");
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

pub async fn process_change(
    st: &AppState,
    tenant_str: &str,
    before: Option<Value>,
    after: Option<Value>,
) {
    let Ok(tenant) = TenantId::new(tenant_str) else {
        return;
    };
    let changes = diff(before.as_ref(), after.as_ref());
    let entity_trigger = match (&before, &after) {
        (None, Some(_)) => "entityCreated",
        (Some(_), None) => "entityDeleted",
        _ => "entityUpdated",
    };
    let eval_doc = after.as_ref().or(before.as_ref());
    let Some(eval_doc) = eval_doc else { return };
    // Candidate lookup by the entity's types and the changed attribute
    // IRIs — no linear scan over all subscriptions.
    let types: Vec<&str> = eval_doc
        .get("type")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let changed_keys: Vec<&str> = changes.iter().map(|(k, _)| k.as_str()).collect();
    let subs = subs_for(st, &tenant, &types, &changed_keys);
    if subs.is_empty() {
        return;
    }
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
        let ctx = sub_context(st, &sub).await;
        if !selector_match(&sub, eval_doc, &ctx) {
            continue;
        }
        if !conditions_match(st, &tenant, &sub, eval_doc, &ctx) {
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
        deliver(st, &tenant, &sub, data, &ctx).await;
    }
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
        Some(last) => last.timestamp_millis() + (interval * 1000.0) as i64,
        None => i64::MIN,
    }
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

/// timeInterval subscriptions: fire when due, with all matching entities.
/// Multi-instance: claim one interval firing under the subscription row
/// lock — N matcher pods race, exactly one wins (single-winner by
/// lock, no leader election). The due-check reruns INSIDE the lock; the
/// winner stamps `lastNotification` as its claim, losers see not-due and
/// roll back. Only engaged in bus=nats mode — single-process behaviour (and
/// its 046_12 bookkeeping ordering) is untouched.
fn claim_interval(
    st: &AppState,
    tenant: &TenantId,
    kind: Kind,
    sub: &Value,
    interval: f64,
) -> bool {
    let Some(id) = sub.get("id").and_then(Value::as_str) else {
        return false;
    };
    let res = st.store.mutate(tenant, kind, id, |doc| {
        if chrono::Utc::now().timestamp_millis() < due_at_ms(doc, interval) {
            return Err(());
        }
        if let Some(n) = doc
            .as_object_mut()
            .expect("subscription object")
            .entry("notification")
            .or_insert_with(|| json!({}))
            .as_object_mut()
        {
            n.insert("lastNotification".into(), Value::String(now_iso()));
        }
        Ok(())
    });
    matches!(res, Ok(Some(Ok(()))))
}

/// One sweep of the interval subscriptions (5.8.6, 5.11.7): "If a
/// Subscription defines a timeInterval member, a Notification shall be sent
/// periodically, when the time interval (in seconds) specified in such value
/// field is reached, regardless of Attribute changes."
///
/// The sweep runs on a fixed tick, so its idle cost is what has to stay
/// small. Two things keep it off the store. A tick that cannot fire anything
/// returns before enumerating tenants: each sweep records the earliest instant
/// a subscription it saw can next be due, subscription writes zero that clock
/// through the mirror, and the Context Source Registration Subscription half —
/// whose writes reach no mirror — is never skipped for longer than
/// `CSUB_SWEEP_HORIZON_MS`. And a due subscription reads only the Entities its
/// own selector can match instead of its tenant's entity set.
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
    for tenant_str in st.store.subscription_tenants().unwrap_or_default() {
        let Ok(tenant) = TenantId::new(&tenant_str) else {
            continue;
        };
        // Same source the matcher reads: the indexed mirror, with the store
        // list only as the never-wired fallback.
        let subs = match (&st.sub_mirror, sweep_subs) {
            (_, false) => Vec::new(),
            (Some(m), _) => m.periodic_docs(tenant.as_str()),
            (None, _) => st
                .store
                .list(&tenant, Kind::Subscription)
                .unwrap_or_default(),
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
            next_sub = next_sub.min(now_ms + (interval * 1000.0) as i64);
            if st.nats && !claim_interval(st, &tenant, Kind::Subscription, &sub, interval) {
                continue;
            }
            let ctx = sub_context(st, &sub).await;
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
                let filter = antares_sql::store::filter::EntityFilter {
                    ids: ids.as_ref().map(|_| id_refs.as_slice()),
                    types: (!type_groups.is_empty()).then_some(type_groups.as_slice()),
                    q: q_ast.as_deref(),
                    scope_q: sub_str(&sub, "scopeQ"),
                    geo: geo_spec.as_ref(),
                    expand: &expand,
                    ..Default::default()
                };
                st.store
                    .query_entities(&tenant, &filter)
                    .map(|o| o.rows)
                    .unwrap_or_default()
            };
            let matching: Vec<Value> = rows
                .into_iter()
                .filter(|d| {
                    selector_match(&sub, d, &ctx) && conditions_match(st, &tenant, &sub, d, &ctx)
                })
                .flat_map(|d| build_data(st, &tenant, &sub, &ctx, None, Some(&d), &[], false, &now))
                .collect();
            if matching.is_empty() {
                // 5.8.6: "If there are no matching Entities, no Notification
                // is sent" — lastNotification stays untouched, so this
                // subscription is still due and every following tick
                // re-checks it.
                next_sub = next_sub.min(due_at);
                continue;
            }
            deliver(st, &tenant, &sub, matching, &ctx).await;
        }
        if !sweep_csubs {
            continue;
        }
        // csource timeInterval subs: periodic CSourceNotification with all
        // matching registrations, independent of changes (5.11.7)
        for sub in st
            .store
            .list(&tenant, Kind::CSourceSubscription)
            .unwrap_or_default()
        {
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
            next_csub = next_csub.min(now_ms + (interval * 1000.0) as i64);
            if st.nats && !claim_interval(st, &tenant, Kind::CSourceSubscription, &sub, interval) {
                continue;
            }
            let ctx = sub_context(st, &sub).await;
            let spec = crate::csource::spec_for_subscription(&sub);
            let data: Vec<Value> = st
                .store
                .list(&tenant, Kind::Registration)
                .unwrap_or_default()
                .into_iter()
                .filter(|r| crate::csource::csr_matches_subscription(&sub, r, &ctx))
                .map(|r| {
                    let mut p = crate::csource::present_registration(
                        &filter_csr(&spec, &r, &ctx),
                        &ctx,
                        false,
                    );
                    arrayify_entity_types(&mut p);
                    p
                })
                .collect();
            deliver_as(
                st,
                &tenant,
                Kind::CSourceSubscription,
                &sub,
                "ContextSourceNotification",
                data,
                &ctx,
                Some("newlyMatching"),
            )
            .await;
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
                next_csub.min(now_ms + CSUB_SWEEP_HORIZON_MS),
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
    deliver_as(
        st,
        tenant,
        Kind::Subscription,
        sub,
        "Notification",
        data,
        ctx,
        None,
    )
    .await
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
    for sub in st
        .store
        .list(tenant, Kind::CSourceSubscription)
        .unwrap_or_default()
    {
        if !is_active(&sub) || sub.get("timeInterval").is_some() {
            continue;
        }
        let ctx = sub_context(st, &sub).await;
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
        if !matches!(
            st.store.get(tenant, Kind::CSourceSubscription, sub_id),
            Ok(Some(_))
        ) {
            continue;
        }
        deliver_as(
            st,
            tenant,
            Kind::CSourceSubscription,
            &job.sub,
            "ContextSourceNotification",
            vec![job.presented],
            &job.ctx,
            Some(job.reason),
        )
        .await;
    }
}

/// Compatibility wrapper: prepare + send in one call (spawned contexts that
/// don't need the phase split).
pub async fn csource_changed(
    st: &AppState,
    tenant: &TenantId,
    before: Option<Value>,
    after: Option<Value>,
) {
    let jobs = prepare_csource_jobs(st, tenant, before, after).await;
    send_csource_jobs(st, tenant, jobs).await;
}

/// Initial / post-update CSourceNotification with all currently matching
/// registrations (5.11.2.4 / 5.11.3.4).
pub async fn csource_initial(st: &AppState, tenant: &TenantId, sub_id: &str) {
    let Some(sub) = st
        .store
        .get(tenant, Kind::CSourceSubscription, sub_id)
        .ok()
        .flatten()
    else {
        return;
    };
    if !is_active(&sub) {
        return;
    }
    let ctx = sub_context(st, &sub).await;
    let spec = crate::csource::spec_for_subscription(&sub);
    let data: Vec<Value> = st
        .store
        .list(tenant, Kind::Registration)
        .unwrap_or_default()
        .into_iter()
        .filter(|r| crate::csource::csr_matches_subscription(&sub, r, &ctx))
        .map(|r| {
            let mut p =
                crate::csource::present_registration(&filter_csr(&spec, &r, &ctx), &ctx, false);
            arrayify_entity_types(&mut p);
            p
        })
        .collect();
    if data.is_empty() {
        return; // nothing currently matching ⇒ no initial notification
    }
    deliver_as(
        st,
        tenant,
        Kind::CSourceSubscription,
        &sub,
        "ContextSourceNotification",
        data,
        &ctx,
        Some("newlyMatching"),
    )
    .await;
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

#[allow(clippy::too_many_arguments)]
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
    let is_mqtt = uri.starts_with("mqtt://") || uri.starts_with("mqtts://");
    if !uri.starts_with("http") && !is_mqtt {
        return; // creation rejects unknown schemes with 422; belt only
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
    if st.egress.is_open(uri) {
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
        let mut fc = crate::entities::to_geojson_collection(entities, None, ctx);
        if !prefer_body_json {
            fc["@context"] = crate::negotiate::served_context(ctx);
        }
        body["data"] = fc;
    }
    let receiver_info: Vec<(String, String)> = ep
        .get("receiverInfo")
        .and_then(Value::as_array)
        .map(|ri| {
            ri.iter()
                .filter_map(|kv| {
                    Some((
                        kv.get("key")?.as_str()?.to_owned(),
                        kv.get("value")?.as_str()?.to_owned(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();

    // 6.3.22: a subscription living under a snapshot's synthetic tenant
    // notifies with the NGSILD-Snapshot header and the OWNER tenant — the
    // internal "snap-…" tenant never leaks.
    let (hdr_tenant, snapshot_id) = match crate::snapshots::snapshot_of_synth(st, tenant.as_str()) {
        Some((owner, sid)) => (owner, Some(sid)),
        None => (tenant.clone(), None),
    };

    // Per-binding send, prepared BEFORE the bookkeeping writeback so the
    // optimistic stamp covers only the in-flight attempt (046_12_01 race).
    enum Outbound {
        Http(reqwest::RequestBuilder, Vec<u8>),
        #[cfg(feature = "mqtt")]
        Mqtt(
            antares_notifier::mqtt::MqttEndpoint,
            antares_notifier::mqtt::MqttParams,
            Vec<u8>,
        ),
    }
    let outbound = if is_mqtt {
        #[cfg(feature = "mqtt")]
        {
            use antares_notifier::mqtt::{build_message, MqttEndpoint, MqttParams};
            // Creation validated both (5.2.15 / Table 7.2-1); a parse failure
            // here means a hand-edited row — log, count as delivery failure.
            let parsed = MqttEndpoint::parse(uri).and_then(|e| {
                let pairs = ep
                    .get("notifierInfo")
                    .and_then(Value::as_array)
                    .map(|ni| {
                        ni.iter()
                            .filter_map(|kv| {
                                Some((kv.get("key")?.as_str()?, kv.get("value")?.as_str()?))
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                Ok((e, MqttParams::from_notifier_info(pairs)?))
            });
            match parsed {
                Ok((endpoint, params)) => {
                    // 7.2 Table 7.2-2: header-borne info moves into "metadata".
                    let link = link_header_value(ctx);
                    let mut ri = receiver_info.clone();
                    if hdr_tenant.as_str() != "default" {
                        ri.push(("NGSILD-Tenant".into(), hdr_tenant.as_str().to_owned()));
                    }
                    if let Some(sid) = &snapshot_id {
                        ri.push(("NGSILD-Snapshot".into(), sid.clone()));
                    }
                    let msg = build_message(&body, accept, Some(&link), &ri);
                    Outbound::Mqtt(endpoint, params, crate::negotiate::ordered_vec(&msg))
                }
                Err(_) => {
                    // The parse error embeds the endpoint URI, which can carry
                    // credentials in its userinfo.
                    tracing::warn!(
                        "mqtt endpoint of subscription {sub_id} unusable: {}",
                        redact_userinfo(uri)
                    );
                    return;
                }
            }
        }
        #[cfg(not(feature = "mqtt"))]
        {
            return; // no sink compiled: creation already answered 422
        }
    } else {
        let mut req = st.http.post(uri);
        if accept == "application/ld+json" {
            req = req.header("Content-Type", "application/ld+json");
        } else {
            // application/json and application/geo+json (5.3.1) both carry
            // the @context via the Link header (6.3.5)
            req = req
                .header("Content-Type", accept)
                .header("Link", link_header_value(ctx));
        }
        for (k, v) in &receiver_info {
            req = req.header(k, v);
        }
        if hdr_tenant.as_str() != "default" {
            req = req.header("NGSILD-Tenant", hdr_tenant.as_str());
        }
        if let Some(sid) = &snapshot_id {
            req = req.header("NGSILD-Snapshot", sid.as_str());
        }
        Outbound::Http(req, crate::negotiate::ordered_vec(&body))
    };
    // Bookkeeping BEFORE the send (5.8.6/5.2.14.2: lastNotification is the
    // instant the notification is sent). The ETSI mock unblocks the test the
    // moment the request ARRIVES, so a post-response-only writeback races the
    // test's immediate Retrieve Subscription (CI flake on 046_12_01).
    // Optimistic ok; a failed attempt is corrected right below — the transient
    // window is the in-flight attempt itself, and the failure TPs wait for the
    // attempt to resolve before asserting.
    let mut prev_success: Option<Value> = None;
    let booked = st
        .store
        .mutate(tenant, kind, &sub_id, |doc| {
            if let Some(o) = doc.as_object_mut() {
                o.remove("status");
            }
            if let Some(n) = doc
                .as_object_mut()
                .and_then(|o| o.get_mut("notification"))
                .and_then(Value::as_object_mut)
            {
                let sent = n.get("timesSent").and_then(Value::as_i64).unwrap_or(0);
                n.insert("timesSent".into(), json!(sent + 1));
                n.insert("lastNotification".into(), Value::String(now.clone()));
                prev_success = n.insert("lastSuccess".into(), Value::String(now.clone()));
                n.insert("status".into(), Value::String("ok".into()));
            }
            Ok::<(), antares_model::NgsiError>(())
        })
        .unwrap_or_else(|e| {
            tracing::warn!("bookkeeping writeback failed: {e}");
            None
        });
    // 5.8.6: notifications are sent for the subscriptions the broker holds.
    // No row to book against means the subscription was deleted (or the
    // store failed) between matching and delivery — nothing may be sent.
    if booked.is_none() {
        return;
    }
    mirror_bookkeeping(st, tenant, kind, &sub_id);
    // The notification endpoint is an egress destination like any other
    // — policy check once, breaker consulted before the attempt.
    // A refusal is a delivery failure for bookkeeping (status "failed",
    // lastSuccess rolled back below) but never breaker state: the policy
    // verdict says nothing about the endpoint's health.
    let refused = match st.egress.check_url(uri).await {
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
    let (ok, timed_out) = if refused {
        (false, false)
    } else {
        match outbound {
            Outbound::Http(req, bytes) => {
                // Wasm: the page sink takes matching endpoints — a page
                // cannot listen on a socket, so this IS its delivery channel.
                #[cfg(target_arch = "wasm32")]
                let page_handled = crate::page_sink::try_deliver(uri, &bytes);
                #[cfg(not(target_arch = "wasm32"))]
                let page_handled = false;
                if page_handled {
                    (true, false)
                } else {
                    // endpoint.timeout (Table 5.2.15-1), clamped
                    match antares_jsonld::io_deadline(
                        req.body(bytes).send(),
                        endpoint_timeout_ms(ep),
                    )
                    .await
                    {
                        Some(Ok(r)) => (r.status().is_success(), false),
                        Some(Err(e)) => (false, e.is_timeout()),
                        None => (false, true),
                    }
                }
            }
            #[cfg(feature = "mqtt")]
            Outbound::Mqtt(endpoint, params, bytes) => {
                match st.mqtt.deliver(&endpoint, params, &bytes).await {
                    Ok(()) => (true, false),
                    Err(e) => {
                        tracing::warn!("mqtt delivery for {sub_id} failed: {e}");
                        // broker/socket-level failure — keep the timeout guard
                        (false, true)
                    }
                }
            }
        }
    };
    if !refused {
        if ok {
            st.egress.record_success(uri);
        } else if timed_out {
            st.egress.record_failure(uri);
        } else {
            // the destination responded (or refused fast): alive — clear
            // any stale consecutive-timeout state
            st.egress.record_success(uri);
        }
    }
    // Delivery counters by sink scheme (facade — no-op without the
    // broker's recorder).
    let scheme = if is_mqtt { "mqtt" } else { "http" };
    if ok {
        metrics::counter!("antares_notifications_sent_total", "scheme" => scheme).increment(1);
    } else {
        metrics::counter!("antares_notifications_failed_total", "scheme" => scheme).increment(1);
    }
    if !ok {
        // 5.8.6 / 5.11.7: subscription status → "failed" on delivery failure;
        // roll back the optimistic lastSuccess stamp.
        let ts = now_iso();
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
                Ok::<(), antares_model::NgsiError>(())
            })
            .unwrap_or_else(|e| {
                tracing::warn!("failure-status writeback failed: {e}");
                None
            });
        mirror_bookkeeping(st, tenant, kind, &sub_id);
    }
}

/// Endpoint URIs may carry credentials in the authority's userinfo
/// (mqtt[s]://username:password@host, clause 7.1) — strip everything
/// between the `//` and the authority's `@` before the URI reaches a log.
fn redact_userinfo(uri: &str) -> String {
    if let Some(scheme_end) = uri.find("//") {
        let rest = &uri[scheme_end + 2..];
        let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        if let Some(at) = rest[..authority_end].rfind('@') {
            return format!("{}{}", &uri[..scheme_end + 2], &rest[at + 1..]);
        }
    }
    uri.to_owned()
}

/// The matcher reads subscriptions from the
/// SubMirror, so every notification bookkeeping writeback must be applied
/// there too — otherwise the mirror copy never gains
/// `notification.lastNotification` and 5.2.12 `throttling` suppresses
/// nothing. In-process apply only: a KV write per notification would not
/// scale to the 100k-sub target, so in bus=nats multi-pod deployments the
/// throttling window is per-pod approximate.
/// Known ceiling: exact distributed throttling = per-notification KV sync or a
/// store read in `throttled()`; add if a deployment needs the strict window.
fn mirror_bookkeeping(st: &AppState, tenant: &TenantId, kind: Kind, sub_id: &str) {
    if kind != Kind::Subscription {
        return;
    }
    if let Some(m) = &st.sub_mirror {
        if let Ok(Some(doc)) = st.store.get(tenant, kind, sub_id) {
            m.apply(tenant.as_str(), sub_id, Some(doc));
        }
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
        std::env::set_var("ANTARES_EGRESS_ALLOW_PRIVATE", "true");
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
mod endpoint_tests {
    use super::*;
    use serde_json::json;

    fn ep(v: Value) -> serde_json::Map<String, Value> {
        v.as_object().expect("map").clone()
    }

    /// Endpoint URIs may carry credentials (mqtt[s]://user:pass@host, 7.1);
    /// log lines must never leak them.
    #[test]
    fn log_redaction_strips_uri_userinfo() {
        let red = redact_userinfo("mqtts://alice:s3cret@broker:8883/topic");
        assert_eq!(red, "mqtts://broker:8883/topic");
        assert!(!red.contains("s3cret"));
        assert!(!red.contains("alice"));
        assert_eq!(
            redact_userinfo("http://host:9090/notify"),
            "http://host:9090/notify"
        );
        // an '@' beyond the authority is path data, not userinfo
        assert_eq!(redact_userinfo("http://h/p@x"), "http://h/p@x");
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

    /// 5.8.6: a matching change notifies. Matching runs on one long-lived
    /// task, so a panic while matching ONE change must not end notification
    /// delivery for the process — the next change still notifies.
    #[tokio::test(flavor = "multi_thread")]
    async fn panicking_change_does_not_stop_the_next_notification() {
        std::env::set_var("ANTARES_EGRESS_ALLOW_PRIVATE", "true");
        let (uri, hits) = counting_endpoint().await;
        let mut st = AppState::new("antares-panic-guard".into());
        wire(&mut st);
        subscribe(&st, "guard", &uri, 2_000).await;
        // A poisoned mirror lock makes the matcher panic on the next change.
        // The panic's source is incidental; the task boundary is the subject.
        let mirror = st.sub_mirror.clone().expect("mirror");
        let m = mirror.clone();
        let _ = std::thread::spawn(move || {
            let _held = m.map.write().expect("mirror lock");
            panic!("matcher panic");
        })
        .join();
        assert_eq!(create_vehicle(&st, 1).await, 201);
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        mirror.map.clear_poison();
        assert_eq!(create_vehicle(&st, 2).await, 201);
        for _ in 0..50 {
            if hits.load(Ordering::SeqCst) > 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        assert_eq!(
            hits.load(Ordering::SeqCst),
            1,
            "the change after a panicking one must still be notified"
        );
    }

    /// The matcher queue is bounded: behind a stalled subscriber the excess
    /// changes are dropped and counted instead of growing without limit.
    #[tokio::test(flavor = "multi_thread")]
    async fn overflowing_change_queue_drops_and_counts() {
        std::env::set_var("ANTARES_EGRESS_ALLOW_PRIVATE", "true");
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
        subscribe(&st, "staller", &format!("http://{addr}/notify"), 30_000).await;
        let before = changes_dropped();
        for n in 0..(CHANGE_QUEUE + 64) {
            assert_eq!(create_vehicle(&st, 1_000 + n).await, 201);
        }
        assert!(
            changes_dropped() > before,
            "a full matcher queue must drop and count, not grow (dropped {} → {})",
            before,
            changes_dropped()
        );
    }

    /// Table 5.2.12-1: "\"entityUpdated\" is equivalent to the combination
    /// \"attributeCreated\", \"attributeUpdated\" and \"attributeDeleted\"",
    /// so such a subscription notifies on a creation exactly like the
    /// spelled-out list does — while "entityDeleted" alone does not.
    #[tokio::test(flavor = "multi_thread")]
    async fn entity_updated_trigger_notifies_on_entity_creation() {
        std::env::set_var("ANTARES_EGRESS_ALLOW_PRIVATE", "true");
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
                "notification": {"endpoint": {"uri": uri, "timeout": 2_000}},
            })
        };
        for body in [
            sub("eu", "entityUpdated", &uri),
            sub("ed", "entityDeleted", &quiet_uri),
        ] {
            assert_eq!(post(&st, "/ngsi-ld/v1/subscriptions", body).await, 201);
        }
        assert_eq!(create_vehicle(&st, 7).await, 201);
        for _ in 0..50 {
            if hits.load(Ordering::SeqCst) > 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        assert_eq!(
            hits.load(Ordering::SeqCst),
            1,
            "entityUpdated implies attributeCreated, so a creation notifies"
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
        std::env::set_var("ANTARES_EGRESS_ALLOW_PRIVATE", "true");
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
        std::env::set_var("ANTARES_EGRESS_ALLOW_PRIVATE", "true");
        let (uri, hits) = endpoint(axum::http::StatusCode::OK).await;
        let st = AppState::new("antares-breaker-bookkeeping".into());
        let tenant = TenantId::new("default").expect("tenant");
        let sub = subscribe(&st, &tenant, &uri);
        for _ in 0..crate::egress::TRIP_AFTER {
            st.egress.record_failure(&uri);
        }
        assert!(st.egress.is_open(&uri), "the destination is open-circuit");

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
        st.egress.record_success(&uri);
        send(&st, &tenant, &sub).await;
        let n = stored_notification(&st, &tenant);
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        assert_eq!(n.get("timesSent"), Some(&json!(1)));
        assert!(n.get("lastNotification").is_some());
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
        std::env::set_var("ANTARES_EGRESS_ALLOW_PRIVATE", "true");
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
        std::env::set_var("ANTARES_EGRESS_ALLOW_PRIVATE", "true");
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
}
