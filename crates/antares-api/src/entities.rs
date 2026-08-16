//! /entities resource (CIM 009 6.4–6.7; operations 5.6.1–5.6.6, 5.6.17,
//! 5.6.18, 5.6.19, 5.6.21, 5.7.1, 5.7.2).

use crate::negotiate::*;
use crate::qeval::eval_q;
use crate::repr::{apply, parse_repr};
use crate::state::{now_iso, AppState};
use antares_jsonld::{
    compact_entity, compact_entity_shallow, expand_entity, is_ngsi_null, ExpandOpts,
};
use antares_model::{NgsiError, TenantId};
use antares_ql::parse_q;
use antares_sql::store::Kind;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{Map, Value};
use std::collections::HashMap;

use crate::negotiate::CleanParams;

fn is_meta(k: &str) -> bool {
    matches!(
        k,
        "id" | "type" | "scope" | "createdAt" | "modifiedAt" | "deletedAt" | "expiresAt"
    )
}

/// Entity Type Selection Language (4.17) match against expanded type IRIs:
/// `,`/`|` = OR of alternatives, `(a;b)` = AND within one alternative.
pub(crate) fn type_selection_matches(
    sel: &str,
    types: &[&str],
    ctx: &antares_jsonld::Context,
) -> bool {
    sel.split([',', '|']).any(|alt| {
        alt.trim()
            .trim_start_matches('(')
            .trim_end_matches(')')
            .split(';')
            .all(|t| types.contains(&ctx.expand_key(t.trim()).as_str()))
    })
}

/// Inject server-managed timestamps into a freshly expanded doc.
pub fn stamp_new(doc: &mut Value, ts: &str) {
    if let Some(obj) = doc.as_object_mut() {
        obj.insert("createdAt".into(), Value::String(ts.to_owned()));
        obj.insert("modifiedAt".into(), Value::String(ts.to_owned()));
        for (k, v) in obj.iter_mut() {
            if !is_meta(k) {
                stamp_instances(v, ts, true);
            }
        }
    }
}

fn stamp_instances(v: &mut Value, ts: &str, created: bool) {
    if let Some(arr) = v.as_array_mut() {
        for inst in arr {
            if let Some(o) = inst.as_object_mut() {
                if created {
                    o.insert("createdAt".into(), Value::String(ts.to_owned()));
                }
                o.entry("createdAt".to_owned())
                    .or_insert_with(|| Value::String(ts.to_owned()));
                o.insert("modifiedAt".into(), Value::String(ts.to_owned()));
                let subs: Vec<String> = o
                    .keys()
                    .filter(|k| crate::repr_reserved(k))
                    .cloned()
                    .collect();
                let _ = subs;
                for (k, sub) in o.iter_mut() {
                    if sub.is_array() && !crate::repr_reserved(k) {
                        stamp_instances(sub, ts, created);
                    }
                }
            }
        }
    }
}

/// Compaction for a shaped doc under a representation: keyValues docs get
/// shallow key renaming only (values are already plain JSON).
pub fn compact_for(
    repr: &crate::repr::Repr,
    shaped: &Value,
    ctx: &antares_jsonld::Context,
) -> Value {
    if repr.key_values {
        compact_entity_shallow(shaped, ctx)
    } else {
        compact_entity(shaped, ctx)
    }
}

// ---------- temporal mirroring (auto-recording; Scorpio ENTITY-topic parity) ----------
//
// Append-side auto-recording (create/update/partial/merge/replace/batch) is
// driven centrally off the store's change hook — see
// `notify::record_temporal_change`. Only the DELETION mirrors below stay as
// explicit handler calls (their typed-null deletion shape is not derivable
// from a plain before/after append).

/// delete_temporal_on_core_delete: entity deletion removes its temporal
/// representation too (suite configuration parity). Skipped on bus=nats
/// api pods — the recorder applies the entityDeleted fence instead.
pub fn mirror_delete_entity(st: &AppState, tenant: &TenantId, id: &str) {
    if !st.record_locally {
        return;
    }
    if let Err(e) = st.store.delete(tenant, Kind::Temporal, id) {
        tracing::warn!("temporal mirror delete failed: {e}");
    }
}

/// 4.5.7/4.5.8: "In case the Property is deleted, an instance of the
/// Property is recorded with its value set to the URI "urn:ngsi-ld:null"
/// and the deletedAt Temporal Property set" (object for a Relationship;
/// typed null shapes for the LanguageProperty/JsonProperty/Vocab/List
/// subtypes). Each recorded instance carries an instanceId — the clause
/// SHOULD that makes 5.6.14/5.6.15 selective modification possible.
pub fn mirror_delete_attr(
    st: &AppState,
    tenant: &TenantId,
    id: &str,
    attr_iri: &str,
    dataset_id: Option<&str>,
    ts: &str,
) -> bool {
    let mut had = false;
    let r = st.store.mutate(tenant, Kind::Temporal, id, |doc| {
        let target = doc.as_object_mut().expect("temporal doc");
        if attr_iri == "scope" {
            // scope deletion: temporal scope becomes an instance array with
            // value [] (the 020_19/020_20 shape)
            had = true;
            let inst = serde_json::json!({
                "type": "Property",
                "value": [],
                "instanceId": format!("urn:ngsi-ld:Instance:{}", uuid::Uuid::new_v4()),
                "deletedAt": ts,
            });
            match target.get_mut("scope").and_then(Value::as_array_mut) {
                Some(arr) if arr.first().is_some_and(|i| i.is_object()) => arr.push(inst),
                _ => {
                    target.insert("scope".into(), Value::Array(vec![inst]));
                }
            }
            return Ok::<(), std::convert::Infallible>(());
        }
        if let Some(arr) = target.get_mut(attr_iri).and_then(Value::as_array_mut) {
            if arr.is_empty() {
                return Ok(());
            }
            had = true;
            let atype = arr
                .first()
                .and_then(|i| i.get("type"))
                .and_then(Value::as_str)
                .unwrap_or("Property")
                .to_owned();
            let mut inst = Map::new();
            inst.insert("type".into(), Value::String(atype.clone()));
            let null = Value::String("urn:ngsi-ld:null".into());
            match atype.as_str() {
                "Relationship" => {
                    inst.insert("object".into(), null);
                }
                "LanguageProperty" => {
                    inst.insert(
                        "languageMap".into(),
                        serde_json::json!({"@none": "urn:ngsi-ld:null"}),
                    );
                }
                "JsonProperty" => {
                    inst.insert("json".into(), null);
                }
                "VocabProperty" => {
                    inst.insert("vocab".into(), null);
                }
                "ListProperty" => {
                    inst.insert("valueList".into(), null);
                }
                "ListRelationship" => {
                    inst.insert("objectList".into(), null);
                }
                _ => {
                    inst.insert("value".into(), null);
                }
            }
            if let Some(ds) = dataset_id {
                inst.insert("datasetId".into(), Value::String(ds.to_owned()));
            }
            inst.insert(
                "instanceId".into(),
                Value::String(format!("urn:ngsi-ld:Instance:{}", uuid::Uuid::new_v4())),
            );
            inst.insert("deletedAt".into(), Value::String(ts.to_owned()));
            arr.push(Value::Object(inst));
        }
        Ok(())
    });
    if let Err(e) = r {
        tracing::warn!("temporal attr mirror failed: {e}");
    }
    had
}

// ---------- POST /entities/ (5.6.1) ----------

pub async fn create_entity(
    State(st): State<AppState>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    match create_entity_inner(&st, &params, &headers, &body).await {
        Ok(r) => r,
        Err(e) => e.into_response(),
    }
}

async fn create_entity_inner(
    st: &AppState,
    params: &HashMap<String, String>,
    headers: &HeaderMap,
    body: &[u8],
) -> ApiResult<Response> {
    let tenant = tenant_from(headers)?;
    check_params(params, &["local"])?;
    let parsed = parse_body(&st.loader, headers, body, BodyKind::Standard).await?;
    let obj = parsed
        .value
        .as_object()
        .ok_or_else(|| NgsiError::InvalidRequest("entity document must be a JSON object".into()))?;
    let mut expanded = expand_entity(obj, &parsed.ctx, ExpandOpts::default())?;
    let id = expanded["id"].as_str().expect("validated id").to_owned();

    // distributed create (4.3.6, 6.4.3.1)
    let types: Option<Vec<String>> = expanded["type"].as_array().map(|a| {
        a.iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect()
    });
    let attr_iris: Vec<String> = expanded
        .as_object()
        .map(|o| o.keys().filter(|k| !is_meta(k)).cloned().collect())
        .unwrap_or_default();
    let spec = crate::csource::CsrSpec {
        ids: Some(vec![id.clone()]),
        types,
        attrs: (!attr_iris.is_empty()).then_some(attr_iris),
        ..Default::default()
    };
    let mut regs = crate::federation::write_regs(st, &tenant, &spec, &parsed.ctx, params, headers);
    if let Some(r) = crate::federation::handle_via_loop(
        headers,
        &crate::federation::alias_for(&st.host_alias, &tenant),
        &tenant,
        &mut regs,
    ) {
        return Ok(r);
    }
    if !regs.is_empty() {
        let mut conflicts = Vec::new();
        let mut fwd = Vec::new();
        for reg in &regs {
            // 5.6.1.4: exclusive/redirect registrations not supporting the
            // Create Entity operation yield an error of type Conflict (and
            // are never contacted); an inclusive one is simply not forwarded.
            if !reg.supports("createEntity") {
                if reg.is_proxy() {
                    conflicts.push(crate::federation::conflict_part("createEntity"));
                }
                continue;
            }
            if let Some(frag) = crate::federation::reduce_to_scope(obj, reg, &parsed.ctx) {
                fwd.push((reg.clone(), frag));
            }
        }
        let proxies: Vec<&crate::federation::FedReg> =
            regs.iter().filter(|r| r.is_proxy()).collect();
        let (rest, has_attrs) = crate::federation::strip_proxied(obj, &proxies, &parsed.ctx);
        let mut parts = Vec::new();
        // local part only when something non-proxied remains (4.3.6.3)
        if has_attrs || proxies.is_empty() {
            let mut local_exp = expand_entity(&rest, &parsed.ctx, ExpandOpts::default())?;
            stamp_new(&mut local_exp, &now_iso());
            if st
                .store
                .create(&tenant, Kind::Entity, &id, local_exp.clone())?
            {
                parts.push(crate::federation::Part {
                    status: 201,
                    detail: "created locally".into(),
                });
            } else {
                parts.push(crate::federation::Part {
                    status: 409,
                    detail: format!("entity {id} already exists"),
                });
            }
        }
        parts.extend(conflicts);
        let ctx_url = crate::federation::ctx_link_url(headers, &parsed.ctx.source);
        for (reg, frag) in fwd {
            parts.push(
                crate::federation::forward_part(
                    st,
                    reqwest::Method::POST,
                    format!("{}/ngsi-ld/v1/entities", reg.endpoint),
                    &[],
                    headers,
                    &tenant,
                    &reg,
                    &ctx_url,
                    Some(frag),
                )
                .await,
            );
        }
        return Ok(crate::federation::combine(
            parts,
            created(format!("/ngsi-ld/v1/entities/{id}"), &tenant),
            &tenant,
        ));
    }

    stamp_new(&mut expanded, &now_iso());
    if !st
        .store
        .create(&tenant, Kind::Entity, &id, expanded.clone())?
    {
        return Err(NgsiError::AlreadyExists(format!("entity {id} already exists")).into());
    }
    Ok(created(format!("/ngsi-ld/v1/entities/{id}"), &tenant))
}

// ---------- GET /entities/{id} (5.7.1) ----------

pub async fn retrieve_entity(
    State(st): State<AppState>,
    Path(id): Path<String>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
) -> Response {
    match retrieve_entity_outer(&st, &id, params, &headers).await {
        Ok(r) => r,
        Err(e) => e.into_response(),
    }
}

/// 5.7.1.4 EntityMap usage on the retrieve: a supplied NGSILD-EntityMap
/// location is retrieved and, if live, is the only source used to determine
/// which registrations match; an unknown/expired reference — or the
/// entityMap=true flag — creates a new map, whose location is returned in
/// the NGSILD-EntityMap response header.
async fn retrieve_entity_outer(
    st: &AppState,
    id: &str,
    params: HashMap<String, String>,
    headers: &HeaderMap,
) -> ApiResult<Response> {
    let tenant = tenant_from(headers)?;
    let map_ref = headers
        .get("NGSILD-EntityMap")
        .and_then(|v| v.to_str().ok())
        .map(|r| r.rsplit('/').next().unwrap_or(r).to_owned());
    if let Some(map) = map_ref
        .as_deref()
        .and_then(|mid| crate::entity_maps::map_get(st, &tenant, mid))
    {
        let mut resp = retrieve_entity_inner(st, id, &params, headers, Some(&map)).await?;
        let mid = map_ref.unwrap_or_default();
        if let Ok(v) = format!("/ngsi-ld/v1/entityMaps/{mid}").parse() {
            resp.headers_mut().insert("NGSILD-EntityMap", v);
        }
        return Ok(resp);
    }
    let want_map = map_ref.is_some() || params.get("entityMap").map(String::as_str) == Some("true");
    let mut resp = retrieve_entity_inner(st, id, &params, headers, None).await?;
    if want_map && resp.status().is_success() {
        let ctx = request_context(&st.loader, headers).await?;
        let local_held = st.store.get(&tenant, Kind::Entity, id)?.is_some();
        let map = crate::entity_maps::build_retrieve_map(
            st, &tenant, &ctx, headers, id, &params, false, local_held,
        )?;
        if let Some(mid) = map.get("id").and_then(Value::as_str) {
            if let Ok(v) = format!("/ngsi-ld/v1/entityMaps/{mid}").parse() {
                resp.headers_mut().insert("NGSILD-EntityMap", v);
            }
        }
    }
    Ok(resp)
}

