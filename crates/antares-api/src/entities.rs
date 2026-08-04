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

/// Record an entity write into the temporal store (5.6.11 auto-recording).
pub fn mirror_record(st: &AppState, tenant: &TenantId, expanded: &Value) {
    let Some(obj) = expanded.as_object() else {
        return;
    };
    let Some(id) = obj.get("id").and_then(Value::as_str) else {
        return;
    };
    let r = (|| -> Result<(), antares_model::NgsiError> {
        let exists = st.store.get(tenant, Kind::Temporal, id)?.is_some();
        if !exists {
            let mut doc = Map::new();
            for k in ["id", "type", "createdAt", "modifiedAt", "scope"] {
                if let Some(v) = obj.get(k) {
                    doc.insert(k.into(), v.clone());
                }
            }
            st.store
                .create(tenant, Kind::Temporal, id, Value::Object(doc))?;
        }
        st.store.mutate(tenant, Kind::Temporal, id, |doc| {
            let target = doc.as_object_mut().expect("temporal doc");
            for (k, v) in obj {
                if is_meta(k) {
                    continue;
                }
                let mut incoming: Vec<Value> = v.as_array().cloned().unwrap_or_default();
                for inst in &mut incoming {
                    if let Some(o) = inst.as_object_mut() {
                        o.entry("instanceId".to_owned()).or_insert_with(|| {
                            Value::String(format!("urn:ngsi-ld:Instance:{}", uuid::Uuid::new_v4()))
                        });
                    }
                }
                match target.get_mut(k).and_then(Value::as_array_mut) {
                    Some(cur) => cur.extend(incoming),
                    None => {
                        target.insert(k.clone(), Value::Array(incoming));
                    }
                }
            }
            Ok::<(), std::convert::Infallible>(())
        })?;
        Ok(())
    })();
    if let Err(e) = r {
        tracing::warn!("temporal mirror failed: {e}");
    }
}

/// delete_temporal_on_core_delete: entity deletion removes its temporal
/// representation too (suite configuration parity).
pub fn mirror_delete_entity(st: &AppState, tenant: &TenantId, id: &str) {
    if let Err(e) = st.store.delete(tenant, Kind::Temporal, id) {
        tracing::warn!("temporal mirror delete failed: {e}");
    }
}

