//! Subscription matching + HTTP notification delivery (5.8.6, 5.3.1).
//!
//! Change detection: the store's change hook feeds every entity write here as
//! a (before, after) pair; attribute-level changes are derived by diffing —
//! one hook point instead of one call per write handler.
//! ponytail: linear scan over a tenant's subscriptions; the (tenant, type)
//! index of §1.1 lands with the 10k-subscription target, not the suite.

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

/// Wire the store hook and background tasks. Call once at startup.
pub fn wire(state: &AppState) {
    let (tx, mut rx) =
        tokio::sync::mpsc::unbounded_channel::<(String, Option<Value>, Option<Value>)>();
    state.store.set_change_hook(Box::new(move |tenant, before, after| {
        let _ = tx.send((tenant.as_str().to_owned(), before, after));
    }));
    let st = state.clone();
    tokio::spawn(async move {
        while let Some((tenant, before, after)) = rx.recv().await {
            process_change(&st, &tenant, before, after).await;
        }
    });
    let st = state.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_millis(500));
        loop {
            tick.tick().await;
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
                let bx: Vec<&Value> =
                    x.as_array().map(|a| a.iter().collect()).unwrap_or_default();
                let by: Vec<&Value> =
                    y.as_array().map(|a| a.iter().collect()).unwrap_or_default();
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

fn sub_str<'a>(sub: &'a Value, key: &str) -> Option<&'a str> {
    sub.get(key).and_then(Value::as_str)
}

fn is_active(sub: &Value) -> bool {
    if sub.get("isActive") == Some(&Value::Bool(false)) {
        return false;
    }
    !sub
        .get("expiresAt")
        .and_then(Value::as_str)
        .is_some_and(|e| e < now_iso().as_str())
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
        let id_ok = e.get("id").and_then(Value::as_str).is_none_or(|i| i == id);
        let pat_ok = e.get("idPattern").and_then(Value::as_str).is_none_or(|p| {
            regex::Regex::new(p).is_ok_and(|re| re.find(id).is_some())
        });
        t_ok && id_ok && pat_ok
    })
}

