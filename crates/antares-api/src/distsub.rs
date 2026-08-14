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
        let _ = st.store.create(tenant, Kind::DistSub, own_id, doc);
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

fn inbound_put(st: &AppState, remote_id: &str, tenant: &TenantId, own_id: &str) {
    if let Some(idx) = ds_index_tenant() {
        let _ = st.store.create(
            &idx,
            Kind::DistSub,
            remote_id,
            json!({"tenant": tenant.as_str(), "own": own_id}),
        );
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

/// 5.8.1.4: "Based on the content of the Subscription, a Context Source
/// Registration Subscription shall be created (clause 5.11.2)" — internal,
/// with the urn:antares:distsub endpoint handled in-process by notify.
pub(crate) fn on_subscription_created(st: &AppState, tenant: &TenantId, sub: &Value) {
    if !distributed(sub) {
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
    for k in ["entities", "watchedAttributes", "__context"] {
        if let Some(v) = sub.get(k) {
            doc[k] = v.clone();
        }
    }
    if st
        .store
        .create(tenant, Kind::CSourceSubscription, &csr_id, doc)
        .unwrap_or(false)
    {
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
                    for k in ["entities", "watchedAttributes"] {
                        match sub.get(k) {
                            Some(v) => doc[k] = v.clone(),
                            None => {
                                doc.as_object_mut().map(|o| o.remove(k));
                            }
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
        let mut copy = reduced_copy(st, &sub, &reg, &remote_id);
        copy.as_object_mut().map(|o| o.remove("id"));
        let (st2, t2) = (st.clone(), tenant.clone());
        crate::spawn(async move {
            forward_sub(
                &st2,
                &t2,
                reqwest::Method::PATCH,
                format!("{endpoint}/ngsi-ld/v1/subscriptions/{remote_id}"),
                &reg_id,
                &reg,
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
        let supported = st
            .store
            .get(tenant, Kind::Registration, &reg_id)
            .ok()
            .flatten()
            .is_none_or(|reg| crate::federation::doc_supports(&reg, "deleteSubscription"));
        if !supported {
            continue;
        }
        let (st2, t2) = (st.clone(), tenant.clone());
        crate::spawn(async move {
            let reg = json!({"id": reg_id, "endpoint": endpoint});
            forward_sub(
                &st2,
                &t2,
                reqwest::Method::DELETE,
                format!("{endpoint}/ngsi-ld/v1/subscriptions/{remote_id}"),
                &reg_id,
                &reg,
                None,
            )
            .await;
        });
    }
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
                let copy = reduced_copy(st, &sub, reg, &remote_id);
                let (status, _) = forward_sub(
                    st,
                    tenant,
                    reqwest::Method::POST,
                    format!("{endpoint}/ngsi-ld/v1/subscriptions"),
                    reg_id,
                    reg,
                    Some(copy),
                )
                .await;
                if (200..300).contains(&status) {
                    inbound_put(st, &remote_id, tenant, own_id);
                    let mut doc = ds_get(st, tenant, own_id);
                    doc["remotes"][reg_id] = json!([endpoint.clone(), remote_id]);
                    ds_put(st, tenant, own_id, doc);
                }
            }
            ("updated", Some((_, remote_id)))
                if crate::federation::doc_supports(reg, "updateSubscription") =>
            {
                let mut copy = reduced_copy(st, &sub, reg, &remote_id);
                copy.as_object_mut().map(|o| o.remove("id"));
                forward_sub(
                    st,
                    tenant,
                    reqwest::Method::PATCH,
                    format!("{endpoint}/ngsi-ld/v1/subscriptions/{remote_id}"),
                    reg_id,
                    reg,
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
                    None,
                )
                .await;
                let mut doc = ds_get(st, tenant, own_id);
                if let Some(m) = doc.get_mut("remotes").and_then(Value::as_object_mut) {
                    m.remove(reg_id);
                }
                ds_put(st, tenant, own_id, doc);
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
fn reduced_copy(st: &AppState, sub: &Value, reg: &Value, remote_id: &str) -> Value {
    let mut copy = sub.clone();
    let Some(o) = copy.as_object_mut() else {
        return copy;
    };
    for k in [
        "status",
        "timesSent",
        "lastNotification",
        "lastSuccess",
        "lastFailure",
        "createdAt",
        "modifiedAt",
        "__context",
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
            e
        })
        .collect();
    if !reg_entities.is_empty() {
        o.insert("entities".into(), Value::Array(reg_entities));
    }
    // watchedAttributes ∩ the registration's attribute scope
    let reg_attrs: Vec<&str> = reg
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
        })
        .collect();
    if !reg_attrs.is_empty() {
        if let Some(w) = o.get("watchedAttributes").and_then(Value::as_array) {
            let kept: Vec<Value> = w
                .iter()
                .filter(|a| a.as_str().is_some_and(|a| reg_attrs.contains(&a)))
                .cloned()
                .collect();
            o.insert("watchedAttributes".into(), Value::Array(kept));
        }
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
    copy
}

/// One forwarded subscription operation, through the shared federation
/// forward (egress policy, Via, contextSourceInfo, tenant mapping).
async fn forward_sub(
    st: &AppState,
    tenant: &TenantId,
    method: reqwest::Method,
    url: String,
    reg_id: &str,
    reg: &Value,
    body: Option<Value>,
) -> (u16, Value) {
    let fed = crate::federation::fed_reg_of(reg_id, reg);
    let ctx_url = crate::federation::ctx_link_url(&HeaderMap::new(), &st.loader.core().source);
    let (status, body, _) = crate::federation::forward(
        st,
        method,
        url,
        &[],
        &HeaderMap::new(),
        tenant,
        &fed,
        &ctx_url,
        body,
    )
    .await;
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
        if crate::notify::conditions_match(st, tenant, sub, &merged, &ctx) {
            out.push(merged);
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
        return Err(ApiError::from(NgsiError::ResourceNotFound(format!(
            "subscription {own_id} is gone"
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
