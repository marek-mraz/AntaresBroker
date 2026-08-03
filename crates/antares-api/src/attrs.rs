//! Attribute-level operations (5.6.2–5.6.5, 5.6.19; resources 6.6/6.7).

use crate::entities::stamp_new;
use crate::negotiate::*;
use crate::state::{now_iso, AppState};
use antares_jsonld::{expand_entity, ExpandOpts};
use antares_model::NgsiError;
use antares_sql::store::Kind;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use serde_json::{Map, Value};
use std::collections::HashMap;

use crate::negotiate::CleanParams;

/// Attribute names in paths must be valid terms/IRIs (4.6.2) — 400 otherwise.
pub(crate) fn check_attr_name(attr: &str) -> Result<(), NgsiError> {
    // 4.6.2 supported names: no '@' (keyword territory), no parens/quotes/etc.
    let ok = !attr.is_empty()
        && attr.chars().all(|c| {
            c.is_ascii_alphanumeric() || "_:.#/%-+".contains(c)
        });
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
    let (regs, local_covered) = attr_fed_plan(st, &tenant, id, &fragment, &parsed.ctx, params);
    if !regs.is_empty() && crate::federation::via_loop(headers, &st.host_alias) {
        return Ok(crate::federation::loop_508(&tenant));
    }
    let fragment = crate::federation::strip_covered_expanded(&fragment, &regs);
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
            if matches!(k.as_str(), "id" | "type" | "scope" | "createdAt" | "modifiedAt") {
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
                        not_updated.push((k.clone(), "attribute already exists (noOverwrite)".into()));
                    }
                }
            }
        }
        target.insert("modifiedAt".into(), Value::String(ts.clone()));
        Ok::<(), NgsiError>(())
    });
    Some(match res {
        None => Err(NgsiError::ResourceNotFound(format!("entity {id} not found")).into()),
        Some(Err(e)) => Err(e.into()),
        Some(Ok(())) => {
            crate::entities::mirror_record(st, &tenant, &fragment);
            Ok(update_result(&tenant, updated, not_updated, &parsed.ctx))
        }
    })
    };
    if regs.is_empty() {
        return local_resp.expect("local path always runs without registrations");
    }
    let mut parts = Vec::new();
    if let Some(r) = local_resp {
        parts.push(part_of(r));
    }
    let mut query = Vec::new();
    if let Some(o) = params.get("options") {
        query.push(("options".to_string(), o.clone()));
    }
    parts.extend(
        crate::federation::fed_attr_parts(
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
        .await,
    );
    Ok(crate::federation::combine(parts, no_content(&tenant), &tenant))
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
) -> (Vec<crate::federation::FedReg>, bool) {
    let attr_iris: Vec<String> = fragment
        .as_object()
        .map(|o| {
            o.keys()
                .filter(|k| {
                    !matches!(k.as_str(), "id" | "type" | "scope" | "createdAt" | "modifiedAt")
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    attr_fed_plan_iris(st, tenant, id, attr_iris, ctx, params)
}

fn attr_fed_plan_iris(
    st: &AppState,
    tenant: &antares_model::TenantId,
    id: &str,
    attr_iris: Vec<String>,
    ctx: &antares_jsonld::Context,
    params: &HashMap<String, String>,
) -> (Vec<crate::federation::FedReg>, bool) {
    let spec = crate::csource::CsrSpec {
        ids: Some(vec![id.to_owned()]),
        attrs: (!attr_iris.is_empty()).then(|| attr_iris.clone()),
        ..Default::default()
    };
    let regs = crate::federation::write_regs(st, tenant, &spec, ctx, params);
    let covered = !regs.is_empty()
        && !attr_iris.is_empty()
        && attr_iris.iter().all(|a| {
            regs.iter().any(|r| r.is_proxy() && r.covers_attr(a))
        });
    (regs, covered)
}

fn part_of(resp: ApiResult<Response>) -> crate::federation::Part {
    let status = match resp {
        Ok(r) => r.status().as_u16(),
        Err(e) => e.into_response().status().as_u16(),
    };
    crate::federation::Part { status, detail: "local operation".into() }
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
    let (regs, local_covered) = attr_fed_plan(st, &tenant, id, &fragment, &parsed.ctx, params);
    if !regs.is_empty() && crate::federation::via_loop(headers, &st.host_alias) {
        return Ok(crate::federation::loop_508(&tenant));
    }
    let fragment = crate::federation::strip_covered_expanded(&fragment, &regs);
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
            if matches!(k.as_str(), "id" | "type" | "scope" | "createdAt" | "modifiedAt") {
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
    });
    Some(match res {
        None => Err(NgsiError::ResourceNotFound(format!("entity {id} not found")).into()),
        Some(Err(e)) => Err(e.into()),
        Some(Ok(())) => {
            crate::entities::mirror_record(st, &tenant, &fragment);
            Ok(update_result(&tenant, updated, not_updated, &parsed.ctx))
        }
    })
    };
    if regs.is_empty() {
        return local_resp.expect("local path always runs without registrations");
    }
    let mut parts = Vec::new();
    if let Some(r) = local_resp {
        parts.push(part_of(r));
    }
    let mut query = Vec::new();
    if let Some(o) = params.get("options") {
        query.push(("options".to_string(), o.clone()));
    }
    parts.extend(
        crate::federation::fed_attr_parts(
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
        .await,
    );
    Ok(crate::federation::combine(parts, no_content(&tenant), &tenant))
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
    let (regs, local_covered) =
        attr_fed_plan_iris(st, &tenant, id, vec![attr_iri.clone()], &parsed.ctx, params);
    if !regs.is_empty() && crate::federation::via_loop(headers, &st.host_alias) {
        return Ok(crate::federation::loop_508(&tenant));
    }
    let want_ds = frag_inst.get("datasetId").and_then(Value::as_str).map(String::from);
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
    });
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
    let mut parts = Vec::new();
    if let Some(r) = local_resp {
        parts.push(part_of(r));
    }
    parts.extend(
        crate::federation::fed_attr_parts(
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
        .await,
    );
    Ok(crate::federation::combine(parts, no_content(&tenant), &tenant))
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
        let incoming_arr = fragment.get(&attr_iri).cloned().ok_or_else(|| {
            NgsiError::BadRequestData("invalid attribute fragment".into())
        })?;
        let new_inst = incoming_arr
            .as_array()
            .and_then(|a| a.first())
            .cloned()
            .ok_or_else(|| NgsiError::BadRequestData("invalid attribute fragment".into()))?;
        let want_ds = new_inst.get("datasetId").and_then(Value::as_str).map(String::from);
        let (regs, local_covered) =
            attr_fed_plan_iris(&st, &tenant, &id, vec![attr_iri.clone()], &parsed.ctx, &params);
        if !regs.is_empty() && crate::federation::via_loop(&headers, &st.host_alias) {
            return Ok(crate::federation::loop_508(&tenant));
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
        });
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
        let mut parts = Vec::new();
        if let Some(r) = local_resp {
            parts.push(part_of(r));
        }
        parts.extend(
            crate::federation::fed_attr_parts(
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
            .await,
        );
        Ok(crate::federation::combine(parts, no_content(&tenant), &tenant))
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
    let (regs, local_covered) =
        attr_fed_plan_iris(st, &tenant, id, vec![attr_iri.clone()], &ctx, params);
    if !regs.is_empty() && crate::federation::via_loop(headers, &st.host_alias) {
        return Ok(crate::federation::loop_508(&tenant));
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
    });
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
    let mut parts = Vec::new();
    if let Some(r) = local_resp {
        parts.push(part_of(r));
    }
    let query: Vec<(String, String)> = ["datasetId", "deleteAll"]
        .iter()
        .filter_map(|k| params.get(*k).map(|v| (k.to_string(), v.clone())))
        .collect();
    parts.extend(
        crate::federation::fed_attr_parts(
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
        .await,
    );
    Ok(crate::federation::combine(parts, no_content(&tenant), &tenant))
}