async fn retrieve_entity_inner(
    st: &AppState,
    id: &str,
    params: &HashMap<String, String>,
    headers: &HeaderMap,
    map: Option<&Value>,
) -> ApiResult<Response> {
    let tenant = tenant_from(headers)?;
    check_params(
        params,
        &[
            "attrs",
            "pick",
            "omit",
            "options",
            "format",
            "lang",
            "type",
            "geometryProperty",
            "datasetId",
            "containedBy",
            "join",
            "joinLevel",
            "local",
            "entityMap",
        ],
    )?;
    let accept = parse_accept_geo(headers)?;
    // 5.7.1.4: geometryProperty is only meaningful for the GeoJSON
    // representation — any other Accept is BadRequestData
    if params.contains_key("geometryProperty") && accept != Accept::GeoJson {
        return Err(NgsiError::BadRequestData(
            "geometryProperty requires Accept: application/geo+json (5.7.1.4)".into(),
        )
        .into());
    }
    let ctx = request_context(&st.loader, headers).await?;
    let repr = parse_repr(params, &ctx)?;
    let join = parse_join(params)?;
    check_linked_projection(&repr, &join)?;
    antares_model::EntityId::new(id)?;
    let local_doc = st.store.get(&tenant, Kind::Entity, id)?;
    let looped = crate::federation::via_loop(
        headers,
        &crate::federation::alias_for(&st.host_alias, &tenant),
    );
    let fed_on = crate::federation::active(params) && !looped;
    // 6.3.17: abnormal distributed-GET outcomes surface as NGSILD-Warning
    let mut warnings: Vec<String> = Vec::new();
    if crate::federation::active(params) && looped {
        let spec = crate::csource::CsrSpec {
            ids: Some(vec![id.to_owned()]),
            ..Default::default()
        };
        // only a loop that suppressed a real forward is abnormal behaviour
        if !crate::federation::matching_regs(st, &tenant, &spec, &ctx, headers).is_empty() {
            warnings.push(crate::federation::warning(
                199,
                &crate::federation::alias_for(&st.host_alias, &tenant),
                "a registration loop has been detected",
            ));
        }
    }
    let doc = if fed_on {
        let fed = crate::federation::fed_retrieve(
            st,
            &tenant,
            headers,
            &ctx,
            id,
            map,
            None,
            &mut warnings,
        )
        .await;
        match local_doc {
            Some(mut base) => {
                for aux_pass in [false, true] {
                    for (aux, d) in &fed {
                        if *aux == aux_pass {
                            crate::federation::merge_docs(&mut base, d, *aux);
                        }
                    }
                }
                base
            }
            None => {
                let first = fed
                    .iter()
                    .find(|(aux, _)| !aux)
                    .map(|(_, d)| d.clone())
                    .or_else(|| fed.first().map(|(_, d)| d.clone()));
                let Some(mut base) = first else {
                    // 6.3.17: abnormal distributed outcomes surface as
                    // NGSILD-Warning even when the retrieve ends 404
                    let mut resp = ApiError::from(NgsiError::ResourceNotFound(format!(
                        "entity {id} not found"
                    )))
                    .into_response();
                    attach_warnings(&mut resp, &warnings);
                    echo_tenant(&tenant, &mut resp);
                    return Ok(resp);
                };
                for aux_pass in [false, true] {
                    for (aux, d) in &fed {
                        if *aux == aux_pass {
                            crate::federation::merge_docs(&mut base, d, *aux);
                        }
                    }
                }
                base
            }
        }
    } else {
        local_doc.ok_or_else(|| NgsiError::ResourceNotFound(format!("entity {id} not found")))?
    };
    // 5.7.1.4: no entity "whose id (URI), and where specified type, is
    // equivalent" — the optional ?type selector (4.17) narrows the target
    if !crate::attrs::matches_type_param(&doc, params, &ctx) {
        return Err(NgsiError::ResourceNotFound(format!(
            "entity {id} does not match the type selector"
        ))
        .into());
    }
    // 5.7.1: attrs projection with no matching attribute ⇒ 404
    if let Some(want) = &repr.attrs {
        if !want.iter().any(|a| doc.get(a).is_some()) {
            return Err(NgsiError::ResourceNotFound(format!(
                "entity {id} has none of the requested attributes"
            ))
            .into());
        }
    }
    let shaped = apply(&doc, &repr);
    if (repr.pick.is_some() || repr.omit.is_some())
        && shaped.as_object().is_some_and(|o| o.is_empty())
    {
        return Err(NgsiError::ResourceNotFound(format!(
            "projection matches nothing on entity {id}"
        ))
        .into());
    }
    let mut payload = compact_for(&repr, &shaped, &ctx);
    if let Some((mode, level)) = &join {
        match mode.as_str() {
            "inline" => {
                inline_join(st, &tenant, &ctx, &repr, &mut payload, *level);
            }
            "flat" => {
                let mut linked = std::collections::BTreeMap::new();
                collect_flat(st, &tenant, &repr, &doc, *level, &mut linked);
                if !linked.is_empty() {
                    let mut arr = vec![payload];
                    for (_, (ldoc, lrepr)) in linked {
                        arr.push(compact_for(&lrepr, &apply(&ldoc, &lrepr), &ctx));
                    }
                    payload = Value::Array(arr);
                }
            }
            _ => {}
        }
    }
    let payload = if accept == Accept::GeoJson {
        to_geojson_feature(payload, params.get("geometryProperty"), &ctx)
    } else {
        payload
    };
    let mut resp = respond_prefer(StatusCode::OK, payload, &ctx, accept, &tenant, headers);
    attach_warnings(&mut resp, &warnings);
    Ok(resp)
}

/// 6.3.17: one `NGSILD-Warning` header per abnormal distributed-GET outcome —
/// scoped by the clause to /entities and /entities/{id}.
pub fn attach_warnings(resp: &mut Response, warnings: &[String]) {
    for w in warnings {
        if let Ok(v) = axum::http::HeaderValue::from_str(w) {
            resp.headers_mut().append("NGSILD-Warning", v);
        }
    }
}

/// 5.7.1.4 / 5.7.2.4: a `{…}` projection selects into Linked Entities —
/// it must be requested via join, and may not select deeper than joinLevel.
fn check_linked_projection(
    repr: &crate::repr::Repr,
    join: &Option<(String, usize)>,
) -> ApiResult<()> {
    let depth = repr
        .pick
        .as_deref()
        .map(crate::repr::proj_depth)
        .unwrap_or(0)
        .max(
            repr.omit
                .as_deref()
                .map(crate::repr::proj_depth)
                .unwrap_or(0),
        );
    if depth == 0 {
        return Ok(());
    }
    match join {
        Some((mode, level)) if mode != "@none" => {
            if depth > *level {
                return Err(NgsiError::BadRequestData(format!(
                    "projected attribute depth {depth} exceeds joinLevel {level} (5.7.1.4/5.7.2.4)"
                ))
                .into());
            }
            Ok(())
        }
        _ => Err(NgsiError::BadRequestData(
            "projection uses Linked Entity selection but join is not specified (5.7.1.4/5.7.2.4)"
                .into(),
        )
        .into()),
    }
}

/// join/joinLevel params (4.5.23). Returns (mode, level).
pub fn parse_join(params: &HashMap<String, String>) -> ApiResult<Option<(String, usize)>> {
    let Some(mode) = params.get("join") else {
        return Ok(None);
    };
    if !["inline", "flat", "@none"].contains(&mode.as_str()) {
        return Err(NgsiError::BadRequestData(format!("invalid join {mode:?}")).into());
    }
    let level = match params.get("joinLevel") {
        Some(l) => l
            .parse::<usize>()
            .ok()
            // Bounded traversal depth
            .filter(|l| *l >= 1 && *l <= crate::bounds::MAX_JOIN_LEVEL)
            .ok_or_else(|| {
                NgsiError::BadRequestData(format!(
                    "invalid joinLevel {l:?} (1..={})",
                    crate::bounds::MAX_JOIN_LEVEL
                ))
            })?,
        None => 1,
    };
    if mode == "@none" {
        return Ok(None);
    }
    Ok(Some((mode.clone(), level)))
}

/// The child representation for a linked entity under `key` (4.21 nested
/// projections apply to the joined entity, not the relationship itself).
fn joined_repr(parent: &crate::repr::Repr, key_compact: &str, key_iri: &str) -> crate::repr::Repr {
    let mut r = crate::repr::Repr {
        sys_attrs: parent.sys_attrs,
        key_values: parent.key_values,
        concise: parent.concise,
        lang: parent.lang.clone(),
        ..Default::default()
    };
    if let Some(pick) = &parent.pick {
        if let Some(n) = pick
            .iter()
            .find(|n| n.raw == key_compact || n.iri == key_iri)
        {
            r.pick = n.children.clone();
        }
    }
    if let Some(omit) = &parent.omit {
        if let Some(n) = omit
            .iter()
            .find(|n| (n.raw == key_compact || n.iri == key_iri) && n.children.is_some())
        {
            r.omit = n.children.clone();
        }
    }
    r
}

/// Linked Entity Retrieval, inline form (4.5.23.2): embed each relationship
/// target under an "entity" member (normalized) or replace the object URI by
/// the linked entity representation (simplified). Operates on COMPACTED docs.
pub fn inline_join(
    st: &AppState,
    tenant: &TenantId,
    ctx: &antares_jsonld::Context,
    repr: &crate::repr::Repr,
    compacted: &mut Value,
    level: usize,
) {
    let Some(obj) = compacted.as_object_mut() else {
        return;
    };
    let metas = ["id", "type", "scope", "createdAt", "modifiedAt", "@context"];
    for (k, v) in obj.iter_mut() {
        if metas.contains(&k.as_str()) {
            continue;
        }
        let child = joined_repr(repr, k, &ctx.expand_key(k));
        inline_join_value(st, tenant, ctx, repr, &child, v, level);
    }
}

fn lookup_joined(
    st: &AppState,
    tenant: &TenantId,
    ctx: &antares_jsonld::Context,
    child: &crate::repr::Repr,
    id: &str,
    level: usize,
) -> Option<Value> {
    let target = st.store.get(tenant, Kind::Entity, id).ok().flatten()?;
    let shaped = apply(&target, child);
    let mut c = compact_for(child, &shaped, ctx);
    if level > 1 {
        inline_join(st, tenant, ctx, child, &mut c, level - 1);
    }
    Some(c)
}

fn inline_join_value(
    st: &AppState,
    tenant: &TenantId,
    ctx: &antares_jsonld::Context,
    repr: &crate::repr::Repr,
    child: &crate::repr::Repr,
    v: &mut Value,
    level: usize,
) {
    match v {
        Value::Array(items) => {
            for i in items {
                inline_join_value(st, tenant, ctx, repr, child, i, level);
            }
        }
        Value::Object(inst) => {
            if repr.key_values {
                return;
            }
            // 4.5.22.2: a ListRelationship's targets join under the
            // output-only "entityList" member (always an array). The
            // compacted objectList carries {"object": URI} entries.
            if let Some(Value::Array(ol)) = inst.get("objectList") {
                let targets: Vec<String> = ol
                    .iter()
                    .filter_map(|e| match e {
                        Value::String(id) => Some(id.clone()),
                        Value::Object(o) => {
                            o.get("object").and_then(Value::as_str).map(str::to_owned)
                        }
                        _ => None,
                    })
                    .collect();
                let joined: Vec<Value> = targets
                    .iter()
                    .filter_map(|id| lookup_joined(st, tenant, ctx, child, id, level))
                    .collect();
                if !joined.is_empty() {
                    inst.insert("entityList".into(), Value::Array(joined));
                }
                return;
            }
            let targets: Vec<String> = match inst.get("object") {
                Some(Value::String(id)) => vec![id.clone()],
                Some(Value::Array(a)) => a
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect(),
                _ => return,
            };
            let mut joined: Vec<Value> = targets
                .iter()
                .filter_map(|id| lookup_joined(st, tenant, ctx, child, id, level))
                .collect();
            if joined.is_empty() {
                return;
            }
            let e = if joined.len() == 1 {
                joined.remove(0)
            } else {
                Value::Array(joined)
            };
            inst.insert("entity".into(), e);
        }
        // simplified: relationship value is the object URI string
        Value::String(id) if repr.key_values => {
            if let Some(joined) = lookup_joined(st, tenant, ctx, child, id, level) {
                *v = joined;
            }
        }
        _ => {}
    }
}

/// Linked Entity Retrieval, flattened form (4.5.23.3): collect targets with
/// the child representation that applies to each.
pub fn collect_flat(
    st: &AppState,
    tenant: &TenantId,
    repr: &crate::repr::Repr,
    internal_doc: &Value,
    level: usize,
    out: &mut std::collections::BTreeMap<String, (Value, crate::repr::Repr)>,
) {
    let Some(obj) = internal_doc.as_object() else {
        return;
    };
    for (k, v) in obj {
        if is_meta(k) {
            continue;
        }
        // only traverse relationships that survive THIS doc's projection
        if let Some(pick) = &repr.pick {
            if !pick.iter().any(|n| n.iri == *k || n.raw == *k) {
                continue;
            }
        }
        if let Some(omit) = &repr.omit {
            if omit
                .iter()
                .any(|n| (n.iri == *k || n.raw == *k) && n.children.is_none())
            {
                continue;
            }
        }
        let Some(instances) = v.as_array() else {
            continue;
        };
        let child = joined_repr(repr, k, k);
        for inst in instances {
            // Relationship objects plus ListRelationship objectList targets
            // (internal form stores bare URIs) — 4.5.23.3 appends both kinds
            // of Linked Entities to the flattened array.
            let targets: Vec<&str> = match (inst.get("object"), inst.get("objectList")) {
                (Some(Value::String(id)), _) => vec![id.as_str()],
                (Some(Value::Array(a)), _) => a.iter().filter_map(Value::as_str).collect(),
                (None, Some(Value::Array(a))) => a.iter().filter_map(Value::as_str).collect(),
                _ => continue,
            };
            for id in targets {
                if out.contains_key(id) {
                    continue;
                }
                if let Some(target) = st.store.get(tenant, Kind::Entity, id).ok().flatten() {
                    out.insert(id.to_owned(), (target.clone(), child.clone()));
                    if level > 1 {
                        collect_flat(st, tenant, &child, &target, level - 1, out);
                    }
                }
            }
        }
    }
}

// ---------- GET /entities/ (5.7.2) ----------

pub async fn query_entities(
    State(st): State<AppState>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
) -> Response {
    match query_entities_outer(&st, params, &headers).await {
        Ok(r) => r,
        Err(e) => e.into_response(),
    }
}