/// q / scopeQ / geoQ conditions against an internal entity doc.
fn conditions_match(sub: &Value, doc: &Value, ctx: &Context) -> bool {
    if let Some(q) = sub_str(sub, "q") {
        // q values in subscription bodies may be percent-encoded (4.9, 046_05)
        let q = crate::negotiate::percent_decode(q.as_bytes());
        match antares_ql::parse_q(&q) {
            Ok(node) => {
                if !crate::qeval::eval_q(&node, doc, ctx) {
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
async fn sub_context(st: &AppState, sub: &Value) -> Arc<Context> {
    let source = sub
        .get("jsonldContext")
        .cloned()
        .or_else(|| sub.get("__context").cloned());
    match source {
        Some(v) if !v.is_null() => st.loader.resolve_quiet(&v).await.unwrap_or_else(|_| st.loader.core()),
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
        n.and_then(|n| n.get(key)).and_then(Value::as_array).map(|a| {
            a.iter().filter_map(Value::as_str).map(str::to_owned).collect()
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
            ds.iter().filter_map(Value::as_str).map(str::to_owned).collect(),
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
                        let Some(arr) = v.as_array_mut() else { continue };
                        for inst in arr {
                            let Some(bi) = b
                                .get(k)
                                .and_then(Value::as_array)
                                .and_then(|ba| {
                                    ba.iter().find(|x| instance_ds(x) == instance_ds(inst))
                                })
                            else {
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
                        gone.iter().map(|di| tombstone(di, sys, show, now)).collect()
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

async fn process_change(
    st: &AppState,
    tenant_str: &str,
    before: Option<Value>,
    after: Option<Value>,
) {
    let Ok(tenant) = TenantId::new(tenant_str) else { return };
    let subs = st.store.list(&tenant, Kind::Subscription);
    if subs.is_empty() {
        return;
    }
    let changes = diff(before.as_ref(), after.as_ref());
    let entity_trigger = match (&before, &after) {
        (None, Some(_)) => "entityCreated",
        (Some(_), None) => "entityDeleted",
        _ => "entityUpdated",
    };
    let eval_doc = after.as_ref().or(before.as_ref());
    let Some(eval_doc) = eval_doc else { return };
    for sub in subs {
        if !is_active(&sub) || sub.get("timeInterval").is_some() {
            continue;
        }
        let triggers: Vec<String> = sub
            .get("notificationTrigger")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect())
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
        if !conditions_match(&sub, eval_doc, &ctx) {
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
        let entity_deleted_fired =
            after.is_none() && triggers.iter().any(|t| t == "entityDeleted");
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
async fn interval_tick(st: &AppState) {
    for tenant_str in st.store.subscription_tenants() {
        let Ok(tenant) = TenantId::new(&tenant_str) else { continue };
        for sub in st.store.list(&tenant, Kind::Subscription) {
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
            let ctx = sub_context(st, &sub).await;
            let now = now_iso();
            let matching: Vec<Value> = st
                .store
                .list(&tenant, Kind::Entity)
                .into_iter()
                .filter(|d| selector_match(&sub, d, &ctx) && conditions_match(&sub, d, &ctx))
                .flat_map(|d| {
                    build_data(st, &tenant, &sub, &ctx, None, Some(&d), &[], false, &now)
                })
                .collect();
            if matching.is_empty() {
                continue;
            }
            deliver(st, &tenant, &sub, matching, &ctx).await;
        }
        // csource timeInterval subs: periodic CSourceNotification with all
        // matching registrations, independent of changes (5.11.7)
        for sub in st.store.list(&tenant, Kind::CSourceSubscription) {
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
            let ctx = sub_context(st, &sub).await;
            let spec = crate::csource::spec_for_subscription(&sub);
            let data: Vec<Value> = st
                .store
                .list(&tenant, Kind::Registration)
                .into_iter()
                .filter(|r| crate::csource::csr_matches_subscription(&sub, r, &ctx))
                .map(|r| {
                    let mut p = crate::csource::present_registration(&filter_csr(&spec, &r, &ctx), &ctx, false);
                    arrayify_entity_types(&mut p);
                    p
                })
                .collect();
            deliver_as(st, &tenant, Kind::CSourceSubscription, &sub, "ContextSourceNotification", data, &ctx, Some("newlyMatching")).await;
        }
    }
}

/// POST the Notification (5.3.1) and write the 5.2.14.2 bookkeeping back.
async fn deliver(st: &AppState, tenant: &TenantId, sub: &Value, data: Vec<Value>, ctx: &Context) {
    deliver_as(st, tenant, Kind::Subscription, sub, "Notification", data, ctx, None).await
}

/// 5.11.7: which csource subs care about a registration change, and why.
fn csource_trigger(sub: &Value, before: Option<&Value>, after: Option<&Value>, ctx: &Context) -> Option<&'static str> {
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
    let Some(infos) = reg.get_mut("information").and_then(Value::as_array_mut) else { return };
    for info in infos {
        let Some(es) = info.get_mut("entities").and_then(Value::as_array_mut) else { continue };
        for e in es {
            if let Some(t) = e.get("type").filter(|t| t.is_string()).cloned() {
                if let Some(o) = e.as_object_mut() {
                    o.insert("type".into(), Value::Array(vec![t]));
                }
            }
        }
    }
}

/// Registration create/update/delete → CSourceNotification fan-out (5.11.7).
pub async fn csource_changed(
    st: &AppState,
    tenant: &TenantId,
    before: Option<Value>,
    after: Option<Value>,
) {
    for sub in st.store.list(tenant, Kind::CSourceSubscription) {
        if !is_active(&sub) || sub.get("timeInterval").is_some() {
            continue;
        }
        let ctx = sub_context(st, &sub).await;
        let Some(reason) = csource_trigger(&sub, before.as_ref(), after.as_ref(), &ctx) else {
            continue;
        };
        let spec = crate::csource::spec_for_subscription(&sub);
        let source = if reason == "noLongerMatching" { &before } else { &after };
        let Some(reg) = source.as_ref().or(before.as_ref()) else { continue };
        let filtered = filter_csr(&spec, reg, &ctx);
        let mut presented = crate::csource::present_registration(&filtered, &ctx, false);
        arrayify_entity_types(&mut presented);
        deliver_as(st, tenant, Kind::CSourceSubscription, &sub, "ContextSourceNotification", vec![presented], &ctx, Some(reason)).await;
    }
}

/// Initial / post-update CSourceNotification with all currently matching
/// registrations (5.11.2.4 / 5.11.3.4).
pub async fn csource_initial(st: &AppState, tenant: &TenantId, sub_id: &str) {
    let Some(sub) = st.store.get(tenant, Kind::CSourceSubscription, sub_id) else { return };
    if !is_active(&sub) {
        return;
    }
    let ctx = sub_context(st, &sub).await;
    let spec = crate::csource::spec_for_subscription(&sub);
    let data: Vec<Value> = st
        .store
        .list(tenant, Kind::Registration)
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
    deliver_as(st, tenant, Kind::CSourceSubscription, &sub, "ContextSourceNotification", data, &ctx, Some("newlyMatching")).await;
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
    let Some(uri) = ep.get("uri").and_then(Value::as_str) else { return };
    if !uri.starts_with("http") {
        return; // mqtt sinks land with the MQTT TPs (feature-gated per §9.2)
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
    let mut req = st.http.post(uri);
    if accept == "application/ld+json" {
        // JSON-LD notifications carry the @context inside each data entity
        // (046_14: data[0] must contain @context; no Link header)
        if let Some(arr) = body.get_mut("data").and_then(Value::as_array_mut) {
            for e in arr.iter_mut() {
                *e = inject_context(e.clone(), ctx);
            }
        }
        req = req.header("Content-Type", "application/ld+json");
    } else {
        req = req
            .header("Content-Type", "application/json")
            .header("Link", link_header_value(ctx));
    }
    if let Some(ri) = ep.get("receiverInfo").and_then(Value::as_array) {
        for kv in ri {
            if let (Some(k), Some(v)) = (
                kv.get("key").and_then(Value::as_str),
                kv.get("value").and_then(Value::as_str),
            ) {
                req = req.header(k, v);
            }
        }
    }
    if tenant.as_str() != "default" {
        req = req.header("NGSILD-Tenant", tenant.as_str());
    }
    let bytes = serde_json::to_vec(&body).unwrap_or_default();
    // Bookkeeping BEFORE the send (5.8.6/5.2.14.2: lastNotification is the
    // instant the notification is sent). The ETSI mock unblocks the test the
    // moment the request ARRIVES, so a post-response-only writeback races the
    // test's immediate Retrieve Subscription (CI flake on 046_12_01).
    // Optimistic ok; a failed attempt is corrected right below — the transient
    // window is the in-flight attempt itself, and the failure TPs wait for the
    // attempt to resolve before asserting.
    let mut prev_success: Option<Value> = None;
    st.store.mutate(tenant, kind, &sub_id, |doc| {
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
    });
    let ok = matches!(req.body(bytes).send().await, Ok(r) if r.status().is_success());
    if !ok {
        // 5.8.6 / 5.11.7: subscription status → "failed" on delivery failure;
        // roll back the optimistic lastSuccess stamp.
        let ts = now_iso();
        st.store.mutate(tenant, kind, &sub_id, |doc| {
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
        });
    }
}
