//! Attribute-level operations (5.6.2–5.6.5, 5.6.19; resources 6.6/6.7).

use crate::negotiate::*;
use crate::state::{now_iso, AppState};
use antares_jsonld::{expand_entity, ExpandOpts};
use antares_model::NgsiError;
use antares_sql::store::Kind;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use serde_json::{Map, Value};
use std::collections::HashMap;

use crate::negotiate::CleanParams;

/// Attribute names in paths must be valid terms/IRIs (4.6.2) — 400 otherwise.
pub(crate) fn check_attr_name(attr: &str) -> Result<(), NgsiError> {
    // 4.6.2 supported names: no '@' (keyword territory), no parens/quotes/etc.
    let ok = !attr.is_empty()
        && attr
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_:.#/%-+".contains(c));
    if ok {
        Ok(())
    } else {
        Err(NgsiError::BadRequestData(format!(
            "invalid attribute name {attr:?}"
        )))
    }
}

/// Outcome of a multi-attribute write: 204 when everything applied, else 207
/// with an UpdateResult (5.2.18).
fn update_result(
    tenant: &antares_model::TenantId,
    updated: Vec<String>,
    not_updated: Vec<(String, String)>,
    ctx: &antares_jsonld::Context,
) -> Response {
    if not_updated.is_empty() {
        return no_content(tenant);
    }
    // 207 bodies carry fully-qualified attribute names (6.3.5: errors and
    // multi-status responses are application/json with expanded names).
    let _ = ctx;
    let payload = serde_json::json!({
        "updated": updated,
        "notUpdated": not_updated
            .iter()
            .map(|(a, r)| serde_json::json!({
                "attributeName": a,
                "reason": r,
            }))
            .collect::<Vec<_>>(),
    });
    multi_status(payload, tenant)
}

// ---------- POST /entities/{id}/attrs/ — Append (5.6.3) ----------

pub async fn append_attrs(
    State(st): State<AppState>,
    Path(id): Path<String>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    match append_attrs_inner(&st, &id, &params, &headers, &body).await {
        Ok(r) => r,
        Err(e) => e.into_response(),
    }
}

async fn append_attrs_inner(
    st: &AppState,
    id: &str,
    params: &HashMap<String, String>,
    headers: &HeaderMap,
    body: &[u8],
) -> ApiResult<Response> {
    let tenant = tenant_from(headers)?;
    antares_model::EntityId::new(id)?;
    check_params(params, &["options", "local", "type"])?;
    let no_overwrite = params
        .get("options")
        .is_some_and(|o| o.split(',').any(|s| s.trim() == "noOverwrite"));
    let parsed = parse_body(&st.loader, headers, body, BodyKind::Standard).await?;
    let obj = parsed
        .value
        .as_object()
        .ok_or_else(|| NgsiError::BadRequestData("fragment must be a JSON object".into()))?;
    let fragment = expand_entity(
        obj,
        &parsed.ctx,
        ExpandOpts {
            fragment: true,
            allow_null: false,
            temporal: false,
            ..Default::default()
        },
    )?;
    let (mut regs, local_covered) =
        attr_fed_plan(st, &tenant, id, &fragment, &parsed.ctx, params, headers);
    if let Some(r) = crate::federation::handle_via_loop(
        headers,
        &crate::federation::alias_for(&st.host_alias, &tenant),
        &tenant,
        &mut regs,
    ) {
        return Ok(r);
    }
    let all_attr_iris = attr_iris_of(&fragment);
    let fragment = crate::federation::strip_covered_expanded(&fragment, &regs);
    let local_iris = attr_iris_of(&fragment);
    let ts = now_iso();
    let mut updated = Vec::new();
    let mut not_updated = Vec::new();
    let local_resp: Option<ApiResult<Response>> = if local_covered {
        None
    } else {
        let res = st.store.mutate(&tenant, Kind::Entity, id, |doc| {
            let target = doc.as_object_mut().expect("entity object");
            let frag = fragment.as_object().expect("fragment object");
            // 5.6.3: appended types extend the type set
            if let Some(new_types) = frag.get("type").and_then(Value::as_array) {
                let mut cur: Vec<Value> = target
                    .get("type")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                for t in new_types {
                    if !cur.contains(t) {
                        cur.push(t.clone());
                    }
                }
                target.insert("type".into(), Value::Array(cur));
                updated.push("type".into());
            }
            // appended scope: overwrite replaces; noOverwrite unions (010_07)
            if let Some(new_scope) = frag.get("scope") {
                if target.contains_key("scope") && no_overwrite {
                    let mut cur: Vec<Value> = target
                        .get("scope")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    for sc in new_scope.as_array().cloned().unwrap_or_default() {
                        if !cur.contains(&sc) {
                            cur.push(sc);
                        }
                    }
                    target.insert("scope".into(), Value::Array(cur));
                } else {
                    target.insert("scope".into(), new_scope.clone());
                }
                updated.push("scope".into());
            }
            for (k, v) in frag {
                if matches!(
                    k.as_str(),
                    "id" | "type" | "scope" | "createdAt" | "modifiedAt"
                ) {
                    continue;
                }
                let mut incoming = v.clone();
                stamp_new_attr(&mut incoming, &ts);
                match target.get_mut(k) {
                    None => {
                        target.insert(k.clone(), incoming);
                        updated.push(k.clone());
                    }
                    Some(existing) => {
                        let merged = merge_instance_sets(existing, &incoming, no_overwrite);
                        if merged {
                            updated.push(k.clone());
                        } else {
                            not_updated
                                .push((k.clone(), "attribute already exists (noOverwrite)".into()));
                        }
                    }
                }
            }
            target.insert("modifiedAt".into(), Value::String(ts.clone()));
            Ok::<(), NgsiError>(())
        })?;
        Some(match res {
            None => Err(NgsiError::ResourceNotFound(format!("entity {id} not found")).into()),
            Some(Err(e)) => Err(e.into()),
            Some(Ok(())) => Ok(update_result(
                &tenant,
                updated.clone(),
                not_updated.clone(),
                &parsed.ctx,
            )),
        })
    };
    if regs.is_empty() {
        return local_resp.expect("local path always runs without registrations");
    }
    let local_outcome = classify_local(&local_resp);
    let mut query = Vec::new();
    if let Some(o) = params.get("options") {
        query.push(("options".to_string(), o.clone()));
    }
    let fed_parts = crate::federation::fed_attr_parts(
        st,
        headers,
        &tenant,
        &parsed.ctx.source,
        &regs,
        "appendAttrs",
        reqwest::Method::POST,
        &format!("/entities/{id}/attrs/"),
        &query,
        Some(Value::Object(without_context_map(obj))),
    )
    .await;
    Ok(combine_attr_parts(
        &tenant,
        &all_attr_iris,
        &local_iris,
        local_outcome,
        updated,
        not_updated.into_iter().map(|(a, r)| (a, r, None)).collect(),
        &regs,
        &fed_parts,
    ))
}

