//! /csourceRegistrations (5.9, 5.10; resources 6.8/6.9).

use crate::negotiate::*;
use crate::state::{now_iso, AppState};
use antares_jsonld::{parse_datetime, Context};
use antares_model::NgsiError;
use antares_sql::store::Kind;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{Map, Value};
use std::collections::HashMap;

type Params = Query<HashMap<String, String>>;

/// Validate + normalize a CSourceRegistration (5.2.9): types and attribute
/// names inside `information` expand to IRIs.
pub fn normalize_registration(
    doc: &Map<String, Value>,
    ctx: &Context,
    is_patch: bool,
) -> Result<Map<String, Value>, NgsiError> {
    let bad = |m: String| NgsiError::BadRequestData(m);
    let mut out = Map::new();
    for (k, v) in doc {
        match k.as_str() {
            "@context" | "createdAt" | "modifiedAt" | "status" => continue,
            "id" => {
                let id = v
                    .as_str()
                    .ok_or_else(|| bad("registration id must be a string URI".into()))?;
                antares_model::EntityId::new(id)?;
                out.insert("id".into(), v.clone());
            }
            "type" => {
                if v.as_str() != Some("ContextSourceRegistration") {
                    return Err(bad(
                        "type must be \"ContextSourceRegistration\" (5.2.9)".into(),
                    ));
                }
                out.insert("type".into(), v.clone());
            }
            "information" => {
                let arr = v
                    .as_array()
                    .filter(|a| !a.is_empty())
                    .ok_or_else(|| bad("information must be a non-empty array (5.2.9)".into()))?;
                let mut infos = Vec::new();
                for info in arr {
                    let io = info
                        .as_object()
                        .ok_or_else(|| bad("information entries must be objects".into()))?;
                    let mut ni = Map::new();
                    for (ik, iv) in io {
                        match ik.as_str() {
                            "entities" => {
                                let es = iv
                                    .as_array()
                                    .filter(|a| !a.is_empty())
                                    .ok_or_else(|| bad("entities must be a non-empty array".into()))?;
                                let mut nes = Vec::new();
                                for e in es {
                                    let eo = e
                                        .as_object()
                                        .ok_or_else(|| bad("entities entries must be objects".into()))?;
                                    let mut ne = Map::new();
                                    for (ek, ev) in eo {
                                        match ek.as_str() {
                                            "type" => {
                                                let t = ev.as_str().filter(|t| !t.is_empty()).ok_or_else(
                                                    || bad("EntityInfo type is required (5.2.8)".into()),
                                                )?;
                                                ne.insert(
                                                    "type".into(),
                                                    Value::String(ctx.expand_key(t)),
                                                );
                                            }
                                            "id" => {
                                                let id = ev.as_str().ok_or_else(|| {
                                                    bad("EntityInfo id must be a URI".into())
                                                })?;
                                                antares_model::EntityId::new(id)?;
                                                ne.insert("id".into(), ev.clone());
                                            }
                                            "idPattern" => {
                                                let p = ev.as_str().ok_or_else(|| {
                                                    bad("idPattern must be a string".into())
                                                })?;
                                                regex::Regex::new(p).map_err(|_| {
                                                    bad(format!("invalid idPattern {p:?}"))
                                                })?;
                                                ne.insert("idPattern".into(), ev.clone());
                                            }
                                            _ => {
                                                ne.insert(ek.clone(), ev.clone());
                                            }
                                        }
                                    }
                                    if !ne.contains_key("type") {
                                        return Err(bad(
                                            "EntityInfo requires type (5.2.8)".into(),
                                        ));
                                    }
                                    nes.push(Value::Object(ne));
                                }
                                ni.insert("entities".into(), Value::Array(nes));
                            }
                            "propertyNames" | "relationshipNames" => {
                                let names = iv
                                    .as_array()
                                    .ok_or_else(|| bad(format!("{ik} must be an array")))?;
                                let mut nn = Vec::new();
                                for n in names {
                                    let s = n
                                        .as_str()
                                        .ok_or_else(|| bad(format!("{ik} entries must be strings")))?;
                                    nn.push(Value::String(ctx.expand_key(s)));
                                }
                                ni.insert(ik.clone(), Value::Array(nn));
                            }
                            _ => {
                                ni.insert(ik.clone(), iv.clone());
                            }
                        }
                    }
                    infos.push(Value::Object(ni));
                }
                out.insert("information".into(), Value::Array(infos));
            }
            "endpoint" => {
                let uri = v
                    .as_str()
                    .ok_or_else(|| bad("endpoint must be a URI string".into()))?;
                antares_model::EntityId::new(uri)
                    .map_err(|_| bad(format!("endpoint is not a valid URI: {uri:?}")))?;
                out.insert("endpoint".into(), v.clone());
            }
            "expiresAt" => {
                let s = v
                    .as_str()
                    .filter(|s| parse_datetime(s))
                    .ok_or_else(|| bad("expiresAt must be an ISO 8601 DateTime".into()))?;
                if s < now_iso().as_str() {
                    return Err(bad("expiresAt is in the past".into()));
                }
                out.insert("expiresAt".into(), v.clone());
            }
            "observationInterval" | "managementInterval" => {
                let o = v
                    .as_object()
                    .ok_or_else(|| bad(format!("{k} must be a TimeInterval object")))?;
                let start = o
                    .get("startAt")
                    .and_then(Value::as_str)
                    .filter(|s| parse_datetime(s));
                if start.is_none() {
                    return Err(bad(format!("{k}.startAt must be an ISO 8601 DateTime")));
                }
                if let Some(e) = o.get("endAt") {
                    e.as_str()
                        .filter(|s| parse_datetime(s))
                        .ok_or_else(|| bad(format!("{k}.endAt must be an ISO 8601 DateTime")))?;
                }
                out.insert(k.clone(), v.clone());
            }
            _ => {
                // tolerant reader (§15.1)
                out.insert(k.clone(), v.clone());
            }
        }
    }
    if !is_patch {
        if !out.contains_key("type") {
            return Err(bad("type must be \"ContextSourceRegistration\" (5.2.9)".into()));
        }
        if !out.contains_key("endpoint") {
            return Err(bad("endpoint is required (5.2.9)".into()));
        }
        if !out.contains_key("information") {
            return Err(bad("information is required (5.2.9)".into()));
        }
    }
    Ok(out)
}

