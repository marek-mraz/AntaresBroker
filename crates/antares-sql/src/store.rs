//! In-memory store — v0 storage backend.
//!
//! DELIBERATE DEVIATION (recorded in docs/adr/): deep-analysis.md targets
//! Postgres; the suite-green loop uses an in-memory backend first. The store
//! API is shaped like §9.3's store traits (tenant first parameter everywhere)
//! so the sqlx implementation can land behind the same seam.
//!
//! Documents are held in the *internal expanded form* produced by
//! `antares_jsonld::expand` (IRI keys, instance arrays), with server-managed
//! timestamps embedded (`createdAt`/`modifiedAt` at entity level and inside
//! each attribute instance) — output layers strip them unless sysAttrs.

pub mod any;
#[cfg(feature = "postgres")]
pub mod entity_map;
pub mod filter;
#[cfg(feature = "postgres")]
pub mod outbox;
#[cfg(feature = "postgres")]
pub mod pg_doc;
#[cfg(feature = "postgres")]
pub mod pg_entity;
#[cfg(feature = "postgres")]
pub mod pg_temporal;

use antares_model::TenantId;
use redb::{Database, Durability, ReadableDatabase, ReadableTableMetadata, TableDefinition};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::RwLock;

// ---- `file` mode: redb write-through shadow (tasks.md §B) ------------------
//
// redb is durability only — queries and the matcher keep running on the
// in-memory maps. Every mutation commits to redb (Durability::Immediate,
// fsync) INSIDE the store's write-critical section, so redb apply order is
// exactly memory apply order, and the commit happens before the store call
// returns — i.e. before the HTTP ack (commit-before-ack, B3). Boot rebuilds
// the maps from the file (B4) and refuses to start on a format mismatch (B11).
//
// Table per resource family, names per §9.1 (spec resource, snake_cased).
// The v0 memory store keeps one temporal doc per entity, so `attr_instances`
// has no separate table; entityMaps are TTL-ephemeral and not durable state.
const T_ENTITIES: TableDefinition<&[u8], &[u8]> = TableDefinition::new("entities");
const T_SUBSCRIPTIONS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("subscriptions");
const T_CSOURCE_REGISTRATIONS: TableDefinition<&[u8], &[u8]> =
    TableDefinition::new("csource_registrations");
const T_CSOURCE_SUBSCRIPTIONS: TableDefinition<&[u8], &[u8]> =
    TableDefinition::new("csource_subscriptions");
const T_TEMPORAL_ENTITIES: TableDefinition<&[u8], &[u8]> =
    TableDefinition::new("temporal_entities");
const T_JSONLD_CONTEXTS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("jsonld_contexts");
const T_META: TableDefinition<&str, &str> = TableDefinition::new("meta");
/// On-disk format version (B11): bump on any key/value shape change; an older
/// or newer file refuses to load rather than being misread as valid data.
const FORMAT_VERSION: &str = "1";

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

fn table_for(kind: Kind) -> TableDefinition<'static, &'static [u8], &'static [u8]> {
    match kind {
        Kind::Entity => T_ENTITIES,
        Kind::Subscription => T_SUBSCRIPTIONS,
        Kind::Registration => T_CSOURCE_REGISTRATIONS,
        Kind::CSourceSubscription => T_CSOURCE_SUBSCRIPTIONS,
        Kind::Temporal => T_TEMPORAL_ENTITIES,
    }
}

const ALL_KINDS: [Kind; 5] = [
    Kind::Entity,
    Kind::Subscription,
    Kind::Registration,
    Kind::CSourceSubscription,
    Kind::Temporal,
];

/// Key = `tenant \0 id` (B2). Unambiguous: TenantId is `[A-Za-z0-9_-]{1,64}`
/// by construction, so it can never contain the separator.
fn key_bytes(tenant: &TenantId, id: &str) -> Vec<u8> {
    let mut k = Vec::with_capacity(tenant.as_str().len() + 1 + id.len());
    k.extend_from_slice(tenant.as_str().as_bytes());
    k.push(0);
    k.extend_from_slice(id.as_bytes());
    k
}

