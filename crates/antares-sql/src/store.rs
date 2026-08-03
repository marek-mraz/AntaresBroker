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

use antares_model::TenantId;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::sync::RwLock;

/// Called with (tenant, before, after) on every entity write — the local-mode
/// change feed (§7): create ⇒ (None, Some), delete ⇒ (Some, None).
pub type ChangeHook = Box<dyn Fn(&TenantId, Option<Value>, Option<Value>) + Send + Sync>;

#[derive(Default)]
pub struct Store {
    inner: RwLock<Inner>,
    hook: RwLock<Option<ChangeHook>>,
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
        let created = {
            let mut inner = self.inner.write().expect("store lock");
            let m = Self::map_mut(&mut inner, kind)
                .entry(tenant.as_str().to_owned())
                .or_default();
            if m.contains_key(id) {
                false
            } else {
                m.insert(id.to_owned(), doc.clone());
                true
            }
        };
        if created && kind == Kind::Entity {
            self.emit(tenant, None, Some(doc));
        }
        created
    }

    /// Insert or replace; returns `true` if it existed before.
    pub fn upsert(&self, tenant: &TenantId, kind: Kind, id: &str, doc: Value) -> bool {
        let prev = {
            let mut inner = self.inner.write().expect("store lock");
            Self::map_mut(&mut inner, kind)
                .entry(tenant.as_str().to_owned())
                .or_default()
                .insert(id.to_owned(), doc.clone())
        };
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
        let removed = {
            let mut inner = self.inner.write().expect("store lock");
            Self::map_mut(&mut inner, kind)
                .get_mut(tenant.as_str())
                .and_then(|m| m.remove(id))
        };
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
        let (result, change) = {
            let mut inner = self.inner.write().expect("store lock");
            let doc = Self::map_mut(&mut inner, kind)
                .get_mut(tenant.as_str())?
                .get_mut(id)?;
            let before = doc.clone();
            let mut candidate = doc.clone();
            match f(&mut candidate) {
                Ok(t) => {
                    let change = (kind == Kind::Entity && candidate != before)
                        .then(|| (before, candidate.clone()));
                    *doc = candidate;
                    (Ok(t), change)
                }
                Err(e) => (Err(e), None),
            }
        };
        if let Some((b, a)) = change {
            self.emit(tenant, Some(b), Some(a));
        }
        Some(result)
    }

    // jsonldContexts (cross-tenant by design)
    pub fn context_put(&self, id: &str, doc: Value) {
        self.inner
            .write()
            .expect("store lock")
            .contexts
            .insert(id.to_owned(), doc);
    }

    pub fn context_get(&self, id: &str) -> Option<Value> {
        self.inner.read().expect("store lock").contexts.get(id).cloned()
    }

    pub fn context_delete(&self, id: &str) -> bool {
        self.inner
            .write()
            .expect("store lock")
            .contexts
            .remove(id)
            .is_some()
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
