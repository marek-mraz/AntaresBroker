//! Subscription matching + HTTP notification delivery (5.8.6, 5.3.1).
//!
//! Change detection: the store's change hook feeds every entity write here as
//! a (before, after) pair; attribute-level changes are derived by diffing —
//! one hook point instead of one call per write handler.
//!
//! L3 (§1.1): candidate lookup is index-shaped. `SubMirror` keeps inverted
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
const ENTITY_META: &[&str] = &["id", "type", "scope", "createdAt", "modifiedAt", "@context"];

#[derive(Clone, Copy, PartialEq, Debug)]
enum ChangeClass {
    Created,
    Updated,
    Deleted,
}

/// F4/F5: a per-instance tenant-keyed document mirror (bus=nats). One
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

/// L3: the subscription mirror — docs plus the inverted candidate index.
///
/// Bucketing per subscription (conservative, union-of-buckets = candidates):
/// - has an `entities` selector whose every entry names a plain expanded
///   type IRI → `by_type[iri]` (idPattern/watchedAttributes narrow FURTHER,
///   so type is the widest exact key);
/// - no selector but `watchedAttributes` → `by_attr[iri]` (such a sub can
///   only fire when a watched attribute changed — 5.8.6);
/// - anything else (4.17 selection expressions, shapes the index cannot
///   prove) → `broad`, evaluated on every change.
#[derive(Default)]
pub struct SubMirror {
    map: std::sync::RwLock<std::collections::HashMap<String, TenantIndex>>,
}

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

    /// The L3 hot path: subscriptions that could possibly fire for a change
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
/// modes wire one — L3), with the store scan only as the never-wired
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
    // L3 (bus=local): the same indexed mirror the nats wiring builds, fed
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

    let (tx, mut rx) =
        tokio::sync::mpsc::unbounded_channel::<(String, Option<Value>, Option<Value>)>();
    // Temporal auto-recording runs SYNCHRONOUSLY on the hook (read-your-writes:
    // the ETSI suite queries history immediately after a write); the matcher
    // work is handed to the async task below. One choke point for every write.
    let st_rec = state.clone();
    state
        .store
        .set_change_hook(Box::new(move |tenant, before, after| {
            record_temporal_change(&st_rec, tenant, before.as_ref(), after.as_ref());
            let _ = tx.send((tenant.as_str().to_owned(), before, after));
        }));
    let st = state.clone();
    crate::spawn(async move {
        while let Some((tenant, before, after)) = rx.recv().await {
            process_change(&st, &tenant, before, after).await;
        }
    });
    let st = state.clone();
    crate::spawn(async move {
        loop {
            // N2: tokio's timer natively; the browser's own timer on wasm32
            // (tokio time never fires without a reactor there).
            #[cfg(not(target_arch = "wasm32"))]
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            #[cfg(target_arch = "wasm32")]
            gloo_timers::future::TimeoutFuture::new(500).await;
            interval_tick(&st).await;
        }
    });
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
fn selector_match(sub: &Value, doc: &Value, ctx: &Context) -> bool {
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
            || e.get("idPattern")
                .and_then(Value::as_str)
                .is_none_or(|p| regex::Regex::new(p).is_ok_and(|re| re.find(id).is_some()));
        t_ok && id_ok && pat_ok
    })
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
        match antares_ql::parse_q(&q) {
            Ok(node) => {
                // 4.9 EXAMPLE 13/14: linked-entity q terms (attr{path})
                // resolve through the local store, same tenant
                let lookup = |uri: &str| st.store.get(tenant, Kind::Entity, uri).ok().flatten();
                if !crate::qeval::eval_q(&node, doc, ctx, &lookup) {
                    return false;
                }
            }
            Err(_) => return false,
        }
    }
    if let Some(sq) = sub_str(sub, "scopeQ") {
        if !crate::scope_matches(sq, doc) {
            return false;
        }
    }
    if let Some(g) = sub.get("geoQ").and_then(Value::as_object) {
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
        match crate::geo::GeoQuery::from_params(&params) {
            Ok(Some(gq)) => {
                if !gq.matches(doc, ctx) {
                    return false;
                }
            }
            _ => return false,
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

struct NotifShape {
    repr: crate::repr::Repr,
    show_changes: bool,
    join: Option<(String, usize)>,
}

fn notif_shape(sub: &Value, ctx: &Context) -> NotifShape {
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
    // L3: candidate lookup by the entity's types and the changed attribute
    // IRIs — the linear scan §1.1 forbids at 10k subs is gone.
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
        let triggers: Vec<String> = sub
            .get("notificationTrigger")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_else(|| DEFAULT_TRIGGERS.iter().map(|s| s.to_string()).collect());
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

/// timeInterval subscriptions: fire when due, with all matching entities.
/// F6 multi-instance: claim one interval firing under the subscription row
/// lock — N matcher pods race, exactly one wins (§3.1.6: single-winner by
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
        let anchor = doc
            .get("notification")
            .and_then(|n| n.get("lastNotification"))
            .and_then(Value::as_str)
            .or_else(|| doc.get("createdAt").and_then(Value::as_str));
        let due = match anchor.and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok()) {
            Some(last) => {
                (chrono::Utc::now() - last.with_timezone(&chrono::Utc)).num_milliseconds()
                    >= (interval * 1000.0) as i64
            }
            None => true,
        };
        if !due {
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

pub async fn interval_tick(st: &AppState) {
    for tenant_str in st.store.subscription_tenants().unwrap_or_default() {
        let Ok(tenant) = TenantId::new(&tenant_str) else {
            continue;
        };
        for sub in st
            .store
            .list(&tenant, Kind::Subscription)
            .unwrap_or_default()
        {
            let Some(interval) = sub.get("timeInterval").and_then(Value::as_f64) else {
                continue;
            };
            if !is_active(&sub) {
                continue;
            }
            let anchor = sub
                .get("notification")
                .and_then(|n| n.get("lastNotification"))
                .and_then(Value::as_str)
                .or_else(|| sub.get("createdAt").and_then(Value::as_str));
            let due = match anchor.and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok()) {
                Some(last) => {
                    (chrono::Utc::now() - last.with_timezone(&chrono::Utc)).num_milliseconds()
                        >= (interval * 1000.0) as i64
                }
                None => true,
            };
            if !due {
                continue;
            }
            if st.nats && !claim_interval(st, &tenant, Kind::Subscription, &sub, interval) {
                continue;
            }
            let ctx = sub_context(st, &sub).await;
            let now = now_iso();
            let matching: Vec<Value> = st
                .store
                .list(&tenant, Kind::Entity)
                .unwrap_or_default()
                .into_iter()
                .filter(|d| {
                    selector_match(&sub, d, &ctx) && conditions_match(st, &tenant, &sub, d, &ctx)
                })
                .flat_map(|d| build_data(st, &tenant, &sub, &ctx, None, Some(&d), &[], false, &now))
                .collect();
            if matching.is_empty() {
                continue;
            }
            deliver(st, &tenant, &sub, matching, &ctx).await;
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
            let anchor = sub
                .get("notification")
                .and_then(|n| n.get("lastNotification"))
                .and_then(Value::as_str)
                .or_else(|| sub.get("createdAt").and_then(Value::as_str));
            let due = match anchor.and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok()) {
                Some(last) => {
                    (chrono::Utc::now() - last.with_timezone(&chrono::Utc)).num_milliseconds()
                        >= (interval * 1000.0) as i64
                }
                None => true,
            };
            if !due {
                continue;
            }
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
        // §4.1 L4 / 5.11.7: re-check the subscription still exists right
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
        return; // creation rejects unknown schemes with 422 (§9.2); belt only
    }
    // V-16: endpoint.cooldown — drop (never queue) while the window is open.
    // Before any bookkeeping: a suppressed notification was never sent, so
    // timesSent/lastNotification must not move.
    if in_cooldown(sub, chrono::Utc::now()) {
        tracing::debug!("subscription {sub_id} in cooldown; notification suppressed (5.2.15)");
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
                Err(e) => {
                    tracing::warn!("mqtt endpoint of subscription {sub_id} unusable: {e}");
                    return;
                }
            }
        }
        #[cfg(not(feature = "mqtt"))]
        {
            return; // no sink compiled: creation already answered 422 (G3)
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
    st.store
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
    // I4: the notification endpoint is an egress destination like any other
    // — policy check once, breaker consulted before the attempt (§16.4).
    // A refusal is a delivery failure for bookkeeping (status "failed",
    // lastSuccess rolled back below) but never breaker state: the policy
    // verdict says nothing about the endpoint's health.
    let refused = match st.egress.check_url(uri).await {
        Ok(()) => false,
        Err(e) => {
            tracing::warn!("notification endpoint {uri} refused by egress policy: {e}");
            true
        }
    };
    let breaker_open = !refused && st.egress.is_open(uri);
    // (delivered, timed_out): only a TIMEOUT-class failure feeds the breaker
    // — §16.7/U1 protects against peers that eat the deadline. An endpoint
    // that ANSWERS (any status) is alive, costs only its own response time,
    // and 6.3.8 says the notification shall be sent — suppressing sends to a
    // responding host:port starves unrelated subscriptions sharing it.
    let (ok, timed_out) = if refused || breaker_open {
        if breaker_open {
            tracing::debug!("notification to {uri} short-circuited (breaker open)");
        }
        (false, false)
    } else {
        match outbound {
            Outbound::Http(req, bytes) => {
                // N3 (wasm): the page sink takes matching endpoints — a page
                // cannot listen on a socket, so this IS its delivery channel.
                #[cfg(target_arch = "wasm32")]
                let page_handled = crate::page_sink::try_deliver(uri, &bytes);
                #[cfg(not(target_arch = "wasm32"))]
                let page_handled = false;
                if page_handled {
                    (true, false)
                } else {
                    // V-17: endpoint.timeout (Table 5.2.15-1), clamped
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
                        // broker/socket-level failure — keep the U1 guard
                        (false, true)
                    }
                }
            }
        }
    };
    if !refused && !breaker_open {
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
    // K12: delivery counters by sink scheme (facade — no-op without the
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
                    n.insert("status".into(), Value::String("failed".into()));
                }
                Ok::<(), antares_model::NgsiError>(())
            })
            .unwrap_or_else(|e| {
                tracing::warn!("failure-status writeback failed: {e}");
                None
            });
    }
}

#[cfg(test)]
mod endpoint_tests {
    use super::*;
    use serde_json::json;

    fn ep(v: Value) -> serde_json::Map<String, Value> {
        v.as_object().expect("map").clone()
    }

    /// Table 5.2.15-1 `timeout` (audit V-17): honored, clamped, defaulted.
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

    /// Table 5.2.15-1 `cooldown` (audit V-16): gate opens only after a
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