fn split_key(key: &[u8]) -> Option<(String, String)> {
    let pos = key.iter().position(|&b| b == 0)?;
    Some((
        String::from_utf8(key[..pos].to_vec()).ok()?,
        String::from_utf8(key[pos + 1..].to_vec()).ok()?,
    ))
}

/// Run `f` off the tokio worker pool when called from a multi-thread runtime
/// (B1b): a per-commit fsync must never stall an async worker. Outside a
/// runtime (unit tests, startup) it just runs inline.
fn on_blocking<T>(f: impl FnOnce() -> T) -> T {
    // wasm32 (§N): single-threaded, no tokio runtime — always inline.
    #[cfg(not(target_arch = "wasm32"))]
    if let Ok(h) = tokio::runtime::Handle::try_current() {
        if h.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread {
            return tokio::task::block_in_place(f);
        }
    }
    f()
}

struct Shadow {
    db: Database,
}

impl Shadow {
    /// One txn per mutation, fsynced before return (B3). A failed commit
    /// aborts the process: the alternative is acking writes the file does not
    /// hold, which is the one lie a durable store must never tell.
    /// (`ponytail:` abort-on-commit-failure; per-request error plumbing only
    /// if a recoverable commit failure mode ever shows up in practice.)
    fn write(&self, table: TableDefinition<&[u8], &[u8]>, key: &[u8], value: Option<&[u8]>) {
        let result = (|| -> Result<(), String> {
            let mut tx = self.db.begin_write().map_err(|e| e.to_string())?;
            tx.set_durability(Durability::Immediate)
                .map_err(|e| e.to_string())?;
            {
                let mut t = tx.open_table(table).map_err(|e| e.to_string())?;
                match value {
                    Some(v) => {
                        t.insert(key, v).map_err(|e| e.to_string())?;
                    }
                    None => {
                        t.remove(key).map_err(|e| e.to_string())?;
                    }
                }
            }
            tx.commit().map_err(|e| e.to_string())
        })();
        if let Err(e) = result {
            tracing::error!("redb commit failed: {e} — aborting: an acked write must be durable");
            std::process::abort();
        }
    }
}

/// Called with (tenant, before, after) on every entity write — the local-mode
/// change feed (§7): create ⇒ (None, Some), delete ⇒ (Some, None).
pub type ChangeHook = Box<dyn Fn(&TenantId, Option<Value>, Option<Value>) + Send + Sync>;

#[derive(Default)]
pub struct Store {
    inner: RwLock<Inner>,
    hook: RwLock<Option<ChangeHook>>,
    /// `file` mode durability shadow; `None` = pure in-memory (`memory` mode).
    shadow: Option<Shadow>,
    /// B13: writers currently queued behind the single write-critical section
    /// (redb has ONE writer, so fsync commits serialize here). Exported via
    /// /q/health; the group-commit lever only gets built if a benchmark shows
    /// this depth sustained at the measured ~3.1k writes/s ceiling.
    write_waiters: std::sync::atomic::AtomicUsize,
    write_waiters_peak: std::sync::atomic::AtomicUsize,
}

#[derive(Default)]
struct Inner {
    /// tenant → id → internal entity doc (BTreeMap: deterministic list order).
    entities: HashMap<String, BTreeMap<String, Value>>,
    subscriptions: HashMap<String, BTreeMap<String, Value>>,
    registrations: HashMap<String, BTreeMap<String, Value>>,
    csource_subscriptions: HashMap<String, BTreeMap<String, Value>>,
    /// tenant → entity id → temporal doc (attr IRI → instance array).
    temporal: HashMap<String, BTreeMap<String, Value>>,
    /// hosted/cached @context documents, shared across tenants by design (§8.3).
    contexts: BTreeMap<String, Value>,
}

