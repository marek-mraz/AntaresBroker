// SPDX-License-Identifier: EUPL-1.2
//! The tenant-keyed document mirrors the notification pipeline and the
//! bus wiring share: a registration mirror, a subscription mirror with
//! its inverted candidate index and sweep clocks, and the change event
//! every write emits. A leaf: nothing here names another module, so the
//! application state can hold these types without depending on the
//! pipeline that consumes them.

use serde_json::Value;

/// One change queue event: tenant, before-image, after-image.
pub type Change = (String, Option<Value>, Option<Value>);

/// A per-instance tenant-keyed document mirror (bus=nats). One
/// instance holds subscriptions (fed by the KV watcher), another holds
/// registrations (fed by `ANTARES_REGISTRY` deltas). Postgres stays the
/// system of record — this map is a cache with exactly one writer (the
/// watcher task); readers only snapshot.
///
/// A snapshot hands out `Arc`s, not copies. When this mirror is installed
/// it IS the federation read path's registration source ([`Self::matching`]
/// replaces the store's `matching_registrations`), and a broker at the
/// 100 000-registration target that deep-copied every document per
/// distributed request paid 84 ms and a hundred megabytes for a set it only
/// ever reads. The documents are immutable once applied — a changed
/// registration arrives as a whole new document — so sharing them costs a
/// refcount and nothing else.
#[derive(Default)]
pub struct DocMirror {
    map: std::sync::RwLock<std::collections::HashMap<String, RegIndex>>,
}

/// One tenant's registrations, plus the two dimensions a distributed read
/// narrows on before it evaluates 5.12 matching per registration.
///
/// The buckets are the mirror's answer to the same question
/// `matching_registrations` pushes into the store's `csource_index`, and
/// they obey the same contract: a bucket may only ever drop a registration
/// `reg_candidate` would reject anyway. So a registration lands in `any_*`
/// — the set no key can exclude — whenever the dimension cannot decide it:
/// a `RegistrationInfo` with no `entities` at all, an `EntityInfo` with no
/// `type` (5.12 restricts such an entry by id alone), or an `EntityInfo`
/// carrying an `idPattern` (5.12 condition 5 forwards on a pattern whatever
/// ids were asked for).
///
/// The membership is per REGISTRATION, not per `EntityInfo` — a
/// registration whose one entry carries the asked-for type and whose other
/// carries the asked-for id is kept, where the store's per-row `WHERE` drops
/// it. Wider than the store, which is the safe direction for a prefilter.
#[derive(Default)]
struct RegIndex {
    docs: std::collections::HashMap<String, std::sync::Arc<Value>>,
    by_type: std::collections::HashMap<String, std::collections::HashSet<String>>,
    by_id: std::collections::HashMap<String, std::collections::HashSet<String>>,
    any_type: std::collections::HashSet<String>,
    any_id: std::collections::HashSet<String>,
}

/// The type and id keys one registration document occupies. `None` for a
/// dimension means no key of it can exclude this registration.
fn reg_keys(doc: &Value) -> (Option<Vec<String>>, Option<Vec<String>>) {
    let (mut types, mut ids) = (Vec::new(), Vec::new());
    let (mut any_type, mut any_id) = (false, false);
    // No `information` at all is not a shape 5.2.9 allows, and nothing can
    // be read off it — the registration stays in both broad sets rather
    // than being narrowed out on a document this code does not understand.
    let Some(infos) = doc.get("information").and_then(Value::as_array) else {
        return (None, None);
    };
    if infos.is_empty() {
        return (None, None);
    }
    for info in infos {
        // 5.2.9: a RegistrationInfo may name only propertyNames /
        // relationshipNames, which restricts attributes and no entity.
        let Some(entities) = info.get("entities").and_then(Value::as_array) else {
            return (None, None);
        };
        if entities.is_empty() {
            return (None, None);
        }
        for ei in entities {
            match crate::registry::ei_types(ei) {
                ts if ts.is_empty() => any_type = true,
                ts => types.extend(ts.into_iter().map(str::to_owned)),
            }
            if ei.get("idPattern").is_some() {
                any_id = true;
            } else {
                match ei.get("id").and_then(Value::as_str) {
                    Some(id) => ids.push(id.to_owned()),
                    None => any_id = true,
                }
            }
        }
    }
    ((!any_type).then_some(types), (!any_id).then_some(ids))
}