/// 5.5.14 / 5.5.9.3: a query referencing an EntityMap (NGSILD-EntityMap
/// request header, 6.4.3.2-2) is fixed to the map's Entities; the filters
/// are re-checked at processing time and local entries that no longer match
/// are removed from the map by its creator. An expired or unknown map means
/// "no inference can be made … a new one shall be created".
async fn query_entities_outer(
    st: &AppState,
    mut params: HashMap<String, String>,
    headers: &HeaderMap,
) -> ApiResult<Response> {
    let Some(map_ref) = headers
        .get("NGSILD-EntityMap")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
    else {
        return query_entities_inner(st, &params, headers).await;
    };
    let tenant = tenant_from(headers)?;
    let map_id = map_ref.rsplit('/').next().unwrap_or(&map_ref).to_owned();
    let Some(mut map) = crate::entity_maps::map_get(st, &tenant, &map_id) else {
        // 5.5.14: expired or inaccessible → a new EntityMap is created
        params.insert("entityMap".into(), "true".into());
        return query_entities_inner(st, &params, headers).await;
    };
    let ctx = request_context(&st.loader, headers).await?;
    // a request that references a live map does not create a new one
    params.remove("entityMap");
    let (offset, limit, count) = page_params(st, &params)?;
    // pagination links carry the ORIGINAL query, never the page's id list
    let link_params = params.clone();
    let accept = parse_accept_geo(headers)?;

    // 5.5.9.3 paged fetch: the map fixes the candidate id set; candidates
    // are fetched (locally + forwarded) chunk by chunk, "filters shall be
    // rechecked before returning results" per chunk, and visited entries
    // that no longer match are removed from the map — "Entities not or no
    // longer fitting the query shall be removed from the Entity map during
    // pagination". Pruning is judgeable only for "@none" (local) entries: a
    // remote-backed id may merely have an unreachable source right now
    // (5.5.14). Memory per request is O(chunk), never O(map) — the reason
    // EntityMaps exist for the distributed case. count=true walks every
    // candidate (the total needs each id checked), still chunk-bounded.
    let ids: Vec<String> = map["entityMap"]
        .as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();
    let looped = crate::federation::via_loop(
        headers,
        &crate::federation::alias_for(&st.host_alias, &tenant),
    );
    let chunk_size = limit.max(20);
    let mut page_ids: Vec<String> = Vec::new();
    let (mut skipped, mut total, mut more, mut visited) = (0usize, 0usize, false, 0usize);
    for chunk in ids.chunks(chunk_size) {
        let mut p = params.clone();
        p.remove("limit");
        p.remove("offset");
        p.remove("count");
        p.insert("id".into(), chunk.join(","));
        // the final page fetch below re-surfaces the same peers' warnings
        let mut chunk_warnings = Vec::new();
        let fed = if crate::federation::active(&p) && !looped {
            crate::federation::fed_query(st, &tenant, headers, &ctx, &p, &mut chunk_warnings).await
        } else {
            Vec::new()
        };
        let docs = filter_entities_fed(st, &tenant, &p, &ctx, fed)?;
        let matched: std::collections::HashSet<&str> = docs
            .iter()
            .filter_map(|d| d.get("id").and_then(Value::as_str))
            .collect();
        if let Some(emap) = map.get_mut("entityMap").and_then(Value::as_object_mut) {
            for id in chunk {
                let local_only = emap
                    .get(id)
                    .and_then(Value::as_array)
                    .is_some_and(|a| a.len() == 1 && a[0] == "@none");
                if local_only && !matched.contains(id.as_str()) {
                    emap.remove(id);
                }
            }
        }
        visited += chunk.len();
        for id in chunk {
            if !matched.contains(id.as_str()) {
                continue;
            }
            total += 1;
            if skipped < offset {
                skipped += 1;
            } else if page_ids.len() < limit {
                page_ids.push(id.clone());
            } else {
                more = true;
            }
        }
        if !count && page_ids.len() == limit {
            // page full — next exists if an extra match was seen or
            // unvisited candidates remain ("pages shall always be filled to
            // the maximum, as long as Entities are available")
            more = more || visited < ids.len();
            break;
        }
    }
    if count {
        more = total > offset + limit;
    }
    crate::entity_maps::map_put(st, &tenant, map.clone());
    // fix the final fetch to exactly the page's survivors (5.5.14: an empty
    // set is fixed to nothing); one extra page-sized fetch keeps the whole
    // repr pipeline shared instead of forked
    params.insert(
        "id".into(),
        if page_ids.is_empty() {
            "urn:ngsi-ld:entitymap:empty".to_owned()
        } else {
            page_ids.join(",")
        },
    );
    params.remove("offset");
    params.remove("count");
    if limit == 0 {
        // count-only shape: the inner default limit is irrelevant against
        // the empty id sentinel
        params.remove("limit");
    }
    let mut resp = query_entities_inner(st, &params, headers).await?;
    // "The location of the EntityMap used in the query operation is
    // returned in the response" (6.4.3.2-2)
    if let Ok(v) = format!("/ngsi-ld/v1/entityMaps/{map_id}").parse() {
        resp.headers_mut().insert("NGSILD-EntityMap", v);
    }
    if count {
        if let Ok(v) = total.to_string().parse() {
            resp.headers_mut().insert("NGSILD-Results-Count", v);
        }
    }
    // 6.3.10 links from the original query — the inner call saw offset 0
    // over exactly one page, so it emitted none
    for (off, rel, cond) in [
        (offset + limit, "next", more && limit > 0),
        (offset.saturating_sub(limit.max(1)), "prev", offset > 0),
    ] {
        if !cond {
            continue;
        }
        let mut qp: Vec<String> = link_params
            .iter()
            .filter(|(k, _)| k.as_str() != "offset")
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        qp.push(format!("offset={off}"));
        qp.sort(); // deterministic order — the suite string-compares links
        let ty = match accept {
            Accept::LdJson => ";type=\"application/ld+json\"",
            Accept::Json => ";type=\"application/json\"",
            Accept::GeoJson => ";type=\"application/geo+json\"",
        };
        if let Ok(v) = format!("</ngsi-ld/v1/entities?{}>; rel=\"{rel}\"{ty}", qp.join("&")).parse()
        {
            resp.headers_mut().append(axum::http::header::LINK, v);
        }
    }
    Ok(resp)
}

pub const QUERY_PARAMS: &[&str] = &[
    "id",
    "idPattern",
    "type",
    "attrs",
    "q",
    "georel",
    "geometry",
    "coordinates",
    "geoproperty",
    "scopeQ",
    "csf",
    "limit",
    "offset",
    "count",
    "options",
    "format",
    "pick",
    "omit",
    "lang",
    "local",
    "entityMap",
    "geometryProperty",
    "expandValues",
    "jsonKeys",
    "datasetId",
    "join",
    "joinLevel",
    "containedBy",
    "orderBy",
    "orderFrom",
    "orderGeometry",
    "collation",
    "entityMapLifetime",
    "splitEntities",
];

async fn query_entities_inner(
    st: &AppState,
    params: &HashMap<String, String>,
    headers: &HeaderMap,
) -> ApiResult<Response> {
    let tenant = tenant_from(headers)?;
    check_params(params, QUERY_PARAMS)?;
    let accept = parse_accept_geo(headers)?;
    let ctx = request_context(&st.loader, headers).await?;

    // 5.7.2.4 a-e: id/idPattern alone are NOT sufficient, and the attrs
    // list / q must include "at least one non-system Attribute" to qualify.
    let q_ast = params.get("q").map(|q| parse_q(q)).transpose()?;
    let has_filter = qualifies_non_wide(params, q_ast.as_ref());
    // 5.7.2.4 validation bullets (p.201), in the spec's own order.
    if params.get("type").map(String::as_str) == Some("*")
        && params.get("local").map(String::as_str) == Some("false")
    {
        return Err(NgsiError::BadRequestData(
            "type=* implies local and shall not be combined with local=false \
             (Table 6.4.3.2-1)"
                .into(),
        )
        .into());
    }
    if params.contains_key("geometryProperty") && accept != Accept::GeoJson {
        return Err(NgsiError::BadRequestData(
            "geometryProperty requires Accept: application/geo+json (5.7.2.4)".into(),
        )
        .into());
    }
    // "If the ordering parameter is present and the execution of the operation
    // is not limited to the local scope then BadRequestData" — reinforced by
    // 4.23.1: "Sort ordering is never applied to distributed operations."
    // The subject is the EXECUTION: a query nothing federates to runs locally
    // regardless of `local=true`, which is why this asks would_federate rather
    // than active (ETSI 019_19 orders without local).
    if params.contains_key("orderBy")
        && crate::federation::would_federate(st, &tenant, &ctx, params, headers)
    {
        return Err(NgsiError::BadRequestData(
            "orderBy requires local scope — ordering is never applied to \
             distributed operations (5.7.2.4, 4.23.1)"
                .into(),
        )
        .into());
    }
    if !has_filter {
        return Err(NgsiError::BadRequestData(
            "query needs at least one of type, attrs, q, georel (5.7.2)".into(),
        )
        .into());
    }

    let repr = parse_repr(params, &ctx)?;
    let join = parse_join(params)?;
    check_linked_projection(&repr, &join)?;
    // 5.7.2.4: filter conditions using Linked Entity attributes need join,
    // and their hop depth may not exceed joinLevel ("too deep query")
    let link_depth = q_ast
        .as_ref()
        .map(antares_ql::QNode::max_link_depth)
        .unwrap_or(0);
    if link_depth > 0 {
        match &join {
            Some((mode, level)) if mode != "@none" => {
                if link_depth > *level {
                    return Err(NgsiError::BadRequestData(format!(
                        "linked attribute query depth {link_depth} exceeds joinLevel {level} \
                         (5.7.2.4 — too deep query)"
                    ))
                    .into());
                }
            }
            _ => {
                return Err(NgsiError::BadRequestData(
                    "q references Linked Entity attributes but join is not specified \
                     (5.7.2.4 — too deep query)"
                        .into(),
                )
                .into());
            }
        }
    }
    // 5.7.2.4: a syntactically invalid context source filter is 400. Named
    // gap: csf is validated but not applied to Context Source matching.
    if let Some(csf) = params.get("csf") {
        parse_q(csf)?;
    }
    // 6.3.17: abnormal distributed-GET outcomes surface as NGSILD-Warning
    let mut warnings: Vec<String> = Vec::new();
    let looped = crate::federation::via_loop(
        headers,
        &crate::federation::alias_for(&st.host_alias, &tenant),
    );
    let fed = if crate::federation::active(params) && !looped {
        crate::federation::fed_query(st, &tenant, headers, &ctx, params, &mut warnings).await
    } else {
        if crate::federation::active(params)
            && looped
            && crate::federation::would_federate(st, &tenant, &ctx, params, headers)
        {
            warnings.push(crate::federation::warning(
                199,
                &crate::federation::alias_for(&st.host_alias, &tenant),
                "a registration loop has been detected",
            ));
        }
        Vec::new()
    };
    // Pushdown gates. Pagination: only when every filter the store cannot
    // see is absent — no federation candidates, no idPattern, no orderBy (its
    // 4.23 datatype comparison order is evaluator-owned), and a real limit
    // (limit=0 is the count-only shape). Projection additionally excludes
    // join (linked-entity walks read page docs) and GeoJSON output.
    let (p_offset, p_limit, _) = page_params(st, params)?;
    let push_page = fed.is_empty()
        && params.get("idPattern").is_none()
        && params.get("orderBy").is_none()
        && p_limit > 0;
    let push_proj = join.is_none() && accept != Accept::GeoJson;
    let filtered = filter_entities_paged(
        st,
        &tenant,
        params,
        &ctx,
        fed,
        push_page.then_some((p_offset, p_limit)),
        push_proj.then_some(&repr),
    )?;
    let mut matches = filtered.docs;
    if let Some(spec) = params.get("orderBy") {
        order_entities(&mut matches, spec, params, &ctx)?;
    }
    let (page, count_hdr, links) = if filtered.paged {
        let total = filtered.total.unwrap_or(matches.len());
        paginate_pre(st, params, matches, "/ngsi-ld/v1/entities", total)?
    } else {
        paginate(st, params, matches, "/ngsi-ld/v1/entities")?
    };

    let mut payload: Vec<Value> = page
        .iter()
        .filter_map(|doc| {
            let shaped = apply(doc, &repr);
            // pick projections that match nothing drop the entity entirely
            if repr.pick.is_some() && shaped.as_object().is_some_and(|o| o.is_empty()) {
                return None;
            }
            Some(compact_for(&repr, &shaped, &ctx))
        })
        .collect();
    if let Some((mode, level)) = &join {
        match mode.as_str() {
            "inline" => {
                for p in &mut payload {
                    inline_join(st, &tenant, &ctx, &repr, p, *level);
                }
            }
            "flat" => {
                let mut linked = std::collections::BTreeMap::new();
                for doc in &page {
                    collect_flat(st, &tenant, &repr, doc, *level, &mut linked);
                }
                let page_ids: Vec<&str> = page.iter().filter_map(|d| d["id"].as_str()).collect();
                for (id, (ldoc, lrepr)) in linked {
                    if !page_ids.contains(&id.as_str()) {
                        payload.push(compact_for(&lrepr, &apply(&ldoc, &lrepr), &ctx));
                    }
                }
            }
            _ => {}
        }
    }
    let mut resp = if accept == Accept::GeoJson {
        let fc = to_geojson_collection(payload, params.get("geometryProperty"), &ctx);
        respond_prefer(StatusCode::OK, fc, &ctx, accept, &tenant, headers)
    } else {
        crate::negotiate::respond_list(StatusCode::OK, payload, &ctx, accept, &tenant)
    };
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
    attach_warnings(&mut resp, &warnings);
    // 6.4.3.2: entityMap=true — the EntityMap for this query is (re)created;
    // the response carries NGSILD-EntityMap and 201 Created.
    if params.get("entityMap").map(String::as_str) == Some("true") {
        let map = crate::entity_maps::build_query_map(st, &tenant, headers, &ctx, params).await?;
        *resp.status_mut() = StatusCode::CREATED;
        if let Some(id) = map.get("id").and_then(Value::as_str) {
            if let Ok(v) = format!("/ngsi-ld/v1/entityMaps/{id}").parse() {
                resp.headers_mut().insert("NGSILD-EntityMap", v);
            }
        }
    }
    Ok(resp)
}

/// 5.7.2.4 / 5.7.4.4 / 5.14.4.4 a-e: a query qualifies (is not "too wide")
/// only with a type selector, an attrs list or q naming at least one
/// non-system Attribute, a geoquery, or local scope.
pub(crate) fn qualifies_non_wide(
    params: &HashMap<String, String>,
    q_ast: Option<&antares_ql::QNode>,
) -> bool {
    let attrs_qualify = params.get("attrs").is_some_and(|a| {
        a.split(',')
            .any(|n| antares_ql::is_non_system_attr(n.trim()))
    });
    let q_qualifies = q_ast.is_some_and(|ast| {
        ast.attribute_paths()
            .iter()
            .any(|h| antares_ql::is_non_system_attr(h))
    });
    params.contains_key("type")
        || attrs_qualify
        || q_qualifies
        || params.contains_key("georel")
        || params.get("local").map(String::as_str) == Some("true")
}

/// Shared entity filtering for query + purge.
pub fn filter_entities(
    st: &AppState,
    tenant: &TenantId,
    params: &HashMap<String, String>,
    ctx: &antares_jsonld::Context,
) -> ApiResult<Vec<Value>> {
    filter_entities_fed(st, tenant, params, ctx, Vec::new())
}

/// Same, with federated candidate docs merged in before filtering (4.3.6.7).
pub fn filter_entities_fed(
    st: &AppState,
    tenant: &TenantId,
    params: &HashMap<String, String>,
    ctx: &antares_jsonld::Context,
    fed: Vec<(bool, Value)>,
) -> ApiResult<Vec<Value>> {
    Ok(filter_entities_paged(st, tenant, params, ctx, fed, None, None)?.docs)
}

/// What the paged variant produced. `paged` = the store already applied
/// ORDER BY id + LIMIT/OFFSET (and `total` is the pre-LIMIT match count), so
/// the caller must NOT slice again.
pub struct Filtered {
    pub docs: Vec<Value>,
    pub paged: bool,
    pub total: Option<usize>,
}

