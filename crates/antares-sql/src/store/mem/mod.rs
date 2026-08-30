// SPDX-License-Identifier: EUPL-1.2
//! In-memory store — v0 storage backend.
//!
//! DELIBERATE DEVIATION (recorded in docs/adr/): the target backend is
//! Postgres; the suite-green loop uses an in-memory backend first. The store
//! API is shaped like the Postgres store traits (tenant first parameter
//! everywhere) so the sqlx implementation can land behind the same seam.
//!
//! Documents are held in the *internal expanded form* produced by
//! `antares_jsonld::expand` (IRI keys, instance arrays), with server-managed
//! timestamps embedded (`createdAt`/`modifiedAt` at entity level and inside
//! each attribute instance) — output layers strip them unless sysAttrs.

use ::redb::{Database, Durability, ReadableDatabase, ReadableTableMetadata, TableDefinition};
use antares_model::TenantId;
use antares_store::{filter, ChangeHook, Kind};

mod redb;
use self::redb::{
    key_bytes, split_key, table_for, Shadow, FORMAT_VERSION, T_ENTITIES, T_JSONLD_CONTEXTS, T_META,
    T_TEMPORAL_ENTITIES,
};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::RwLock;

/// How many `Cached` @context entries the broker keeps (5.13.1). The bound is
/// on the CACHE only: it exists because one entry is stored per distinct
/// external @context URL a request references, which is client-controlled,
/// while the working set of real @contexts a deployment uses is small. Every
/// entry holds a whole @context body, so the count is the memory bound too.
pub const MAX_CACHED_CONTEXTS: usize = 1_000;

/// The `Cached` entry ids to drop so at most `MAX_CACHED_CONTEXTS` remain,
/// oldest `createdAt` first (5.13.1 stamps every stored entry with one; an
/// entry without a stamp sorts oldest and goes first). Other kinds are never
/// candidates.
fn oldest_cached(contexts: &BTreeMap<String, Value>) -> Vec<String> {
    let mut cached: Vec<(&str, &str)> = contexts
        .iter()
        .filter(|(_, d)| d.get("kind").and_then(Value::as_str) == Some("Cached"))
        .map(|(id, d)| {
            (
                id.as_str(),
                d.get("createdAt").and_then(Value::as_str).unwrap_or(""),
            )
        })
        .collect();
    if cached.len() <= MAX_CACHED_CONTEXTS {
        return Vec::new();
    }
    cached.sort_unstable_by(|(a_id, a_ts), (b_id, b_ts)| (a_ts, a_id).cmp(&(b_ts, b_id)));
    cached[..cached.len() - MAX_CACHED_CONTEXTS]
        .iter()
        .map(|(id, _)| (*id).to_owned())
        .collect()
}

/// Drop attribute instances whose `expiresAt` passed (4.22) from an internal
/// entity or temporal doc; an attribute whose last instance expired is
/// removed entirely. Returns whether the doc changed.
fn prune_expired_instances(doc: &mut Value, now: &str) -> bool {
    const DOC_META: [&str; 8] = [
        "id",
        "type",
        "scope",
        "@context",
        "createdAt",
        "modifiedAt",
        "deletedAt",
        "expiresAt",
    ];
    let Some(obj) = doc.as_object_mut() else {
        return false;
    };
    let mut changed = false;
    let attrs: Vec<String> = obj
        .keys()
        .filter(|k| !DOC_META.contains(&k.as_str()))
        .cloned()
        .collect();
    for k in attrs {
        let Some(arr) = obj.get_mut(&k).and_then(Value::as_array_mut) else {
            continue;
        };
        let before = arr.len();
        arr.retain(|inst| {
            !inst
                .get("expiresAt")
                .and_then(Value::as_str)
                .is_some_and(|e| e < now)
        });
        if arr.len() != before {
            changed = true;
            if arr.is_empty() {
                obj.remove(&k);
            }
        }
    }
    changed
}

/// The 4.22 "now" the write paths judge `expiresAt` against — the same UTC-Z
/// millisecond form the read boundary uses.
fn now_stamp() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

const ALL_KINDS: [Kind; 9] = [
    Kind::Entity,
    Kind::Subscription,
    Kind::Registration,
    Kind::CSourceSubscription,
    Kind::Temporal,
    Kind::Snapshot,
    Kind::EntityMap,
    Kind::DistSub,
    Kind::DeadLetter,
];

/// Run `f` off the tokio worker pool when called from a multi-thread runtime:
/// a per-commit fsync must never stall an async worker. Outside a
/// runtime (unit tests, startup) it just runs inline.
fn on_blocking<T>(f: impl FnOnce() -> T) -> T {
    // wasm32: single-threaded, no tokio runtime — always inline.
    #[cfg(not(target_arch = "wasm32"))]
    if let Ok(h) = tokio::runtime::Handle::try_current() {
        if h.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread {
            return tokio::task::block_in_place(f);
        }
    }
    f()
}

#[derive(Default)]
pub struct Store {
    inner: RwLock<Inner>,
    hook: RwLock<Option<ChangeHook>>,
    /// Set when this instance serves only the temporal seam: it never holds
    /// the entities, so the append guard must not look for them here.
    pub temporal_only: bool,
    /// `file` mode durability shadow; `None` = pure in-memory (`memory` mode).
    shadow: Option<Shadow>,
    /// Writers currently queued behind the single write-critical section
    /// (redb has ONE writer, so fsync commits serialize here). Exported via
    /// /q/health; the group-commit lever only gets built if a benchmark shows
    /// this depth sustained at the measured ~3.1k writes/s ceiling.
    write_waiters: std::sync::atomic::AtomicUsize,
    write_waiters_peak: std::sync::atomic::AtomicUsize,
    /// Change hooks fire in COMMIT order. The hook runs after the
    /// write-critical section (it may write other kinds through the store,
    /// so running it inside would deadlock), which lets a later commit's
    /// hook overtake an earlier one — the consumer would record stale state
    /// as newest. Entity writes therefore hold this from before the commit
    /// until the emit is done. Only the local single-process path needs it:
    /// across processes the transactional outbox is the ordered channel.
    emit_order: std::sync::Mutex<()>,
}

#[derive(Default)]
struct Inner {
    /// tenant → id → internal entity doc (BTreeMap: deterministic list order).
    entities: HashMap<String, BTreeMap<String, Value>>,
    subscriptions: HashMap<String, BTreeMap<String, Value>>,
    registrations: HashMap<String, BTreeMap<String, Value>>,
    csource_subscriptions: HashMap<String, BTreeMap<String, Value>>,
    snapshots: HashMap<String, BTreeMap<String, Value>>,
    entity_map_docs: HashMap<String, BTreeMap<String, Value>>,
    dist_subs: HashMap<String, BTreeMap<String, Value>>,
    dead_letters: HashMap<String, BTreeMap<String, Value>>,
    /// tenant → entity id → temporal doc (attr IRI → instance array).
    temporal: HashMap<String, BTreeMap<String, Value>>,
    /// hosted/cached @context documents, shared across tenants by design.
    contexts: BTreeMap<String, Value>,
}

