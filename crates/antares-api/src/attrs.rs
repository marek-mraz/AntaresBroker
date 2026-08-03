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

type Params = Query<HashMap<String, String>>;

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
    let payload = serde_json::json!({
        "updated": updated.iter().map(|u| ctx.compact_iri(u)).collect::<Vec<_>>(),
        "notUpdated": not_updated
            .iter()
            .map(|(a, r)| serde_json::json!({
                "attributeName": ctx.compact_iri(a),
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
    Query(params): Params,
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
        },
    )?;
    let ts = now_iso();
    let mut updated = Vec::new();
    let mut not_updated = Vec::new();
    let res = st.store.mutate(&tenant, Kind::Entity, id, |doc| {
        let target = doc.as_object_mut().expect("entity object");
        for (k, v) in fragment.as_object().expect("fragment object") {
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
    match res {
        None => Err(NgsiError::ResourceNotFound(format!("entity {id} not found")).into()),
        Some(Err(e)) => Err(e.into()),
        Some(Ok(())) => {
            crate::entities::mirror_record(st, &tenant, &fragment);
            Ok(update_result(&tenant, updated, not_updated, &parsed.ctx))
        }
    }
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
    Query(params): Params,
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
            allow_null: false,
            temporal: false,
        },
    )?;
    let ts = now_iso();
    let mut updated = Vec::new();
    let mut not_updated = Vec::new();
    let res = st.store.mutate(&tenant, Kind::Entity, id, |doc| {
        let target = doc.as_object_mut().expect("entity object");
        for (k, v) in fragment.as_object().expect("fragment object") {
            if matches!(k.as_str(), "id" | "type" | "scope" | "createdAt" | "modifiedAt") {
                continue;
            }
            match target.get_mut(k) {
                None => not_updated.push((k.clone(), "attribute does not exist".into())),
                Some(existing) => {
                    let mut incoming = v.clone();
                    stamp_new_attr(&mut incoming, &ts);
                    merge_instance_sets(existing, &incoming, false);
                    updated.push(k.clone());
                }
            }
        }
        target.insert("modifiedAt".into(), Value::String(ts.clone()));
        Ok::<(), NgsiError>(())
    });
    match res {
        None => Err(NgsiError::ResourceNotFound(format!("entity {id} not found")).into()),
        Some(Err(e)) => Err(e.into()),
        Some(Ok(())) => {
            crate::entities::mirror_record(st, &tenant, &fragment);
            Ok(update_result(&tenant, updated, not_updated, &parsed.ctx))
        }
    }
}

// ---------- PATCH /entities/{id}/attrs/{attrId} — Partial update (5.6.4) ----------

pub async fn partial_update_attr(
    State(st): State<AppState>,
    Path((id, attr)): Path<(String, String)>,
    Query(params): Params,
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
    check_params(params, &["local", "type"])?;
    let parsed = parse_body(&st.loader, headers, body, BodyKind::MergePatch).await?;
    let obj = parsed
        .value
        .as_object()
        .ok_or_else(|| NgsiError::BadRequestData("fragment must be a JSON object".into()))?;
    // fragment for ONE attribute: wrap under the attr name then expand
    let mut wrapper = Map::new();
    wrapper.insert(attr.to_owned(), Value::Object(without_context_map(obj)));
    let fragment = expand_entity(
        &wrapper,
        &parsed.ctx,
        ExpandOpts {
            fragment: true,
            allow_null: true,
            temporal: false,
        },
    )?;
    let attr_iri = parsed.ctx.expand_key(attr);
    let frag_inst = fragment
        .get(&attr_iri)
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .cloned()
        .ok_or_else(|| NgsiError::BadRequestData("invalid attribute fragment".into()))?;
    let want_ds = frag_inst.get("datasetId").and_then(Value::as_str).map(String::from);
    let ts = now_iso();
    let mut found = false;
    let res = st.store.mutate(&tenant, Kind::Entity, id, |doc| {
        let target = doc.as_object_mut().expect("entity object");
        if let Some(existing) = target.get_mut(&attr_iri).and_then(Value::as_array_mut) {
            if let Some(inst) = existing.iter_mut().find(|ci| {
                ci.get("datasetId").and_then(Value::as_str) == want_ds.as_deref()
            }) {
                found = true;
                let t = inst.as_object_mut().expect("instance object");
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
        if found {
            target.insert("modifiedAt".into(), Value::String(ts.clone()));
        }
        Ok::<(), NgsiError>(())
    });
    match res {
        None => Err(NgsiError::ResourceNotFound(format!("entity {id} not found")).into()),
        Some(Err(e)) => Err(e.into()),
        Some(Ok(())) if found => Ok(no_content(&tenant)),
        Some(Ok(())) => {
            Err(NgsiError::ResourceNotFound(format!("attribute {attr} not found")).into())
        }
    }
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
    Query(params): Params,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
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
            },
        )?;
        let attr_iri = parsed.ctx.expand_key(&attr);
        let mut incoming = fragment.get(&attr_iri).cloned().ok_or_else(|| {
            NgsiError::BadRequestData("invalid attribute fragment".into())
        })?;
        let ts = now_iso();
        stamp_new_attr(&mut incoming, &ts);
        let mut found = false;
        let res = st.store.mutate(&tenant, Kind::Entity, &id, |doc| {
            let target = doc.as_object_mut().expect("entity object");
            if target.contains_key(&attr_iri) {
                found = true;
                target.insert(attr_iri.clone(), incoming.clone());
                target.insert("modifiedAt".into(), Value::String(ts.clone()));
            }
            Ok::<(), NgsiError>(())
        });
        match res {
            None => Err(NgsiError::ResourceNotFound(format!("entity {id} not found")).into()),
            Some(Err(e)) => Err(ApiError::from(e)),
            Some(Ok(())) if found => Ok(no_content(&tenant)),
            Some(Ok(())) => {
                Err(NgsiError::ResourceNotFound(format!("attribute {attr} not found")).into())
            }
        }
    };
    go.await.unwrap_or_else(|e: ApiError| e.into_response())
}

// ---------- DELETE /entities/{id}/attrs/{attrId} (5.6.5) ----------

pub async fn delete_attr(
    State(st): State<AppState>,
    Path((id, attr)): Path<(String, String)>,
    Query(params): Params,
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
    check_params(params, &["datasetId", "deleteAll", "local", "type"])?;
    let ctx = request_context(&st.loader, headers).await?;
    let attr_iri = if attr == "scope" {
        "scope".to_owned()
    } else {
        ctx.expand_key(attr)
    };
    let delete_all = params.get("deleteAll").map(String::as_str) == Some("true");
    let want_ds = params.get("datasetId").cloned();
    let ts = now_iso();
    let mut found = false;
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
    match res {
        None if temporal_had => Ok(no_content(&tenant)),
        None => Err(NgsiError::ResourceNotFound(format!("entity {id} not found")).into()),
        Some(Err(e)) => Err(e.into()),
        Some(Ok(())) if found || temporal_had => Ok(no_content(&tenant)),
        Some(Ok(())) => {
            Err(NgsiError::ResourceNotFound(format!("attribute {attr} not found")).into())
        }
    }
}