/// Shared federation plan for attribute writes: matching non-aux
/// registrations + whether proxies cover every touched attribute.
fn attr_fed_plan(
    st: &AppState,
    tenant: &antares_model::TenantId,
    id: &str,
    fragment: &Value,
    ctx: &antares_jsonld::Context,
    params: &HashMap<String, String>,
    headers: &axum::http::HeaderMap,
) -> (Vec<crate::federation::FedReg>, bool) {
    let attr_iris: Vec<String> = fragment
        .as_object()
        .map(|o| {
            o.keys()
                .filter(|k| {
                    !matches!(
                        k.as_str(),
                        "id" | "type" | "scope" | "createdAt" | "modifiedAt"
                    )
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    attr_fed_plan_iris(st, tenant, id, attr_iris, ctx, params, headers)
}

fn attr_fed_plan_iris(
    st: &AppState,
    tenant: &antares_model::TenantId,
    id: &str,
    attr_iris: Vec<String>,
    ctx: &antares_jsonld::Context,
    params: &HashMap<String, String>,
    headers: &axum::http::HeaderMap,
) -> (Vec<crate::federation::FedReg>, bool) {
    let spec = crate::csource::CsrSpec {
        ids: Some(vec![id.to_owned()]),
        attrs: (!attr_iris.is_empty()).then(|| attr_iris.clone()),
        ..Default::default()
    };
    let regs = crate::federation::write_regs(st, tenant, &spec, ctx, params, headers);
    let covered = !regs.is_empty()
        && !attr_iris.is_empty()
        && attr_iris
            .iter()
            .all(|a| regs.iter().any(|r| r.is_proxy() && r.covers_attr(a)));
    (regs, covered)
}

/// Attribute IRIs of an expanded fragment (entity meta members excluded).
fn attr_iris_of(fragment: &Value) -> Vec<String> {
    fragment
        .as_object()
        .map(|o| {
            o.keys()
                .filter(|k| {
                    !matches!(
                        k.as_str(),
                        "id" | "type" | "scope" | "createdAt" | "modifiedAt"
                    )
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

/// Fold the buffered local response into a `LocalOutcome` without losing the
/// ProblemDetails reason.
fn classify_local(resp: &Option<ApiResult<Response>>) -> LocalOutcome {
    match resp {
        None => LocalOutcome::Skipped,
        Some(Ok(_)) => LocalOutcome::Ok,
        Some(Err(ApiError::Ngsi(e))) => {
            let pd = e.to_problem_details();
            if pd.status == 404 {
                LocalOutcome::NotFound(pd.detail)
            } else {
                LocalOutcome::Failed(pd.detail)
            }
        }
        Some(Err(_)) => LocalOutcome::Failed("local operation failed".into()),
    }
}

/// How the LOCAL half of a distributed /attrs operation ended.
enum LocalOutcome {
    /// proxies cover every touched attribute — no local write attempted
    Skipped,
    /// entity (or the addressed attribute) unknown locally
    NotFound(String),
    /// local write failed for another reason
    Failed(String),
    Ok,
}

/// V-15: a distributed /attrs operation answers 204, 404, or **207 with an
/// UpdateResult** (Tables 6.6.3.1-2, 6.6.3.2-2, 6.7.3.1-2, 6.7.3.2-2,
/// 6.7.3.3-2; 5.2.18 applies "regardless of whether local or distributed")
/// — never the batch {success, errors} shape. Per-registration failures are
/// listed per covered attribute with `registrationId` (5.2.19).
#[allow(clippy::too_many_arguments)]
fn combine_attr_parts(
    tenant: &antares_model::TenantId,
    attr_iris: &[String],
    local_iris: &[String],
    local: LocalOutcome,
    mut updated: Vec<String>,
    mut not_updated: Vec<(String, String, Option<String>)>,
    regs: &[crate::federation::FedReg],
    fed_parts: &[crate::federation::Part],
) -> Response {
    match &local {
        LocalOutcome::NotFound(d) | LocalOutcome::Failed(d) => {
            for a in local_iris {
                not_updated.push((a.clone(), d.clone(), None));
            }
        }
        _ => {}
    }
    let mut any_fed_ok = false;
    for (reg, part) in regs.iter().zip(fed_parts) {
        let covered: Vec<&String> = attr_iris.iter().filter(|a| reg.covers_attr(a)).collect();
        if part.ok() {
            any_fed_ok = true;
            for a in covered {
                if !updated.iter().any(|u| u == a.as_str()) {
                    updated.push(a.clone());
                }
            }
        } else {
            for a in covered {
                not_updated.push((a.clone(), part.detail.clone(), Some(reg.reg_id.clone())));
            }
        }
    }
    // nothing was found anywhere → 404 ProblemDetails (6.6/6.7 tables)
    if matches!(&local, LocalOutcome::NotFound(_)) && !any_fed_ok && updated.is_empty() {
        if let LocalOutcome::NotFound(d) = local {
            return ApiError::from(NgsiError::ResourceNotFound(d)).into_response();
        }
    }
    if not_updated.is_empty() {
        return no_content(tenant);
    }
    let nu: Vec<Value> = not_updated
        .into_iter()
        .map(|(a, r, reg_id)| {
            let mut m = Map::new();
            m.insert("attributeName".into(), Value::String(a));
            m.insert("reason".into(), Value::String(r));
            if let Some(rid) = reg_id.filter(|r| !r.is_empty()) {
                m.insert("registrationId".into(), Value::String(rid));
            }
            Value::Object(m)
        })
        .collect();
    multi_status(
        serde_json::json!({"updated": updated, "notUpdated": nu}),
        tenant,
    )
}

fn stamp_new_attr(v: &mut Value, ts: &str) {
    if let Some(arr) = v.as_array_mut() {
        for inst in arr {
            if let Some(o) = inst.as_object_mut() {
                o.insert("createdAt".into(), Value::String(ts.to_owned()));
                o.insert("modifiedAt".into(), Value::String(ts.to_owned()));
            }
        }
    }
}

/// Merge incoming instances into an existing instance array by datasetId.
/// Deletion-marker instances (urn:ngsi-ld:null) remove the matched instance.
/// Returns false when nothing was applied (noOverwrite and all existed).
fn merge_instance_sets(existing: &mut Value, incoming: &Value, no_overwrite: bool) -> bool {
    let (Some(cur), Some(inc)) = (existing.as_array_mut(), incoming.as_array()) else {
        return false;
    };
    let mut any = false;
    for ni in inc {
        let ds = ni.get("datasetId").and_then(Value::as_str);
        let pos = cur
            .iter()
            .position(|ci| ci.get("datasetId").and_then(Value::as_str) == ds);
        if antares_jsonld::is_deletion_instance(ni) {
            if let Some(p) = pos {
                cur.remove(p);
                any = true;
            }
            continue;
        }
        match pos {
            Some(p) => {
                if !no_overwrite {
                    // keep original createdAt
                    let created = cur[p].get("createdAt").cloned();
                    cur[p] = ni.clone();
                    if let (Some(o), Some(c)) = (cur[p].as_object_mut(), created) {
                        o.insert("createdAt".into(), c);
                    }
                    any = true;
                }
            }
            None => {
                cur.push(ni.clone());
                any = true;
            }
        }
    }
    any
}

// ---------- PATCH /entities/{id}/attrs/ — Update (5.6.2) ----------

pub async fn update_attrs(
    State(st): State<AppState>,
    Path(id): Path<String>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    match update_attrs_inner(&st, &id, &params, &headers, &body).await {
        Ok(r) => r,
        Err(e) => e.into_response(),
    }
}

async fn update_attrs_inner(
    st: &AppState,
    id: &str,
    params: &HashMap<String, String>,
    headers: &HeaderMap,
    body: &[u8],
) -> ApiResult<Response> {
    let tenant = tenant_from(headers)?;
    antares_model::EntityId::new(id)?;
    check_params(params, &["options", "local", "type"])?;
    let parsed = parse_body(&st.loader, headers, body, BodyKind::MergePatch).await?;
    let obj = parsed
        .value
        .as_object()
        .ok_or_else(|| NgsiError::BadRequestData("fragment must be a JSON object".into()))?;
    let fragment = expand_entity(
        obj,
        &parsed.ctx,
        ExpandOpts {
            fragment: true,
            allow_null: true,
            temporal: false,
            ..Default::default()
        },
    )?;
    let (mut regs, local_covered) =
        attr_fed_plan(st, &tenant, id, &fragment, &parsed.ctx, params, headers);
    if let Some(r) = crate::federation::handle_via_loop(
        headers,
        &crate::federation::alias_for(&st.host_alias, &tenant),
        &tenant,
        &mut regs,
    ) {
        return Ok(r);
    }
    let all_attr_iris = attr_iris_of(&fragment);
    let fragment = crate::federation::strip_covered_expanded(&fragment, &regs);
    let local_iris = attr_iris_of(&fragment);
    let ts = now_iso();
    let mut updated = Vec::new();
    let mut not_updated = Vec::new();
    let local_resp: Option<ApiResult<Response>> = if local_covered {
        None
    } else {
        let res = st.store.mutate(&tenant, Kind::Entity, id, |doc| {
            let target = doc.as_object_mut().expect("entity object");
            let frag = fragment.as_object().expect("fragment object");
            // 5.6.2: appended/updated types extend the type set
            if let Some(new_types) = frag.get("type").and_then(Value::as_array) {
                let mut cur: Vec<Value> = target
                    .get("type")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                for t in new_types {
                    if !cur.contains(t) {
                        cur.push(t.clone());
                    }
                }
                target.insert("type".into(), Value::Array(cur));
                updated.push("type".into());
            }
            // 5.6.2: scope updates only when the entity already has one
            if let Some(new_scope) = frag.get("scope") {
                if target.contains_key("scope") {
                    target.insert("scope".into(), new_scope.clone());
                    updated.push("scope".into());
                } else {
                    not_updated.push(("scope".into(), "entity has no scope".into()));
                }
            }
            for (k, v) in frag {
                if matches!(
                    k.as_str(),
                    "id" | "type" | "scope" | "createdAt" | "modifiedAt"
                ) {
                    continue;
                }
                let mut incoming = v.clone();
                stamp_new_attr(&mut incoming, &ts);
                match target.get_mut(k) {
                    // 5.6.2 + 011_01_03: unknown attributes are appended silently
                    None => {
                        let live: Vec<Value> = incoming
                            .as_array()
                            .cloned()
                            .unwrap_or_default()
                            .into_iter()
                            .filter(|i| !antares_jsonld::is_deletion_instance(i))
                            .collect();
                        if !live.is_empty() {
                            target.insert(k.clone(), Value::Array(live));
                        }
                        updated.push(k.clone());
                    }
                    Some(existing) => {
                        merge_instance_sets(existing, &incoming, false);
                        if existing.as_array().is_some_and(Vec::is_empty) {
                            target.remove(k);
                        }
                        updated.push(k.clone());
                    }
                }
            }
            target.insert("modifiedAt".into(), Value::String(ts.clone()));
            Ok::<(), NgsiError>(())
        })?;
        Some(match res {
            None => Err(NgsiError::ResourceNotFound(format!("entity {id} not found")).into()),
            Some(Err(e)) => Err(e.into()),
            Some(Ok(())) => Ok(update_result(
                &tenant,
                updated.clone(),
                not_updated.clone(),
                &parsed.ctx,
            )),
        })
    };
    if regs.is_empty() {
        return local_resp.expect("local path always runs without registrations");
    }
    let local_outcome = classify_local(&local_resp);
    let mut query = Vec::new();
    if let Some(o) = params.get("options") {
        query.push(("options".to_string(), o.clone()));
    }
    let fed_parts = crate::federation::fed_attr_parts(
        st,
        headers,
        &tenant,
        &parsed.ctx.source,
        &regs,
        "updateEntity",
        reqwest::Method::PATCH,
        &format!("/entities/{id}/attrs/"),
        &query,
        Some(Value::Object(without_context_map(obj))),
    )
    .await;
    Ok(combine_attr_parts(
        &tenant,
        &all_attr_iris,
        &local_iris,
        local_outcome,
        updated,
        not_updated.into_iter().map(|(a, r)| (a, r, None)).collect(),
        &regs,
        &fed_parts,
    ))
}

// ---------- PATCH /entities/{id}/attrs/{attrId} — Partial update (5.6.4) ----------

pub async fn partial_update_attr(
    State(st): State<AppState>,
    Path((id, attr)): Path<(String, String)>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    match partial_update_inner(&st, &id, &attr, &params, &headers, &body).await {
        Ok(r) => r,
        Err(e) => e.into_response(),
    }
}

async fn partial_update_inner(
    st: &AppState,
    id: &str,
    attr: &str,
    params: &HashMap<String, String>,
    headers: &HeaderMap,
    body: &[u8],
) -> ApiResult<Response> {
    let tenant = tenant_from(headers)?;
    antares_model::EntityId::new(id)?;
    check_attr_name(attr)?;
    check_params(params, &["local", "type"])?;
    let parsed = parse_body(&st.loader, headers, body, BodyKind::MergePatch).await?;
    let obj = parsed
        .value
        .as_object()
        .ok_or_else(|| NgsiError::BadRequestData("fragment must be a JSON object".into()))?;
    let frag_inst = antares_jsonld::expand_attr_fragment(obj, &parsed.ctx)?;
    let attr_iri = parsed.ctx.expand_key(attr);
    let (mut regs, local_covered) = attr_fed_plan_iris(
        st,
        &tenant,
        id,
        vec![attr_iri.clone()],
        &parsed.ctx,
        params,
        headers,
    );
    if let Some(r) = crate::federation::handle_via_loop(
        headers,
        &crate::federation::alias_for(&st.host_alias, &tenant),
        &tenant,
        &mut regs,
    ) {
        return Ok(r);
    }
    let want_ds = frag_inst
        .get("datasetId")
        .and_then(Value::as_str)
        .map(String::from);
    let is_deletion = antares_jsonld::is_deletion_instance(&frag_inst);
    let ts = now_iso();
    let mut found = false;
    let local_resp: Option<ApiResult<Response>> = if local_covered {
        None
    } else {
        let res = st.store.mutate(&tenant, Kind::Entity, id, |doc| {
            let target = doc.as_object_mut().expect("entity object");
            if let Some(existing) = target.get_mut(&attr_iri).and_then(Value::as_array_mut) {
                let pos = existing.iter().position(|ci| {
                    ci.get("datasetId").and_then(Value::as_str) == want_ds.as_deref()
                });
                if let Some(p) = pos {
                    found = true;
                    if is_deletion {
                        existing.remove(p);
                    } else {
                        // 5.6.4.4: the fragment may not change the Attribute type
                        if let (Some(ft), Some(et)) = (
                            frag_inst.get("type").and_then(Value::as_str),
                            existing[p].get("type").and_then(Value::as_str),
                        ) {
                            if ft != et {
                                return Err(NgsiError::BadRequestData(format!(
                                    "attribute type mismatch: {ft} != {et} (5.6.4)"
                                )));
                            }
                        }
                        let t = existing[p].as_object_mut().expect("instance object");
                        for (k, v) in frag_inst.as_object().expect("fragment instance") {
                            if matches!(k.as_str(), "createdAt" | "modifiedAt") {
                                continue;
                            }
                            if v.is_null() || antares_jsonld::is_ngsi_null(v) {
                                t.remove(k);
                            } else {
                                t.insert(k.clone(), v.clone());
                            }
                        }
                        t.insert("modifiedAt".into(), Value::String(ts.clone()));
                    }
                }
                if existing.is_empty() {
                    target.remove(&attr_iri);
                }
            }
            if found {
                target.insert("modifiedAt".into(), Value::String(ts.clone()));
            }
            Ok::<(), NgsiError>(())
        })?;
        Some(match res {
            None => Err(NgsiError::ResourceNotFound(format!("entity {id} not found")).into()),
            Some(Err(e)) => Err(e.into()),
            Some(Ok(())) if found => Ok(no_content(&tenant)),
            Some(Ok(())) => {
                Err(NgsiError::ResourceNotFound(format!("attribute {attr} not found")).into())
            }
        })
    };
    if regs.is_empty() {
        return local_resp.expect("local path always runs without registrations");
    }
    let local_outcome = classify_local(&local_resp);
    let all_attr_iris = vec![attr_iri.clone()];
    let local_iris = if local_covered {
        Vec::new()
    } else {
        vec![attr_iri.clone()]
    };
    let updated = if matches!(local_outcome, LocalOutcome::Ok) {
        vec![attr_iri.clone()]
    } else {
        Vec::new()
    };
    let fed_parts = crate::federation::fed_attr_parts(
        st,
        headers,
        &tenant,
        &parsed.ctx.source,
        &regs,
        "updateAttrs",
        reqwest::Method::PATCH,
        &format!("/entities/{id}/attrs/{attr}"),
        &[],
        Some(Value::Object(without_context_map(obj))),
    )
    .await;
    Ok(combine_attr_parts(
        &tenant,
        &all_attr_iris,
        &local_iris,
        local_outcome,
        updated,
        Vec::new(),
        &regs,
        &fed_parts,
    ))
}

fn without_context_map(o: &Map<String, Value>) -> Map<String, Value> {
    let mut o = o.clone();
    o.remove("@context");
    o
}

// ---------- PUT /entities/{id}/attrs/{attrId} — Replace attribute (5.6.19) ----------

pub async fn replace_attr(
    State(st): State<AppState>,
    Path((id, attr)): Path<(String, String)>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        antares_model::EntityId::new(&id)?;
        check_attr_name(&attr)?;
        check_params(&params, &["local", "type"])?;
        let parsed = parse_body(&st.loader, &headers, &body, BodyKind::Standard).await?;
        let obj = parsed
            .value
            .as_object()
            .ok_or_else(|| NgsiError::BadRequestData("fragment must be a JSON object".into()))?;
        let mut wrapper = Map::new();
        wrapper.insert(attr.clone(), Value::Object(without_context_map(obj)));
        let fragment = expand_entity(
            &wrapper,
            &parsed.ctx,
            ExpandOpts {
                fragment: true,
                allow_null: false,
                temporal: false,
                ..Default::default()
            },
        )?;
        let attr_iri = parsed.ctx.expand_key(&attr);
        let incoming_arr = fragment
            .get(&attr_iri)
            .cloned()
            .ok_or_else(|| NgsiError::BadRequestData("invalid attribute fragment".into()))?;
        let new_inst = incoming_arr
            .as_array()
            .and_then(|a| a.first())
            .cloned()
            .ok_or_else(|| NgsiError::BadRequestData("invalid attribute fragment".into()))?;
        let want_ds = new_inst
            .get("datasetId")
            .and_then(Value::as_str)
            .map(String::from);
        let (mut regs, local_covered) = attr_fed_plan_iris(
            &st,
            &tenant,
            &id,
            vec![attr_iri.clone()],
            &parsed.ctx,
            &params,
            &headers,
        );
        if let Some(r) = crate::federation::handle_via_loop(
            &headers,
            &crate::federation::alias_for(&st.host_alias, &tenant),
            &tenant,
            &mut regs,
        ) {
            return Ok(r);
        }
        let ts = now_iso();
        let mut found = false;
        let local_resp: Option<ApiResult<Response>> = if local_covered {
            None
        } else {
            let res = st.store.mutate(&tenant, Kind::Entity, &id, |doc| {
                let target = doc.as_object_mut().expect("entity object");
                if let Some(existing) = target.get_mut(&attr_iri).and_then(Value::as_array_mut) {
                    // 5.6.19: only the instance with the matching datasetId is
                    // replaced; its createdAt survives (055_01/055_02)
                    if let Some(p) = existing.iter().position(|ci| {
                        ci.get("datasetId").and_then(Value::as_str) == want_ds.as_deref()
                    }) {
                        found = true;
                        let created = existing[p].get("createdAt").cloned();
                        let mut ni = new_inst.clone();
                        if let Some(o) = ni.as_object_mut() {
                            if let Some(c) = created {
                                o.insert("createdAt".into(), c);
                            } else {
                                o.insert("createdAt".into(), Value::String(ts.clone()));
                            }
                            o.insert("modifiedAt".into(), Value::String(ts.clone()));
                        }
                        existing[p] = ni;
                    }
                }
                if found {
                    target.insert("modifiedAt".into(), Value::String(ts.clone()));
                }
                Ok::<(), NgsiError>(())
            })?;
            Some(match res {
                None => Err(NgsiError::ResourceNotFound(format!("entity {id} not found")).into()),
                Some(Err(e)) => Err(ApiError::from(e)),
                Some(Ok(())) if found => Ok(no_content(&tenant)),
                Some(Ok(())) => {
                    Err(NgsiError::ResourceNotFound(format!("attribute {attr} not found")).into())
                }
            })
        };
        if regs.is_empty() {
            return local_resp.expect("local path always runs without registrations");
        }
        let local_outcome = classify_local(&local_resp);
        let all_attr_iris = vec![attr_iri.clone()];
        let local_iris = if local_covered {
            Vec::new()
        } else {
            vec![attr_iri.clone()]
        };
        let updated = if matches!(local_outcome, LocalOutcome::Ok) {
            vec![attr_iri.clone()]
        } else {
            Vec::new()
        };
        let fed_parts = crate::federation::fed_attr_parts(
            &st,
            &headers,
            &tenant,
            &parsed.ctx.source,
            &regs,
            "replaceAttrs",
            reqwest::Method::PUT,
            &format!("/entities/{id}/attrs/{attr}"),
            &[],
            Some(Value::Object(without_context_map(obj))),
        )
        .await;
        Ok(combine_attr_parts(
            &tenant,
            &all_attr_iris,
            &local_iris,
            local_outcome,
            updated,
            Vec::new(),
            &regs,
            &fed_parts,
        ))
    };
    go.await.unwrap_or_else(|e: ApiError| e.into_response())
}

// ---------- DELETE /entities/{id}/attrs/{attrId} (5.6.5) ----------

pub async fn delete_attr(
    State(st): State<AppState>,
    Path((id, attr)): Path<(String, String)>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
) -> Response {
    match delete_attr_inner(&st, &id, &attr, &params, &headers).await {
        Ok(r) => r,
        Err(e) => e.into_response(),
    }
}

async fn delete_attr_inner(
    st: &AppState,
    id: &str,
    attr: &str,
    params: &HashMap<String, String>,
    headers: &HeaderMap,
) -> ApiResult<Response> {
    let tenant = tenant_from(headers)?;
    antares_model::EntityId::new(id)?;
    check_attr_name(attr)?;
    check_params(params, &["datasetId", "deleteAll", "local", "type"])?;
    let ctx = request_context(&st.loader, headers).await?;
    let attr_iri = if attr == "scope" {
        "scope".to_owned()
    } else {
        ctx.expand_key(attr)
    };
    let delete_all = params.get("deleteAll").map(String::as_str) == Some("true");
    let want_ds = params.get("datasetId").cloned();
    let (mut regs, local_covered) = attr_fed_plan_iris(
        st,
        &tenant,
        id,
        vec![attr_iri.clone()],
        &ctx,
        params,
        headers,
    );
    if let Some(r) = crate::federation::handle_via_loop(
        headers,
        &crate::federation::alias_for(&st.host_alias, &tenant),
        &tenant,
        &mut regs,
    ) {
        return Ok(r);
    }
    let ts = now_iso();
    let mut found = false;
    let local_resp: Option<ApiResult<Response>> = if local_covered {
        None
    } else {
        let res = st.store.mutate(&tenant, Kind::Entity, id, |doc| {
            if attr_iri == "scope" {
                let target = doc.as_object_mut().expect("entity object");
                found = target.remove("scope").is_some();
                if found {
                    target.insert("modifiedAt".into(), Value::String(ts.clone()));
                }
                return Ok(());
            }
            let target = doc.as_object_mut().expect("entity object");
            if let Some(existing) = target.get_mut(&attr_iri).and_then(Value::as_array_mut) {
                if delete_all {
                    found = !existing.is_empty();
                    existing.clear();
                } else {
                    let pos = existing.iter().position(|ci| {
                        ci.get("datasetId").and_then(Value::as_str) == want_ds.as_deref()
                    });
                    if let Some(p) = pos {
                        existing.remove(p);
                        found = true;
                    }
                }
                if existing.is_empty() {
                    target.remove(&attr_iri);
                }
            }
            if found {
                target.insert("modifiedAt".into(), Value::String(ts.clone()));
            }
            Ok::<(), NgsiError>(())
        })?;
        // The temporal representation records the deletion (4.8 deletedAt) — and
        // the entity may exist ONLY temporally (created via 5.6.11).
        let temporal_had = crate::entities::mirror_delete_attr(
            st,
            &tenant,
            id,
            &attr_iri,
            want_ds.as_deref(),
            &ts,
        );
        Some(match res {
            None if temporal_had => Ok(no_content(&tenant)),
            None => Err(NgsiError::ResourceNotFound(format!("entity {id} not found")).into()),
            Some(Err(e)) => Err(e.into()),
            Some(Ok(())) if found || temporal_had => Ok(no_content(&tenant)),
            Some(Ok(())) => {
                Err(NgsiError::ResourceNotFound(format!("attribute {attr} not found")).into())
            }
        })
    };
    if regs.is_empty() {
        return local_resp.expect("local path always runs without registrations");
    }
    let local_outcome = classify_local(&local_resp);
    let all_attr_iris = vec![attr_iri.clone()];
    let local_iris = if local_covered {
        Vec::new()
    } else {
        vec![attr_iri.clone()]
    };
    let updated = if matches!(local_outcome, LocalOutcome::Ok) {
        vec![attr_iri.clone()]
    } else {
        Vec::new()
    };
    let query: Vec<(String, String)> = ["datasetId", "deleteAll"]
        .iter()
        .filter_map(|k| params.get(*k).map(|v| (k.to_string(), v.clone())))
        .collect();
    let fed_parts = crate::federation::fed_attr_parts(
        st,
        headers,
        &tenant,
        &ctx.source,
        &regs,
        "deleteAttrs",
        reqwest::Method::DELETE,
        &format!("/entities/{id}/attrs/{attr}"),
        &query,
        None,
    )
    .await;
    Ok(combine_attr_parts(
        &tenant,
        &all_attr_iris,
        &local_iris,
        local_outcome,
        updated,
        Vec::new(),
        &regs,
        &fed_parts,
    ))
}

#[cfg(test)]
mod update_result_tests {
    use super::*;
    use crate::federation::{FedReg, Part};
    use http_body_util::BodyExt;

    fn reg(reg_id: &str, attrs: Option<Vec<String>>) -> FedReg {
        FedReg {
            reg_id: reg_id.into(),
            endpoint: "http://peer:9090".into(),
            mode: "inclusive".into(),
            attrs,
            ..Default::default()
        }
    }

    async fn body_of(resp: Response) -> Value {
        let bytes = resp.into_body().collect().await.expect("body").to_bytes();
        serde_json::from_slice(&bytes).expect("json")
    }

    /// V-15: Tables 6.6.3.1-2 / 6.7.3.1-2 — the /attrs 207 body is an
    /// UpdateResult (updated: String[], notUpdated: NotUpdatedDetails[] with
    /// mandatory attributeName+reason, optional registrationId), never the
    /// batch {success, errors} shape.
    #[tokio::test]
    async fn distributed_attr_207_is_an_update_result() {
        let tenant = antares_model::TenantId::new("default").expect("tenant");
        let speed = "https://uri.etsi.org/ngsi-ld/default-context/speed".to_owned();
        let brand = "https://uri.etsi.org/ngsi-ld/default-context/brandName".to_owned();
        let all = vec![speed.clone(), brand.clone()];
        let regs = vec![reg(
            "urn:ngsi-ld:ContextSourceRegistration:csr1",
            Some(vec![brand.clone()]),
        )];
        let parts = vec![Part {
            status: 504,
            detail: "distributed operation timed out".into(),
        }];
        let resp = combine_attr_parts(
            &tenant,
            &all,
            std::slice::from_ref(&speed),
            LocalOutcome::Ok,
            vec![speed.clone()],
            Vec::new(),
            &regs,
            &parts,
        );
        assert_eq!(resp.status().as_u16(), 207);
        let body = body_of(resp).await;
        assert_eq!(body["updated"], serde_json::json!([speed]));
        assert_eq!(body["notUpdated"][0]["attributeName"], brand);
        assert_eq!(
            body["notUpdated"][0]["registrationId"],
            "urn:ngsi-ld:ContextSourceRegistration:csr1"
        );
        assert!(body["notUpdated"][0]["reason"].is_string());
        assert!(body.get("success").is_none(), "not the batch shape");
        assert!(body.get("errors").is_none(), "not the batch shape");
    }

    /// All halves succeeded → 204; everything missing → 404.
    #[tokio::test]
    async fn distributed_attr_success_and_not_found_edges() {
        let tenant = antares_model::TenantId::new("default").expect("tenant");
        let speed = "https://uri.etsi.org/ngsi-ld/default-context/speed".to_owned();
        let regs = vec![reg("urn:ngsi-ld:ContextSourceRegistration:csr1", None)];
        let ok_parts = vec![Part {
            status: 204,
            detail: "ok".into(),
        }];
        let resp = combine_attr_parts(
            &tenant,
            std::slice::from_ref(&speed),
            std::slice::from_ref(&speed),
            LocalOutcome::Ok,
            vec![speed.clone()],
            Vec::new(),
            &regs,
            &ok_parts,
        );
        assert_eq!(resp.status().as_u16(), 204);
        // entity unknown locally AND every forward failed → 404 ProblemDetails
        let bad_parts = vec![Part {
            status: 404,
            detail: "not found".into(),
        }];
        let resp = combine_attr_parts(
            &tenant,
            std::slice::from_ref(&speed),
            std::slice::from_ref(&speed),
            LocalOutcome::NotFound("entity urn:x not found".into()),
            Vec::new(),
            Vec::new(),
            &regs,
            &bad_parts,
        );
        assert_eq!(resp.status().as_u16(), 404);
    }
}
