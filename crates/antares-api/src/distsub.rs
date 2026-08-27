// SPDX-License-Identifier: EUPL-1.2
//! The CONSUMER half of distributed subscriptions (5.8.1.4 / 5.8.2.4 /
//! 5.8.5.4): an entity Subscription with localOnly != true creates an
//! internal Context Source Registration Subscription (5.11.2) whose
//! CSource notifications drive per-registration remote subscription
//! create/update/delete (triggerReason newlyMatching / updated /
//! noLongerMatching), with subscriptionId mappings stored so inbound
//! remote notifications forward to the original subscriber.

use crate::negotiate::{ApiError, ApiResult};
use crate::state::{now_iso, AppState};
use antares_model::{NgsiError, TenantId};
use antares_sql::store::Kind;
use antares_store::CurrentStateDriverExt;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{json, Value};

/// 5.8.1.4: "The mapping of the received subscriptionId with the own
/// Subscription identifier is stored" (inbound), "a mapping of the id of
/// the Context Source Registration to the received subscriptionId is
/// stored" (remotes), and "the mapping of the id of the Subscription to the
/// … Context Source Registration Subscription shall be stored" (csr_sub).
/// All three live in the store (Kind::DistSub) so persistent modes keep the
/// consumer half across restarts: one doc per (tenant, own Subscription id)
/// = {"csr_sub": id, "remotes": {reg_id: [endpoint, remote sub id]}}, plus
/// inbound index docs under the internal "distsub-index" tenant
/// (id = remote subscriptionId, doc = {"tenant", "own"}).
fn ds_index_tenant() -> Option<TenantId> {
    TenantId::new("distsub-index").ok()
}

fn ds_get(st: &AppState, tenant: &TenantId, own_id: &str) -> Value {
    st.store
        .get(tenant, Kind::DistSub, own_id)
        .ok()
        .flatten()
        .unwrap_or_else(|| json!({}))
}

fn ds_put(st: &AppState, tenant: &TenantId, own_id: &str, doc: Value) {
    let updated = st
        .store
        .mutate(tenant, Kind::DistSub, own_id, |d| {
            *d = doc.clone();
            Ok::<_, std::convert::Infallible>(())
        })
        .ok()
        .flatten()
        .is_some();
    if !updated {
        if let Err(e) = st.store.create(tenant, Kind::DistSub, own_id, doc) {
            tracing::warn!("subscription {own_id}: distributed mapping not stored: {e}");
        }
    }
}