/// The full filtering path (5.7.2). `page` = (offset, limit) to push into the
/// store — pass it ONLY when every filter the store cannot see is absent
/// (idPattern, federation, orderBy); the store still refuses unless its own
/// predicates compiled exactly. `proj` = the parsed representation, offered
/// for projection pushdown (pick/omit/attrs top-level heads) under the same
/// exactness gate.
pub fn filter_entities_paged(
    st: &AppState,
    tenant: &TenantId,
    params: &HashMap<String, String>,
    ctx: &antares_jsonld::Context,
    fed: Vec<(bool, Value)>,
    page: Option<(usize, usize)>,
    proj: Option<&crate::repr::Repr>,
) -> ApiResult<Filtered> {
    // a pushed page over local rows cannot be merged with federated
    // candidates — refuse here so no caller can create that page
    let page = if fed.is_empty() { page } else { None };
    let ids: Option<Vec<&str>> = params.get("id").map(|s| s.split(',').collect());
    if let Some(ids) = &ids {
        for id in ids {
            antares_model::EntityId::new(id)?;
        }
    }
    let id_pattern = match params.get("idPattern") {
        Some(p) => {
            if ["**", "++", "*+", "+*"].iter().any(|q| p.contains(q)) {
                return Err(NgsiError::BadRequestData(format!("invalid idPattern {p:?}")).into());
            }
            Some(
                regex::Regex::new(p)
                    .map_err(|_| NgsiError::BadRequestData(format!("invalid idPattern {p:?}")))?,
            )
        }
        None => None,
    };
    // Entity Type Selection Language (4.17): `,`/`|` = OR, `(a;b)` = AND.
    // Table 6.4.3.2-1: `"*"` selects every Entity Type, i.e. no type predicate
    // at all. Expanding it as a term yields an IRI nothing matches, which is
    // how `type=*` silently returned an empty array.
    let type_sel: Option<Vec<Vec<String>>> = params.get("type").filter(|s| *s != "*").map(|s| {
        s.split([',', '|'])
            .map(|alt| {
                alt.trim()
                    .trim_start_matches('(')
                    .trim_end_matches(')')
                    .split(';')
                    .map(|t| ctx.expand_key(t.trim()))
                    .collect()
            })
            .collect()
    });
    let attr_filter: Option<Vec<String>> = params
        .get("attrs")
        .map(|s| s.split(',').map(|t| ctx.expand_key(t.trim())).collect());
    let q_ast = match params.get("q") {
        // 4.9 expandValues: "attributes whose values should be expanded
        // against the supplied @context using JSON-LD type coercion prior to
        // executing the query" (EXAMPLE 12). jsonKeys needs no action — raw
        // JSON targets are navigated without term expansion by default.
        Some(q) => Some(crate::qeval::apply_expand_values(
            parse_q(q)?,
            params.get("expandValues").map(String::as_str),
            ctx,
        )),
        None => None,
    };
    let scope_q = params.get("scopeQ");
    let geo = crate::geo::GeoQuery::from_params(params)?;

    // Hand the store what it can filter on. A backend that can push
    // the predicate down (postgres/timescale) returns fewer rows — and says
    // via `decided` whether it applied EVERY present predicate exactly, which
    // is what licenses pagination/projection pushdown and lets the loop below
    // skip re-deciding. A backend that cannot (memory/file) returns the
    // snapshot and the loop stays the arbiter.
    let expand = |t: &str| ctx.expand_key(t);
    let geo_spec = geo.as_ref().and_then(|g| g.to_sql_spec(ctx));
    // A geo query whose spec declined to compile (non-default geoproperty) is
    // INVISIBLE to the store — the store would truthfully claim `decided`
    // about what it saw, projection would strip the very member the evaluator
    // still needs, and a pushed page would page over the wrong set. Forfeit
    // every pushdown up front and mask `decided` after.
    let geo_uncompiled = geo.is_some() && geo_spec.is_none();
    let page = if geo_uncompiled { None } else { page };
    // pick (or attrs) heads to keep / whole-attr omit heads to drop; core
    // members are never SQL-dropped (only `://` IRIs qualify) — repr::apply
    // stays the decider for those.
    let keep_attrs: Option<Vec<String>> = proj.and_then(|r| {
        r.pick
            .as_ref()
            .map(|nodes| nodes.iter().map(|n| n.iri.clone()).collect())
            .or_else(|| r.attrs.clone())
    });
    let drop_attrs: Option<Vec<String>> = proj
        .and_then(|r| {
            r.omit.as_ref().map(|nodes| {
                nodes
                    .iter()
                    .filter(|n| n.children.is_none() && n.iri.contains("://"))
                    .map(|n| n.iri.clone())
                    .collect::<Vec<_>>()
            })
        })
        .filter(|v| !v.is_empty());
    // 5.7.2.4 split entities (p.202): the filters (q, geoquery, Scope query,
    // Attributes) apply only AFTER remote parts and local information have
    // been aggregated — so with federated candidates present the store must
    // not drop (or pre-project) the LOCAL half of a split entity. The
    // post-merge loop below applies them instead (`decided` is already
    // false whenever `fed` is non-empty).
    let split_agg = crate::federation::split_entities(params) && !fed.is_empty();
    let outcome = st.store.query_entities(
        tenant,
        &antares_sql::store::filter::EntityFilter {
            ids: ids.as_deref(),
            types: type_sel.as_deref(),
            attrs: if split_agg {
                None
            } else {
                attr_filter.as_deref()
            },
            q: if split_agg { None } else { q_ast.as_ref() },
            scope_q: if split_agg {
                None
            } else {
                scope_q.map(String::as_str)
            },
            geo: if split_agg { None } else { geo_spec.as_ref() },
            expand: &expand,
            page: page.map(|(offset, limit)| antares_sql::store::filter::Page {
                offset: offset as i64,
                limit: limit as i64,
            }),
            keep_attrs: if geo_uncompiled || split_agg {
                None
            } else {
                keep_attrs.as_deref()
            },
            drop_attrs: if geo_uncompiled || split_agg {
                None
            } else {
                drop_attrs.as_deref()
            },
        },
    )?;
    let decided = outcome.decided && fed.is_empty() && !geo_uncompiled;
    let paged = outcome.paged && fed.is_empty();
    let total = outcome.total.map(|t| t as usize);
    let all = crate::federation::merge_candidates(outcome.rows, fed);
    let mut out = Vec::new();
    for doc in all {
        let id = doc["id"].as_str().unwrap_or("");
        if let Some(ids) = &ids {
            if !decided && !ids.contains(&id) {
                continue;
            }
        }
        // 5.2.33: "id takes precedence over idPattern" — the pattern only
        // filters when no id selector was given.
        if ids.is_none() {
            if let Some(re) = &id_pattern {
                // idPattern is invisible to the store — applied even when
                // decided
                if !re.is_match(id) {
                    continue;
                }
            }
        }
        if !decided {
            if let Some(sel) = &type_sel {
                let etypes: Vec<&str> = doc["type"]
                    .as_array()
                    .map(|a| a.iter().filter_map(Value::as_str).collect())
                    .unwrap_or_default();
                let matched = sel
                    .iter()
                    .any(|and_group| and_group.iter().all(|w| etypes.contains(&w.as_str())));
                if !matched {
                    continue;
                }
            }
            if let Some(attrs) = &attr_filter {
                if !attrs.iter().any(|a| doc.get(a).is_some()) {
                    continue;
                }
            }
            if let Some(ast) = &q_ast {
                // 4.9 linked-entity subqueries (attr{path}) resolve through
                // the local store, same tenant.
                let lookup = |uri: &str| st.store.get(tenant, Kind::Entity, uri).ok().flatten();
                if !eval_q(ast, &doc, ctx, &lookup) {
                    continue;
                }
            }
            if let Some(sq) = scope_q {
                if !crate::scope_matches(sq, &doc) {
                    continue;
                }
            }
            if let Some(g) = &geo {
                if !g.matches(&doc, ctx) {
                    continue;
                }
            }
        }
        out.push(doc);
    }
    Ok(Filtered {
        docs: out,
        paged,
        total,
    })
}

/// limit/offset/count handling (6.3.10). Returns (page, count, link headers).
/// 4.12/5.5.9.1 Pagination: L = client limit (Mc) or the default (Md); at
/// most L elements per page; remaining elements are flagged with a next
/// pointer carrying every parameter needed to fetch the page, prev on every
/// iteration but the first, and only prev on the last. Shared by every
/// paginated list operation (5.7.2, 5.7.4, 5.8.4, 5.10.2, 5.11.5).
pub fn paginate(
    st: &AppState,
    params: &HashMap<String, String>,
    matches: Vec<Value>,
    path: &str,
) -> ApiResult<(Vec<Value>, Option<usize>, Vec<String>)> {
    paginate_impl(st, params, matches, path, Accept::Json, None)
}

/// The store already applied ORDER BY id + LIMIT/OFFSET and counted the
/// match set — `matches` IS the page; only count/links remain.
pub fn paginate_pre(
    st: &AppState,
    params: &HashMap<String, String>,
    page: Vec<Value>,
    path: &str,
    total: usize,
) -> ApiResult<(Vec<Value>, Option<usize>, Vec<String>)> {
    paginate_impl(st, params, page, path, Accept::Json, Some(total))
}

/// 4.12 Pagination: clients specify a limit (page size), the server defines
/// a default page size, and a hard ceiling is rejected with TooManyResults
/// rather than silently clamped. The limit/offset/count triple of 6.3.10,
/// validated (ceilings included). Shared by `paginate_impl` and the
/// pushdown gate so the two paths can never disagree on what a page is.
pub fn page_params(
    st: &AppState,
    params: &HashMap<String, String>,
) -> ApiResult<(usize, usize, bool)> {
    let count = params.get("count").map(String::as_str) == Some("true");
    let limit: usize = match params.get("limit") {
        Some(l) => l
            .parse()
            .map_err(|_| NgsiError::BadRequestData(format!("invalid limit {l:?}")))?,
        None => st.default_limit,
    };
    // 5.5.6: "so many results that can potentially exhaust client or server
    // resources" — the implementation threshold is max_limit; 403
    // TooManyResults, not silent clamping.
    if limit > st.max_limit {
        return Err(NgsiError::TooManyResults(format!(
            "limit {limit} exceeds the server maximum {}",
            st.max_limit
        ))
        .into());
    }
    if limit == 0 && !count {
        return Err(
            NgsiError::BadRequestData("limit=0 requires count=true (6.3.10)".into()).into(),
        );
    }
    let offset: usize = match params.get("offset") {
        Some(o) => o
            .parse()
            .map_err(|_| NgsiError::BadRequestData(format!("invalid offset {o:?}")))?,
        None => 0,
    };
    // An offset above i64::MAX wraps negative when bound as SQL `$n::bigint`
    // (Postgres then rejects a negative OFFSET → 500). Reject it as a bad
    // precondition instead.
    if offset > i64::MAX as usize {
        return Err(NgsiError::BadRequestData(format!("offset {offset} is out of range")).into());
    }
    Ok((offset, limit, count))
}

/// 6.3.10: next/prev Links carry the response media type; the suite asserts
/// `;type="application/ld+json"` on ld+json list responses (031_02).
pub fn paginate_accept(
    st: &AppState,
    params: &HashMap<String, String>,
    matches: Vec<Value>,
    path: &str,
    accept: Accept,
) -> ApiResult<(Vec<Value>, Option<usize>, Vec<String>)> {
    paginate_impl(st, params, matches, path, accept, None)
}

fn paginate_impl(
    st: &AppState,
    params: &HashMap<String, String>,
    matches: Vec<Value>,
    path: &str,
    accept: Accept,
    pre: Option<usize>,
) -> ApiResult<(Vec<Value>, Option<usize>, Vec<String>)> {
    let (offset, limit, count) = page_params(st, params)?;
    let total = pre.unwrap_or(matches.len());
    let page: Vec<Value> = match pre {
        Some(_) => matches, // already exactly the page (store pushdown)
        None => matches.into_iter().skip(offset).take(limit).collect(),
    };
    let mut links = Vec::new();
    // csource resources: the suite string-compares links against
    // `?other…&limit=N&offset=M` order with an unconditional ld+json type
    // suffix (037_11, 041_03); entity lists keep sorted params + accept-based
    // suffix (031_02).
    let csource_style = path.contains("csource");
    let mut mk = |off: usize, rel: &str| {
        let mut qp: Vec<String>;
        if csource_style {
            qp = params
                .iter()
                .filter(|(k, _)| !matches!(k.as_str(), "offset" | "limit"))
                .map(|(k, v)| format!("{k}={v}"))
                .collect();
            qp.sort();
            if let Some(l) = params.get("limit") {
                qp.push(format!("limit={l}"));
            }
            qp.push(format!("offset={off}"));
        } else {
            qp = params
                .iter()
                .filter(|(k, _)| k.as_str() != "offset")
                .map(|(k, v)| format!("{k}={v}"))
                .collect();
            qp.push(format!("offset={off}"));
            qp.sort(); // deterministic order — the suite string-compares links
        }
        // 6.3.10: "At least, the type Link Target Attribute shall be included
        // ... and its value shall be exactly equal to the media type resulting
        // from the original request" — for EVERY media type, not just ld+json.
        let ty = match accept {
            _ if csource_style => ";type=\"application/ld+json\"",
            Accept::LdJson => ";type=\"application/ld+json\"",
            Accept::Json => ";type=\"application/json\"",
            Accept::GeoJson => ";type=\"application/geo+json\"",
        };
        links.push(format!("<{path}?{}>; rel=\"{rel}\"{ty}", qp.join("&")));
    };
    if offset + limit < total && limit > 0 {
        mk(offset + limit, "next");
    }
    if offset > 0 {
        mk(offset.saturating_sub(limit.max(1)), "prev");
    }
    Ok((page, count.then_some(total), links))
}

// ---------- DELETE /entities/{id} (5.6.6) ----------// ---------- DELETE /entities/{id} (5.6.6) ----------