/// The registrations a set of keys can reach: the ones filed under a named
/// key, plus every one the dimension cannot decide. `None` in, `None` out —
/// the caller asked nothing of this dimension, so it narrows nothing.
fn bucketed<'a>(
    keys: Option<&[String]>,
    by: &'a std::collections::HashMap<String, std::collections::HashSet<String>>,
    any: &'a std::collections::HashSet<String>,
) -> Option<std::collections::HashSet<&'a str>> {
    let keys = keys?;
    let mut out: std::collections::HashSet<&str> = any.iter().map(String::as_str).collect();
    for k in keys {
        if let Some(ids) = by.get(k) {
            out.extend(ids.iter().map(String::as_str));
        }
    }
    Some(out)
}

impl RegIndex {
    /// Drop a registration from the documents and from every bucket it is
    /// filed under. The keys come from the document being removed, so the
    /// index never needs a second copy of them.
    fn remove(&mut self, id: &str) {
        let Some(old) = self.docs.remove(id) else {
            return;
        };
        let (types, ids) = reg_keys(&old);
        let unfile =
            |keys: Option<Vec<String>>,
             by: &mut std::collections::HashMap<String, std::collections::HashSet<String>>,
             any: &mut std::collections::HashSet<String>| {
                match keys {
                    None => {
                        any.remove(id);
                    }
                    Some(ks) => {
                        for k in ks {
                            if let Some(set) = by.get_mut(&k) {
                                set.remove(id);
                                if set.is_empty() {
                                    by.remove(&k);
                                }
                            }
                        }
                    }
                }
            };
        unfile(types, &mut self.by_type, &mut self.any_type);
        unfile(ids, &mut self.by_id, &mut self.any_id);
    }

    fn insert(&mut self, id: &str, doc: Value) {
        self.remove(id);
        let (types, ids) = reg_keys(&doc);
        let file =
            |keys: Option<Vec<String>>,
             by: &mut std::collections::HashMap<String, std::collections::HashSet<String>>,
             any: &mut std::collections::HashSet<String>| {
                match keys {
                    None => {
                        any.insert(id.to_owned());
                    }
                    Some(ks) => {
                        for k in ks {
                            by.entry(k).or_default().insert(id.to_owned());
                        }
                    }
                }
            };
        file(types, &mut self.by_type, &mut self.any_type);
        file(ids, &mut self.by_id, &mut self.any_id);
        self.docs.insert(id.to_owned(), std::sync::Arc::new(doc));
    }

    fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }
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
    pub(crate) next_sub_sweep_ms: std::sync::atomic::AtomicI64,
    /// The same clock for Context Source Registration Subscriptions
    /// (5.11.7). They are not mirrored as documents — the sweep reads them
    /// from the store — so a write signals this clock through
    /// [`SubMirror::csub_written`] instead, and a sweep falls back to
    /// `CSUB_SWEEP_BACKSTOP_MS` in case a signal was lost.
    pub(crate) next_csub_sweep_ms: std::sync::atomic::AtomicI64,
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
pub(crate) const CSUB_SWEEP_BACKSTOP_MS: i64 = 60_000;

#[derive(Default)]
struct TenantIndex {
    // Shared, not owned: `candidates` runs once per change and hands back
    // every subscription that could fire. Cloning the documents there deep-
    // copied each one — every string and every nested map — on the hot path,
    // so the cost of a change grew with the size of the subscriptions it was
    // evaluated against rather than with the work it caused. An `Arc` clone
    // is a refcount bump, and the registration mirror above already holds
    // its documents this way.
    docs: std::collections::HashMap<String, std::sync::Arc<Value>>,
    by_type: std::collections::HashMap<String, std::collections::HashSet<String>>,
    by_attr: std::collections::HashMap<String, std::collections::HashSet<String>>,
    broad: std::collections::HashSet<String>,
}

/// Which index bucket(s) one stored subscription doc belongs in.
pub(crate) enum Keys {
    Types(Vec<String>),
    Attrs(Vec<String>),
    Broad,
}