/// remotes of one own Subscription: reg id → (endpoint, remote sub id)
fn ds_remotes(doc: &Value) -> Vec<(String, (String, String))> {
    doc.get("remotes")
        .and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .filter_map(|(reg, v)| {
                    Some((
                        reg.clone(),
                        (
                            v.get(0)?.as_str()?.to_owned(),
                            v.get(1)?.as_str()?.to_owned(),
                        ),
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Insert or remove ONE `remotes` entry under the store's own lock, and
/// only if the document still exists. A targeted mutate, never a
/// read-modify-write of the whole document: the per-registration branches of
/// 5.8.1.4 interleave at their forward `await`s, so a full-document write
/// would drop a sibling registration's mapping, and a Delete Subscription
/// (5.8.5.4) that lands mid-forward would be undone by a write that
/// resurrects the deleted document. `false` = the Subscription's mapping
/// document is gone, or the registration already holds a remote subscription
/// — 5.8.1.4 stores ONE subscriptionId per Context Source Registration, and
/// two notifications racing for the same pair would otherwise each create a
/// remote copy and orphan the loser's at the source. That check runs inside
/// the closure the store executes under its write lock, so the same lock
/// decides and acts.
fn ds_set_remote(
    st: &AppState,
    tenant: &TenantId,
    own_id: &str,
    reg_id: &str,
    entry: Option<Value>,
) -> bool {
    st.store
        .mutate(tenant, Kind::DistSub, own_id, |d| {
            if let Some(o) = d.as_object_mut() {
                match &entry {
                    Some(v) => {
                        if !o.get("remotes").is_some_and(Value::is_object) {
                            o.insert("remotes".into(), json!({}));
                        }
                        if let Some(m) = o.get_mut("remotes").and_then(Value::as_object_mut) {
                            if m.contains_key(reg_id) {
                                return Err(());
                            }
                            m.insert(reg_id.to_owned(), v.clone());
                        }
                    }
                    None => {
                        if let Some(m) = o.get_mut("remotes").and_then(Value::as_object_mut) {
                            m.remove(reg_id);
                        }
                    }
                }
            }
            Ok(())
        })
        .ok()
        .flatten()
        .is_some_and(|r: Result<(), ()>| r.is_ok())
}

fn inbound_put(st: &AppState, remote_id: &str, tenant: &TenantId, own_id: &str) {
    if let Some(idx) = ds_index_tenant() {
        // Without this index every notification the source sends is answered
        // 404 and the subscription silently never notifies — a store failure
        // here has to be visible to an operator.
        if let Err(e) = st.store.create(
            &idx,
            Kind::DistSub,
            remote_id,
            json!({"tenant": tenant.as_str(), "own": own_id}),
        ) {
            tracing::warn!(
                "subscription {own_id}: inbound mapping for {remote_id} not stored: {e}"
            );
        }
    }
}

fn inbound_get(st: &AppState, remote_id: &str) -> Option<(String, String)> {
    let idx = ds_index_tenant()?;
    let doc = st
        .store
        .get(&idx, Kind::DistSub, remote_id)
        .ok()
        .flatten()?;
    Some((
        doc.get("tenant")?.as_str()?.to_owned(),
        doc.get("own")?.as_str()?.to_owned(),
    ))
}

fn inbound_delete(st: &AppState, remote_id: &str) {
    if let Some(idx) = ds_index_tenant() {
        let _ = st.store.delete(&idx, Kind::DistSub, remote_id);
    }
}

fn distributed(sub: &Value) -> bool {
    sub.get("localOnly").and_then(Value::as_bool) != Some(true)
}

/// 6.3.17/6.3.18: the Via chain the Subscription arrived with, rebuilt as
/// the header the outbound forward extends (`federation::forward` appends
/// this broker's alias). Stored on the Subscription as the broker-internal
/// `__via` member (`__context` is the precedent) by create.
fn sub_via_headers(sub: &Value) -> HeaderMap {
    let mut h = HeaderMap::new();
    if let Some(v) = sub
        .get("__via")
        .and_then(Value::as_str)
        .and_then(|v| axum::http::HeaderValue::from_str(v).ok())
    {
        h.insert("via", v);
    }
    h
}

/// The Subscription members 5.11.2.4 matches a Context Source Registration
/// on, plus the @context the internal Registration Subscription is read
/// under. Kept in one place: create and update must offer the very same
/// document, or an update silently widens which sources are matched.
const CSR_MATCH_MEMBERS: [&str; 6] = [
    "entities",
    "watchedAttributes",
    "csf",
    "geoQ",
    "scopeQ",
    "temporalQ",
];

/// 5.8.1.4: "Based on the content of the Subscription, a Context Source
/// Registration Subscription shall be created (clause 5.11.2)" — internal,
/// with the urn:antares:distsub endpoint handled in-process by notify.
pub(crate) fn on_subscription_created(st: &AppState, tenant: &TenantId, sub: &Value) {
    if !distributed(sub) {
        return;
    }
    // 6.3.18 ("to avoid infinite loops"): a forwarded copy whose Via chain
    // already names this broker has come full circle — it serves locally,
    // and the distributed half is not created, so mutually registered
    // brokers cannot re-forward copies of copies without bound. via_loop
    // also enforces the MAX_VIA_HOPS ceiling on a forged chain.
    if crate::federation::via_loop(
        &sub_via_headers(sub),
        &crate::federation::alias_for(&st.host_alias, tenant),
    ) {
        return;
    }
    let Some(own_id) = sub.get("id").and_then(Value::as_str) else {
        return;
    };
    let csr_id = format!(
        "urn:ngsi-ld:CSourceSubscription:distsub:{}",
        uuid::Uuid::new_v4()
    );
    let ts = now_iso();
    let mut doc = json!({
        "id": csr_id,
        "type": "Subscription",
        "isActive": true,
        "status": "active",
        "createdAt": ts,
        "modifiedAt": ts,
        "notification": {"endpoint": {"uri":
            format!("urn:antares:distsub:{}\n{own_id}", tenant.as_str())}},
    });
    // 5.11.2.4 matches a registration on all of these, so the Registration
    // Subscription carries every member that decides the match — a copy
    // built from the entity selectors alone offers the Subscription to
    // sources the subscriber's csf, geoQ, scopeQ or temporalQ excluded.
    // `q` is deliberately absent: on a Context Source Registration
    // Subscription it would filter registration properties, not Entity
    // Attributes.
    for k in CSR_MATCH_MEMBERS.iter().chain(["__context"].iter()) {
        if let Some(v) = sub.get(k) {
            doc[*k] = v.clone();
        }
    }
    if let Some(a) = sub.get("notification").and_then(|n| n.get("attributes")) {
        doc["notification"]["attributes"] = a.clone();
    }
    let created = st
        .store
        .create(tenant, Kind::CSourceSubscription, &csr_id, doc)
        .unwrap_or_else(|e| {
            tracing::warn!("subscription {own_id}: Registration Subscription not created: {e}");
            false
        });
    if created {
        let mut doc = ds_get(st, tenant, own_id);
        doc["csr_sub"] = Value::String(csr_id.clone());
        ds_put(st, tenant, own_id, doc);
        // 5.11.2.4 initial notification with all matching registrations —
        // this is what turns already-known registrations into newlyMatching
        let (st2, t2) = (st.clone(), tenant.clone());
        crate::spawn(async move {
            crate::notify::csource_initial(&st2, &t2, &csr_id).await;
        });
    }
}

/// 5.8.2.4: keep the internal CSR subscription in step and forward reduced
/// updates to every mapped remote supporting updateSubscription (5.11.3).
pub(crate) fn on_subscription_updated(st: &AppState, tenant: &TenantId, own_id: &str) {
    let Some(sub) = st
        .store
        .get(tenant, Kind::Subscription, own_id)
        .ok()
        .flatten()
    else {
        return;
    };
    // 5.8.1.4 gates the distributed half on "If localOnly=false": a
    // Subscription updated to localOnly=true is torn down like a deleted
    // one — the internal Registration Subscription goes, and every remote
    // copy already created is deleted at its source.
    if !distributed(&sub) {
        on_subscription_deleted(st, tenant, own_id);
        return;
    }
    let csr_id = ds_get(st, tenant, own_id)
        .get("csr_sub")
        .and_then(Value::as_str)
        .map(str::to_owned);
    match csr_id {
        None => {
            // became distributed only now (e.g. localOnly flipped off)
            on_subscription_created(st, tenant, &sub);
        }
        Some(csr_id) => {
            let _ = st
                .store
                .mutate(tenant, Kind::CSourceSubscription, &csr_id, |doc| {
                    for k in CSR_MATCH_MEMBERS {
                        match sub.get(k) {
                            Some(v) => doc[k] = v.clone(),
                            None => {
                                doc.as_object_mut().map(|o| o.remove(k));
                            }
                        }
                    }
                    match sub.get("notification").and_then(|n| n.get("attributes")) {
                        Some(a) => doc["notification"]["attributes"] = a.clone(),
                        None => {
                            doc["notification"]
                                .as_object_mut()
                                .map(|n| n.remove("attributes"));
                        }
                    }
                    doc["modifiedAt"] = Value::String(now_iso());
                    Ok::<(), NgsiError>(())
                });
        }
    }
    let remotes: Vec<(String, (String, String))> = ds_remotes(&ds_get(st, tenant, own_id));
    for (reg_id, (endpoint, remote_id)) in remotes {
        let Some(reg) = st
            .store
            .get(tenant, Kind::Registration, &reg_id)
            .ok()
            .flatten()
        else {
            continue;
        };
        if !crate::federation::doc_supports(&reg, "updateSubscription") {
            continue;
        }
        let (st2, t2, sub2) = (st.clone(), tenant.clone(), sub.clone());
        let ctx_url = sub_ctx_url(st, &sub);
        crate::spawn(async move {
            let ctx = crate::notify::sub_context(&st2, &sub2).await;
            let Some(mut copy) = reduced_copy(&st2, &sub2, &reg, &remote_id, &ctx) else {
                return;
            };
            copy.as_object_mut().map(|o| o.remove("id"));
            forward_sub(
                &st2,
                &t2,
                reqwest::Method::PATCH,
                format!("{endpoint}/ngsi-ld/v1/subscriptions/{remote_id}"),
                &reg_id,
                &reg,
                &ctx_url,
                &sub_via_headers(&sub2),
                Some(copy),
            )
            .await;
        });
    }
}

/// 5.8.5.4: delete the internal CSR subscription (5.11.6) and forward the
/// delete to every mapped remote supporting deleteSubscription.
pub(crate) fn on_subscription_deleted(st: &AppState, tenant: &TenantId, own_id: &str) {
    let doc = ds_get(st, tenant, own_id);
    let csr_id = doc
        .get("csr_sub")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let remotes = ds_remotes(&doc);
    for (_, (_, remote_id)) in &remotes {
        inbound_delete(st, remote_id);
    }
    let _ = st.store.delete(tenant, Kind::DistSub, own_id);
    if let Some(csr_id) = csr_id {
        let _ = st.store.delete(tenant, Kind::CSourceSubscription, &csr_id);
    }
    for (reg_id, (endpoint, remote_id)) in remotes {
        let stored = st
            .store
            .get(tenant, Kind::Registration, &reg_id)
            .ok()
            .flatten();
        if stored
            .as_ref()
            .is_some_and(|reg| !crate::federation::doc_supports(reg, "deleteSubscription"))
        {
            continue;
        }
        let (st2, t2) = (st.clone(), tenant.clone());
        let ctx_url = sub_ctx_url(st, &Value::Null);
        crate::spawn(async move {
            let reg = forward_reg(stored, &reg_id, &endpoint);
            // the Subscription is already deleted, so its stored chain is
            // gone — a delete-forward cannot create a copy, so a fresh
            // one-hop chain is loop-safe
            forward_sub(
                &st2,
                &t2,
                reqwest::Method::DELETE,
                format!("{endpoint}/ngsi-ld/v1/subscriptions/{remote_id}"),
                &reg_id,
                &reg,
                &ctx_url,
                &HeaderMap::new(),
                None,
            )
            .await;
        });
    }
}

/// 4.14 / 5.8.5.4: the document a forwarded delete travels with. The stored
/// Context Source Registration carries the tenant, the contextSourceInfo and
/// the timeout/cooldown the forward needs; the id/endpoint pair is the
/// fallback for a registration that is already gone.
fn forward_reg(stored: Option<Value>, reg_id: &str, endpoint: &str) -> Value {
    stored.unwrap_or_else(|| json!({"id": reg_id, "endpoint": endpoint}))
}

/// 5.8.1.4: "The @context to be used for sending Notifications related to
/// this Subscription shall be the one specified in the jsonldContext field."
/// The forwarded copy carries the subscriber's own terms, so it is shipped
/// under the Subscription's @context, falling back to the core context.
fn sub_ctx_url(st: &AppState, sub: &Value) -> String {
    let source = sub
        .get("jsonldContext")
        .or_else(|| sub.get("__context"))
        .filter(|v| !v.is_null())
        .cloned()
        .unwrap_or_else(|| st.loader.core().source.clone());
    crate::federation::ctx_link_url(&HeaderMap::new(), &source)
}

/// The 5.8.1.4 localOnly=false block: one CSource notification for the
/// internal CSR subscription, dispatched per registration and triggerReason.
pub(crate) async fn on_csource_notification(
    st: &AppState,
    tenant: &TenantId,
    own_id: &str,
    reason: Option<&str>,
    regs: &[Value],
) {
    let Some(sub) = st
        .store
        .get(tenant, Kind::Subscription, own_id)
        .ok()
        .flatten()
    else {
        return;
    };
    // 5.8.1.4: "If localOnly=false, each time a Context Source Notification
    // … is received" — a Subscription that has since been flipped to
    // localOnly creates no further remote copy.
    if !distributed(&sub) {
        return;
    }
    let ctx = crate::notify::sub_context(st, &sub).await;
    let ctx_url = sub_ctx_url(st, &sub);
    let via = sub_via_headers(&sub);
    let seen = crate::federation::via_tokens(&via);
    let reason = reason.unwrap_or("updated");
    for reg in regs {
        let Some(reg_id) = reg.get("id").and_then(Value::as_str) else {
            continue;
        };
        // auxiliary registrations take no part (5.8.1.4 lists exclusive,
        // redirect and inclusive only)
        if reg.get("mode").and_then(Value::as_str) == Some("auxiliary") {
            continue;
        }
        // Table 6.3.18-2: the Via listing "is used when determining matching
        // registrations" — a Context Source the Subscription already
        // travelled through must not receive a copy of it back.
        if reg
            .get("contextSourceAlias")
            .and_then(Value::as_str)
            .is_some_and(|a| seen.iter().any(|t| t == a))
        {
            continue;
        }
        let Some(endpoint) = reg.get("endpoint").and_then(Value::as_str).map(|e| {
            e.trim_end_matches('/')
                .trim_end_matches("/ngsi-ld/v1")
                .to_owned()
        }) else {
            continue;
        };
        let mapped = ds_remotes(&ds_get(st, tenant, own_id))
            .into_iter()
            .find(|(r, _)| r == reg_id)
            .map(|(_, v)| v);
        match (reason, mapped) {
            // an unmapped registration that starts (or keeps) matching gets
            // the reduced copy — newlyMatching, or "updated" reported by an
            // initial notification when no mapping exists yet
            ("newlyMatching" | "updated", None)
                if crate::federation::doc_supports(reg, "createSubscription") =>
            {
                let remote_id =
                    format!("urn:ngsi-ld:Subscription:distsub:{}", uuid::Uuid::new_v4());
                let Some(copy) = reduced_copy(st, &sub, reg, &remote_id, &ctx) else {
                    // nothing this registration covers is watched — there is
                    // no reduced copy to forward to it
                    continue;
                };
                // 5.8.1.4/5.8.5.4: the mapping is stored BEFORE the forward —
                // the remote id is broker-generated, and a delete Subscription
                // arriving while the create-forward's response is still in
                // flight must find the mapping or the delete-forward is lost
                // (the ETSI 5814_01_01 pg race). A failed forward rolls the
                // mapping back below.
                inbound_put(st, &remote_id, tenant, own_id);
                if !ds_set_remote(
                    st,
                    tenant,
                    own_id,
                    reg_id,
                    Some(json!([endpoint.clone(), remote_id])),
                ) {
                    // the Subscription was deleted while this notification
                    // was in flight — no remote copy is created for it
                    inbound_delete(st, &remote_id);
                    continue;
                }
                let (status, _) = forward_sub(
                    st,
                    tenant,
                    reqwest::Method::POST,
                    format!("{endpoint}/ngsi-ld/v1/subscriptions"),
                    reg_id,
                    reg,
                    &ctx_url,
                    &via,
                    Some(copy),
                )
                .await;
                if !(200..300).contains(&status) {
                    inbound_delete(st, &remote_id);
                    ds_set_remote(st, tenant, own_id, reg_id, None);
                }
            }
            ("updated", Some((_, remote_id)))
                if crate::federation::doc_supports(reg, "updateSubscription") =>
            {
                let Some(mut copy) = reduced_copy(st, &sub, reg, &remote_id, &ctx) else {
                    continue;
                };
                copy.as_object_mut().map(|o| o.remove("id"));
                forward_sub(
                    st,
                    tenant,
                    reqwest::Method::PATCH,
                    format!("{endpoint}/ngsi-ld/v1/subscriptions/{remote_id}"),
                    reg_id,
                    reg,
                    &ctx_url,
                    &via,
                    Some(copy),
                )
                .await;
            }
            ("noLongerMatching", Some((_, remote_id)))
                if crate::federation::doc_supports(reg, "deleteSubscription") =>
            {
                forward_sub(
                    st,
                    tenant,
                    reqwest::Method::DELETE,
                    format!("{endpoint}/ngsi-ld/v1/subscriptions/{remote_id}"),
                    reg_id,
                    reg,
                    &ctx_url,
                    &via,
                    None,
                )
                .await;
                ds_set_remote(st, tenant, own_id, reg_id, None);
                inbound_delete(st, &remote_id);
            }
            _ => {}
        }
    }
}

/// 5.8.1.4: "a copy of the original Subscription shall be reduced to what
/// is matched by the registration information"; with splitEntities the
/// q/geoQ/scopeQ members are removed; the notification attributes/pick/omit
/// members are removed; the endpoint is set to the local broker.
///
/// 5.5.7 Term to URI expansion: the Subscription comes out of the store with
/// its names and types EXPANDED, while a registration delivered by a Context
/// Source Notification carries them COMPACTED. Every registration-derived
/// name is expanded against `ctx` before it is compared or inserted, so the
/// reduction happens in one representation — `expand_key` is idempotent on
/// an absolute IRI, which is what the stored registration already holds.
fn reduced_copy(
    st: &AppState,
    sub: &Value,
    reg: &Value,
    remote_id: &str,
    ctx: &antares_jsonld::Context,
) -> Option<Value> {
    let mut copy = sub.clone();
    let Some(o) = copy.as_object_mut() else {
        return Some(copy);
    };
    // __via travels as the Via HTTP header the forward extends (6.3.17),
    // never as a body member
    for k in [
        "status",
        "timesSent",
        "lastNotification",
        "lastSuccess",
        "lastFailure",
        "createdAt",
        "modifiedAt",
        "__context",
        "__via",
        "localOnly",
    ] {
        o.remove(k);
    }
    o.insert("id".into(), Value::String(remote_id.to_owned()));
    // reduce the entity selectors to the registration information
    let reg_entities: Vec<Value> = reg
        .get("information")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|i| i.get("entities").and_then(Value::as_array))
        .flatten()
        .map(|e| {
            let mut e = e.clone();
            // notification presentation arrayifies type — EntitySelector
            // wants the plain form back
            if let Some(t) = e
                .get("type")
                .and_then(Value::as_array)
                .and_then(|a| a.first())
            {
                e["type"] = t.clone();
            }
            if let Some(t) = e.get("type").and_then(Value::as_str) {
                e["type"] = Value::String(ctx.expand_key(t));
            }
            e
        })
        .collect();
    if !reg_entities.is_empty() {
        o.insert("entities".into(), Value::Array(reg_entities));
    }
    // watchedAttributes ∩ the registration's attribute scope
    let reg_attrs: Vec<String> = reg
        .get("information")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|i| {
            ["propertyNames", "relationshipNames"]
                .into_iter()
                .filter_map(|k| i.get(k).and_then(Value::as_array))
                .flatten()
                .filter_map(Value::as_str)
                .map(|n| ctx.expand_key(n))
        })
        .collect();
    if !reg_attrs.is_empty() {
        let watch: Vec<Value> = match o.get("watchedAttributes").and_then(Value::as_array) {
            Some(w) => w
                .iter()
                .filter(|a| a.as_str().is_some_and(|a| reg_attrs.iter().any(|r| r == a)))
                .cloned()
                .collect(),
            // 5.8.1.4 "reduced to what is matched by the registration
            // information": a watch-everything Subscription still only
            // watches the registered names at the source — otherwise an
            // unregistered attribute change notifies through the chain.
            None => reg_attrs.iter().map(|a| Value::String(a.clone())).collect(),
        };
        // Nothing the subscriber watches is matched by this registration
        // information: there is no reduced copy to forward — and an empty
        // watchedAttributes is a payload 5.2.12 forbids.
        if watch.is_empty() {
            return None;
        }
        o.insert("watchedAttributes".into(), Value::Array(watch));
    }
    // 5.8.1.4: with splitEntities the remote sees only fragments — the
    // q/geoQ/scopeQ conditions are evaluated LOCALLY after the 5.8.6 merge
    // (splitEntities is a Subscription member, 5.2.12)
    if sub.get("splitEntities").and_then(Value::as_bool) == Some(true) {
        for k in ["q", "geoQ", "scopeQ"] {
            o.remove(k);
        }
    }
    if let Some(n) = o.get_mut("notification").and_then(Value::as_object_mut) {
        for k in ["attributes", "pick", "omit"] {
            n.remove(k);
        }
        n.insert(
            "endpoint".into(),
            json!({"uri": format!("{}/ngsi-ld/ex/remote-notify", st.public_url)}),
        );
    }
    Some(copy)
}

/// One forwarded subscription operation, through the shared federation
/// forward (egress policy, Via, contextSourceInfo, tenant mapping).
/// `via` is the chain the Subscription arrived with ([`sub_via_headers`]);
/// the forward appends this broker's alias to it (6.3.17), so downstream
/// brokers see the full path and can cut a loop.
#[allow(clippy::too_many_arguments)] // mirrors the wire: one param per forwarded request part
async fn forward_sub(
    st: &AppState,
    tenant: &TenantId,
    method: reqwest::Method,
    url: String,
    reg_id: &str,
    reg: &Value,
    ctx_url: &str,
    via: &HeaderMap,
    body: Option<Value>,
) -> (u16, Value) {
    let fed = crate::federation::fed_reg_of(reg_id, reg);
    let (status, body, _) =
        crate::federation::forward(st, method, url, &[], via, tenant, &fed, ctx_url, body).await;
    (status, body)
}

/// 5.8.6 splitEntities=true inbound merge: each notified Entity "shall be
/// retrieved locally and from all Context Sources that have information
/// about these Entities, except for the one from which the Notification has
/// been received", merged with the notified fragment, and "all Entities
/// that do not match the query, geoquery and Scope query conditions of the
/// local Subscription shall be removed".
async fn split_merge(
    st: &AppState,
    tenant: &TenantId,
    sub: &Value,
    origin_reg: Option<&str>,
    data: Vec<Value>,
) -> Vec<Value> {
    let ctx = crate::notify::sub_context(st, sub).await;
    let headers = HeaderMap::new();
    let mut out = Vec::new();
    for ent in data {
        let Some(obj) = ent.as_object() else { continue };
        // inbound notification presentation → the expanded storage form
        let Ok(mut merged) =
            antares_jsonld::expand_entity(obj, &ctx, antares_jsonld::ExpandOpts::default())
        else {
            continue;
        };
        let Some(id) = merged.get("id").and_then(Value::as_str).map(str::to_owned) else {
            continue;
        };
        if let Ok(Some(local)) = st.store.get(tenant, Kind::Entity, &id) {
            crate::federation::merge_docs(&mut merged, &local, false);
        }
        let mut warnings = Vec::new();
        let fed = crate::federation::fed_retrieve(
            st,
            tenant,
            &headers,
            &ctx,
            &id,
            None,
            origin_reg,
            &mut warnings,
        )
        .await;
        for aux_pass in [false, true] {
            for (aux, doc) in &fed {
                if *aux == aux_pass {
                    crate::federation::merge_docs(&mut merged, doc, *aux);
                }
            }
        }
        if crate::notify::conditions_match(
            sub,
            &merged,
            &ctx,
            &crate::notify::store_lookup(st, tenant),
        ) {
            // 5.3.1/5.8.6: notification data carries Entities in their
            // API representation — shape and compact the merged storage
            // form exactly like the local notify path.
            let shape = crate::notify::notif_shape(sub, &ctx);
            let shaped = crate::repr::apply(&merged, &shape.repr);
            out.push(crate::entities::compact_for(&shape.repr, &shaped, &ctx));
        }
    }
    out
}

/// POST /ngsi-ld/ex/remote-notify — the local broker's endpoint for
/// notifications from forwarded subscription copies. 5.8.1.4: "the mapping
/// of the received subscriptionId with the own Subscription identifier …
/// to enable forwarding received notifications to the original subscriber."
pub async fn remote_notify(State(st): State<AppState>, body: Bytes) -> Response {
    match remote_notify_inner(&st, &body).await {
        Ok(r) => r,
        Err(e) => e.into_response(),
    }
}

async fn remote_notify_inner(st: &AppState, body: &[u8]) -> ApiResult<Response> {
    let v: Value = serde_json::from_slice(body)
        .map_err(|e| NgsiError::InvalidRequest(format!("body is not valid JSON: {e}")))?;
    // Peer-facing entry point: the Entity count is capped before any store
    // touch or per-Entity work, under the same ceiling a client batch gets
    // (ANTARES_MAX_BATCH_ITEMS). One notification drives one local retrieve
    // and one federated fan-out per Entity in the 5.8.6 merge, so an
    // uncapped data array is an amplification lever.
    let cap = *crate::bounds::MAX_BATCH_ITEMS;
    if v.get("data")
        .and_then(Value::as_array)
        .is_some_and(|a| a.len() > cap)
    {
        return Err(NgsiError::BadRequestData(format!(
            "notification data carries more than {cap} Entities"
        ))
        .into());
    }
    let sid = v
        .get("subscriptionId")
        .and_then(Value::as_str)
        .ok_or_else(|| NgsiError::BadRequestData("notification without subscriptionId".into()))?;
    let Some((tenant, own_id)) = inbound_get(st, sid) else {
        return Err(ApiError::from(NgsiError::ResourceNotFound(format!(
            "no distributed subscription maps {sid}"
        ))));
    };
    let tenant = TenantId::new(&tenant)
        .map_err(|_| NgsiError::InternalError("stored tenant invalid".into()))?;
    let Some(sub) = st
        .store
        .get(&tenant, Kind::Subscription, &own_id)
        .ok()
        .flatten()
    else {
        // the subscriber is gone: prune the mapping on touch so a remote
        // that keeps notifying cannot pin a dead index entry forever, and
        // answer about the peer's own id — the local Subscription id is not
        // the peer's to learn
        inbound_delete(st, sid);
        return Err(ApiError::from(NgsiError::ResourceNotFound(format!(
            "no distributed subscription maps {sid}"
        ))));
    };
    // the origin of this notification is the registration its remote
    // subscription was created at
    let origin_reg_id = ds_remotes(&ds_get(st, &tenant, &own_id))
        .into_iter()
        .find(|(_, (_, rid))| rid.as_str() == sid)
        .map(|(reg_id, _)| reg_id);
    // 5.8.6: "if a Context Source filter is defined, then only the
    // subscribed Entities whose origin Context Source matches the referred
    // filter shall be included".
    if let Some(csf) = sub.get("csf").and_then(Value::as_str) {
        if let Ok(ast) = antares_ql::parse_q(csf) {
            let origin_reg = origin_reg_id.as_ref().and_then(|reg_id| {
                st.store
                    .get(&tenant, Kind::Registration, reg_id)
                    .ok()
                    .flatten()
            });
            let ctx = st.loader.core();
            let matches =
                origin_reg.is_some_and(|reg| crate::csource::csf_matches(&ast, &reg, &ctx));
            if !matches {
                // origin gated out — acknowledged, nothing forwarded
                return Ok(StatusCode::OK.into_response());
            }
        }
    }
    let mut data: Vec<Value> = v
        .get("data")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    // 5.2.33 / 5.8.1.4: the remote copy carries the REGISTRATION's entity
    // scope, which may be broader than the original Subscription's own
    // entities selector — re-filter inbound entities against the original
    // selector (id over idPattern precedence included) before forwarding.
    let sel_ctx = crate::notify::sub_context(st, &sub).await;
    data.retain(|e| {
        let id = e.get("id").and_then(Value::as_str).unwrap_or("");
        let types: Vec<Value> = match e.get("type") {
            Some(Value::String(t)) => vec![Value::String(sel_ctx.expand_key(t))],
            Some(Value::Array(a)) => a
                .iter()
                .filter_map(Value::as_str)
                .map(|t| Value::String(sel_ctx.expand_key(t)))
                .collect(),
            _ => Vec::new(),
        };
        let shim = serde_json::json!({"id": id, "type": types});
        crate::notify::selector_match(&sub, &shim, &sel_ctx)
    });
    if data.is_empty() {
        // acknowledged; nothing the original Subscription selected
        return Ok(StatusCode::OK.into_response());
    }
    // 5.8.6 splitEntities=true: the notified Entities are fragments —
    // retrieve them locally and from all other Context Sources (except the
    // origin), merge, and re-filter by the local Subscription's conditions.
    if sub.get("splitEntities").and_then(Value::as_bool) == Some(true) {
        data = split_merge(st, &tenant, &sub, origin_reg_id.as_deref(), data).await;
        if data.is_empty() {
            // "If there are Entities in the data member of the Notification
            // copy, the Notification copy shall be forwarded" — none left
            return Ok(StatusCode::OK.into_response());
        }
    }
    // 5.8.6: forward to the original subscriber under the OWN subscriptionId
    let (st2, sub2, t2) = (st.clone(), sub.clone(), tenant.clone());
    crate::spawn(async move {
        let ctx = crate::notify::sub_context(&st2, &sub2).await;
        crate::notify::deliver(&st2, &t2, &sub2, data, &ctx).await;
    });
    Ok(StatusCode::OK.into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The default @context prefix every Term expands to (5.5.7).
    const DC: &str = "https://uri.etsi.org/ngsi-ld/default-context/";

    /// A registration exactly as `on_csource_notification` receives it: out
    /// of a Context Source Notification, whose names and types
    /// `present_registration` has COMPACTED.
    fn reg_doc() -> Value {
        json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:r1",
            "endpoint": "http://source.example.org",
            "operations": ["createSubscription", "updateSubscription", "deleteSubscription"],
            "information": [{
                "entities": [{"type": ["Vehicle"], "id": "urn:ngsi-ld:Vehicle:1"}],
                "propertyNames": ["speed"],
            }],
        })
    }

    /// A Subscription exactly as the store holds it: names and types
    /// EXPANDED by `normalize_subscription` (5.5.7).
    fn sub_doc() -> Value {
        json!({
            "id": "urn:ngsi-ld:Subscription:own",
            "type": "Subscription",
            "entities": [{"type": format!("{DC}Vehicle")}, {"type": format!("{DC}Device")}],
            "watchedAttributes": [format!("{DC}speed"), format!("{DC}brand")],
            "q": "speed>10",
            "geoQ": {"georel": "near;maxDistance==1000"},
            "scopeQ": "/A",
            "localOnly": false,
            "status": "active",
            "timesSent": 7,
            "createdAt": "2026-01-01T00:00:00Z",
            "modifiedAt": "2026-01-02T00:00:00Z",
            "__context": "http://example.org/ctx.jsonld",
            "__via": "1.1 upstream-broker",
            "notification": {
                "attributes": [format!("{DC}speed")],
                "pick": ["speed"],
                "omit": ["brand"],
                "endpoint": {"uri": "http://subscriber.example.org/cb"},
            },
        })
    }

    /// 5.8.1.4: "a copy of the original Subscription shall be reduced to
    /// what is matched by the registration information … Also from the
    /// notification member, the attributes, pick and omit members are to be
    /// removed. The copied Subscription is then forwarded to the Context
    /// Source as a new Subscription where the notification endpoint is set
    /// to that of the local Broker." Nothing outside the registration's
    /// scope, and no local bookkeeping, may travel with the copy.
    #[test]
    fn clause_5_8_1_reduced_copy_carries_only_the_registration_scope() {
        let st = AppState::new("antares-ds-reduce".into());
        let copy = reduced_copy(
            &st,
            &sub_doc(),
            &reg_doc(),
            "urn:ngsi-ld:Subscription:remote1",
            &st.loader.core(),
        )
        .expect("a copy the registration covers");
        assert_eq!(copy["id"], json!("urn:ngsi-ld:Subscription:remote1"));
        assert_ne!(
            copy["id"],
            json!("urn:ngsi-ld:Subscription:own"),
            "the remote must be told the broker-generated id, not the local one"
        );
        let ents = copy["entities"].as_array().expect("entities");
        assert_eq!(ents.len(), 1, "{copy}");
        assert_eq!(ents[0]["type"], json!(format!("{DC}Vehicle")));
        assert!(
            !copy.to_string().contains(&format!("{DC}Device")),
            "a selector the registration does not cover must NOT be forwarded: {copy}"
        );
        assert_eq!(
            copy["watchedAttributes"],
            json!([format!("{DC}speed")]),
            "watchedAttributes are intersected with the registered names"
        );
        // local-only bookkeeping never leaves the broker — the Via chain
        // travels as the HTTP header the forward extends, never in the body
        for k in [
            "status",
            "timesSent",
            "lastNotification",
            "lastSuccess",
            "lastFailure",
            "createdAt",
            "modifiedAt",
            "__context",
            "__via",
            "localOnly",
        ] {
            assert!(copy.get(k).is_none(), "{k} must not be forwarded: {copy}");
        }
        let n = &copy["notification"];
        for k in ["attributes", "pick", "omit"] {
            assert!(
                n.get(k).is_none(),
                "notification.{k} must be removed: {copy}"
            );
        }
        let uri = n["endpoint"]["uri"].as_str().expect("endpoint uri");
        assert!(uri.ends_with("/ngsi-ld/ex/remote-notify"), "{uri}");
        assert_ne!(
            uri, "http://subscriber.example.org/cb",
            "the source must never learn the original subscriber's endpoint"
        );
        // splitEntities is absent here, so the filters stay on the copy
        assert_eq!(copy["q"], json!("speed>10"));
    }

    /// 5.8.1.4: "If the splitEntities member is explicitly set to true …
    /// the members q, geoQ and scopeQ shall be removed from the created
    /// copy"; and a Subscription with no watchedAttributes is still reduced
    /// to the registered names, so an unregistered Attribute change cannot
    /// notify through the chain.
    #[test]
    fn clause_5_8_1_reduced_copy_split_and_watch_everything() {
        let st = AppState::new("antares-ds-split".into());
        let mut sub = sub_doc();
        sub["splitEntities"] = json!(true);
        let copy = reduced_copy(
            &st,
            &sub,
            &reg_doc(),
            "urn:ngsi-ld:Subscription:remote2",
            &st.loader.core(),
        )
        .expect("copy");
        for k in ["q", "geoQ", "scopeQ"] {
            assert!(
                copy.get(k).is_none(),
                "{k} is evaluated locally after the 5.8.6 merge: {copy}"
            );
        }
        let mut watch_all = sub_doc();
        watch_all
            .as_object_mut()
            .expect("object")
            .remove("watchedAttributes");
        let copy = reduced_copy(
            &st,
            &watch_all,
            &reg_doc(),
            "urn:ngsi-ld:Subscription:remote3",
            &st.loader.core(),
        )
        .expect("copy");
        assert_eq!(
            copy["watchedAttributes"],
            json!([format!("{DC}speed")]),
            "a watch-everything Subscription still only watches the \
             registered names at the source"
        );
    }

    /// 5.8.1.4 "reduced to what is matched by the registration information",
    /// with 5.5.7 Term to URI expansion: the registration arrives through a
    /// Context Source Notification (names COMPACTED), the Subscription comes
    /// out of the store (names EXPANDED). The intersection must be taken in
    /// ONE representation — an empty `watchedAttributes` is a payload 5.2.12
    /// forbids, and it narrows the forwarded copy to nothing.
    #[test]
    fn clause_5_8_1_reduced_copy_intersects_across_representations() {
        let st = AppState::new("antares-ds-expand".into());
        let copy = reduced_copy(
            &st,
            &sub_doc(),
            &reg_doc(),
            "urn:ngsi-ld:Subscription:remote4",
            &st.loader.core(),
        )
        .expect("a copy the registration covers");
        assert_eq!(
            copy["watchedAttributes"],
            json!([format!("{DC}speed")]),
            "the compacted registered name must be expanded before the \
             intersection: {copy}"
        );
        assert_ne!(
            copy["watchedAttributes"],
            json!([]),
            "5.2.12: watchedAttributes, when present, is a non-empty array"
        );
        assert!(
            !copy.to_string().contains(&format!("{DC}brand")),
            "an unregistered watched name must not be forwarded: {copy}"
        );
        assert_eq!(
            copy["entities"],
            json!([{"type": format!("{DC}Vehicle"), "id": "urn:ngsi-ld:Vehicle:1"}]),
            "the registration's selector travels in the Subscription's own \
             representation: {copy}"
        );
    }

    /// 5.8.1.4: "Based on the content of the Subscription, a Context Source
    /// Registration Subscription shall be created (clause 5.11.2)" — 5.11.2.4
    /// matches registrations on csf, geoQ, scopeQ, temporalQ and the
    /// notification attributes as well, so a Registration Subscription built
    /// from entities and watchedAttributes alone offers the Subscription to
    /// sources the subscriber excluded.
    #[tokio::test]
    async fn clause_5_8_1_csr_subscription_carries_every_matching_member() {
        let st = AppState::new("antares-ds-csr".into());
        let t = TenantId::default();
        let mut sub = sub_doc();
        sub["csf"] = json!("name==\"SourceA\"");
        sub["temporalQ"] = json!({"timerel": "before", "timeAt": "2026-01-01T00:00:00Z"});
        let own = sub["id"].as_str().expect("id").to_owned();
        st.store
            .create(&t, Kind::Subscription, &own, sub.clone())
            .expect("create");
        on_subscription_created(&st, &t, &sub);
        let csr_id = ds_get(&st, &t, &own)["csr_sub"]
            .as_str()
            .expect("csr_sub")
            .to_owned();
        let csr = st
            .store
            .get(&t, Kind::CSourceSubscription, &csr_id)
            .ok()
            .flatten()
            .expect("csr subscription");
        for k in [
            "entities",
            "watchedAttributes",
            "csf",
            "geoQ",
            "scopeQ",
            "temporalQ",
        ] {
            assert!(
                csr.get(k).is_some(),
                "{k} decides which registrations match: {csr}"
            );
        }
        assert_eq!(
            csr["notification"]["attributes"],
            json!([format!("{DC}speed")]),
            "5.11.2.4 unions notification.attributes into the match spec: {csr}"
        );
        assert!(
            csr.get("q").is_none(),
            "q filters Entity Attributes, not registration properties: {csr}"
        );
    }

    /// 6.3.18: the Via header exists "to avoid infinite loops". A forwarded
    /// Subscription copy (5.8.1.4) arrives with the Via chain of the brokers
    /// it has already passed through; a chain that names THIS broker means
    /// the copy has looped back, so the Subscription serves locally and the
    /// distributed half is NOT created — otherwise two mutually registered
    /// brokers re-forward copies of copies without bound.
    #[tokio::test]
    async fn clause_6_3_18_looping_via_chain_suppresses_the_distributed_half() {
        let st = AppState::new("antares-ds-viahost".into());
        let t = TenantId::default();
        // the copy's chain already names this broker's own alias
        let mut looped = sub_doc();
        looped["__via"] = json!("1.1 sourceX, 1.1 antares-ds-viahost");
        let own = looped["id"].as_str().expect("id").to_owned();
        st.store
            .create(&t, Kind::Subscription, &own, looped.clone())
            .expect("create");
        on_subscription_created(&st, &t, &looped);
        assert!(
            ds_get(&st, &t, &own).get("csr_sub").is_none(),
            "a looped copy must not create the internal Registration Subscription"
        );
        // positive control: a chain naming only OTHER brokers is not a loop
        let mut chained = sub_doc();
        chained["id"] = json!("urn:ngsi-ld:Subscription:chained");
        chained["__via"] = json!("1.1 sourceX");
        st.store
            .create(
                &t,
                Kind::Subscription,
                "urn:ngsi-ld:Subscription:chained",
                chained.clone(),
            )
            .expect("create");
        on_subscription_created(&st, &t, &chained);
        assert!(
            ds_get(&st, &t, "urn:ngsi-ld:Subscription:chained")
                .get("csr_sub")
                .is_some(),
            "a pass-through chain (A->B->C) must keep the distributed half"
        );
    }

    /// 5.8.1.4 gates the whole distributed block on "If localOnly=false": a
    /// Subscription updated to localOnly=true forwards no further copy, and
    /// the internal Context Source Registration Subscription plus the
    /// mappings it already holds are torn down.
    #[tokio::test]
    async fn clause_5_8_1_local_only_flip_tears_the_distributed_half_down() {
        let st = AppState::new("antares-ds-local".into());
        let t = TenantId::default();
        let mut sub = sub_doc();
        sub["localOnly"] = json!(true);
        let own = sub["id"].as_str().expect("id").to_owned();
        let csr_id = "urn:ngsi-ld:CSourceSubscription:distsub:x";
        st.store
            .create(&t, Kind::Subscription, &own, sub.clone())
            .expect("create");
        st.store
            .create(&t, Kind::CSourceSubscription, csr_id, json!({"id": csr_id}))
            .expect("create");
        ds_put(&st, &t, &own, json!({"csr_sub": csr_id}));
        // a Context Source Notification arriving after the flip creates
        // nothing at the source
        on_csource_notification(&st, &t, &own, Some("newlyMatching"), &[reg_doc()]).await;
        assert!(
            ds_remotes(&ds_get(&st, &t, &own)).is_empty(),
            "a local-only Subscription forwards no copy"
        );
        on_subscription_updated(&st, &t, &own);
        assert!(
            st.store
                .get(&t, Kind::CSourceSubscription, csr_id)
                .ok()
                .flatten()
                .is_none(),
            "the internal Registration Subscription must not survive the flip"
        );
        assert!(
            st.store
                .get(&t, Kind::DistSub, &own)
                .ok()
                .flatten()
                .is_none(),
            "the mapping document must not survive the flip"
        );
    }

    /// 5.8.1.4 stores ONE remote subscriptionId per (Subscription,
    /// registration) pair. Two Context Source Notifications for the same pair
    /// interleave at their forward, so the second insert is refused under the
    /// store's own lock instead of overwriting — an overwritten mapping
    /// orphans a live remote subscription at the source and doubles every
    /// notification the subscriber receives.
    #[test]
    fn clause_5_8_1_second_mapping_for_a_registration_is_refused() {
        let st = AppState::new("antares-ds-cas".into());
        let t = TenantId::default();
        let own = "urn:ngsi-ld:Subscription:own";
        ds_put(&st, &t, own, json!({"csr_sub": "urn:csr:1"}));
        assert!(ds_set_remote(
            &st,
            &t,
            own,
            "urn:reg:1",
            Some(json!(["http://s", "urn:remote:1"]))
        ));
        assert!(
            !ds_set_remote(
                &st,
                &t,
                own,
                "urn:reg:1",
                Some(json!(["http://s", "urn:remote:2"]))
            ),
            "the registration already has a remote subscription"
        );
        let got = ds_remotes(&ds_get(&st, &t, own));
        assert_eq!(got.len(), 1);
        assert_eq!(
            got[0].1 .1, "urn:remote:1",
            "the first mapping survives — the loser rolls its own copy back"
        );
    }

    /// 4.14: "the Tenant information from the Context Source Registration has
    /// to be used" — the 5.8.5.4 delete-forward must travel with the stored
    /// registration, not a synthetic id/endpoint pair, or it lands in the
    /// peer's default tenant and the remote subscription is never deleted.
    /// The synthetic fallback stays for the already-deleted registration.
    #[test]
    fn clause_5_8_5_delete_forward_carries_the_stored_registration() {
        let stored = json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:r1",
            "endpoint": "http://source.example.org",
            "tenant": "cityB",
            "contextSourceInfo": [{"key": "Authorization", "value": "Bearer t"}],
        });
        let reg = forward_reg(
            Some(stored.clone()),
            "urn:ngsi-ld:ContextSourceRegistration:r1",
            "http://source.example.org",
        );
        assert_eq!(reg["tenant"], json!("cityB"), "{reg}");
        assert_eq!(reg["contextSourceInfo"], stored["contextSourceInfo"]);
        let gone = forward_reg(
            None,
            "urn:ngsi-ld:ContextSourceRegistration:r1",
            "http://source.example.org",
        );
        assert_eq!(gone["endpoint"], json!("http://source.example.org"));
        assert!(
            gone.get("tenant").is_none(),
            "a deleted registration carries nothing to forward with: {gone}"
        );
    }

    /// 5.8.1.4: "The @context to be used for sending Notifications related to
    /// this Subscription shall be the one specified in the jsonldContext
    /// field." The forwarded copy carries the subscriber's own terms (`q` is
    /// stored verbatim), so it must be shipped under the Subscription's
    /// @context — under the core context those terms name Attributes that do
    /// not exist at the source.
    #[test]
    fn clause_5_8_1_forwarded_copy_is_shipped_under_the_subscription_context() {
        let st = AppState::new("antares-ds-ctx".into());
        let core = st.loader.core().source.clone();
        let hosted = json!({"jsonldContext": "http://broker.example.org/jsonldContexts/abc",
                            "__context": "http://example.org/ctx.jsonld"});
        assert_eq!(
            sub_ctx_url(&st, &hosted),
            "http://broker.example.org/jsonldContexts/abc",
            "jsonldContext wins — it is the member 5.8.1.4 names"
        );
        let own = json!({"__context": "http://example.org/ctx.jsonld"});
        assert_eq!(sub_ctx_url(&st, &own), "http://example.org/ctx.jsonld");
        assert_ne!(
            sub_ctx_url(&st, &own),
            crate::federation::ctx_link_url(&HeaderMap::new(), &core),
            "a Subscription with its own vocabulary is not forwarded under \
             the core context"
        );
        assert_eq!(
            sub_ctx_url(&st, &json!({})),
            crate::federation::ctx_link_url(&HeaderMap::new(), &core),
            "with no context of its own the core context is the fallback"
        );
    }

    /// 5.8.1.4 stores three mappings; every store touch takes the tenant
    /// first (4.14: "an NGSI-LD system shall behave as if the tenants were
    /// separate systems"). The inbound index is keyed by the
    /// broker-generated remote subscriptionId under its own reserved
    /// tenant, so it never collides with a tenant's own mapping documents.
    #[test]
    fn clause_5_8_1_mappings_are_tenant_scoped() {
        let st = AppState::new("antares-ds-tenant".into());
        let a = TenantId::new("alpha").expect("tenant");
        let b = TenantId::new("beta").expect("tenant");
        let own = "urn:ngsi-ld:Subscription:own";
        ds_put(
            &st,
            &a,
            own,
            json!({"csr_sub": "urn:csr:1",
                   "remotes": {"urn:reg:1": ["http://s", "urn:remote:1"]}}),
        );
        assert_eq!(ds_remotes(&ds_get(&st, &a, own)).len(), 1);
        assert!(
            ds_remotes(&ds_get(&st, &b, own)).is_empty(),
            "another tenant must not read the remote mapping"
        );
        assert!(ds_get(&st, &b, own).get("csr_sub").is_none());
        inbound_put(&st, "urn:remote:1", &a, own);
        assert_eq!(
            inbound_get(&st, "urn:remote:1"),
            Some(("alpha".to_owned(), own.to_owned()))
        );
        assert!(
            st.store
                .get(&a, Kind::DistSub, "urn:remote:1")
                .ok()
                .flatten()
                .is_none(),
            "the index entry must not land in the subscriber's own namespace"
        );
        inbound_delete(&st, "urn:remote:1");
        assert!(inbound_get(&st, "urn:remote:1").is_none());
    }

    /// The remotes index is read back from storage, so every malformed
    /// shape is dropped rather than indexed or unwrapped.
    #[test]
    fn clause_5_8_1_remotes_index_drops_malformed_entries() {
        let doc = json!({"remotes": {
            "urn:reg:ok": ["http://s", "urn:remote:1"],
            "urn:reg:short": ["http://s"],
            "urn:reg:nonstring": [1, 2],
            "urn:reg:notarray": "http://s",
            "urn:reg:empty": [],
        }});
        let got = ds_remotes(&doc);
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].0, "urn:reg:ok");
        assert_eq!(got[0].1 .1, "urn:remote:1");
        assert!(ds_remotes(&json!({"remotes": "nope"})).is_empty());
        assert!(ds_remotes(&json!({})).is_empty());
    }

    /// 5.8.1.4 / 5.8.5.4: the per-registration branches interleave at their
    /// forward, so one registration's mapping write must not drop another's,
    /// and a mapping document deleted by Delete Subscription must stay
    /// deleted — an in-flight branch may not write it back.
    #[test]
    fn clause_5_8_5_mapping_writes_never_resurrect_a_deleted_subscription() {
        let st = AppState::new("antares-ds-resurrect".into());
        let t = TenantId::default();
        let own = "urn:ngsi-ld:Subscription:own";
        ds_put(&st, &t, own, json!({"csr_sub": "urn:csr:1"}));
        assert!(ds_set_remote(
            &st,
            &t,
            own,
            "urn:reg:1",
            Some(json!(["http://s1", "urn:remote:1"]))
        ));
        assert!(ds_set_remote(
            &st,
            &t,
            own,
            "urn:reg:2",
            Some(json!(["http://s2", "urn:remote:2"]))
        ));
        assert_eq!(
            ds_remotes(&ds_get(&st, &t, own)).len(),
            2,
            "a second registration's write must not drop the first"
        );
        assert!(ds_set_remote(&st, &t, own, "urn:reg:1", None));
        assert_eq!(ds_remotes(&ds_get(&st, &t, own)).len(), 1);
        let _ = st.store.delete(&t, Kind::DistSub, own);
        assert!(
            !ds_set_remote(
                &st,
                &t,
                own,
                "urn:reg:3",
                Some(json!(["http://s3", "urn:remote:3"]))
            ),
            "there is no mapping document left to write to"
        );
        assert!(
            st.store
                .get(&t, Kind::DistSub, own)
                .ok()
                .flatten()
                .is_none(),
            "a deleted mapping document must stay deleted"
        );
    }

    /// The inbound notification endpoint is peer-facing: the Entity count is
    /// capped before any store touch, because the 5.8.6 merge runs one local
    /// retrieve and one federated fan-out per notified Entity.
    #[tokio::test]
    async fn clause_5_8_6_inbound_notification_entity_count_is_capped() {
        let st = AppState::new("antares-ds-cap".into());
        let cap = *crate::bounds::MAX_BATCH_ITEMS;
        let body = |n: usize| {
            json!({
                "type": "Notification",
                "subscriptionId": "urn:ngsi-ld:Subscription:remote1",
                "data": vec![json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle"}); n],
            })
            .to_string()
        };
        let over = remote_notify(State(st.clone()), Bytes::from(body(cap + 1))).await;
        assert_eq!(
            over.status(),
            StatusCode::BAD_REQUEST,
            "an over-cap notification is rejected before the mapping lookup"
        );
        let at_cap = remote_notify(State(st.clone()), Bytes::from(body(cap))).await;
        assert_eq!(
            at_cap.status(),
            StatusCode::NOT_FOUND,
            "the ceiling itself is accepted — the unknown mapping is what stops it"
        );
    }

    /// 5.8.1.4 stores the mapping "to enable forwarding received
    /// notifications to the original subscriber": once that subscriber is
    /// gone the mapping is dead weight, so it is pruned on touch, and the
    /// peer is answered about its own id — never told the local one.
    #[tokio::test]
    async fn clause_5_8_1_mapping_to_a_deleted_subscription_is_pruned() {
        let st = AppState::new("antares-ds-prune".into());
        let t = TenantId::new("alpha").expect("tenant");
        let remote = "urn:ngsi-ld:Subscription:remote9";
        inbound_put(&st, remote, &t, "urn:ngsi-ld:Subscription:gone");
        let body =
            json!({"type": "Notification", "subscriptionId": remote, "data": []}).to_string();
        let resp = remote_notify(State(st.clone()), Bytes::from(body)).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        assert!(
            inbound_get(&st, remote).is_none(),
            "the dead mapping must not survive the touch"
        );
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let text = String::from_utf8_lossy(&bytes);
        assert!(
            !text.contains("urn:ngsi-ld:Subscription:gone"),
            "the local Subscription id must not leak to the peer: {text}"
        );
    }
}