pub async fn delete_entity(
    State(st): State<AppState>,
    Path(id): Path<String>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        antares_model::EntityId::new(&id)?;
        check_params(&params, &["local", "type"])?;
        let ctx = st.loader.core();
        // 4.17/5.6.6.4: the type selector gates the target — a registration
        // for a different type must not receive the forwarded delete.
        let spec = crate::csource::CsrSpec {
            ids: Some(vec![id.clone()]),
            types: params
                .get("type")
                .map(|s| s.split(',').map(|t| ctx.expand_key(t.trim())).collect()),
            ..Default::default()
        };
        let mut regs = crate::federation::write_regs(&st, &tenant, &spec, &ctx, &params, &headers);
        if let Some(r) = crate::federation::handle_via_loop(
            &headers,
            &crate::federation::alias_for(&st.host_alias, &tenant),
            &tenant,
            &mut regs,
        ) {
            return Ok(r);
        }
        // 5.6.6.4: the ?type selector narrows the target — an entity of a
        // non-matching type is "not known" for this delete.
        let type_gate = |doc: Option<Value>| -> Option<Value> {
            doc.filter(|d| crate::attrs::matches_type_param(d, &params, &ctx))
        };
        if !regs.is_empty() {
            let local_exists = type_gate(st.store.get(&tenant, Kind::Entity, &id)?).is_some();
            let proxy_match = regs.iter().any(|r| r.is_proxy());
            let mut parts = Vec::new();
            if local_exists || !proxy_match {
                if local_exists && st.store.delete(&tenant, Kind::Entity, &id)? {
                    mirror_delete_entity(&st, &tenant, &id);
                    parts.push(crate::federation::Part {
                        status: 204,
                        detail: "deleted locally".into(),
                    });
                } else {
                    parts.push(crate::federation::Part {
                        status: 404,
                        detail: format!("entity {id} not found locally"),
                    });
                }
            }
            let ctx_url = crate::federation::ctx_link_url(&headers, &ctx.source);
            for reg in &regs {
                // 5.6.6.4: proxy modes not supporting Delete Entity are an
                // error of type Conflict; inclusive ones are not forwarded.
                if !reg.supports("deleteEntity") {
                    if reg.is_proxy() {
                        parts.push(crate::federation::conflict_part("deleteEntity"));
                    }
                    continue;
                }
                parts.push(
                    crate::federation::forward_part(
                        &st,
                        reqwest::Method::DELETE,
                        format!("{}/ngsi-ld/v1/entities/{id}", reg.endpoint),
                        &[],
                        &headers,
                        &tenant,
                        reg,
                        &ctx_url,
                        None,
                    )
                    .await,
                );
            }
            return Ok(crate::federation::combine(
                parts,
                no_content(&tenant),
                &tenant,
            ));
        }
        if type_gate(st.store.get(&tenant, Kind::Entity, &id)?).is_some()
            && st.store.delete(&tenant, Kind::Entity, &id)?
        {
            mirror_delete_entity(&st, &tenant, &id);
            Ok::<_, ApiError>(no_content(&tenant))
        } else {
            Err(NgsiError::ResourceNotFound(format!("entity {id} not found")).into())
        }
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

// ---------- DELETE /entities/ — Purge (5.6.21) ----------

/// 5.6.21 Purge Entities: delete (or keep=/drop=-prune) all entities matched
/// by the query; output data is none — 204 (5.6.21.5). Too-wide queries,
/// Linked Entity paths and invalid id/q/geo/csf are BadRequestData
/// (5.6.21.4); matched registrations forward only with purgeEntity support.
pub async fn purge_entities(
    State(st): State<AppState>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
) -> Response {
    match purge_inner(&st, &params, &headers).await {
        Ok(r) => r,
        Err(e) => e.into_response(),
    }
}

async fn purge_inner(
    st: &AppState,
    params: &HashMap<String, String>,
    headers: &HeaderMap,
) -> ApiResult<Response> {
    let tenant = tenant_from(headers)?;
    check_params(
        params,
        &[
            "id",
            "idPattern",
            "type",
            "attrs",
            "q",
            "georel",
            "geometry",
            "coordinates",
            "geoproperty",
            "scopeQ",
            "csf",
            "keep",
            "drop",
            "local",
            "limit",
        ],
    )?;
    let ctx = request_context(&st.loader, headers).await?;
    // 5.6.21.4: exactly five qualifying conditions —
    //   a) selector of Entity Types
    //   b) list of Attribute names, including at least one non-system Attribute
    //   c) NGSI-LD Query, including at least one non-system Attribute
    //   d) NGSI-LD GeoQuery
    //   e) local scope (5.5.13)
    // "If none of the above is provided, then an error of type BadRequestData
    // shall be raised (too wide query)."
    //
    // id/idPattern are legal input data (5.6.21.3) and DO filter, but they are
    // never sufficient on their own: "it is not possible to purge a set of
    // entities by only specifying desired Entity identifiers". Listing them
    // here is how `DELETE /entities?idPattern=.*` became a tenant wipe.
    let attrs_qualify = params.get("attrs").is_some_and(|a| {
        a.split(',')
            .any(|n| antares_ql::is_non_system_attr(n.trim()))
    });
    let q_ast = params.get("q").map(|q| parse_q(q)).transpose()?;
    let q_qualifies = q_ast.as_ref().is_some_and(|ast| {
        ast.attribute_paths()
            .iter()
            .any(|h| antares_ql::is_non_system_attr(h))
    });
    // 5.6.21.4: Linked Entity retrieval in the projection attributes, or
    // Linked Entity attributes in the filter conditions → BadRequestData.
    if q_ast
        .as_ref()
        .is_some_and(antares_ql::QNode::has_linked_paths)
    {
        return Err(NgsiError::BadRequestData(
            "purge q must not reference Linked Entity attributes (5.6.21.4)".into(),
        )
        .into());
    }
    if params.get("attrs").is_some_and(|a| a.contains('{')) {
        return Err(NgsiError::BadRequestData(
            "purge attrs must not use Linked Entity retrieval (5.6.21.4)".into(),
        )
        .into());
    }
    // 5.6.21.4: a syntactically invalid context source filter is
    // BadRequestData. Known gap: csf is validated here but not yet applied
    // to Context Source matching (broker-wide).
    if let Some(csf) = params.get("csf") {
        parse_q(csf)?;
    }
    let has_filter = params.contains_key("type")
        || attrs_qualify
        || q_qualifies
        || params.contains_key("georel")
        || params.get("local").map(String::as_str) == Some("true");
    if !has_filter {
        return Err(NgsiError::BadRequestData(
            "purge needs at least one of: type, attrs or q naming a non-system \
             Attribute, georel, or local=true (5.6.21.4 — too wide query)"
                .into(),
        )
        .into());
    }
    if params.contains_key("keep") && params.contains_key("drop") {
        return Err(NgsiError::BadRequestData(
            "keep and drop are mutually exclusive (5.6.21)".into(),
        )
        .into());
    }
    let matches = filter_entities(st, &tenant, params, &ctx)?;
    let keep: Option<Vec<String>> = params
        .get("keep")
        .map(|s| s.split(',').map(|t| ctx.expand_key(t.trim())).collect());
    let drop: Option<Vec<String>> = params
        .get("drop")
        .map(|s| s.split(',').map(|t| ctx.expand_key(t.trim())).collect());
    for doc in &matches {
        let Some(id) = doc["id"].as_str() else {
            continue;
        };
        if keep.is_none() && drop.is_none() {
            st.store.delete(&tenant, Kind::Entity, id)?;
            mirror_delete_entity(st, &tenant, id);
            continue;
        }
        // keep=/drop= prune attributes; the entity itself survives (5.6.21)
        st.store.mutate(&tenant, Kind::Entity, id, |doc| {
            let target = doc.as_object_mut().expect("entity object");
            let attrs: Vec<String> = target.keys().filter(|k| !is_meta(k)).cloned().collect();
            for a in attrs {
                let purge = match (&keep, &drop) {
                    (Some(keep), _) => !keep.contains(&a),
                    (_, Some(drop)) => drop.contains(&a),
                    _ => true,
                };
                if purge {
                    target.remove(&a);
                }
            }
            Ok::<(), NgsiError>(())
        })?;
    }

    // distributed purge (5.6.21 / 6.4.3.3)
    let spec = crate::csource::CsrSpec {
        types: params
            .get("type")
            .map(|s| s.split(',').map(|t| ctx.expand_key(t.trim())).collect()),
        ids: params
            .get("id")
            .map(|s| s.split(',').map(str::to_owned).collect()),
        // 5.12: the purge's idPattern is part of the Entity specification too
        id_pattern: params.get("idPattern").cloned(),
        csf: params.get("csf").and_then(|c| antares_ql::parse_q(c).ok()),
        ..Default::default()
    };
    let mut regs = crate::federation::write_regs(st, &tenant, &spec, &ctx, params, headers);
    if let Some(r) = crate::federation::handle_via_loop(
        headers,
        &crate::federation::alias_for(&st.host_alias, &tenant),
        &tenant,
        &mut regs,
    ) {
        return Ok(r);
    }
    if !regs.is_empty() {
        let mut parts = vec![crate::federation::Part {
            status: 204,
            detail: "purged locally".into(),
        }];
        let ctx_url = crate::federation::ctx_link_url(headers, &ctx.source);
        let query: Vec<(String, String)> = ["type", "id", "idPattern", "q", "attrs"]
            .iter()
            .filter_map(|k| params.get(*k).map(|v| (k.to_string(), v.clone())))
            .collect();
        for reg in &regs {
            // 5.6.21.4: matching input data is forwarded only when the
            // registration supports Purge Entity; an unsupported matched
            // registration — any mode — is an error of type Conflict
            // (partial success when other parts succeeded).
            if !reg.supports("purgeEntity") {
                parts.push(crate::federation::conflict_part("purgeEntity"));
                continue;
            }
            parts.push(
                crate::federation::forward_part(
                    st,
                    reqwest::Method::DELETE,
                    format!("{}/ngsi-ld/v1/entities", reg.endpoint),
                    &query,
                    headers,
                    &tenant,
                    reg,
                    &ctx_url,
                    None,
                )
                .await,
            );
        }
        return Ok(crate::federation::combine(
            parts,
            no_content(&tenant),
            &tenant,
        ));
    }
    Ok(no_content(&tenant))
}

// ---------- PATCH /entities/{id} — Merge (5.6.17 / 5.5.12) ----------

pub async fn merge_entity(
    State(st): State<AppState>,
    Path(id): Path<String>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    match merge_entity_inner(&st, &id, &params, &headers, &body).await {
        Ok(r) => r,
        Err(e) => e.into_response(),
    }
}

async fn merge_entity_inner(
    st: &AppState,
    id: &str,
    params: &HashMap<String, String>,
    headers: &HeaderMap,
    body: &[u8],
) -> ApiResult<Response> {
    let tenant = tenant_from(headers)?;
    antares_model::EntityId::new(id)?;
    check_params(
        params,
        &["options", "format", "observedAt", "lang", "local", "type"],
    )?;
    let parsed = parse_body(&st.loader, headers, body, BodyKind::MergePatch).await?;
    let obj = parsed
        .value
        .as_object()
        .ok_or_else(|| NgsiError::BadRequestData("fragment must be a JSON object".into()))?;
    if let Some(bid) = obj.get("id").and_then(Value::as_str) {
        if bid != id {
            return Err(NgsiError::BadRequestData("fragment id mismatch".into()).into());
        }
    }
    let fragment = expand_entity(
        obj,
        &parsed.ctx,
        ExpandOpts {
            fragment: true,
            allow_null: true,
            merge: true,
            temporal: false,
            ..Default::default()
        },
    )?;
    let ts = now_iso();

    let spec = crate::csource::CsrSpec {
        ids: Some(vec![id.to_owned()]),
        ..Default::default()
    };
    let mut regs = crate::federation::write_regs(st, &tenant, &spec, &parsed.ctx, params, headers);
    if let Some(r) = crate::federation::handle_via_loop(
        headers,
        &crate::federation::alias_for(&st.host_alias, &tenant),
        &tenant,
        &mut regs,
    ) {
        return Ok(r);
    }
    if !regs.is_empty() {
        let proxies: Vec<&crate::federation::FedReg> =
            regs.iter().filter(|r| r.is_proxy()).collect();
        let mut parts = Vec::new();
        let (rest, has_attrs) = crate::federation::strip_proxied(obj, &proxies, &parsed.ctx);
        let local_exists = st.store.get(&tenant, Kind::Entity, id)?.is_some();
        if (local_exists || proxies.is_empty()) && has_attrs {
            let local_frag = expand_entity(
                &rest,
                &parsed.ctx,
                ExpandOpts {
                    fragment: true,
                    allow_null: true,
                    merge: true,
                    temporal: false,
                    ..Default::default()
                },
            )?;
            let res = st.store.mutate(&tenant, Kind::Entity, id, |doc| {
                merge_into(doc, &local_frag, &ts);
                Ok::<(), NgsiError>(())
            })?;
            parts.push(match res {
                Some(Ok(())) => crate::federation::Part {
                    status: 204,
                    detail: "merged locally".into(),
                },
                _ => crate::federation::Part {
                    status: 404,
                    detail: format!("entity {id} not found locally"),
                },
            });
        }
        let ctx_url = crate::federation::ctx_link_url(headers, &parsed.ctx.source);
        for reg in &regs {
            // 5.6.17.4: proxy modes without Merge Entity support are an
            // error of type Conflict; inclusive ones are not forwarded.
            if !reg.supports("mergeEntity") {
                if reg.is_proxy() {
                    parts.push(crate::federation::conflict_part("mergeEntity"));
                }
                continue;
            }
            let Some(frag) = crate::federation::reduce_to_scope(obj, reg, &parsed.ctx) else {
                continue;
            };
            parts.push(
                crate::federation::forward_part(
                    st,
                    reqwest::Method::PATCH,
                    format!("{}/ngsi-ld/v1/entities/{id}", reg.endpoint),
                    &[],
                    headers,
                    &tenant,
                    reg,
                    &ctx_url,
                    Some(frag),
                )
                .await,
            );
        }
        return Ok(crate::federation::combine(
            parts,
            no_content(&tenant),
            &tenant,
        ));
    }

    let res = st.store.mutate(&tenant, Kind::Entity, id, |doc| {
        // 5.6.17.4: the ?type selector narrows the merge target
        if !crate::attrs::matches_type_param(doc, params, &parsed.ctx) {
            return Err(NgsiError::ResourceNotFound(format!(
                "entity {id} does not match the type selector"
            )));
        }
        merge_into(doc, &fragment, &ts);
        Ok::<(), NgsiError>(())
    })?;
    match res {
        None => Err(NgsiError::ResourceNotFound(format!("entity {id} not found")).into()),
        Some(Err(e)) => Err(e.into()),
        Some(Ok(())) => Ok(no_content(&tenant)),
    }
}

/// JSON Merge-Patch over internal docs (5.5.12).
pub fn merge_into(doc: &mut Value, fragment: &Value, ts: &str) {
    let (Some(target), Some(frag)) = (doc.as_object_mut(), fragment.as_object()) else {
        return;
    };
    for (k, v) in frag {
        match k.as_str() {
            "id" | "createdAt" | "modifiedAt" => continue,
            "type" => {
                // union of types
                let mut cur: Vec<Value> = target
                    .get("type")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                for t in v.as_array().cloned().unwrap_or_default() {
                    if !cur.contains(&t) {
                        cur.push(t);
                    }
                }
                target.insert("type".into(), Value::Array(cur));
            }
            "scope" => {
                target.insert("scope".into(), v.clone());
            }
            // 4.22: expiresAt is a settable Entity member (5.2.4, not in the
            // read-only Table 5.2.2-1) — merge updates the storage expiry;
            // an NGSI-LD Null removes it (5.5.12). Without this arm it fell
            // through to the attribute path, where a bare string has no
            // instances and the member was silently dropped.
            "expiresAt" => {
                if is_ngsi_null(v) {
                    target.remove("expiresAt");
                } else {
                    target.insert("expiresAt".into(), v.clone());
                }
            }
            _ => {
                let frag_instances = v.as_array().cloned().unwrap_or_default();
                let mut cur: Vec<Value> = target
                    .get(k)
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                for fi in frag_instances {
                    let is_delete = antares_jsonld::is_deletion_instance(&fi);
                    let want_ds = fi.get("datasetId").and_then(Value::as_str);
                    let pos = cur
                        .iter()
                        .position(|ci| ci.get("datasetId").and_then(Value::as_str) == want_ds);
                    match (is_delete, pos) {
                        (true, Some(p)) => {
                            cur.remove(p);
                        }
                        (true, None) => {}
                        (false, Some(p)) => {
                            merge_instance(&mut cur[p], &fi, ts);
                        }
                        (false, None) => {
                            let mut ni = fi.clone();
                            if let Some(o) = ni.as_object_mut() {
                                o.insert("createdAt".into(), Value::String(ts.to_owned()));
                                o.insert("modifiedAt".into(), Value::String(ts.to_owned()));
                            }
                            cur.push(ni);
                        }
                    }
                }
                if cur.is_empty() {
                    target.remove(k);
                } else {
                    target.insert(k.clone(), Value::Array(cur));
                }
            }
        }
    }
    target.insert("modifiedAt".into(), Value::String(ts.to_owned()));
}

fn merge_instance(target: &mut Value, frag: &Value, ts: &str) {
    let (Some(t), Some(f)) = (target.as_object_mut(), frag.as_object()) else {
        return;
    };
    for (k, v) in f {
        if k == "createdAt" || k == "modifiedAt" {
            continue;
        }
        if v.is_null() || is_ngsi_null(v) {
            t.remove(k);
        } else if let (Some(cur), Some(patch)) =
            (t.get_mut(k).and_then(Value::as_object_mut), v.as_object())
        {
            // 5.5.12: the merge goes "into JSON objects representing a
            // Property value" — RFC 7396 with the NGSI-LD Null as removal.
            merge_value_object(cur, patch);
        } else {
            t.insert(k.clone(), v.clone());
        }
    }
    t.insert("modifiedAt".into(), Value::String(ts.to_owned()));
}

/// RFC 7396 merge patch over a compound (JSON object) member value, with
/// "urn:ngsi-ld:null" / JSON null as the key-removal marker (5.5.12); the
/// sentinel itself is never stored (5.5.4).
fn merge_value_object(target: &mut Map<String, Value>, patch: &Map<String, Value>) {
    for (k, v) in patch {
        if v.is_null() || is_ngsi_null(v) {
            target.remove(k);
        } else if let (Some(cur), Some(po)) = (
            target.get_mut(k).and_then(Value::as_object_mut),
            v.as_object(),
        ) {
            merge_value_object(cur, po);
        } else {
            target.insert(k.clone(), v.clone());
        }
    }
}

// ---------- PUT /entities/{id} — Replace (5.6.18) ----------

pub async fn replace_entity(
    State(st): State<AppState>,
    Path(id): Path<String>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        antares_model::EntityId::new(&id)?;
        check_params(&params, &["local", "type"])?;
        let ctx0 = st.loader.core();
        // 5.6.18.4: the ?type selector narrows the target — a non-matching
        // entity is "not known" for this replace.
        let local_doc = st
            .store
            .get(&tenant, Kind::Entity, &id)?
            .filter(|d| crate::attrs::matches_type_param(d, &params, &ctx0));
        let spec = crate::csource::CsrSpec {
            ids: Some(vec![id.clone()]),
            ..Default::default()
        };
        let mut regs = crate::federation::write_regs(&st, &tenant, &spec, &ctx0, &params, &headers);
        if let Some(r) = crate::federation::handle_via_loop(
            &headers,
            &crate::federation::alias_for(&st.host_alias, &tenant),
            &tenant,
            &mut regs,
        ) {
            return Ok(r);
        }
        if regs.is_empty() {
            // 5.6.18: an unknown target is 404 before body validation (057_03)
            let old = local_doc
                .ok_or_else(|| NgsiError::ResourceNotFound(format!("entity {id} not found")))?;
            let parsed = parse_body(&st.loader, &headers, &body, BodyKind::Standard).await?;
            let obj = parsed
                .value
                .as_object()
                .ok_or_else(|| NgsiError::BadRequestData("entity must be a JSON object".into()))?;
            let mut expanded = expand_entity(obj, &parsed.ctx, ExpandOpts::default())?;
            if expanded["id"].as_str() != Some(id.as_str()) {
                return Err(NgsiError::BadRequestData("entity id mismatch".into()).into());
            }
            let ts = now_iso();
            stamp_new(&mut expanded, &ts);
            if let (Some(o), Some(created)) = (expanded.as_object_mut(), old.get("createdAt")) {
                o.insert("createdAt".into(), created.clone());
            }
            st.store
                .upsert(&tenant, Kind::Entity, &id, expanded.clone())?;
            return Ok::<_, ApiError>(no_content(&tenant));
        }
        let parsed = parse_body(&st.loader, &headers, &body, BodyKind::Standard).await?;
        let obj = parsed
            .value
            .as_object()
            .ok_or_else(|| NgsiError::BadRequestData("entity must be a JSON object".into()))?;
        let mut expanded = expand_entity(obj, &parsed.ctx, ExpandOpts::default())?;
        if expanded["id"].as_str() != Some(id.as_str()) {
            return Err(NgsiError::BadRequestData("entity id mismatch".into()).into());
        }
        let mut parts = Vec::new();
        let proxies: Vec<&crate::federation::FedReg> =
            regs.iter().filter(|r| r.is_proxy()).collect();
        let proxy_match = !proxies.is_empty();
        if local_doc.is_some() || !proxy_match {
            match &local_doc {
                Some(old) => {
                    let (rest, _) = crate::federation::strip_proxied(obj, &proxies, &parsed.ctx);
                    let mut local_exp = expand_entity(&rest, &parsed.ctx, ExpandOpts::default())?;
                    let ts = now_iso();
                    stamp_new(&mut local_exp, &ts);
                    if let (Some(o), Some(created)) =
                        (local_exp.as_object_mut(), old.get("createdAt"))
                    {
                        o.insert("createdAt".into(), created.clone());
                    }
                    st.store
                        .upsert(&tenant, Kind::Entity, &id, local_exp.clone())?;
                    parts.push(crate::federation::Part {
                        status: 204,
                        detail: "replaced locally".into(),
                    });
                }
                None => parts.push(crate::federation::Part {
                    status: 404,
                    detail: format!("entity {id} not found locally"),
                }),
            }
        }
        let ctx_url = crate::federation::ctx_link_url(&headers, &parsed.ctx.source);
        for reg in &regs {
            // 5.6.18.4: proxy modes without Replace Entity support are an
            // error of type Conflict; inclusive ones are not forwarded.
            if !reg.supports("replaceEntity") {
                if reg.is_proxy() {
                    parts.push(crate::federation::conflict_part("replaceEntity"));
                }
                continue;
            }
            let Some(frag) = crate::federation::reduce_to_scope(obj, reg, &parsed.ctx) else {
                continue;
            };
            parts.push(
                crate::federation::forward_part(
                    &st,
                    reqwest::Method::PUT,
                    format!("{}/ngsi-ld/v1/entities/{id}", reg.endpoint),
                    &[],
                    &headers,
                    &tenant,
                    reg,
                    &ctx_url,
                    Some(frag),
                )
                .await,
            );
        }
        let _ = expanded.take();
        Ok(crate::federation::combine(
            parts,
            no_content(&tenant),
            &tenant,
        ))
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

// ---------- Entity Ordering (4.23) ----------

/// Sort by an orderBy spec: comma-separated `member[;asc|desc]`.
/// 4.23 Entity Ordering: orderBy = AttrName[;direction] *(, …) with asc
/// (default) / desc / dist-asc / dist-desc (4.23.3); distance keys need the
/// orderFrom reference coordinates (orderGeometry, default Point) and apply
/// to GeoProperties — non-GeoProperties fall back to value order after them
/// (4.23.2). Mixed datatypes rank Numbers < Strings < Object < Array <
/// Boolean < Time < Date < DateTime < Null < absent (4.23.2). Paths may be
/// dotted (EXAMPLE 5) or carry one trailing [member.path] bracket
/// (EXAMPLE 4). String comparison is codepoint order by default; the
/// `collation` parameter selects an ICU collation (4.23.3 EXAMPLES 6/7).
///
/// 4.23.3 EXAMPLES 6/7: the ICU collator for an RFC 6067 collation tag
/// (e.g. und-u-ks-identic, de-u-co-phonebk). The co/kf/kn keywords travel
/// via CollatorPreferences; the -u-ks strength keyword maps onto
/// CollatorOptions. Invalid/unsupported tags are BadRequestData.
fn build_collator(tag: &str) -> Result<icu_collator::CollatorBorrowed<'static>, NgsiError> {
    let bad = |m: String| NgsiError::BadRequestData(m);
    let locale: icu_locale_core::Locale = tag.parse().map_err(|_| {
        bad(format!(
            "collation is not an RFC 6067 tag: {tag:?} (4.23.3)"
        ))
    })?;
    let mut opts = icu_collator::options::CollatorOptions::default();
    use icu_collator::options::Strength;
    use icu_locale_core::extensions::unicode::key;
    if let Some(ks) = locale.extensions.unicode.keywords.get(&key!("ks")) {
        opts.strength = Some(match ks.to_string().as_str() {
            "level1" => Strength::Primary,
            "level2" => Strength::Secondary,
            "level3" => Strength::Tertiary,
            "level4" => Strength::Quaternary,
            "identic" => Strength::Identical,
            other => {
                return Err(bad(format!(
                    "unknown collation strength {other:?} (4.23.3)"
                )))
            }
        });
    }
    icu_collator::Collator::try_new((&locale).into(), opts)
        .map_err(|e| bad(format!("unsupported collation {tag:?}: {e} (4.23.3)")))
}

pub fn order_entities(
    docs: &mut [Value],
    spec: &str,
    params: &HashMap<String, String>,
    ctx: &antares_jsonld::Context,
) -> Result<(), NgsiError> {
    #[derive(PartialEq)]
    enum Dir {
        Asc,
        Desc,
        DistAsc,
        DistDesc,
    }
    struct Key {
        path: Vec<String>,
        bracket: Option<Vec<String>>,
        dir: Dir,
    }
    let bad = |m: String| NgsiError::BadRequestData(m);
    let mut keys = Vec::new();
    for part in spec.split(',') {
        let part = part.trim();
        let (member, dir) = match part.split_once(';') {
            Some((m, d)) => (m.trim(), d.trim()),
            None => (part, "asc"),
        };
        let dir = match dir {
            "asc" => Dir::Asc,
            "desc" => Dir::Desc,
            "dist-asc" => Dir::DistAsc,
            "dist-desc" => Dir::DistDesc,
            _ => {
                return Err(bad(format!(
                    "invalid orderBy direction in {spec:?} (4.23.3)"
                )))
            }
        };
        // one trailing [member.path] bracket (EXAMPLE 4)
        let (head, bracket) = match member.split_once('[') {
            Some((h, rest)) => {
                let inner = rest
                    .strip_suffix(']')
                    .ok_or_else(|| bad(format!("unclosed bracket in orderBy {spec:?}")))?;
                (h, Some(inner.split('.').map(str::to_owned).collect()))
            }
            None => (member, None),
        };
        if head.is_empty() {
            return Err(bad(format!("invalid orderBy {spec:?} (4.23)")));
        }
        keys.push(Key {
            path: head.split('.').map(str::to_owned).collect(),
            bracket,
            dir,
        });
    }
    // 4.23.3 EXAMPLES 6/7: collation names an ICU ordering for strings
    let collator = params
        .get("collation")
        .map(|t| build_collator(t))
        .transpose()?;
    // dist-* keys need the orderFrom reference geometry (4.23.3 EXAMPLE 8-10)
    let refg = if keys
        .iter()
        .any(|k| matches!(k.dir, Dir::DistAsc | Dir::DistDesc))
    {
        let coords_raw = params
            .get("orderFrom")
            .ok_or_else(|| bad("dist ordering requires orderFrom (4.23.3)".into()))?;
        let coords: Value = serde_json::from_str(coords_raw)
            .map_err(|_| bad(format!("invalid orderFrom {coords_raw:?}")))?;
        let gtype = params
            .get("orderGeometry")
            .cloned()
            .unwrap_or_else(|| "Point".into());
        Some(crate::geo::parse_ref_geometry(&gtype, &coords).map_err(bad)?)
    } else {
        None
    };
    fn order_value(doc: &Value, k: &Key, ctx: &antares_jsonld::Context) -> Option<Value> {
        let path = &k.path;
        let head = path.first()?;
        let base = match head.as_str() {
            "id" | "createdAt" | "modifiedAt" => doc.get(head.as_str()).cloned(),
            "type" => doc["type"].as_array().and_then(|a| a.first()).cloned(),
            _ => {
                let iri = ctx.expand_key(head);
                let inst = doc.get(&iri).and_then(Value::as_array)?.first()?;
                let mut cur = inst;
                for seg in &path[1..] {
                    match seg.as_str() {
                        "createdAt" | "modifiedAt" | "observedAt" | "datasetId" | "unitCode" => {
                            cur = cur.get(seg.as_str())?;
                        }
                        _ => {
                            let siri = ctx.expand_key(seg);
                            cur = cur
                                .get(&siri)
                                .and_then(Value::as_array)
                                .and_then(|a| a.first())?;
                        }
                    }
                }
                match cur.get("value").or_else(|| cur.get("object")) {
                    Some(v) => Some(v.clone()),
                    None => Some(cur.clone()),
                }
            }
        }?;
        match &k.bracket {
            None => Some(base),
            Some(b) => {
                let mut cur = &base;
                for seg in b {
                    cur = cur.get(seg)?;
                }
                Some(cur.clone())
            }
        }
    }
    /// 4.23.2 datatype rank: Numbers < Strings < Object < Array < Boolean <
    /// Time < Date < DateTime < Null (absent is handled as Option::None).
    fn rank(v: &Value) -> u8 {
        match v {
            Value::Number(_) => 0,
            Value::String(s) => {
                if antares_jsonld::parse_datetime(s) {
                    7
                } else if is_date(s) {
                    6
                } else if is_time(s) {
                    5
                } else {
                    1
                }
            }
            Value::Object(_) => 2,
            Value::Array(_) => 3,
            Value::Bool(_) => 4,
            Value::Null => 8,
        }
    }
    /// 4.6.3 Date: YYYY-MM-DD, all components present.
    fn is_date(s: &str) -> bool {
        let b = s.as_bytes();
        b.len() == 10
            && b[4] == b'-'
            && b[7] == b'-'
            && b.iter()
                .enumerate()
                .all(|(i, c)| matches!(i, 4 | 7) || c.is_ascii_digit())
    }
    /// 4.6.3 Time: hh:mm:ss[.f*]Z.
    fn is_time(s: &str) -> bool {
        let b = s.as_bytes();
        b.len() >= 9
            && b[b.len() - 1] == b'Z'
            && b[2] == b':'
            && b[5] == b':'
            && b[..2].iter().all(u8::is_ascii_digit)
            && b[3..5].iter().all(u8::is_ascii_digit)
            && b[6..8].iter().all(u8::is_ascii_digit)
    }
    fn cmp_vals(
        a: &Option<Value>,
        b: &Option<Value>,
        coll: Option<&icu_collator::CollatorBorrowed<'static>>,
    ) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        match (a, b) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Greater, // absent sorts last (4.23.2)
            (Some(_), None) => Ordering::Less,
            (Some(x), Some(y)) => {
                let (rx, ry) = (rank(x), rank(y));
                if rx != ry {
                    return rx.cmp(&ry);
                }
                match (x, y) {
                    (Value::Number(_), Value::Number(_)) => x
                        .as_f64()
                        .unwrap_or(f64::NAN)
                        .total_cmp(&y.as_f64().unwrap_or(f64::NAN)),
                    (Value::Bool(bx), Value::Bool(by)) => bx.cmp(by),
                    (Value::String(sx), Value::String(sy)) => {
                        if rx == 7 {
                            // DateTime: canonical key so equal instants in
                            // different 4.6.3 fraction spellings tie (4.11)
                            crate::temporal::dt_key(sx).cmp(&crate::temporal::dt_key(sy))
                        } else if let Some(c) = coll {
                            // 4.23.3 EXAMPLES 6/7: the named ICU collation
                            c.compare(sx, sy)
                        } else {
                            // 4.23.1 default: codepoint order
                            sx.cmp(sy)
                        }
                    }
                    _ => x.to_string().cmp(&y.to_string()),
                }
            }
        }
    }
    docs.sort_by(|a, b| {
        use std::cmp::Ordering;
        for k in &keys {
            let o = match k.dir {
                Dir::Asc | Dir::Desc => {
                    let va = order_value(a, k, ctx);
                    let vb = order_value(b, k, ctx);
                    let mut o = cmp_vals(&va, &vb, collator.as_ref());
                    if k.dir == Dir::Desc {
                        o = o.reverse();
                    }
                    o
                }
                Dir::DistAsc | Dir::DistDesc => {
                    let refg = refg.as_ref().expect("checked above");
                    let da =
                        order_value(a, k, ctx).and_then(|v| crate::geo::order_distance_m(refg, &v));
                    let db =
                        order_value(b, k, ctx).and_then(|v| crate::geo::order_distance_m(refg, &v));
                    match (da, db) {
                        (Some(x), Some(y)) => {
                            let mut o = x.total_cmp(&y);
                            if k.dir == Dir::DistDesc {
                                o = o.reverse();
                            }
                            o
                        }
                        // 4.23.2 distance order: GeoProperties (by distance)
                        // rank before non-GeoProperties (by value)
                        (Some(_), None) => Ordering::Less,
                        (None, Some(_)) => Ordering::Greater,
                        (None, None) => {
                            let va = order_value(a, k, ctx);
                            let vb = order_value(b, k, ctx);
                            cmp_vals(&va, &vb, collator.as_ref())
                        }
                    }
                }
            };
            if o != Ordering::Equal {
                return o;
            }
        }
        Ordering::Equal
    });
    Ok(())
}