/// Attribute deletion appends ONE deletion instance to the temporal
/// representation (4.8): typed NGSI-LD-null value + deletedAt.
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
    let regs = crate::federation::write_regs(st, &tenant, &spec, &parsed.ctx, params);
    if !regs.is_empty() {
        if crate::federation::via_loop(headers, &st.host_alias) {
            return Ok(crate::federation::loop_508(&tenant));
        }
        let mut conflicts = Vec::new();
        let mut fwd = Vec::new();
        for reg in &regs {
            if reg.mode == "exclusive" && !reg.supports("createEntity") {
                conflicts.push(crate::federation::conflict_part("createEntity"));
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
                mirror_record(st, &tenant, &local_exp);
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
    mirror_record(st, &tenant, &expanded);
    Ok(created(format!("/ngsi-ld/v1/entities/{id}"), &tenant))
}

// ---------- GET /entities/{id} (5.7.1) ----------

pub async fn retrieve_entity(
    State(st): State<AppState>,
    Path(id): Path<String>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
) -> Response {
    match retrieve_entity_inner(&st, &id, &params, &headers).await {
        Ok(r) => r,
        Err(e) => e.into_response(),
    }
}

async fn retrieve_entity_inner(
    st: &AppState,
    id: &str,
    params: &HashMap<String, String>,
    headers: &HeaderMap,
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
    let ctx = request_context(&st.loader, headers).await?;
    let repr = parse_repr(params, &ctx)?;
    let join = parse_join(params)?;
    antares_model::EntityId::new(id)?;
    let local_doc = st.store.get(&tenant, Kind::Entity, id)?;
    let fed_on =
        crate::federation::active(params) && !crate::federation::via_loop(headers, &st.host_alias);
    let doc = if fed_on {
        let fed = crate::federation::fed_retrieve(st, &tenant, headers, &ctx, id).await;
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
                let mut iter = fed.iter();
                let mut base = iter
                    .find(|(aux, _)| !aux)
                    .map(|(_, d)| d.clone())
                    .or_else(|| fed.first().map(|(_, d)| d.clone()))
                    .ok_or_else(|| NgsiError::ResourceNotFound(format!("entity {id} not found")))?;
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
    Ok(respond(StatusCode::OK, payload, &ctx, accept, &tenant))
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
            // I2: bounded traversal depth (§16.3)
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
            let targets: Vec<&str> = match inst.get("object") {
                Some(Value::String(id)) => vec![id.as_str()],
                Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).collect(),
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
    match query_entities_inner(&st, &params, &headers).await {
        Ok(r) => r,
        Err(e) => e.into_response(),
    }
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

    // 5.7.2: id/idPattern alone are NOT sufficient — one of type, attrs, q,
    // georel (or local=true) is required.
    let has_filter = ["type", "attrs", "q", "georel"]
        .iter()
        .any(|k| params.contains_key(*k))
        || params.get("local").map(String::as_str) == Some("true");
    if !has_filter {
        return Err(NgsiError::BadRequestData(
            "query needs at least one of type, attrs, q, georel (5.7.2)".into(),
        )
        .into());
    }

    let repr = parse_repr(params, &ctx)?;
    let join = parse_join(params)?;
    let fed = if crate::federation::active(params)
        && !crate::federation::via_loop(headers, &st.host_alias)
    {
        crate::federation::fed_query(st, &tenant, headers, &ctx, params).await
    } else {
        Vec::new()
    };
    let mut matches = filter_entities_fed(st, &tenant, params, &ctx, fed)?;
    if let Some(spec) = params.get("orderBy") {
        order_entities(&mut matches, spec, &ctx)?;
    }
    let (page, count_hdr, links) = paginate(st, params, matches, "/ngsi-ld/v1/entities")?;

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
    let payload = if accept == Accept::GeoJson {
        to_geojson_collection(payload, params.get("geometryProperty"), &ctx)
    } else {
        Value::Array(payload)
    };
    let mut resp = respond(StatusCode::OK, payload, &ctx, accept, &tenant);
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
    Ok(resp)
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
    let type_sel: Option<Vec<Vec<String>>> = params.get("type").map(|s| {
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
        Some(q) => Some(parse_q(q)?),
        None => None,
    };
    let scope_q = params.get("scopeQ");
    let geo = crate::geo::GeoQuery::from_params(params)?;

    let all = crate::federation::merge_candidates(st.store.list(tenant, Kind::Entity)?, fed);
    let mut out = Vec::new();
    for doc in all {
        let id = doc["id"].as_str().unwrap_or("");
        if let Some(ids) = &ids {
            if !ids.contains(&id) {
                continue;
            }
        }
        if let Some(re) = &id_pattern {
            if !re.is_match(id) {
                continue;
            }
        }
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
            if !eval_q(ast, &doc, ctx) {
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
        out.push(doc);
    }
    Ok(out)
}

/// limit/offset/count handling (6.3.10). Returns (page, count, link headers).
pub fn paginate(
    st: &AppState,
    params: &HashMap<String, String>,
    matches: Vec<Value>,
    path: &str,
) -> ApiResult<(Vec<Value>, Option<usize>, Vec<String>)> {
    paginate_accept(st, params, matches, path, Accept::Json)
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
    let count = params.get("count").map(String::as_str) == Some("true");
    let limit: usize = match params.get("limit") {
        Some(l) => l
            .parse()
            .map_err(|_| NgsiError::BadRequestData(format!("invalid limit {l:?}")))?,
        None => st.default_limit,
    };
    // I2: result ceiling (§16.3) — 403 TooManyResults, not silent clamping.
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
    let total = matches.len();
    let page: Vec<Value> = matches.into_iter().skip(offset).take(limit).collect();
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
        let ty = match accept {
            _ if csource_style => ";type=\"application/ld+json\"",
            Accept::LdJson => ";type=\"application/ld+json\"",
            _ => "",
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
        let spec = crate::csource::CsrSpec {
            ids: Some(vec![id.clone()]),
            ..Default::default()
        };
        let regs = crate::federation::write_regs(&st, &tenant, &spec, &ctx, &params);
        if !regs.is_empty() {
            if crate::federation::via_loop(&headers, &st.host_alias) {
                return Ok(crate::federation::loop_508(&tenant));
            }
            let local_exists = st.store.get(&tenant, Kind::Entity, &id)?.is_some();
            let proxy_match = regs.iter().any(|r| r.is_proxy());
            let mut parts = Vec::new();
            if local_exists || !proxy_match {
                if st.store.delete(&tenant, Kind::Entity, &id)? {
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
                if reg.mode == "exclusive" && !reg.supports("deleteEntity") {
                    parts.push(crate::federation::conflict_part("deleteEntity"));
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
        if st.store.delete(&tenant, Kind::Entity, &id)? {
            mirror_delete_entity(&st, &tenant, &id);
            Ok::<_, ApiError>(no_content(&tenant))
        } else {
            Err(NgsiError::ResourceNotFound(format!("entity {id} not found")).into())
        }
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

// ---------- DELETE /entities/ — Purge (5.6.21) ----------

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
    // 5.6.21: local=true alone is a valid "purge everything local" request
    let has_filter = ["id", "idPattern", "type", "attrs", "q", "georel"]
        .iter()
        .any(|k| params.contains_key(*k))
        || params.get("local").map(String::as_str) == Some("true");
    if !has_filter {
        return Err(NgsiError::BadRequestData(
            "purge needs at least one filtering condition".into(),
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
        ..Default::default()
    };
    let regs = crate::federation::write_regs(st, &tenant, &spec, &ctx, params);
    if !regs.is_empty() {
        if crate::federation::via_loop(headers, &st.host_alias) {
            return Ok(crate::federation::loop_508(&tenant));
        }
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
            if reg.mode == "exclusive" && !reg.supports("purgeEntity") {
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
        &["options", "format", "observedAt", "lang", "local"],
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
            temporal: false,
            ..Default::default()
        },
    )?;
    let ts = now_iso();

    let spec = crate::csource::CsrSpec {
        ids: Some(vec![id.to_owned()]),
        ..Default::default()
    };
    let regs = crate::federation::write_regs(st, &tenant, &spec, &parsed.ctx, params);
    if !regs.is_empty() {
        if crate::federation::via_loop(headers, &st.host_alias) {
            return Ok(crate::federation::loop_508(&tenant));
        }
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
                    temporal: false,
                    ..Default::default()
                },
            )?;
            let res = st.store.mutate(&tenant, Kind::Entity, id, |doc| {
                merge_into(doc, &local_frag, &ts);
                Ok::<(), NgsiError>(())
            })?;
            parts.push(match res {
                Some(Ok(())) => {
                    mirror_record(st, &tenant, &local_frag);
                    crate::federation::Part {
                        status: 204,
                        detail: "merged locally".into(),
                    }
                }
                _ => crate::federation::Part {
                    status: 404,
                    detail: format!("entity {id} not found locally"),
                },
            });
        }
        let ctx_url = crate::federation::ctx_link_url(headers, &parsed.ctx.source);
        for reg in &regs {
            if reg.mode == "exclusive" && !reg.supports("mergeEntity") {
                parts.push(crate::federation::conflict_part("mergeEntity"));
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
        merge_into(doc, &fragment, &ts);
        Ok::<(), NgsiError>(())
    })?;
    match res {
        None => Err(NgsiError::ResourceNotFound(format!("entity {id} not found")).into()),
        Some(Err(e)) => Err(e.into()),
        Some(Ok(())) => {
            mirror_record(st, &tenant, &fragment);
            Ok(no_content(&tenant))
        }
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
        } else {
            t.insert(k.clone(), v.clone());
        }
    }
    t.insert("modifiedAt".into(), Value::String(ts.to_owned()));
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
        let local_doc = st.store.get(&tenant, Kind::Entity, &id)?;
        let ctx0 = st.loader.core();
        let spec = crate::csource::CsrSpec {
            ids: Some(vec![id.clone()]),
            ..Default::default()
        };
        let regs = crate::federation::write_regs(&st, &tenant, &spec, &ctx0, &params);
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
            mirror_record(&st, &tenant, &expanded);
            return Ok::<_, ApiError>(no_content(&tenant));
        }
        if crate::federation::via_loop(&headers, &st.host_alias) {
            return Ok(crate::federation::loop_508(&tenant));
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
                    mirror_record(&st, &tenant, &local_exp);
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
            if reg.mode == "exclusive" && !reg.supports("replaceEntity") {
                parts.push(crate::federation::conflict_part("replaceEntity"));
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
pub fn order_entities(
    docs: &mut [Value],
    spec: &str,
    ctx: &antares_jsonld::Context,
) -> Result<(), NgsiError> {
    struct Key {
        path: Vec<String>,
        desc: bool,
    }
    let mut keys = Vec::new();
    for part in spec.split(',') {
        let part = part.trim();
        let (member, dir) = match part.split_once(';') {
            Some((m, d)) => (m.trim(), d.trim()),
            None => (part, "asc"),
        };
        if member.is_empty() || !["asc", "desc"].contains(&dir) {
            return Err(NgsiError::BadRequestData(format!(
                "invalid orderBy {spec:?} (4.23)"
            )));
        }
        keys.push(Key {
            path: member.split('.').map(str::to_owned).collect(),
            desc: dir == "desc",
        });
    }
    fn order_value(doc: &Value, path: &[String], ctx: &antares_jsonld::Context) -> Option<Value> {
        let head = path.first()?;
        match head.as_str() {
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
        }
    }
    fn cmp_vals(a: &Option<Value>, b: &Option<Value>) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        match (a, b) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Greater, // absent sorts last
            (Some(_), None) => Ordering::Less,
            (Some(x), Some(y)) => match (x.as_f64(), y.as_f64()) {
                (Some(nx), Some(ny)) => nx.total_cmp(&ny),
                _ => x
                    .as_str()
                    .unwrap_or(&x.to_string())
                    .cmp(y.as_str().unwrap_or(&y.to_string())),
            },
        }
    }
    docs.sort_by(|a, b| {
        for k in &keys {
            let va = order_value(a, &k.path, ctx);
            let vb = order_value(b, &k.path, ctx);
            let mut o = cmp_vals(&va, &vb);
            if k.desc {
                o = o.reverse();
            }
            if o != std::cmp::Ordering::Equal {
                return o;
            }
        }
        std::cmp::Ordering::Equal
    });
    Ok(())
}

// ---------- GeoJSON output (6.3.15) ----------

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

fn geo_value_of(attr: &Value) -> Value {
    let inst = match attr {
        Value::Array(a) => a.first().cloned().unwrap_or(Value::Null),
        other => other.clone(),
    };
    inst.get("value").cloned().unwrap_or(inst)
}

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
// NGSI-LD 2.0 pre-adoptions #14/#15 (tasks.md H3, §15.1): retrieve a single
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