impl Store {
    /// Open (or create) the `file`-mode store: redb at `dir/antares.redb`,
    /// format-checked, in-memory maps rebuilt from the file.
    /// Any open/format/decode error refuses to start — never silently serve
    /// partial data.
    pub fn open_file(dir: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
        let path = dir.join("antares.redb");
        let db = Database::create(&path).map_err(|e| format!("open {}: {e}", path.display()))?;
        Self::from_database(db, &path.display().to_string())
    }

    /// The target-independent half of `open_file`: format check +
    /// boot rebuild over an already-constructed redb `Database`. The
    /// browser build calls this with an OPFS-backed database — same shadow,
    /// same commit-before-ack, different `StorageBackend`.
    pub fn from_database(db: Database, label: &str) -> Result<Self, String> {
        // Format marker. Absent marker + existing data = a file this
        // binary cannot vouch for; refuse rather than guess.
        let stored_format = {
            let rt = db.begin_read().map_err(|e| e.to_string())?;
            match rt.open_table(T_META) {
                Ok(t) => t
                    .get("format")
                    .map_err(|e| e.to_string())?
                    .map(|v| v.value().to_owned()),
                Err(::redb::TableError::TableDoesNotExist(_)) => None,
                Err(e) => return Err(e.to_string()),
            }
        };
        match stored_format.as_deref() {
            Some(FORMAT_VERSION) => {}
            Some(other) => {
                return Err(format!(
                    "data file {label} has format {other}, this binary supports {FORMAT_VERSION} — \
                     refusing to start"
                ));
            }
            None => {
                let rt = db.begin_read().map_err(|e| e.to_string())?;
                for kind in ALL_KINDS {
                    if let Ok(t) = rt.open_table(table_for(kind)) {
                        if t.len().map_err(|e| e.to_string())? > 0 {
                            return Err(format!(
                                "data file {label} holds data but no format marker — refusing to start"
                            ));
                        }
                    }
                }
                drop(rt);
                let mut tx = db.begin_write().map_err(|e| e.to_string())?;
                tx.set_durability(Durability::Immediate)
                    .map_err(|e| e.to_string())?;
                {
                    let mut t = tx.open_table(T_META).map_err(|e| e.to_string())?;
                    t.insert("format", FORMAT_VERSION)
                        .map_err(|e| e.to_string())?;
                }
                tx.commit().map_err(|e| e.to_string())?;
            }
        }

        // Boot rebuild — scan every table into the in-memory maps.
        let mut inner = Inner::default();
        let rt = db.begin_read().map_err(|e| e.to_string())?;
        for kind in ALL_KINDS {
            let table = match rt.open_table(table_for(kind)) {
                Ok(t) => t,
                Err(::redb::TableError::TableDoesNotExist(_)) => continue,
                Err(e) => return Err(e.to_string()),
            };
            let map = match kind {
                Kind::Entity => &mut inner.entities,
                Kind::Subscription => &mut inner.subscriptions,
                Kind::Registration => &mut inner.registrations,
                Kind::CSourceSubscription => &mut inner.csource_subscriptions,
                Kind::Temporal => &mut inner.temporal,
                Kind::Snapshot => &mut inner.snapshots,
                Kind::EntityMap => &mut inner.entity_map_docs,
                Kind::DistSub => &mut inner.dist_subs,
                Kind::DeadLetter => &mut inner.dead_letters,
            };
            for row in ::redb::ReadableTable::iter(&table).map_err(|e| e.to_string())? {
                let (k, v) = row.map_err(|e| e.to_string())?;
                let (tenant, id) = split_key(k.value())
                    .ok_or_else(|| format!("undecodable key in table {kind:?}"))?;
                let doc: Value = serde_json::from_slice(v.value())
                    .map_err(|e| format!("undecodable value for {tenant}/{id}: {e}"))?;
                map.entry(tenant).or_default().insert(id, doc);
            }
        }
        if let Ok(t) = rt.open_table(T_JSONLD_CONTEXTS) {
            for row in ::redb::ReadableTable::iter(&t).map_err(|e| e.to_string())? {
                let (k, v) = row.map_err(|e| e.to_string())?;
                let id = String::from_utf8(k.value().to_vec())
                    .map_err(|e| format!("undecodable context id: {e}"))?;
                let doc: Value = serde_json::from_slice(v.value())
                    .map_err(|e| format!("undecodable context {id}: {e}"))?;
                inner.contexts.insert(id, doc);
            }
        }
        drop(rt);

        Ok(Self {
            inner: RwLock::new(inner),
            hook: RwLock::new(None),
            emit_order: std::sync::Mutex::new(()),
            shadow: Some(Shadow { db }),
            temporal_only: false,
            write_waiters: Default::default(),
            write_waiters_peak: Default::default(),
        })
    }

    /// Write-through for one doc (must be called inside the write-critical
    /// section so redb order equals memory order).
    fn persist(&self, table: TableDefinition<&[u8], &[u8]>, key: &[u8], doc: Option<&Value>) {
        if let Some(shadow) = &self.shadow {
            // None means DELETE the key, so a document that will not encode
            // must skip the write rather than collapse into one.
            let bytes = match doc {
                Some(d) => match serde_json::to_vec(d) {
                    Ok(b) => Some(b),
                    Err(_) => return,
                },
                None => None,
            };
            shadow.write(table, key, bytes.as_deref());
        }
    }