// ---------- GeoJSON output (6.3.15) ----------

/// 4.5.16.2 GeoJSON Feature, members per Table 5.2.29-1 (5.2.29 Feature):
/// id = entity id (URI), fixed type "Feature", geometry = the selected
/// GeoProperty's value or null (4.5.16.1: geometryProperty parameter,
/// default "location"), properties = the 5.2.31 FeatureProperties (entity
/// type + attributes). The @context member is added by respond() (6.3.6).
pub fn to_geojson_feature(
    entity: Value,
    geometry_property: Option<&String>,
    ctx: &antares_jsonld::Context,
) -> Value {
    let geom_term = geometry_property
        .cloned()
        .unwrap_or_else(|| "location".into());
    let _ = ctx;
    let geometry = entity
        .get(&geom_term)
        .map(geo_value_of)
        .unwrap_or(Value::Null);
    let id = entity.get("id").cloned().unwrap_or(Value::Null);
    let mut props = entity.as_object().cloned().unwrap_or_default();
    props.remove("id");
    let mut feature = Map::new();
    feature.insert("id".into(), id);
    feature.insert("type".into(), Value::String("Feature".into()));
    feature.insert("geometry".into(), geometry);
    feature.insert("properties".into(), Value::Object(props));
    Value::Object(feature)
}