pub(crate) fn index_keys(doc: &Value) -> Keys {
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
            t.docs.insert(id.to_owned(), std::sync::Arc::new(d));
        }
        if map.get(tenant).is_some_and(|t| t.docs.is_empty()) {
            map.remove(tenant);
        }
    }

    /// 5.11.7: a Context Source Registration Subscription was written. Only
    /// the clock moves — these are matched against registrations rather than
    /// entities, so they are not carried in the candidate index, and the
    /// sweep reads the tenant's rows from the store once it wakes.
    pub(crate) fn csub_written(&self) {
        self.next_csub_sweep_ms
            .store(0, std::sync::atomic::Ordering::Relaxed);
    }

    /// The hot path: subscriptions that could possibly fire for a change
    /// touching these entity types and these changed attributes. Union of
    /// the type hits, the attr hits and the broad bucket — a superset of
    /// the firing set, never a subset.
    pub fn candidates(
        &self,
        tenant: &str,
        types: &[&str],
        changed_attrs: &[&str],
    ) -> Vec<std::sync::Arc<Value>> {
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

    #[cfg(any(test, feature = "test-kit"))]
    pub fn docs(&self, tenant: &str) -> Vec<std::sync::Arc<Value>> {
        self.map
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(tenant)
            .map(|t| t.docs.values().cloned().collect())
            .unwrap_or_default()
    }

    /// The interval sweep's whole input: the tenant's periodic (5.2.12
    /// `timeInterval`) subscriptions. The walk is over every subscription the
    /// tenant holds, but only the periodic ones are carried out of it — the
    /// sweep clocks keep a tick with nothing due from reaching this at all.
    pub(crate) fn periodic_docs(&self, tenant: &str) -> Vec<std::sync::Arc<Value>> {
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

    #[cfg(any(test, feature = "test-kit"))]
    pub fn tenants(&self) -> Vec<String> {
        self.map
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .keys()
            .cloned()
            .collect()
    }
}

#[cfg(test)]
impl SubMirror {
    /// Poison the index lock from a thread that panics while holding it:
    /// what a matcher that dies mid-write leaves behind, for the
    /// supervision tests.
    pub(crate) fn poison(self: &std::sync::Arc<Self>) {
        let m = std::sync::Arc::clone(self);
        let _ = std::thread::spawn(move || {
            let _held = m
                .map
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            panic!("poison the mirror");
        })
        .join();
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
                map.entry(tenant.to_owned()).or_default().insert(id, d);
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

    /// The registrations of one tenant that can match these ids and types,
    /// shared rather than copied.
    ///
    /// Both dimensions are optional and each narrows on its own; an absent
    /// one is a dimension the caller asked nothing about, never an empty
    /// answer. With both given a registration has to survive both, which is
    /// the store's rule too.
    pub fn matching(
        &self,
        tenant: &str,
        ids: Option<&[String]>,
        types: Option<&[String]>,
    ) -> Vec<std::sync::Arc<Value>> {
        let map = self
            .map
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(t) = map.get(tenant) else {
            return Vec::new();
        };
        let by_type = bucketed(types, &t.by_type, &t.any_type);
        let by_id = bucketed(ids, &t.by_id, &t.any_id);
        let pick = |keys: std::collections::HashSet<&str>| -> Vec<std::sync::Arc<Value>> {
            keys.into_iter()
                .filter_map(|k| t.docs.get(k).map(std::sync::Arc::clone))
                .collect()
        };
        match (by_type, by_id) {
            (None, None) => t.docs.values().map(std::sync::Arc::clone).collect(),
            (Some(k), None) | (None, Some(k)) => pick(k),
            (Some(a), Some(b)) => pick(a.intersection(&b).copied().collect()),
        }
    }

    pub fn docs(&self, tenant: &str) -> Vec<Value> {
        self.matching(tenant, None, None)
            .into_iter()
            .map(|d| (*d).clone())
            .collect()
    }

    #[cfg(any(test, feature = "test-kit"))]
    pub fn tenants(&self) -> Vec<String> {
        self.map
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .keys()
            .cloned()
            .collect()
    }
}