/// Output shaping: compact IRIs.
pub fn present_registration(doc: &Value, ctx: &Context, sys_attrs: bool) -> Value {
    let Some(obj) = doc.as_object() else {
        return doc.clone();
    };
    let mut out = Map::new();
    for (k, v) in obj {
        match k.as_str() {
            "createdAt" | "modifiedAt" if !sys_attrs => continue,
            "information" => {
                let infos: Vec<Value> = v
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .map(|info| {
                        let mut ni = info.as_object().cloned().unwrap_or_default();
                        if let Some(es) = ni.get("entities").and_then(Value::as_array) {
                            let nes: Vec<Value> = es
                                .iter()
                                .map(|e| {
                                    let mut ne = e.as_object().cloned().unwrap_or_default();
                                    if let Some(t) = ne.get("type").and_then(Value::as_str) {
                                        ne.insert(
                                            "type".into(),
                                            Value::String(ctx.compact_iri(t)),
                                        );
                                    }
                                    Value::Object(ne)
                                })
                                .collect();
                            ni.insert("entities".into(), Value::Array(nes));
                        }
                        for names_key in ["propertyNames", "relationshipNames"] {
                            if let Some(names) = ni.get(names_key).and_then(Value::as_array) {
                                let nn: Vec<Value> = names
                                    .iter()
                                    .filter_map(Value::as_str)
                                    .map(|n| Value::String(ctx.compact_iri(n)))
                                    .collect();
                                ni.insert(names_key.into(), Value::Array(nn));
                            }
                        }
                        Value::Object(ni)
                    })
                    .collect();
                out.insert("information".into(), Value::Array(infos));
            }
            _ => {
                out.insert(k.clone(), v.clone());
            }
        }
    }
    Value::Object(out)
}