/// 4.5.16.1: with multiple instances the default one (no datasetId) is
/// selected unless a datasetId filter already narrowed the set to one; a
/// missing GeoProperty or a value that "does not hold a valid GeoJSON
/// geometry object" yields null — "which is syntactically valid GeoJSON".
fn geo_value_of(attr: &Value) -> Value {
    let inst = match attr {
        Value::Array(a) => match a.iter().find(|i| i.get("datasetId").is_none()) {
            Some(default) => default.clone(),
            None if a.len() == 1 => a[0].clone(),
            None => return Value::Null,
        },
        other => other.clone(),
    };
    let v = inst.get("value").cloned().unwrap_or(inst);
    // 4.5.17.1: in the simplified representation a multi-instance GeoProperty
    // is the {"dataset": {…}} map — the default ("@none") instance is the
    // 4.5.16.1 selection.
    let v = match v.as_object() {
        Some(o) if o.len() == 1 && o.contains_key("dataset") => {
            o["dataset"].get("@none").cloned().unwrap_or(Value::Null)
        }
        _ => v,
    };
    match antares_jsonld::expand::validate_geojson("geometry", &v) {
        Ok(()) => v,
        Err(_) => Value::Null,
    }
}

/// 4.5.16.3 GeoJSON FeatureCollection, members per Table 5.2.30-1 (5.2.30
/// FeatureCollection): fixed type "FeatureCollection" + features array of
/// 4.5.16.2 Feature objects — empty array when no matches, no per-Feature
/// @context; the top-level @context is added by respond() (6.3.6).
pub fn to_geojson_collection(
    entities: Vec<Value>,
    geometry_property: Option<&String>,
    ctx: &antares_jsonld::Context,
) -> Value {
    let features: Vec<Value> = entities
        .into_iter()
        .map(|e| to_geojson_feature(e, geometry_property, ctx))
        .collect();
    serde_json::json!({"type": "FeatureCollection", "features": features})
}

// ---------- GET /entities/{id}/attrs/{attrId} [+ /value] ----------
// NGSI-LD 2.0 pre-adoptions #14/#15: retrieve a single
// attribute of an entity, and its bare value. Additive-only: 2.0 defines the
// resources, 1.9.1 clients never see them unless asked.

pub async fn retrieve_entity_attr(
    State(st): State<AppState>,
    Path((id, attr)): Path<(String, String)>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
) -> Response {
    match retrieve_attr_inner(&st, &id, &attr, false, &params, &headers).await {
        Ok(r) => r,
        Err(e) => e.into_response(),
    }
}

pub async fn retrieve_entity_attr_value(
    State(st): State<AppState>,
    Path((id, attr)): Path<(String, String)>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
) -> Response {
    match retrieve_attr_inner(&st, &id, &attr, true, &params, &headers).await {
        Ok(r) => r,
        Err(e) => e.into_response(),
    }
}

async fn retrieve_attr_inner(
    st: &AppState,
    id: &str,
    attr: &str,
    value_only: bool,
    params: &HashMap<String, String>,
    headers: &HeaderMap,
) -> ApiResult<Response> {
    let tenant = tenant_from(headers)?;
    check_params(params, &["options", "format", "lang", "datasetId", "local"])?;
    let ctx = request_context(&st.loader, headers).await?;
    let repr = parse_repr(params, &ctx)?;
    antares_model::EntityId::new(id)?;
    crate::attrs::check_attr_name(attr)?;
    let doc = st
        .store
        .get(&tenant, Kind::Entity, id)?
        .ok_or_else(|| NgsiError::ResourceNotFound(format!("entity {id} not found")))?;
    let attr_iri = ctx.expand_key(attr);
    let node = doc.get(&attr_iri).ok_or_else(|| {
        NgsiError::ResourceNotFound(format!("entity {id} has no attribute {attr}"))
    })?;
    // Compact through the entity pipeline so the attribute serializes exactly
    // as it would inside a full retrieve.
    let mini = serde_json::json!({
        "id": doc.get("id").cloned().unwrap_or_default(),
        "type": doc.get("type").cloned().unwrap_or_default(),
        attr_iri.clone(): node.clone(),
    });
    let shaped = crate::repr::apply(&mini, &repr);
    let compacted = compact_for(&repr, &shaped, &ctx);
    let key = ctx.compact_iri(&attr_iri);
    let member = compacted
        .get(&key)
        .cloned()
        .ok_or_else(|| NgsiError::ResourceNotFound(format!("attribute {attr} not present")))?;
    let body = if value_only {
        // #15: the bare value — value / object / languageMap, whichever the
        // attribute type carries; multi-instance attributes yield an array.
        fn bare(v: &Value) -> Value {
            match v {
                Value::Array(a) => Value::Array(a.iter().map(bare).collect()),
                Value::Object(o) => o
                    .get("value")
                    .or_else(|| o.get("object"))
                    .or_else(|| o.get("languageMap"))
                    .or_else(|| o.get("json"))
                    .or_else(|| o.get("vocab"))
                    .or_else(|| o.get("valueList"))
                    .or_else(|| o.get("objectList"))
                    .cloned()
                    .unwrap_or(Value::Null),
                other => other.clone(),
            }
        }
        bare(&member)
    } else {
        member
    };
    let accept = parse_accept(headers)?;
    Ok(respond(StatusCode::OK, body, &ctx, accept, &tenant))
}

#[cfg(test)]
mod tests {
    use super::merge_into;
    use serde_json::json;

    /// 5.5.12 EXAMPLE 1 + the datasetId/type bullets: a merge updates the
    /// named sub-attributes and leaves the others untouched; a fragment
    /// instance with an unknown datasetId is ADDED (not replacing the
    /// default); entity types are unioned.
    #[test]
    fn clause_5_5_12_merge_algorithm() {
        let mut doc = json!({"id": "urn:x", "type": ["https://uri.etsi.org/ngsi-ld/default-context/T"],
            "https://uri.etsi.org/ngsi-ld/default-context/temperature": [{
                "type": "Property", "value": 25, "unitCode": "CEL",
                "observedAt": "2022-03-14T01:59:26.535Z"}]});
        merge_into(
            &mut doc,
            &json!({
            "type": ["https://uri.etsi.org/ngsi-ld/default-context/T",
                     "https://uri.etsi.org/ngsi-ld/default-context/U"],
            "https://uri.etsi.org/ngsi-ld/default-context/temperature": [
                {"type": "Property", "value": 100,
                 "observedAt": "2022-03-14T13:00:00.000Z"},
                {"type": "Property", "value": 7,
                 "datasetId": "urn:ngsi-ld:Dataset:extra"}
            ]}),
            "2026-08-11T00:00:00Z",
        );
        // EXAMPLE 1: value/observedAt updated, unitCode untouched
        let t = &doc["https://uri.etsi.org/ngsi-ld/default-context/temperature"];
        let default = t
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i.get("datasetId").is_none())
            .expect("default instance");
        assert_eq!(default["value"], 100);
        assert_eq!(default["observedAt"], "2022-03-14T13:00:00.000Z");
        assert_eq!(
            default["unitCode"], "CEL",
            "unmentioned sub-attribute survives"
        );
        // unknown datasetId is added as a NEW instance
        assert_eq!(t.as_array().unwrap().len(), 2);
        // entity types are unioned, no duplicates
        assert_eq!(
            doc["type"],
            json!([
                "https://uri.etsi.org/ngsi-ld/default-context/T",
                "https://uri.etsi.org/ngsi-ld/default-context/U"
            ])
        );
    }

    /// 5.5.12: merge "merges the provided information with the existing
    /// information up to an arbitrary depth, e.g. including going into JSON
    /// objects representing a Property value" (RFC 7396 with the NGSI-LD
    /// Null) — untouched keys survive, null-valued keys are removed, and the
    /// null sentinel never lands in the stored document (5.5.4).
    #[test]
    fn merge_goes_into_compound_property_values() {
        let mut doc = json!({"id": "urn:x", "type": ["T"],
            "https://uri.etsi.org/ngsi-ld/default-context/address": [{
                "type": "Property",
                "value": {"street": "Straße des 17. Juni", "city": "Berlin",
                          "country": "Germany"}}]});
        merge_into(
            &mut doc,
            &json!({"https://uri.etsi.org/ngsi-ld/default-context/address": [{
                "type": "Property",
                "value": {"street": "Pariser Platz",
                          "country": "urn:ngsi-ld:null"}}]}),
            "2026-08-11T00:00:00Z",
        );
        let v = &doc["https://uri.etsi.org/ngsi-ld/default-context/address"][0]["value"];
        assert_eq!(v["street"], "Pariser Platz");
        assert_eq!(v["city"], "Berlin", "untouched keys survive the merge");
        assert!(v.get("country").is_none(), "null removes the key");
        assert!(
            !doc.to_string().contains("urn:ngsi-ld:null"),
            "the null sentinel must never be stored"
        );
    }

    #[test]
    fn merge_sets_and_null_removes_expires_at() {
        let mut doc = json!({"id": "urn:x", "type": ["T"]});
        merge_into(
            &mut doc,
            &json!({"expiresAt": "2030-01-01T00:00:00Z"}),
            "2026-08-08T00:00:00Z",
        );
        assert_eq!(doc["expiresAt"], "2030-01-01T00:00:00Z");
        merge_into(
            &mut doc,
            &json!({"expiresAt": "urn:ngsi-ld:null"}),
            "2026-08-08T00:00:01Z",
        );
        assert!(doc.get("expiresAt").is_none(), "NGSI-LD Null removes it");
    }

    /// 4.5.16.1/4.5.16.2/4.5.16.3: geometry selection (default instance,
    /// datasetId-narrowed single, invalid value -> null) and the
    /// Feature/FeatureCollection shapes.
    #[test]
    fn geojson_feature_selection_and_shape() {
        use super::{to_geojson_collection, to_geojson_feature};
        use serde_json::Value;
        let ctx = antares_jsonld::Loader::new().core();
        let entity = json!({
            "id": "urn:ngsi-ld:V:1", "type": "Vehicle",
            "location": [
                {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [9.0, 9.0]},
                 "datasetId": "urn:ngsi-ld:Dataset:gps"},
                {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]}}
            ],
            "speed": {"type": "Property", "value": 5}
        });
        let f = to_geojson_feature(entity.clone(), None, &ctx);
        assert_eq!(f["type"], "Feature");
        assert_eq!(f["id"], "urn:ngsi-ld:V:1");
        // default instance (no datasetId) wins over the first array element
        assert_eq!(
            f["geometry"],
            json!({"type": "Point", "coordinates": [1.0, 2.0]})
        );
        assert_eq!(f["properties"]["type"], "Vehicle");
        assert!(
            f["properties"].get("id").is_none(),
            "id only at Feature level"
        );
        assert!(f["properties"].get("speed").is_some());

        // geometryProperty naming a non-geometry Property -> null geometry
        let f2 = to_geojson_feature(entity.clone(), Some(&"speed".to_string()), &ctx);
        assert_eq!(f2["geometry"], Value::Null);
        // absent GeoProperty -> null geometry
        let f3 = to_geojson_feature(entity.clone(), Some(&"missing".to_string()), &ctx);
        assert_eq!(f3["geometry"], Value::Null);

        // 4.5.17.1: simplified multi-instance GeoProperty = dataset map;
        // the "@none" (default) entry is the geometry
        let simplified = json!({
            "id": "urn:ngsi-ld:V:2", "type": "Vehicle",
            "location": {"dataset": {
                "urn:ngsi-ld:Dataset:gps": {"type": "Point", "coordinates": [9.0, 9.0]},
                "@none": {"type": "Point", "coordinates": [3.0, 4.0]}
            }},
            "speed": 5
        });
        let fs = to_geojson_feature(simplified, None, &ctx);
        assert_eq!(
            fs["geometry"],
            json!({"type": "Point", "coordinates": [3.0, 4.0]})
        );
        assert_eq!(fs["properties"]["speed"], 5);

        let fc = to_geojson_collection(vec![entity], None, &ctx);
        assert_eq!(fc["type"], "FeatureCollection");
        assert_eq!(fc["features"].as_array().map(Vec::len), Some(1));
        assert!(
            fc["features"][0].get("@context").is_none(),
            "no per-Feature @context"
        );
        // Table 5.2.30-1: "In the case that no matches are found, features
        // will be an empty array"
        let empty = to_geojson_collection(vec![], None, &ctx);
        assert_eq!(empty["type"], "FeatureCollection");
        assert_eq!(empty["features"], json!([]));
    }
}