    /// Acquire the write-critical section, counting queued writers.
    ///
    /// Poison recovery (`into_inner`) is deliberate: a panic inside a caller's
    /// mutate closure unwinds one request; poisoning would turn it into a
    /// whole-process brick (every later store call panicking until restart).
    /// Consistency holds because mutations work on a clone and swap last.
    fn write_inner(&self) -> std::sync::RwLockWriteGuard<'_, Inner> {
        use std::sync::atomic::Ordering;
        let depth = self.write_waiters.fetch_add(1, Ordering::Relaxed) + 1;
        self.write_waiters_peak.fetch_max(depth, Ordering::Relaxed);
        let guard = self
            .inner
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.write_waiters.fetch_sub(1, Ordering::Relaxed);
        guard
    }

    /// Whether writes reach a durability shadow (`file` mode, and the
    /// browser build over OPFS) rather than living only in memory.
    pub fn shadowed(&self) -> bool {
        self.shadow.is_some()
    }

    /// (Currently queued writers, peak since start). The peak going
    /// nowhere near sustained depth is the evidence that the group-commit
    /// lever stays unbuilt.
    pub fn commit_queue(&self) -> (usize, usize) {
        use std::sync::atomic::Ordering;
        (
            self.write_waiters.load(Ordering::Relaxed),
            self.write_waiters_peak.load(Ordering::Relaxed),
        )
    }

    pub fn set_change_hook(&self, h: ChangeHook) {
        *self
            .hook
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(h);
    }

    /// Held from before an entity commit until its emit returns (see
    /// `emit_order`). Not for other kinds: their writes emit nothing, and a
    /// hook that writes them re-enters the store.
    fn emit_ordered(&self) -> std::sync::MutexGuard<'_, ()> {
        self.emit_order
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

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

    /// 4.22 garbage collection for the memory/file arm: remove entity docs
    /// whose `expiresAt` (byte-compared against the UTC-Z `now` stamp, same
    /// as the read filter) has passed, and prune expired ATTRIBUTE instances
    /// from current-state and temporal docs — the read filter hides them,
    /// but without physical removal a long-running store (the browser's OPFS
    /// file under ticking sensors) grows without bound. `file` mode persists
    /// each removal. Returns how many docs were reaped or pruned.
    ///
    /// Runs through `on_blocking` like every other mutating path: it holds
    /// the write-critical section for a full scan and issues one
    /// `Durability::Immediate` (fsync) commit per reaped doc, which must
    /// never happen on an async worker thread.
    pub fn sweep_expired(&self, now: &str) -> usize {
        on_blocking(|| self.sweep_expired_locked(now))
    }

    fn sweep_expired_locked(&self, now: &str) -> usize {
        let mut inner = self.write_inner();
        let mut reaped = 0usize;
        let mut dead: Vec<(String, String)> = Vec::new();
        for (tenant, docs) in &inner.entities {
            for (id, doc) in docs {
                if doc
                    .get("expiresAt")
                    .and_then(Value::as_str)
                    .is_some_and(|e| e < now)
                {
                    dead.push((tenant.clone(), id.clone()));
                }
            }
        }
        for (tenant, id) in dead {
            if let Some(docs) = inner.entities.get_mut(&tenant) {
                self.persist(T_ENTITIES, &key_bytes(&tenant, &id), None);
                docs.remove(&id);
                reaped += 1;
            }
        }
        let Inner {
            entities, temporal, ..
        } = &mut *inner;
        for (table, map) in [
            (
                T_ENTITIES,
                entities as &mut HashMap<String, BTreeMap<String, Value>>,
            ),
            (T_TEMPORAL_ENTITIES, temporal),
        ] {
            for (tenant, docs) in map.iter_mut() {
                for (id, doc) in docs.iter_mut() {
                    if prune_expired_instances(doc, now) {
                        reaped += 1;
                        self.persist(table, &key_bytes(tenant, id), Some(doc));
                    }
                }
            }
        }
        reaped
    }

    /// Tenants that hold any subscriptions (interval-firing scan).
    /// 5.5.10: a Tenant exists once any create operation implicitly created
    /// it (any resource kind was ever written under it).
    pub fn tenant_exists(&self, tenant: &TenantId) -> bool {
        let inner = self
            .inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        ALL_KINDS
            .iter()
            .any(|k| Self::map(&inner, *k).contains_key(tenant.as_str()))
    }

    /// Every tenant name, sorted; the default tenant is always present.
    pub fn tenant_ids(&self) -> Vec<String> {
        let inner = self
            .inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut names: std::collections::BTreeSet<String> =
            std::iter::once(TenantId::DEFAULT.to_string()).collect();
        for kind in ALL_KINDS {
            names.extend(Self::map(&inner, kind).keys().cloned());
        }
        names.into_iter().collect()
    }

    /// What one tenant holds. Existence is the caller's question — an
    /// unknown tenant simply counts zero of everything.
    pub fn tenant_stats_one(&self, tenant: &TenantId) -> antares_store::TenantStats {
        let inner = self
            .inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let n = |k: Kind| {
            Self::map(&inner, k)
                .get(tenant.as_str())
                .map_or(0, |m| m.len() as u64)
        };
        antares_store::TenantStats {
            entities: n(Kind::Entity),
            subscriptions: n(Kind::Subscription),
            registrations: n(Kind::Registration),
            csource_subscriptions: n(Kind::CSourceSubscription),
            snapshots: n(Kind::Snapshot),
            entity_maps: n(Kind::EntityMap),
            dist_subs: n(Kind::DistSub),
            created_at: None,
            tenant: tenant.as_str().to_owned(),
        }
    }

    /// Attribute instances held in the tenant's temporal documents.
    pub fn attr_instance_count(&self, tenant: &TenantId) -> u64 {
        let inner = self
            .inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner.temporal.get(tenant.as_str()).map_or(0, |docs| {
            docs.values()
                .filter_map(Value::as_object)
                .flat_map(|d| d.values().filter_map(Value::as_array))
                .map(|a| a.iter().filter(|i| i.is_object()).count() as u64)
                .sum()
        })
    }

    /// Drop every document of the given kinds for one tenant; `true` when
    /// the tenant held any of them. Persisted per key in `file` mode.
    pub fn purge_kinds(&self, tenant: &TenantId, kinds: &[Kind]) -> bool {
        on_blocking(|| {
            let mut inner = self.write_inner();
            let mut hit = false;
            for kind in kinds {
                if let Some(docs) = Self::map_mut(&mut inner, *kind).remove(tenant.as_str()) {
                    hit = true;
                    for id in docs.keys() {
                        self.persist(table_for(*kind), &key_bytes(tenant.as_str(), id), None);
                    }
                }
            }
            hit
        })
    }

    /// Remove the tenant from every kind, history included.
    pub fn purge_tenant(&self, tenant: &TenantId) -> bool {
        self.purge_kinds(tenant, &ALL_KINDS)
    }

    pub fn subscription_tenants(&self) -> Vec<String> {
        let inner = self
            .inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut out: Vec<String> = inner
            .subscriptions
            .iter()
            .chain(inner.csource_subscriptions.iter())
            .filter(|(_, m)| !m.is_empty())
            .map(|(t, _)| t.clone())
            .collect();
        out.sort();
        out.dedup();
        out
    }

    fn map(inner: &Inner, kind: Kind) -> &HashMap<String, BTreeMap<String, Value>> {
        match kind {
            Kind::Entity => &inner.entities,
            Kind::Subscription => &inner.subscriptions,
            Kind::Registration => &inner.registrations,
            Kind::CSourceSubscription => &inner.csource_subscriptions,
            Kind::Temporal => &inner.temporal,
            Kind::Snapshot => &inner.snapshots,
            Kind::EntityMap => &inner.entity_map_docs,
            Kind::DistSub => &inner.dist_subs,
            Kind::DeadLetter => &inner.dead_letters,
        }
    }

    fn map_mut(inner: &mut Inner, kind: Kind) -> &mut HashMap<String, BTreeMap<String, Value>> {
        match kind {
            Kind::Entity => &mut inner.entities,
            Kind::Subscription => &mut inner.subscriptions,
            Kind::Registration => &mut inner.registrations,
            Kind::CSourceSubscription => &mut inner.csource_subscriptions,
            Kind::Temporal => &mut inner.temporal,
            Kind::Snapshot => &mut inner.snapshots,
            Kind::EntityMap => &mut inner.entity_map_docs,
            Kind::DistSub => &mut inner.dist_subs,
            Kind::DeadLetter => &mut inner.dead_letters,
        }
    }

    /// 4.22: "expiresAt is defined as the system temporal Property at which a
    /// certain Entity, Property or Relationship shall become invalid." An
    /// entity past its expiry is invalid the moment the stamp passes, ahead
    /// of the sweep that physically reaps it, so every write path treats it
    /// as absent — otherwise the same id 404s on retrieve and 409s on create
    /// for a whole sweep interval. Only entities carry the entity-level
    /// stamp; subscriptions and registrations have their own expiry rules.
    fn is_expired(&self, inner: &Inner, kind: Kind, tenant: &str, id: &str) -> bool {
        kind == Kind::Entity
            && Self::map(inner, kind)
                .get(tenant)
                .and_then(|m| m.get(id))
                .is_some_and(|d| filter::expired_at(d, &now_stamp()))
    }

    /// Insert a new resource; `false` if the id already exists.
    pub fn create(&self, tenant: &TenantId, kind: Kind, id: &str, doc: Value) -> bool {
        let _order = (kind == Kind::Entity).then(|| self.emit_ordered());
        let created = on_blocking(|| {
            let mut inner = self.write_inner();
            let expired = self.is_expired(&inner, kind, tenant.as_str(), id);
            let m = Self::map_mut(&mut inner, kind)
                .entry(tenant.as_str().to_owned())
                .or_default();
            if m.contains_key(id) && !expired {
                false
            } else {
                m.insert(id.to_owned(), doc.clone());
                self.persist(table_for(kind), &key_bytes(tenant.as_str(), id), Some(&doc));
                true
            }
        });
        if created && kind == Kind::Entity {
            self.emit(tenant, None, Some(doc));
        }
        created
    }

    /// Insert or replace; returns `true` if it existed before. An expired
    /// entity did not (4.22), so the upsert reports CREATED and the caller
    /// answers 201 with a Location header instead of a silent 204.
    pub fn upsert(&self, tenant: &TenantId, kind: Kind, id: &str, doc: Value) -> bool {
        let _order = (kind == Kind::Entity).then(|| self.emit_ordered());
        let (prev, expired) = on_blocking(|| {
            let mut inner = self.write_inner();
            let expired = self.is_expired(&inner, kind, tenant.as_str(), id);
            let prev = Self::map_mut(&mut inner, kind)
                .entry(tenant.as_str().to_owned())
                .or_default()
                .insert(id.to_owned(), doc.clone());
            self.persist(table_for(kind), &key_bytes(tenant.as_str(), id), Some(&doc));
            (prev, expired)
        });
        let existed = prev.is_some() && !expired;
        let prev = if expired { None } else { prev };
        if kind == Kind::Entity {
            self.emit(tenant, prev, Some(doc));
        }
        existed
    }

    pub fn get(&self, tenant: &TenantId, kind: Kind, id: &str) -> Option<Value> {
        let inner = self
            .inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Self::map(&inner, kind)
            .get(tenant.as_str())
            .and_then(|m| m.get(id))
            .cloned()
    }

    /// An expired entity is already invalid (4.22): deleting it is a 404,
    /// the same answer retrieving it gives, not a 204 for something the API
    /// stopped serving. The row itself still goes — the sweep would take it
    /// anyway, and leaving it would resurrect the 409.
    pub fn delete(&self, tenant: &TenantId, kind: Kind, id: &str) -> bool {
        let _order = (kind == Kind::Entity).then(|| self.emit_ordered());
        let removed = on_blocking(|| {
            let mut inner = self.write_inner();
            // an expired doc is left in place for the sweep to reap: removing
            // it here without persisting the removal would resurrect it on
            // the next boot of a `file`-mode store
            if self.is_expired(&inner, kind, tenant.as_str(), id) {
                return None;
            }
            let removed = Self::map_mut(&mut inner, kind)
                .get_mut(tenant.as_str())
                .and_then(|m| m.remove(id));
            if removed.is_some() {
                self.persist(table_for(kind), &key_bytes(tenant.as_str(), id), None);
            }
            removed
        });
        let hit = removed.is_some();
        if kind == Kind::Entity {
            if let Some(old) = removed {
                self.emit(tenant, Some(old), None);
            }
        }
        hit
    }

    /// Snapshot of all docs of a kind for one tenant (id order).
    pub fn list(&self, tenant: &TenantId, kind: Kind) -> Vec<Value> {
        let inner = self
            .inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Self::map(&inner, kind)
            .get(tenant.as_str())
            .map(|m| m.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Read-modify-write on one document. Returns `None` when absent; the
    /// closure's error aborts without writing.
    pub fn mutate<T, E>(
        &self,
        tenant: &TenantId,
        kind: Kind,
        id: &str,
        f: impl FnOnce(&mut Value) -> Result<T, E>,
    ) -> Option<Result<T, E>> {
        let _order = (kind == Kind::Entity).then(|| self.emit_ordered());
        let (result, change) = on_blocking(|| {
            let mut inner = self.write_inner();
            if self.is_expired(&inner, kind, tenant.as_str(), id) {
                return None; // 4.22: invalid, so absent
            }
            let doc = Self::map_mut(&mut inner, kind)
                .get_mut(tenant.as_str())
                .and_then(|m| m.get_mut(id))?;
            let before = doc.clone();
            let mut candidate = doc.clone();
            Some(match f(&mut candidate) {
                Ok(t) => {
                    if candidate != before {
                        self.persist(
                            table_for(kind),
                            &key_bytes(tenant.as_str(), id),
                            Some(&candidate),
                        );
                    }
                    let change = (kind == Kind::Entity && candidate != before)
                        .then(|| (before, candidate.clone()));
                    *doc = candidate;
                    (Ok(t), change)
                }
                Err(e) => (Err(e), None),
            })
        })?;
        if let Some((b, a)) = change {
            self.emit(tenant, Some(b), Some(a));
        }
        Some(result)
    }

    // jsonldContexts: one keyspace for the whole process (key = context id, no
    // tenant prefix) — Cached rows are copies of public documents shared by
    // every tenant. Ownership of the tenant-authored kinds (Hosted,
    // ImplicitlyCreated, 5.13.1) travels in the document's "owner" member and
    // is enforced where the entries are served, listed and deleted (5.13).
    pub fn context_put(&self, id: &str, doc: Value) {
        on_blocking(|| {
            let mut inner = self.write_inner();
            self.persist(T_JSONLD_CONTEXTS, id.as_bytes(), Some(&doc));
            let cached = doc.get("kind").and_then(Value::as_str) == Some("Cached");
            inner.contexts.insert(id.to_owned(), doc);
            // 5.13.1: "Implementations shall periodically invalidate the
            // 'Cached' @contexts." One entry is stored per distinct external
            // URL a request references, so without a ceiling a client that
            // references fresh URLs grows this keyspace (and, in `file` mode,
            // the store on disk) forever. Oldest-first, and only for the
            // Cached kind — Hosted/ImplicitlyCreated entries are resources
            // the broker serves on demand, never a cache.
            if cached && inner.contexts.len() > MAX_CACHED_CONTEXTS {
                let dead = oldest_cached(&inner.contexts);
                for id in dead {
                    self.persist(T_JSONLD_CONTEXTS, id.as_bytes(), None);
                    inner.contexts.remove(&id);
                }
            }
        });
    }

    pub fn context_get(&self, id: &str) -> Option<Value> {
        self.inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contexts
            .get(id)
            .cloned()
    }

    pub fn context_delete(&self, id: &str) -> bool {
        on_blocking(|| {
            let mut inner = self.write_inner();
            let hit = inner.contexts.remove(id).is_some();
            if hit {
                self.persist(T_JSONLD_CONTEXTS, id.as_bytes(), None);
            }
            hit
        })
    }

    pub fn context_list(&self) -> Vec<Value> {
        self.inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contexts
            .values()
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A change hook that PANICS must cost only its own caller: the panic
    /// unwinds through that writer, and every later writer finds the store
    /// usable — locks recover from the poisoning instead of turning one bad
    /// hook into a dead store.
    #[test]
    fn a_panicking_hook_leaves_the_store_usable() {
        use std::sync::Arc;
        let s = Arc::new(Store::default());
        s.set_change_hook(Box::new(|_t, _b, after| {
            if after.as_ref().and_then(|a| a.get("boom")).is_some() {
                panic!("hook panic");
            }
        }));
        let t = TenantId::new("hook-panic").expect("tenant");
        let s2 = s.clone();
        let t2 = t.clone();
        let poisoned = std::thread::spawn(move || {
            s2.upsert(
                &t2,
                Kind::Entity,
                "urn:x:boom",
                json!({"id": "urn:x:boom", "type": ["T"], "boom": true}),
            );
        })
        .join();
        assert!(poisoned.is_err(), "the hook's panic reaches its own caller");
        // the NEXT writer and reader are untouched
        assert!(s.create(
            &t,
            Kind::Entity,
            "urn:x:after",
            json!({"id": "urn:x:after", "type": ["T"]})
        ));
        assert!(
            s.get(&t, Kind::Entity, "urn:x:after").is_some(),
            "one panicking hook must not take the store down"
        );
        assert!(
            s.get(&t, Kind::Entity, "urn:x:boom").is_some(),
            "the write that triggered the panic still committed — the hook runs after the commit"
        );
    }

    /// Two writers to the same entity: the change hook fires in COMMIT
    /// order. The first writer's hook is gated open only after the second
    /// writer has had every chance to overtake it — if the second commit may
    /// emit before the first commit's emit, the notification pipeline sees
    /// the versions reversed and records stale state as newest.
    #[test]
    fn change_hook_fires_in_commit_order_per_entity() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        let s = Arc::new(Store::default());
        let seen: Arc<std::sync::Mutex<Vec<i64>>> = Arc::default();
        let entered = Arc::new(AtomicBool::new(false));
        let released = Arc::new(AtomicBool::new(false));
        {
            let (seen, entered, released) = (seen.clone(), entered.clone(), released.clone());
            s.set_change_hook(Box::new(move |_t, _b, after| {
                let v = after
                    .as_ref()
                    .and_then(|a| a.get("v"))
                    .and_then(Value::as_i64)
                    .unwrap_or(-1);
                if v == 1 {
                    entered.store(true, Ordering::SeqCst);
                    while !released.load(Ordering::SeqCst) {
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                }
                seen.lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(v);
            }));
        }
        let t = TenantId::new("emit-order").expect("tenant");
        let doc = |v: i64| json!({"id": "urn:x:1", "type": ["T"], "v": v});
        let s1 = s.clone();
        let t1c = t.clone();
        let w1 = std::thread::spawn(move || {
            s1.upsert(&t1c, Kind::Entity, "urn:x:1", doc(1));
        });
        while !entered.load(std::sync::atomic::Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        // writer 1 has committed and sits inside its hook. Writer 2 now has
        // every chance to commit AND emit before writer 1's emit finishes.
        let s2 = s.clone();
        let t2c = t.clone();
        let w2 = std::thread::spawn(move || {
            s2.upsert(&t2c, Kind::Entity, "urn:x:1", doc(2));
        });
        std::thread::sleep(std::time::Duration::from_millis(60));
        released.store(true, std::sync::atomic::Ordering::SeqCst);
        w1.join().expect("writer 1");
        w2.join().expect("writer 2");
        assert_eq!(
            *seen
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            vec![1, 2],
            "hooks must fire in commit order, or the consumer records stale state as newest"
        );
    }

    #[test]
    fn commit_queue_counts_writers() {
        // Every write passes through the counted critical section.
        let s = Store::default();
        assert_eq!(s.commit_queue(), (0, 0));
        let t = TenantId::new("t").unwrap();
        s.create(&t, Kind::Entity, "urn:a", json!({"id": "urn:a"}));
        let (depth, peak) = s.commit_queue();
        assert_eq!(depth, 0, "no writer in flight after the call returns");
        assert!(peak >= 1, "the write itself must register in the peak");
    }

    /// What a store reports about itself, for an operator reading
    /// `/q/health`: the memory and file modes are one backend with two
    /// durability shapes, and the health body must not present them as the
    /// same thing.
    #[test]
    fn a_store_reports_the_engine_it_actually_runs() {
        let dir = tempdir("engine");
        let mem = crate::store::any::AnyStore::Mem(Store::default());
        let file = crate::store::any::AnyStore::Mem(Store::open_file(&dir).expect("open"));
        assert_eq!(mem.version_info()["engine"], "memory");
        assert_eq!(file.version_info()["engine"], "redb");
    }

    /// The commit queue is a `file`-mode signal: it exists because redb has
    /// one writer and commits fsync through it. A pure in-memory store has no
    /// such committer, so it reports nothing rather than a number that would
    /// read as the same thing and mean something else.
    #[test]
    fn only_a_shadowed_store_reports_a_commit_queue() {
        use crate::store::any::AnyStore;
        assert_eq!(AnyStore::Mem(Store::default()).commit_queue(), None);
        let dir = tempdir("commit-queue");
        let s = Store::open_file(&dir).expect("open");
        assert!(
            AnyStore::Mem(s).commit_queue().is_some(),
            "a durable store reports the queue behind its single committer"
        );
    }

    #[test]
    fn tenant_isolation() {
        let s = Store::default();
        let t1 = TenantId::new("t1").unwrap();
        let t2 = TenantId::new("t2").unwrap();
        assert!(s.create(&t1, Kind::Entity, "urn:a", json!({"id": "urn:a"})));
        assert!(s.get(&t2, Kind::Entity, "urn:a").is_none());
        assert!(s.get(&t1, Kind::Entity, "urn:a").is_some());
        assert!(!s.create(&t1, Kind::Entity, "urn:a", json!({})));
        assert!(s.create(&t2, Kind::Entity, "urn:a", json!({})));
    }

    fn tempdir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("antares-store-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("tempdir");
        dir
    }

    /// Every kind + contexts round-trip through a close/reopen.
    #[test]
    fn file_mode_survives_reopen() {
        let dir = tempdir("reopen");
        let t = TenantId::new("tenant_a-1").expect("tenant");
        {
            let s = Store::open_file(&dir).expect("open");
            for kind in ALL_KINDS {
                assert!(s.create(&t, kind, "urn:x:1", json!({"kind": format!("{kind:?}")})));
            }
            s.context_put("ctx1", json!({"@context": {}}));
        }
        let s = Store::open_file(&dir).expect("reopen");
        for kind in ALL_KINDS {
            assert_eq!(
                s.get(&t, kind, "urn:x:1").expect("survives")["kind"],
                format!("{kind:?}")
            );
        }
        assert!(s.context_get("ctx1").is_some());
        // tenant isolation intact after rebuild
        assert!(s
            .get(&TenantId::default(), Kind::Entity, "urn:x:1")
            .is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Deletes and mutations reach redb — no phantom state after restart.
    #[test]
    fn file_mode_deletes_and_updates_persist() {
        let dir = tempdir("delete");
        let t = TenantId::default();
        {
            let s = Store::open_file(&dir).expect("open");
            s.create(&t, Kind::Entity, "urn:gone", json!({"n": 1}));
            s.create(&t, Kind::Entity, "urn:kept", json!({"n": 1}));
            assert!(s.delete(&t, Kind::Entity, "urn:gone"));
            let r: Option<Result<(), ()>> = s.mutate(&t, Kind::Entity, "urn:kept", |d| {
                d["n"] = json!(2);
                Ok(())
            });
            assert!(matches!(r, Some(Ok(()))));
            s.context_put("ctx", json!({}));
            assert!(s.context_delete("ctx"));
        }
        let s = Store::open_file(&dir).expect("reopen");
        assert!(
            s.get(&t, Kind::Entity, "urn:gone").is_none(),
            "phantom 409 trap"
        );
        assert_eq!(s.get(&t, Kind::Entity, "urn:kept").expect("kept")["n"], 2);
        assert!(s.context_get("ctx").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A future-format file refuses to load with a clear message.
    #[test]
    fn file_mode_refuses_format_mismatch() {
        let dir = tempdir("format");
        {
            let db = Database::create(dir.join("antares.redb")).expect("db");
            let mut tx = db.begin_write().expect("tx");
            tx.set_durability(Durability::Immediate).expect("dur");
            tx.open_table(T_META)
                .expect("meta")
                .insert("format", "999")
                .expect("insert");
            tx.commit().expect("commit");
        }
        let err = match Store::open_file(&dir) {
            Err(e) => e,
            Ok(_) => panic!("must refuse a format-999 file"),
        };
        assert!(err.contains("format 999"), "err: {err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A file that HOLDS rows but carries no format marker was written by a
    /// binary whose key/value shape this one cannot vouch for. Refusing is
    /// the whole point of the marker — serving it would answer requests from
    /// data that may be misread. An empty file, by contrast, is just a fresh
    /// one and gets the marker stamped.
    #[test]
    fn file_mode_refuses_data_without_a_format_marker() {
        let dir = tempdir("nomarker");
        {
            let db = Database::create(dir.join("antares.redb")).expect("db");
            let mut tx = db.begin_write().expect("tx");
            tx.set_durability(Durability::Immediate).expect("dur");
            {
                let mut t = tx.open_table(T_ENTITIES).expect("entities");
                let mut k = b"plain".to_vec();
                k.push(0);
                k.extend_from_slice(b"urn:e:1");
                let bytes = serde_json::to_vec(&json!({"id": "urn:e:1"})).expect("serialize");
                t.insert(k.as_slice(), bytes.as_slice()).expect("insert");
            }
            // deliberately NO meta table
            tx.commit().expect("commit");
        }
        let err = match Store::open_file(&dir) {
            Err(e) => e,
            Ok(_) => panic!("must refuse data with no format marker"),
        };
        assert!(err.contains("no format marker"), "err: {err}");
        assert!(err.contains("antares.redb"), "the file is named: {err}");
        let _ = std::fs::remove_dir_all(&dir);

        // an EMPTY unmarked file is a fresh store, not a refusal
        let fresh = tempdir("nomarker-empty");
        {
            let db = Database::create(fresh.join("antares.redb")).expect("db");
            let tx = db.begin_write().expect("tx");
            tx.commit().expect("commit");
        }
        Store::open_file(&fresh).expect("an empty file is a fresh store");
        let _ = std::fs::remove_dir_all(&fresh);
    }

    /// 4.22: "expiresAt is defined as the system temporal Property at which a
    /// certain Entity, Property or Relationship shall become invalid." The
    /// clause sanctions the DELETION lagging, not the invalidity — so the
    /// write paths must agree with the read boundary the instant the stamp
    /// passes. Before this, the same id was simultaneously a 404 on retrieve,
    /// a 409 on create and a 204 on delete for a whole sweep interval.
    #[test]
    fn an_expired_entity_is_absent_to_writes_too() {
        let s = Store::default();
        let t = TenantId::default();
        let dead = json!({"id": "urn:e", "type": ["T"], "expiresAt": "2000-01-01T00:00:00Z"});
        let live = json!({"id": "urn:l", "type": ["T"], "expiresAt": "2999-01-01T00:00:00Z"});
        assert!(s.create(&t, Kind::Entity, "urn:e", dead.clone()));
        assert!(s.create(&t, Kind::Entity, "urn:l", live.clone()));

        // patching or deleting something already invalid is a 404, not a 204
        assert!(s
            .mutate(&t, Kind::Entity, "urn:e", |_d| Ok::<(), ()>(()))
            .is_none());
        assert!(
            !s.delete(&t, Kind::Entity, "urn:e"),
            "expired delete is 404"
        );
        // …and creating over it succeeds instead of raising AlreadyExists
        assert!(
            s.create(&t, Kind::Entity, "urn:e", json!({"id": "urn:e", "n": 1})),
            "an expired id must not 409 a create"
        );
        assert_eq!(
            s.get(&t, Kind::Entity, "urn:e").expect("recreated")["n"],
            1,
            "the create must have replaced the expired document"
        );

        // an UNEXPIRED entity keeps every one of those answers
        assert!(!s.create(&t, Kind::Entity, "urn:l", live.clone()), "409");
        assert!(s
            .mutate(&t, Kind::Entity, "urn:l", |_d| Ok::<(), ()>(()))
            .is_some());
        assert!(s.delete(&t, Kind::Entity, "urn:l"));

        // upsert over an expired id reports CREATED (201 + Location), not
        // updated
        s.create(&t, Kind::Entity, "urn:x", dead.clone());
        assert!(
            !s.upsert(&t, Kind::Entity, "urn:x", json!({"id": "urn:x"})),
            "an expired id must upsert as created"
        );
        assert!(
            s.upsert(&t, Kind::Entity, "urn:x", json!({"id": "urn:x", "n": 2})),
            "and the live one that replaced it as updated"
        );

        // 4.22 is an ENTITY stamp: other kinds keep their own expiry rules
        assert!(s.create(&t, Kind::Subscription, "urn:s", dead.clone()));
        assert!(
            !s.create(&t, Kind::Subscription, "urn:s", dead),
            "a subscription id still 409s"
        );
        assert!(s.delete(&t, Kind::Subscription, "urn:s"));
    }

    /// The change hook drives every notification and all temporal
    /// auto-recording, so its contract is: create emits (None, Some), delete
    /// emits (Some, None), a real mutate emits both images — and, just as
    /// load-bearing, a NON-entity write and a no-op mutate emit NOTHING. A
    /// subscription leaking into the hook would be mirrored into temporal
    /// storage; a no-op emitting would re-notify every subscriber on every
    /// idempotent PATCH.
    #[test]
    fn the_change_hook_fires_for_entity_changes_only() {
        use std::sync::{Arc, Mutex};
        type Images = (Option<Value>, Option<Value>);
        let seen: Arc<Mutex<Vec<Images>>> = Arc::default();
        let s = Store::default();
        let t = TenantId::default();
        let rec = Arc::clone(&seen);
        s.set_change_hook(Box::new(move |_t, before, after| {
            rec.lock().expect("record").push((before, after));
        }));

        s.create(&t, Kind::Entity, "urn:e", json!({"id": "urn:e", "n": 1}));
        // a write on another kind must not reach the hook at all
        s.create(&t, Kind::Subscription, "urn:s", json!({"id": "urn:s"}));
        s.upsert(
            &t,
            Kind::Subscription,
            "urn:s",
            json!({"id": "urn:s", "n": 9}),
        );
        s.delete(&t, Kind::Subscription, "urn:s");
        // a mutate that changes nothing is not a change
        let _ = s.mutate(&t, Kind::Entity, "urn:e", |_d| Ok::<(), ()>(()));
        // a real one is
        let _ = s.mutate(&t, Kind::Entity, "urn:e", |d| {
            d["n"] = json!(2);
            Ok::<(), ()>(())
        });
        // an aborted mutate writes nothing and emits nothing
        let _ = s.mutate(&t, Kind::Entity, "urn:e", |d| {
            d["n"] = json!(3);
            Err::<(), &str>("no")
        });
        s.delete(&t, Kind::Entity, "urn:e");

        let seen = seen.lock().expect("read");
        assert_eq!(seen.len(), 3, "emitted: {seen:?}");
        assert!(seen[0].0.is_none() && seen[0].1.is_some(), "create");
        assert_eq!(seen[1].0.as_ref().expect("before")["n"], 1);
        assert_eq!(seen[1].1.as_ref().expect("after")["n"], 2);
        assert!(seen[2].0.is_some() && seen[2].1.is_none(), "delete");
        // the aborted mutate left the document alone
        assert!(s.get(&t, Kind::Entity, "urn:e").is_none());
    }

    /// The supported backup route is stop-copy — close the broker, copy
    /// the file, reopen the copy. This test IS that route.
    #[test]
    fn file_mode_stop_copy_backup_restores() {
        let dir = tempdir("backup");
        let restore = tempdir("restore");
        let t = TenantId::default();
        {
            let s = Store::open_file(&dir).expect("open");
            s.create(&t, Kind::Entity, "urn:b", json!({"v": 42}));
        } // broker stopped — file quiescent
        std::fs::copy(dir.join("antares.redb"), restore.join("antares.redb")).expect("copy");
        let s = Store::open_file(&restore).expect("open backup");
        assert_eq!(s.get(&t, Kind::Entity, "urn:b").expect("restored")["v"], 42);
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&restore);
    }

    /// 4.22: reaping an expired entity is a state change like any other, so
    /// it must be durable — after a sweep the doc stays gone across a reopen,
    /// whatever the tenant key on disk looks like, while a doc that has not
    /// expired survives both.
    #[test]
    fn file_mode_sweep_removals_persist() {
        let dir = tempdir("sweep-persist");
        // Seed the file directly: one tenant key the boot rebuild accepts but
        // `TenantId::new` rejects, alongside an ordinary one.
        {
            let db = Database::create(dir.join("antares.redb")).expect("db");
            let mut tx = db.begin_write().expect("tx");
            tx.set_durability(Durability::Immediate).expect("dur");
            {
                let mut m = tx.open_table(T_META).expect("meta");
                m.insert("format", FORMAT_VERSION).expect("insert");
            }
            {
                let mut t = tx.open_table(T_ENTITIES).expect("entities");
                for (tenant, id, expires) in [
                    ("odd.tenant", "urn:e:1", "2000-01-01T00:00:00Z"),
                    ("odd.tenant", "urn:e:2", "2999-01-01T00:00:00Z"),
                    ("plain", "urn:e:3", "2000-01-01T00:00:00Z"),
                ] {
                    let mut k = tenant.as_bytes().to_vec();
                    k.push(0);
                    k.extend_from_slice(id.as_bytes());
                    let doc = json!({"id": id, "type": ["T"], "expiresAt": expires});
                    let bytes = serde_json::to_vec(&doc).expect("serialize");
                    t.insert(k.as_slice(), bytes.as_slice()).expect("insert");
                }
            }
            tx.commit().expect("commit");
        }
        {
            let s = Store::open_file(&dir).expect("open");
            assert_eq!(s.sweep_expired("2026-01-01T00:00:00Z"), 2, "both expired");
        }
        let s = Store::open_file(&dir).expect("reopen");
        let inner = s.inner.read().expect("lock");
        assert!(
            !inner
                .entities
                .get("odd.tenant")
                .is_some_and(|d| d.contains_key("urn:e:1")),
            "swept entity resurrected on reopen"
        );
        assert!(
            inner
                .entities
                .get("odd.tenant")
                .is_some_and(|d| d.contains_key("urn:e:2")),
            "unexpired entity must outlive the sweep"
        );
        assert!(
            !inner
                .entities
                .get("plain")
                .is_some_and(|d| d.contains_key("urn:e:3")),
            "swept entity resurrected on reopen"
        );
        drop(inner);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sweep_prunes_expired_attribute_instances() {
        let s = Store::default();
        let t = TenantId::default();
        s.create(
            &t,
            Kind::Entity,
            "urn:e",
            json!({"id": "urn:e", "type": ["T"],
                "attr": [
                    {"type": "Property", "value": 1, "expiresAt": "2000-01-01T00:00:00Z"},
                    {"type": "Property", "value": 2, "expiresAt": "2999-01-01T00:00:00Z"}],
                "gone": [
                    {"type": "Property", "value": 3, "expiresAt": "2000-01-01T00:00:00Z"}]}),
        );
        s.create(
            &t,
            Kind::Temporal,
            "urn:e",
            json!({"id": "urn:e", "type": ["T"],
                "attr": [
                    {"value": 1, "expiresAt": "2000-01-01T00:00:00Z"},
                    {"value": 2, "expiresAt": "2999-01-01T00:00:00Z"},
                    {"value": 3}]}),
        );
        assert_eq!(s.sweep_expired("2026-01-01T00:00:00Z"), 2);
        let e = s.get(&t, Kind::Entity, "urn:e").expect("entity survives");
        assert_eq!(e["attr"].as_array().expect("attr").len(), 1);
        assert_eq!(e["attr"][0]["value"], 2);
        assert!(e.get("gone").is_none(), "fully-expired attribute removed");
        let tp = s
            .get(&t, Kind::Temporal, "urn:e")
            .expect("temporal survives");
        let vals: Vec<i64> = tp["attr"]
            .as_array()
            .expect("instances")
            .iter()
            .map(|i| i["value"].as_i64().expect("value"))
            .collect();
        assert_eq!(vals, [2, 3], "expired pruned, no-expiry instance kept");
        assert_eq!(s.sweep_expired("2026-01-01T00:00:00Z"), 0, "idempotent");
    }

    /// 5.13.1: "Implementations shall periodically invalidate the 'Cached'
    /// @contexts." The memory/file arm holds one entry per distinct external
    /// URL a request referenced — client-controlled — so the same ceiling and
    /// oldest-first eviction the Pg arm applies holds here, and it applies to
    /// the Cached kind ONLY: a Hosted entry is client-owned data (5.13.2).
    #[test]
    fn clause_5_13_1_cached_contexts_are_capped_oldest_first() {
        let s = Store::default();
        let entry = |kind: &str, created: &str| json!({"kind": kind, "createdAt": created, "body": {"@context": {}}});
        // a Hosted entry older than every Cached one: age must not decide
        s.context_put("hosted", entry("Hosted", "2000-01-01T00:00:00Z"));
        for i in 0..MAX_CACHED_CONTEXTS {
            s.context_put(
                &format!("cached-{i:05}"),
                entry("Cached", &format!("2026-01-01T00:00:{:02}Z", i % 60)),
            );
        }
        let cached = |s: &Store| {
            s.context_list()
                .iter()
                .filter(|d| d["kind"] == "Cached")
                .count()
        };
        assert_eq!(
            cached(&s),
            MAX_CACHED_CONTEXTS,
            "at the ceiling, nothing lost"
        );

        // one more Cached entry evicts exactly one — the oldest
        s.context_put("cached-new", entry("Cached", "2026-06-01T00:00:00Z"));
        assert_eq!(cached(&s), MAX_CACHED_CONTEXTS, "the ceiling holds");
        assert!(s.context_get("cached-new").is_some(), "the new entry stays");
        assert!(
            s.context_get("cached-00000").is_none(),
            "the oldest Cached entry is the one evicted"
        );
        assert!(
            s.context_get("cached-00001").is_some(),
            "eviction stops at the ceiling"
        );
        assert!(
            s.context_get("hosted").is_some(),
            "a Hosted entry is never a candidate, however old"
        );
    }

    #[test]
    fn mutate_aborts_on_error() {
        let s = Store::default();
        let t = TenantId::default();
        s.create(&t, Kind::Entity, "urn:a", json!({"n": 1}));
        let r: Option<Result<(), &str>> = s.mutate(&t, Kind::Entity, "urn:a", |d| {
            d["n"] = json!(2);
            Err("nope")
        });
        assert!(matches!(r, Some(Err("nope"))));
        assert_eq!(s.get(&t, Kind::Entity, "urn:a").unwrap()["n"], 1);
    }

    /// A purged tenant is gone from every kind, the neighbour tenant is
    /// untouched, and in `file` mode the removal survives a reopen.
    #[test]
    fn purge_tenant_empties_every_kind_and_survives_reopen() {
        let dir = tempdir("purge");
        let a = TenantId::new("purge_a").expect("tenant");
        let b = TenantId::new("purge_b").expect("tenant");
        {
            let s = Store::open_file(&dir).expect("open");
            for t in [&a, &b] {
                for kind in ALL_KINDS {
                    assert!(s.create(t, kind, "urn:x:1", json!({"id": "urn:x:1"})));
                }
            }
            assert!(s.tenant_ids().iter().any(|t| t == "purge_a"), "listed");
            let row = s.tenant_stats_one(&a);
            assert_eq!((row.entities, row.subscriptions, row.dist_subs), (1, 1, 1));
            assert!(s.purge_tenant(&a));
            assert!(!s.purge_tenant(&a), "second purge finds nothing");
            assert!(!s.tenant_exists(&a));
            assert!(s.tenant_exists(&b));
            for kind in ALL_KINDS {
                assert!(s.get(&a, kind, "urn:x:1").is_none(), "{kind:?} row left");
                assert!(s.get(&b, kind, "urn:x:1").is_some(), "{kind:?} lost for b");
            }
            assert!(s.tenant_ids().iter().all(|t| t != "purge_a"));
            assert_eq!(
                s.tenant_stats_one(&a).entities,
                0,
                "a purged tenant counts nothing"
            );
        }
        let s = Store::open_file(&dir).expect("reopen");
        assert!(!s.tenant_exists(&a), "purge must be persisted");
        assert!(s.tenant_exists(&b));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
