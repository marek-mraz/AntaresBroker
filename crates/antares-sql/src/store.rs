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

pub mod pg_doc;
pub mod pg_entity;

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
    match tokio::runtime::Handle::try_current() {
        Ok(h) if h.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(f)
        }
        _ => f(),
    }
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
                    "data file {} has format {other}, this binary supports {FORMAT_VERSION} — \
                     refusing to start",
                    path.display()
                ));
            }
            None => {
                let rt = db.begin_read().map_err(|e| e.to_string())?;
                for kind in ALL_KINDS {
                    if let Ok(t) = rt.open_table(table_for(kind)) {
                        if t.len().map_err(|e| e.to_string())? > 0 {
                            return Err(format!(
                                "data file {} holds data but no format marker — refusing to start",
                                path.display()
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
                    t.insert("format", FORMAT_VERSION).map_err(|e| e.to_string())?;
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

    pub fn set_change_hook(&self, h: ChangeHook) {
        *self.hook.write().expect("hook lock") = Some(h);
    }

    fn emit(&self, tenant: &TenantId, before: Option<Value>, after: Option<Value>) {
        if let Some(h) = self.hook.read().expect("hook lock").as_ref() {
            h(tenant, before, after);
        }
    }

    /// Tenants that hold any subscriptions (interval-firing scan).
    pub fn subscription_tenants(&self) -> Vec<String> {
        let inner = self.inner.read().expect("store lock");
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

    fn map<'a>(inner: &'a Inner, kind: Kind) -> &'a HashMap<String, BTreeMap<String, Value>> {
        match kind {
            Kind::Entity => &inner.entities,
            Kind::Subscription => &inner.subscriptions,
            Kind::Registration => &inner.registrations,
            Kind::CSourceSubscription => &inner.csource_subscriptions,
            Kind::Temporal => &inner.temporal,
        }
    }

    fn map_mut<'a>(
        inner: &'a mut Inner,
        kind: Kind,
    ) -> &'a mut HashMap<String, BTreeMap<String, Value>> {
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
            let mut inner = self.inner.write().expect("store lock");
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
            let mut inner = self.inner.write().expect("store lock");
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
        let inner = self.inner.read().expect("store lock");
        Self::map(&inner, kind)
            .get(tenant.as_str())
            .and_then(|m| m.get(id))
            .cloned()
    }

    pub fn delete(&self, tenant: &TenantId, kind: Kind, id: &str) -> bool {
        let removed = on_blocking(|| {
            let mut inner = self.inner.write().expect("store lock");
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
        let inner = self.inner.read().expect("store lock");
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
            let mut inner = self.inner.write().expect("store lock");
            let Some(doc) = Self::map_mut(&mut inner, kind)
                .get_mut(tenant.as_str())
                .and_then(|m| m.get_mut(id))
            else {
                return None;
            };
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
            let mut inner = self.inner.write().expect("store lock");
            self.persist(T_JSONLD_CONTEXTS, id.as_bytes(), Some(&doc));
            inner.contexts.insert(id.to_owned(), doc);
        });
    }

    pub fn context_get(&self, id: &str) -> Option<Value> {
        self.inner.read().expect("store lock").contexts.get(id).cloned()
    }

    pub fn context_delete(&self, id: &str) -> bool {
        on_blocking(|| {
            let mut inner = self.inner.write().expect("store lock");
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
            .expect("store lock")
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
        assert!(s.get(&TenantId::default(), Kind::Entity, "urn:x:1").is_none());
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
        assert!(s.get(&t, Kind::Entity, "urn:gone").is_none(), "phantom 409 trap");
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