#[cfg(test)]
mod clause_4_12 {
    use super::*;
    use serde_json::json;

    fn state() -> AppState {
        AppState::new("http://localhost:9090".into())
    }

    fn params(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    fn items(n: usize) -> Vec<Value> {
        (0..n).map(|i| json!({"id": format!("urn:{i}")})).collect()
    }

    /// 4.12: clients specify a limit (page size); a next link flags remaining
    /// elements; prev enables backwards iteration; absent on the edges.
    #[test]
    fn next_and_prev_flag_remaining_elements() {
        let st = state();
        let (page, _, links) = paginate(&st, &params(&[("limit", "1")]), items(3), "/e").unwrap();
        assert_eq!(page.len(), 1);
        assert!(links.iter().any(|l| l.contains("rel=\"next\"")));
        assert!(
            !links.iter().any(|l| l.contains("rel=\"prev\"")),
            "no prev on the first page"
        );
        let (_, _, links) = paginate(
            &st,
            &params(&[("limit", "1"), ("offset", "1")]),
            items(3),
            "/e",
        )
        .unwrap();
        assert!(links.iter().any(|l| l.contains("rel=\"next\"")));
        assert!(links.iter().any(|l| l.contains("rel=\"prev\"")));
        let (_, _, links) = paginate(
            &st,
            &params(&[("limit", "1"), ("offset", "2")]),
            items(3),
            "/e",
        )
        .unwrap();
        assert!(
            !links.iter().any(|l| l.contains("rel=\"next\"")),
            "no next on the last page"
        );
        assert!(links.iter().any(|l| l.contains("rel=\"prev\"")));
    }

    /// 4.12: "define a default limit (default page size)" — applied when the
    /// client sends none.
    #[test]
    fn default_page_size_applies() {
        let st = state();
        let n = st.default_limit + 5;
        let (page, _, links) = paginate(&st, &params(&[]), items(n), "/e").unwrap();
        assert_eq!(page.len(), st.default_limit);
        assert!(links.iter().any(|l| l.contains("rel=\"next\"")));
    }

    /// 4.12 should: a hard result-size ceiling, rejected with TooManyResults
    /// (not silently clamped).
    #[test]
    fn limit_above_the_ceiling_is_too_many_results() {
        let st = state();
        let over = (st.max_limit + 1).to_string();
        let err = paginate(&st, &params(&[("limit", &over)]), items(1), "/e").unwrap_err();
        assert!(format!("{err:?}").contains("TooManyResults"));
    }
}

#[cfg(test)]
mod clause_4_13 {
    use super::*;
    use serde_json::json;

    fn params(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    /// 4.13: the result count is relayed "whenever this is requested by the
    /// client" — and only then.
    #[test]
    fn count_is_returned_only_on_request() {
        let st = AppState::new("http://localhost:9090".into());
        let items: Vec<Value> = (0..3).map(|i| json!({"id": format!("urn:{i}")})).collect();
        let (_, count, _) =
            paginate(&st, &params(&[("count", "true")]), items.clone(), "/e").unwrap();
        assert_eq!(count, Some(3));
        let (_, count, _) = paginate(&st, &params(&[]), items, "/e").unwrap();
        assert_eq!(count, None, "no count member unless requested");
    }

    /// 4.13: "a client can issue a query that limits to zero the number of
    /// desired results but asks for the count to be present" — limit=0 is
    /// only valid together with count.
    #[test]
    fn limit_zero_with_count_yields_an_empty_page_and_the_total() {
        let st = AppState::new("http://localhost:9090".into());
        let items: Vec<Value> = (0..7).map(|i| json!({"id": format!("urn:{i}")})).collect();
        let (page, count, links) = paginate(
            &st,
            &params(&[("limit", "0"), ("count", "true")]),
            items.clone(),
            "/e",
        )
        .unwrap();
        assert!(page.is_empty());
        assert_eq!(count, Some(7));
        assert!(links.is_empty(), "limit=0 pages have no next/prev");
        assert!(
            paginate(&st, &params(&[("limit", "0")]), items, "/e").is_err(),
            "limit=0 without count is rejected"
        );
    }
}

#[cfg(test)]
mod clause_4_16 {
    use super::*;
    use serde_json::json;

    /// 4.16: "Entity Types can be implicitly added by all operations that
    /// update or append attributes. There is no operation to remove Entity
    /// Types from an Entity."
    #[test]
    fn merge_unions_types_and_never_removes() {
        let mut doc = json!({"id": "urn:x", "type": ["A"],
            "https://ex/p": [{"type": "Property", "value": 1}]});
        merge_into(
            &mut doc,
            &json!({"type": ["B"]}),
            "2026-08-11T00:00:00.000Z",
        );
        assert_eq!(doc["type"], json!(["A", "B"]), "types union");
        // a fragment naming FEWER types must not shrink the set
        merge_into(
            &mut doc,
            &json!({"type": ["A"]}),
            "2026-08-11T00:00:00.000Z",
        );
        assert_eq!(
            doc["type"],
            json!(["A", "B"]),
            "no operation removes Entity Types"
        );
        // duplicates are not accumulated
        merge_into(
            &mut doc,
            &json!({"type": ["B"]}),
            "2026-08-11T00:00:00.000Z",
        );
        assert_eq!(doc["type"], json!(["A", "B"]));
    }
}

#[cfg(test)]
mod clause_4_17 {
    use super::*;
    use antares_jsonld::Loader;

    /// 4.17: disjunction via `|` or `,`, conjunction via `(a;b)`; short
    /// names expand against the @context.
    #[test]
    fn selection_language_semantics() {
        let ctx = Loader::new().core();
        const D: &str = "https://uri.etsi.org/ngsi-ld/default-context/";
        let home = format!("{D}Home");
        let vehicle = format!("{D}Vehicle");
        let motorhome = format!("{D}Motorhome");
        let both: Vec<&str> = vec![&home, &vehicle];
        let only_home: Vec<&str> = vec![&home];
        let only_motor: Vec<&str> = vec![&motorhome];
        // EXAMPLE 1: OR, both spellings
        assert!(type_selection_matches("Building|Home", &only_home, &ctx));
        assert!(type_selection_matches("Building,Home", &only_home, &ctx));
        assert!(!type_selection_matches("Building|House", &only_home, &ctx));
        // EXAMPLE 2: conjunction — ALL listed types required
        assert!(type_selection_matches("(Home;Vehicle)", &both, &ctx));
        assert!(
            !type_selection_matches("(Home;Vehicle)", &only_home, &ctx),
            "an entity with only Home must NOT match the conjunction"
        );
        // EXAMPLE 3: (Home;Vehicle)|Motorhome in both alternative spellings
        for sel in ["(Home;Vehicle)|Motorhome", "(Home;Vehicle),Motorhome"] {
            assert!(type_selection_matches(sel, &both, &ctx), "{sel}");
            assert!(type_selection_matches(sel, &only_motor, &ctx), "{sel}");
            assert!(!type_selection_matches(sel, &only_home, &ctx), "{sel}");
        }
    }
}

#[cfg(test)]
mod clause_4_23 {
    use super::*;
    use antares_jsonld::Loader;
    use serde_json::json;

    const D: &str = "https://uri.etsi.org/ngsi-ld/default-context/";

    fn ent(id: &str, attr: &str, v: Value) -> Value {
        json!({"id": id, "type": ["T"],
            format!("{D}{attr}"): [{"type": "Property", "value": v}]})
    }

    fn ids(docs: &[Value]) -> Vec<&str> {
        docs.iter().map(|d| d["id"].as_str().unwrap()).collect()
    }

    fn params(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    /// 4.23.2: mixed datatypes order as Numbers < Strings < Object < Array <
    /// Boolean < Time < Date < DateTime < Null < absent.
    #[test]
    fn datatype_comparison_order() {
        let ctx = Loader::new().core();
        let mut docs = vec![
            ent("urn:null", "x", Value::Null),
            ent("urn:datetime", "x", json!("2020-01-01T00:00:00Z")),
            ent("urn:bool", "x", json!(true)),
            json!({"id": "urn:absent", "type": ["T"]}),
            ent("urn:array", "x", json!([1, 2])),
            ent("urn:string", "x", json!("abc")),
            ent("urn:date", "x", json!("2020-01-01")),
            ent("urn:object", "x", json!({"k": 1})),
            ent("urn:number", "x", json!(5)),
            ent("urn:time", "x", json!("12:00:00Z")),
        ];
        order_entities(&mut docs, "x", &params(&[]), &ctx).expect("order");
        assert_eq!(
            ids(&docs),
            vec![
                "urn:number",
                "urn:string",
                "urn:object",
                "urn:array",
                "urn:bool",
                "urn:time",
                "urn:date",
                "urn:datetime",
                "urn:null",
                "urn:absent"
            ]
        );
    }

    /// 4.23.3 EXAMPLES 8/9: dist-asc / dist-desc rank by haversine distance
    /// from the orderFrom reference; a non-GeoProperty under a dist ordering
    /// falls back to value ordering after the geo-ranked ones (4.23.2).
    #[test]
    fn distance_ordering() {
        let ctx = Loader::new().core();
        let geo = |id: &str, lon: f64, lat: f64| {
            json!({"id": id, "type": ["T"],
                "https://uri.etsi.org/ngsi-ld/location": [
                    {"type": "GeoProperty",
                     "value": {"type": "Point", "coordinates": [lon, lat]}}]})
        };
        let mut docs = vec![
            geo("urn:far", 10.0, 45.0),
            geo("urn:near", 8.01, 40.01),
            geo("urn:mid", 9.0, 41.0),
        ];
        let p = params(&[("orderFrom", "[8,40]")]);
        order_entities(&mut docs, "location;dist-asc", &p, &ctx).expect("order");
        assert_eq!(ids(&docs), vec!["urn:near", "urn:mid", "urn:far"]);
        order_entities(&mut docs, "location;dist-desc", &p, &ctx).expect("order");
        assert_eq!(ids(&docs), vec!["urn:far", "urn:mid", "urn:near"]);
        // dist without orderFrom is a violation
        assert!(order_entities(&mut docs, "location;dist-asc", &params(&[]), &ctx).is_err());
        // non-GeoProperty entities sort after the geo-ranked ones
        let mut mixed = vec![
            ent("urn:plain", "location", json!("not-geo")),
            geo("urn:g", 8.0, 40.0),
        ];
        order_entities(&mut mixed, "location;dist-asc", &p, &ctx).expect("order");
        assert_eq!(ids(&mixed), vec!["urn:g", "urn:plain"]);
    }

    /// 4.23.3 EXAMPLE 4: a trailing [path] addresses a compound-value
    /// subitem; EXAMPLE 3: per-key directions apply sequentially.
    #[test]
    fn bracket_paths_and_sequential_keys() {
        let ctx = Loader::new().core();
        let addr = |id: &str, city: &str| ent(id, "address", json!({"city": city}));
        let mut docs = vec![addr("urn:b", "Berlin"), addr("urn:a", "Amsterdam")];
        order_entities(&mut docs, "address[city]", &params(&[]), &ctx).expect("order");
        assert_eq!(ids(&docs), vec!["urn:a", "urn:b"]);
        // name asc, then age desc among equals (EXAMPLE 3)
        let two = |id: &str, name: &str, age: i64| {
            json!({"id": id, "type": ["T"],
                format!("{D}name"): [{"type": "Property", "value": name}],
                format!("{D}age"): [{"type": "Property", "value": age}]})
        };
        let mut docs = vec![
            two("urn:x1", "same", 1),
            two("urn:x9", "same", 9),
            two("urn:a", "aaa", 5),
        ];
        order_entities(&mut docs, "name,age;desc", &params(&[]), &ctx).expect("order");
        assert_eq!(ids(&docs), vec!["urn:a", "urn:x9", "urn:x1"]);
    }
}

#[cfg(test)]
mod clause_5_2_2 {
    use super::*;
    use antares_jsonld::{ExpandOpts, Loader};
    use serde_json::json;

    /// 5.2.2: createdAt/modifiedAt/deletedAt "shall not be provided by
    /// Context Producers. In the event that they are provided ... NGSI-LD
    /// implementations shall ignore them" — server stamps win, no error.
    #[test]
    fn client_provided_system_timestamps_are_ignored() {
        let ctx = Loader::new().core();
        let doc = json!({"id": "urn:x", "type": "T",
            "createdAt": "1999-01-01T00:00:00Z",
            "modifiedAt": "1999-01-01T00:00:00Z",
            "deletedAt": "1999-01-01T00:00:00Z",
            "p": {"type": "Property", "value": 1,
                  "createdAt": "1999-01-01T00:00:00Z"}});
        let mut out = antares_jsonld::expand_entity(
            doc.as_object().expect("obj"),
            &ctx,
            ExpandOpts::default(),
        )
        .expect("providing common members is not an error");
        stamp_new(&mut out, "2026-08-11T00:00:00.000Z");
        assert_eq!(out["createdAt"], "2026-08-11T00:00:00.000Z");
        assert_eq!(out["modifiedAt"], "2026-08-11T00:00:00.000Z");
        assert!(out.get("deletedAt").is_none(), "deletedAt never creatable");
        let inst = &out["https://uri.etsi.org/ngsi-ld/default-context/p"][0];
        assert_eq!(
            inst["createdAt"], "2026-08-11T00:00:00.000Z",
            "instance-level client timestamp ignored too"
        );
    }

    /// 5.2.2: common members are only generated "when the Context Consumer
    /// explicitly asks for their inclusion" (sysAttrs, 6.3.11).
    #[test]
    fn common_members_appear_only_on_request() {
        let doc = json!({"id": "urn:x", "type": ["T"],
            "createdAt": "2026-08-11T00:00:00.000Z",
            "modifiedAt": "2026-08-11T00:00:00.000Z",
            "https://uri.etsi.org/ngsi-ld/default-context/p": [
                {"type": "Property", "value": 1,
                 "createdAt": "2026-08-11T00:00:00.000Z",
                 "modifiedAt": "2026-08-11T00:00:00.000Z"}]});
        let plain = crate::repr::apply(&doc, &crate::repr::Repr::default());
        assert!(plain.get("createdAt").is_none());
        assert!(plain["https://uri.etsi.org/ngsi-ld/default-context/p"][0]
            .get("modifiedAt")
            .is_none());
        let sys = crate::repr::apply(
            &doc,
            &crate::repr::Repr {
                sys_attrs: true,
                ..Default::default()
            },
        );
        assert!(sys.get("createdAt").is_some());
        assert!(sys["https://uri.etsi.org/ngsi-ld/default-context/p"][0]
            .get("modifiedAt")
            .is_some());
    }
}