/// Which resource family an operation touches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Entity,
    Subscription,
    Registration,
    CSourceSubscription,
    Temporal,
}

impl Store {
    /// Open (or create) the `file`-mode store: redb at `dir/antares.redb`,
    /// format-checked (B11), in-memory maps rebuilt from the file (B4).
    /// Any open/format/decode error refuses to start — never silently serve
    /// partial data.
    pub fn open_file(dir: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
        let path = dir.join("antares.redb");
        let db = Database::create(&path).map_err(|e| format!("open {}: {e}", path.display()))?;
        Self::from_database(db, &path.display().to_string())
    }

    /// The target-independent half of `open_file` (N4): format check (B11) +
    /// boot rebuild (B4) over an already-constructed redb `Database`. The
    /// browser build calls this with an OPFS-backed database — same shadow,
    /// same commit-before-ack, different `StorageBackend`.
    pub fn from_database(db: Database, label: &str) -> Result<Self, String> {
        // B11: format marker. Absent marker + existing data = a file this
        // binary cannot vouch for; refuse rather than guess.
        let stored_format = {
            let rt = db.begin_read().map_err(|e| e.to_string())?;
            match rt.open_table(T_META) {
                Ok(t) => t
                    .get("format")
                    .map_err(|e| e.to_string())?
                    .map(|v| v.value().to_owned()),
                Err(redb::TableError::TableDoesNotExist(_)) => None,
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

        // B4: boot rebuild — scan every table into the in-memory maps.
        let mut inner = Inner::default();
        let rt = db.begin_read().map_err(|e| e.to_string())?;
        for kind in ALL_KINDS {
            let table = match rt.open_table(table_for(kind)) {
                Ok(t) => t,
                Err(redb::TableError::TableDoesNotExist(_)) => continue,
                Err(e) => return Err(e.to_string()),
            };
            let map = match kind {
                Kind::Entity => &mut inner.entities,
                Kind::Subscription => &mut inner.subscriptions,
                Kind::Registration => &mut inner.registrations,
                Kind::CSourceSubscription => &mut inner.csource_subscriptions,
                Kind::Temporal => &mut inner.temporal,
            };
            for row in redb::ReadableTable::iter(&table).map_err(|e| e.to_string())? {
                let (k, v) = row.map_err(|e| e.to_string())?;
                let (tenant, id) = split_key(k.value())
                    .ok_or_else(|| format!("undecodable key in table {kind:?}"))?;
                let doc: Value = serde_json::from_slice(v.value())
                    .map_err(|e| format!("undecodable value for {tenant}/{id}: {e}"))?;
                map.entry(tenant).or_default().insert(id, doc);
            }
        }
        if let Ok(t) = rt.open_table(T_JSONLD_CONTEXTS) {
            for row in redb::ReadableTable::iter(&t).map_err(|e| e.to_string())? {
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
            shadow: Some(Shadow { db }),
            write_waiters: Default::default(),
            write_waiters_peak: Default::default(),
        })
    }

    /// Write-through for one doc (must be called inside the write-critical
    /// section so redb order equals memory order).
    fn persist(&self, table: TableDefinition<&[u8], &[u8]>, key: &[u8], doc: Option<&Value>) {
        if let Some(shadow) = &self.shadow {
            let bytes = doc.map(|d| serde_json::to_vec(d).expect("serialize doc"));
            shadow.write(table, key, bytes.as_deref());
        }
    }

    /// B13: acquire the write-critical section, counting queued writers.
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

    /// B13: (currently queued writers, peak since start). The peak going
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
    pub fn sweep_expired(&self, now: &str) -> usize {
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
            if let Ok(t) = TenantId::new(&tenant) {
                self.persist(T_ENTITIES, &key_bytes(&t, &id), None);
            }
            if let Some(docs) = inner.entities.get_mut(&tenant) {
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
                        if let Ok(t) = TenantId::new(tenant) {
                            self.persist(table, &key_bytes(&t, id), Some(doc));
                        }
                    }
                }
            }
        }
        reaped
    }

    /// Tenants that hold any subscriptions (interval-firing scan).
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
        }
    }

    fn map_mut(inner: &mut Inner, kind: Kind) -> &mut HashMap<String, BTreeMap<String, Value>> {
        match kind {
            Kind::Entity => &mut inner.entities,
            Kind::Subscription => &mut inner.subscriptions,
            Kind::Registration => &mut inner.registrations,
            Kind::CSourceSubscription => &mut inner.csource_subscriptions,
            Kind::Temporal => &mut inner.temporal,
        }
    }

    /// Insert a new resource; `false` if the id already exists.
    pub fn create(&self, tenant: &TenantId, kind: Kind, id: &str, doc: Value) -> bool {
        let created = on_blocking(|| {
            let mut inner = self.write_inner();
            let m = Self::map_mut(&mut inner, kind)
                .entry(tenant.as_str().to_owned())
                .or_default();
            if m.contains_key(id) {
                false
            } else {
                m.insert(id.to_owned(), doc.clone());
                self.persist(table_for(kind), &key_bytes(tenant, id), Some(&doc));
                true
            }
        });
        if created && kind == Kind::Entity {
            self.emit(tenant, None, Some(doc));
        }
        created
    }

    /// Insert or replace; returns `true` if it existed before.
    pub fn upsert(&self, tenant: &TenantId, kind: Kind, id: &str, doc: Value) -> bool {
        let prev = on_blocking(|| {
            let mut inner = self.write_inner();
            let prev = Self::map_mut(&mut inner, kind)
                .entry(tenant.as_str().to_owned())
                .or_default()
                .insert(id.to_owned(), doc.clone());
            self.persist(table_for(kind), &key_bytes(tenant, id), Some(&doc));
            prev
        });
        let existed = prev.is_some();
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

    pub fn delete(&self, tenant: &TenantId, kind: Kind, id: &str) -> bool {
        let removed = on_blocking(|| {
            let mut inner = self.write_inner();
            let removed = Self::map_mut(&mut inner, kind)
                .get_mut(tenant.as_str())
                .and_then(|m| m.remove(id));
            if removed.is_some() {
                self.persist(table_for(kind), &key_bytes(tenant, id), None);
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
        let (result, change) = on_blocking(|| {
            let mut inner = self.write_inner();
            let doc = Self::map_mut(&mut inner, kind)
                .get_mut(tenant.as_str())
                .and_then(|m| m.get_mut(id))?;
            let before = doc.clone();
            let mut candidate = doc.clone();
            Some(match f(&mut candidate) {
                Ok(t) => {
                    if candidate != before {
                        self.persist(table_for(kind), &key_bytes(tenant, id), Some(&candidate));
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

    // jsonldContexts (cross-tenant by design; key = context id, no tenant prefix)
    pub fn context_put(&self, id: &str, doc: Value) {
        on_blocking(|| {
            let mut inner = self.write_inner();
            self.persist(T_JSONLD_CONTEXTS, id.as_bytes(), Some(&doc));
            inner.contexts.insert(id.to_owned(), doc);
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

    #[test]
    fn commit_queue_counts_writers() {
        // B13: every write passes through the counted critical section.
        let s = Store::default();
        assert_eq!(s.commit_queue(), (0, 0));
        let t = TenantId::new("t").unwrap();
        s.create(&t, Kind::Entity, "urn:a", json!({"id": "urn:a"}));
        let (depth, peak) = s.commit_queue();
        assert_eq!(depth, 0, "no writer in flight after the call returns");
        assert!(peak >= 1, "the write itself must register in the peak");
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

    /// B8/B4: every kind + contexts round-trip through a close/reopen.
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

    /// B10: deletes and mutations reach redb — no phantom state after restart.
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

    /// B11: a future-format file refuses to load with a clear message.
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

    /// B12: the supported backup route is stop-copy — close the broker, copy
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
}