// ---------- handlers ----------

pub async fn create_registration(
    State(st): State<AppState>,
    Query(params): Params,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        check_params(&params, &["local"])?;
        let parsed = parse_body(&st.loader, &headers, &body, BodyKind::Standard).await?;
        let obj = parsed
            .value
            .as_object()
            .ok_or_else(|| NgsiError::BadRequestData("registration must be a JSON object".into()))?;
        let mut norm = normalize_registration(obj, &parsed.ctx, false)?;
        let id = match norm.get("id").and_then(Value::as_str) {
            Some(id) => id.to_owned(),
            None => {
                let id = format!("urn:ngsi-ld:ContextSourceRegistration:{}", uuid::Uuid::new_v4());
                norm.insert("id".into(), Value::String(id.clone()));
                id
            }
        };
        let ts = now_iso();
        norm.insert("createdAt".into(), Value::String(ts.clone()));
        norm.insert("modifiedAt".into(), Value::String(ts));
        if !st.store.create(&tenant, Kind::Registration, &id, Value::Object(norm)) {
            return Err(NgsiError::AlreadyExists(format!("registration {id} already exists")).into());
        }
        Ok::<_, ApiError>(created(
            format!("/ngsi-ld/v1/csourceRegistrations/{id}"),
            &tenant,
        ))
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

pub async fn retrieve_registration(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Query(params): Params,
    headers: HeaderMap,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        check_params(&params, &["options", "format", "local"])?;
        let accept = parse_accept(&headers)?;
        let ctx = request_context(&st.loader, &headers).await?;
        let doc = st
            .store
            .get(&tenant, Kind::Registration, &id)
            .ok_or_else(|| NgsiError::ResourceNotFound(format!("registration {id} not found")))?;
        let sys = params
            .get("options")
            .is_some_and(|o| o.split(',').any(|s| s.trim() == "sysAttrs"));
        Ok::<_, ApiError>(respond(
            StatusCode::OK,
            present_registration(&doc, &ctx, sys),
            &ctx,
            accept,
            &tenant,
        ))
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

pub async fn query_registrations(
    State(st): State<AppState>,
    Query(params): Params,
    headers: HeaderMap,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        check_params(
            &params,
            &[
                "id", "idPattern", "type", "attrs", "q", "georel", "geometry", "coordinates",
                "geoproperty", "timeproperty", "timerel", "timeAt", "endTimeAt", "csf",
                "limit", "offset", "count", "options", "format", "local", "scopeQ",
            ],
        )?;
        let accept = parse_accept(&headers)?;
        let ctx = request_context(&st.loader, &headers).await?;
        let ids: Option<Vec<&str>> = params.get("id").map(|s| s.split(',').collect());
        let types: Option<Vec<String>> = params
            .get("type")
            .map(|s| s.split(',').map(|t| ctx.expand_key(t.trim())).collect());
        let attrs: Option<Vec<String>> = params
            .get("attrs")
            .map(|s| s.split(',').map(|t| ctx.expand_key(t.trim())).collect());
        let all = st.store.list(&tenant, Kind::Registration);
        let matches: Vec<Value> = all
            .into_iter()
            .filter(|doc| {
                if let Some(ids) = &ids {
                    if !ids.contains(&doc["id"].as_str().unwrap_or("")) {
                        return false;
                    }
                }
                if let Some(types) = &types {
                    if !registration_matches_types(doc, types) {
                        return false;
                    }
                }
                if let Some(attrs) = &attrs {
                    if !registration_matches_attrs(doc, attrs) {
                        return false;
                    }
                }
                true
            })
            .collect();
        let (page, count_hdr, links) = crate::entities::paginate(
            &st,
            &params,
            matches,
            "/ngsi-ld/v1/csourceRegistrations",
        )?;
        let sys = params
            .get("options")
            .is_some_and(|o| o.split(',').any(|s| s.trim() == "sysAttrs"));
        let payload: Vec<Value> = page
            .iter()
            .map(|d| present_registration(d, &ctx, sys))
            .collect();
        let mut resp = respond(StatusCode::OK, Value::Array(payload), &ctx, accept, &tenant);
        if let Some(total) = count_hdr {
            if let Ok(v) = total.to_string().parse() {
                resp.headers_mut().insert("NGSILD-Results-Count", v);
            }
        }
        for l in links {
            if let Ok(v) = l.parse() {
                resp.headers_mut().append(axum::http::header::LINK, v);
            }
        }
        Ok::<_, ApiError>(resp)
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

/// 5.12 matching: a registration matches a type when any information entry
/// either names it or has no entities restriction at all.
pub fn registration_matches_types(doc: &Value, types: &[String]) -> bool {
    let Some(infos) = doc.get("information").and_then(Value::as_array) else {
        return false;
    };
    infos.iter().any(|info| {
        match info.get("entities").and_then(Value::as_array) {
            None => true,
            Some(es) => es.iter().any(|e| {
                e.get("type")
                    .and_then(Value::as_str)
                    .is_some_and(|t| types.iter().any(|w| w == t))
            }),
        }
    })
}

pub fn registration_matches_attrs(doc: &Value, attrs: &[String]) -> bool {
    let Some(infos) = doc.get("information").and_then(Value::as_array) else {
        return false;
    };
    infos.iter().any(|info| {
        let props = info.get("propertyNames").and_then(Value::as_array);
        let rels = info.get("relationshipNames").and_then(Value::as_array);
        if props.is_none() && rels.is_none() {
            return true;
        }
        let has = |list: Option<&Vec<Value>>| {
            list.is_some_and(|l| {
                l.iter()
                    .filter_map(Value::as_str)
                    .any(|n| attrs.iter().any(|w| w == n))
            })
        };
        has(props) || has(rels)
    })
}

pub async fn update_registration(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Query(params): Params,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        check_params(&params, &["local"])?;
        let parsed = parse_body(&st.loader, &headers, &body, BodyKind::MergePatch).await?;
        let obj = parsed
            .value
            .as_object()
            .ok_or_else(|| NgsiError::BadRequestData("fragment must be a JSON object".into()))?;
        let norm = normalize_registration(obj, &parsed.ctx, true)?;
        let ts = now_iso();
        let res = st.store.mutate(&tenant, Kind::Registration, &id, |doc| {
            let target = doc.as_object_mut().expect("registration object");
            for (k, v) in &norm {
                if k == "id" {
                    continue;
                }
                if v.is_null() {
                    target.remove(k);
                } else {
                    target.insert(k.clone(), v.clone());
                }
            }
            target.insert("modifiedAt".into(), Value::String(ts.clone()));
            Ok::<(), NgsiError>(())
        });
        match res {
            None => Err(NgsiError::ResourceNotFound(format!("registration {id} not found")).into()),
            Some(Err(e)) => Err(ApiError::from(e)),
            Some(Ok(())) => Ok(no_content(&tenant)),
        }
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

pub async fn delete_registration(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Query(params): Params,
    headers: HeaderMap,
) -> Response {
    let go = || -> ApiResult<Response> {
        let tenant = tenant_from(&headers)?;
        check_params(&params, &["local"])?;
        if st.store.delete(&tenant, Kind::Registration, &id) {
            Ok(no_content(&tenant))
        } else {
            Err(NgsiError::ResourceNotFound(format!("registration {id} not found")).into())
        }
    };
    go().unwrap_or_else(|e| e.into_response())
}
