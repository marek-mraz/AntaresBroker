// SPDX-License-Identifier: EUPL-1.2
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
use antares_store::CurrentStateDriverExt;
use antares_store::Kind;
use antares_store::TemporalDriverExt as _;
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

pub(crate) use antares_ql::type_selection_matches;

/// Inject server-managed timestamps into a freshly expanded doc.
pub fn stamp_new(doc: &mut Value, ts: &str) {
    if let Some(obj) = doc.as_object_mut() {
        obj.insert("createdAt".into(), Value::String(ts.to_owned()));
        obj.insert("modifiedAt".into(), Value::String(ts.to_owned()));
        for (k, v) in obj.iter_mut() {
            if !is_meta(k) {
                stamp_instances(v, ts);
            }
        }
    }
}

/// Stamp one Attribute's instances, and their sub-Attributes, with the
/// 4.8 timestamps: createdAt and modifiedAt are "the temporal Property at
/// which the Entity, Property or Relationship was entered into"/"last
/// modified in an NGSI-LD system", a sub-Property is a Property, and the
/// value is server-generated — whatever the client sent is overwritten.
/// Every write path that brings a new Attribute in uses this one: 5.6.1
/// through `stamp_new`, 5.6.2 and 5.6.3 through `attrs.rs`, so the served
/// representation does not depend on which operation wrote the Attribute.
/// The temporal write path stamps differently (`temporal::stamp_instances`):
/// there an instance is the unit of history and carries an instanceId, and
/// sub-Attributes are part of the instance, not stamped separately.
pub(crate) fn stamp_instances(v: &mut Value, ts: &str) {
    if let Some(arr) = v.as_array_mut() {
        for inst in arr {
            if let Some(o) = inst.as_object_mut() {
                o.insert("createdAt".into(), Value::String(ts.to_owned()));
                o.insert("modifiedAt".into(), Value::String(ts.to_owned()));
                for (k, sub) in o.iter_mut() {
                    if sub.is_array() && !crate::repr_reserved(k) {
                        stamp_instances(sub, ts);
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
    if !st.record_locally() {
        return;
    }
    if let Err(e) = st.temporal.delete(tenant, id) {
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
    let r = st.temporal.mutate(tenant, id, |doc| {
        // The mirror writes nothing into a document the temporal driver
        // handed back in a shape the contract forbids; `had` stays false and
        // the caller reports that nothing was mirrored.
        let Some(target) = doc.as_object_mut() else {
            return Ok::<(), std::convert::Infallible>(());
        };
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

/// 5.6.1.5: the output of a successful Create Entity is "the URI of the
/// created Entity" — the resource URL carried in the Location header. The id
/// is one path segment (RFC 3986 clause 3.3), so it is percent-encoded:
/// spliced raw, a `#` in the id turns the rest of it into a fragment and the
/// client is handed a URL addressing a different resource.
fn entity_location(id: &str) -> String {
    format!(
        "/ngsi-ld/v1/entities/{}",
        crate::federation::path_segment(id)
    )
}

/// The selector of Entity types is input data of Delete (5.6.6.3), Merge
/// (5.6.17.3) and Replace Entity (5.6.18.3), and each of those clauses
/// forwards "matching input data ... to the Registration endpoint". A
/// registration may cover several Entity types, so a forward that drops the
/// selector lets the peer act on an entity the client's selector excluded.
fn type_selector_query(params: &HashMap<String, String>) -> Vec<(String, String)> {
    params
        .get("type")
        .map(|v| ("type".to_owned(), v.clone()))
        .into_iter()
        .collect()
}

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
    let obj = parsed.object(NgsiError::InvalidRequest(
        "entity document must be a JSON object".into(),
    ))?;
    let mut expanded = expand_entity(obj, &parsed.ctx, ExpandOpts::default())?;
    let id = antares_jsonld::expanded_id(&expanded)?.to_owned();

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
            created(entity_location(&id), &tenant),
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
    Ok(created(entity_location(&id), &tenant))
}

// ---------- GET /entities/{id} (5.7.1) ----------

pub async fn retrieve_entity(
    State(st): State<AppState>,
    Path(id): Path<String>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
) -> Response {
    match retrieve_entity_outer(&st, &id, &params, &headers).await {
        Ok(r) => r,
        Err(e) => e.into_response(),
    }
}

/// 5.7.1.4 Retrieve Entity: the EntityMap half of the clause is the shared
/// rule (`entity_maps::retrieve_with_map`); this is the retrieve it wraps.
async fn retrieve_entity_outer(
    st: &AppState,
    id: &str,
    params: &HashMap<String, String>,
    headers: &HeaderMap,
) -> ApiResult<Response> {
    crate::entity_maps::retrieve_with_map(st, id, params, headers, false, |map| async move {
        retrieve_entity_inner(st, id, params, headers, map.as_ref()).await
    })
    .await
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
        let held = contained_by(params);
        let complete = match mode.as_str() {
            "inline" => inline_join_beyond(
                st,
                &tenant,
                &ctx,
                &repr,
                &mut payload,
                *level,
                &held,
                &mut { MAX_JOIN_LOOKUPS },
            ),
            "flat" => {
                let mut linked = std::collections::BTreeMap::new();
                let complete = collect_flat_beyond(
                    st,
                    &tenant,
                    &repr,
                    &doc,
                    *level,
                    &mut linked,
                    &held,
                    &mut { MAX_JOIN_LOOKUPS },
                );
                if !linked.is_empty() {
                    let mut arr = vec![payload];
                    for (_, (ldoc, lrepr)) in linked {
                        arr.push(compact_for(&lrepr, &apply(&ldoc, &lrepr), &ctx));
                    }
                    payload = Value::Array(arr);
                }
                complete
            }
            _ => true,
        };
        if !complete {
            warnings.push(crate::federation::warning(
                199,
                &crate::federation::alias_for(&st.host_alias, &tenant),
                "the linked entity retrieval was truncated",
            ));
        }
    }
    let payload = if accept == Accept::GeoJson {
        to_geojson_feature(payload, params.get("geometryProperty"))
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

/// 4.5.23.1: "When retrieving Linked Entities, it is necessary to limit
/// retrieval to avoid cascades of an excessive length, duplicates or loops."
/// joinLevel bounds the DEPTH of the walk; this bounds its WIDTH — the total
/// number of Linked Entity reads a single request may buy, so that a densely
/// linked graph cannot turn one retrieval into an unbounded store scan.
const MAX_JOIN_LOOKUPS: usize = 1_000;

/// State of one Linked Entity Retrieval walk (4.5.23.1): the entity ids
/// already resolved — a loop or a duplicate is never walked a second time —
/// and the remaining lookup budget. `complete` goes false as soon as the walk
/// left something out, which the caller reports as an NGSILD-Warning.
struct JoinWalk {
    seen: std::collections::BTreeSet<String>,
    budget: usize,
    complete: bool,
}

impl JoinWalk {
    /// The Linking Entity is already part of the response, so it counts as
    /// resolved before the walk starts — and so does every id the client
    /// passed in `containedBy`. `budget` is what is LEFT of the request's
    /// allowance: a page walks one entity at a time and each walk hands the
    /// remainder to the next, so the ceiling bounds the request rather than
    /// each of its entities.
    fn rooted(root: Option<&str>, contained_by: &[String], budget: usize) -> Self {
        let mut seen: std::collections::BTreeSet<String> = contained_by.iter().cloned().collect();
        if let Some(id) = root {
            seen.insert(id.to_owned());
        }
        JoinWalk {
            seen,
            budget,
            complete: true,
        }
    }
}

/// Table 6.4.3.2-1 `containedBy`: "List of entity ids which have previously
/// been encountered whilst retrieving the Entity Graph. Only applicable if
/// joinLevel is present." They are already in the graph the client is
/// assembling, so 4.5.23.1's "avoid ... duplicates or loops" counts them as
/// resolved and the walk does not follow them again.
pub fn contained_by(params: &HashMap<String, String>) -> Vec<String> {
    params
        .get("containedBy")
        .map(|s| {
            s.split(',')
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// Linked Entity Retrieval, inline form (4.5.23.2): embed each relationship
/// target under an "entity" member (normalized) or replace the object URI by
/// the linked entity representation (simplified). Operates on COMPACTED docs.
/// Returns false when 4.5.23.1 truncated the walk (loop, duplicate, budget).
pub fn inline_join(
    st: &AppState,
    tenant: &TenantId,
    ctx: &antares_jsonld::Context,
    repr: &crate::repr::Repr,
    compacted: &mut Value,
    level: usize,
) -> bool {
    inline_join_beyond(st, tenant, ctx, repr, compacted, level, &[], &mut {
        MAX_JOIN_LOOKUPS
    })
}

/// Same, continuing an Entity Graph the client is already holding: the
/// `containedBy` ids count as encountered (Table 6.4.3.2-1).
#[allow(clippy::too_many_arguments)]
pub fn inline_join_beyond(
    st: &AppState,
    tenant: &TenantId,
    ctx: &antares_jsonld::Context,
    repr: &crate::repr::Repr,
    compacted: &mut Value,
    level: usize,
    contained_by: &[String],
    budget: &mut usize,
) -> bool {
    let mut walk = JoinWalk::rooted(
        compacted.get("id").and_then(Value::as_str),
        contained_by,
        *budget,
    );
    inline_join_walk(st, tenant, ctx, repr, compacted, level, &mut walk);
    *budget = walk.budget;
    walk.complete
}

fn inline_join_walk(
    st: &AppState,
    tenant: &TenantId,
    ctx: &antares_jsonld::Context,
    repr: &crate::repr::Repr,
    compacted: &mut Value,
    level: usize,
    walk: &mut JoinWalk,
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
        inline_join_value(st, tenant, ctx, repr, &child, v, level, walk);
    }
}

fn lookup_joined(
    st: &AppState,
    tenant: &TenantId,
    ctx: &antares_jsonld::Context,
    child: &crate::repr::Repr,
    id: &str,
    level: usize,
    walk: &mut JoinWalk,
) -> Option<Value> {
    if walk.budget == 0 {
        walk.complete = false;
        return None;
    }
    walk.budget -= 1;
    let target = st.store.get(tenant, Kind::Entity, id).ok().flatten()?;
    let shaped = apply(&target, child);
    let mut c = compact_for(child, &shaped, ctx);
    if level > 1 {
        if walk.seen.insert(id.to_owned()) {
            inline_join_walk(st, tenant, ctx, child, &mut c, level - 1, walk);
        } else {
            // 4.5.23.1: an already-resolved target is a loop or a duplicate —
            // it is still embedded, but its own links are not walked again.
            walk.complete = false;
        }
    }
    Some(c)
}

#[allow(clippy::too_many_arguments)]
fn inline_join_value(
    st: &AppState,
    tenant: &TenantId,
    ctx: &antares_jsonld::Context,
    repr: &crate::repr::Repr,
    child: &crate::repr::Repr,
    v: &mut Value,
    level: usize,
    walk: &mut JoinWalk,
) {
    match v {
        Value::Array(items) => {
            for i in items {
                inline_join_value(st, tenant, ctx, repr, child, i, level, walk);
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
                let mut joined: Vec<Value> = Vec::new();
                for id in &targets {
                    if let Some(j) = lookup_joined(st, tenant, ctx, child, id, level, walk) {
                        joined.push(j);
                    }
                }
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
            let mut joined: Vec<Value> = Vec::new();
            for id in &targets {
                if let Some(j) = lookup_joined(st, tenant, ctx, child, id, level, walk) {
                    joined.push(j);
                }
            }
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
            if let Some(joined) = lookup_joined(st, tenant, ctx, child, id, level, walk) {
                *v = joined;
            }
        }
        _ => {}
    }
}

/// Linked Entity Retrieval, flattened form (4.5.23.3): collect targets with
/// the child representation that applies to each. The Linking Entity is
/// already in the flattened array, so 4.5.23.1 ("avoid ... duplicates or
/// loops") keeps it out of `out` even when a Relationship points back at it.
/// Returns false when the walk was truncated by the lookup budget.
pub fn collect_flat(
    st: &AppState,
    tenant: &TenantId,
    repr: &crate::repr::Repr,
    internal_doc: &Value,
    level: usize,
    out: &mut std::collections::BTreeMap<String, (Value, crate::repr::Repr)>,
) -> bool {
    collect_flat_beyond(st, tenant, repr, internal_doc, level, out, &[], &mut {
        MAX_JOIN_LOOKUPS
    })
}

/// Same, continuing an Entity Graph the client is already holding: the
/// `containedBy` ids count as encountered (Table 6.4.3.2-1).
#[allow(clippy::too_many_arguments)]
pub fn collect_flat_beyond(
    st: &AppState,
    tenant: &TenantId,
    repr: &crate::repr::Repr,
    internal_doc: &Value,
    level: usize,
    out: &mut std::collections::BTreeMap<String, (Value, crate::repr::Repr)>,
    contained_by: &[String],
    budget: &mut usize,
) -> bool {
    let mut walk = JoinWalk::rooted(
        internal_doc.get("id").and_then(Value::as_str),
        contained_by,
        *budget,
    );
    walk.seen.extend(out.keys().cloned());
    collect_flat_walk(st, tenant, repr, internal_doc, level, out, &mut walk);
    *budget = walk.budget;
    walk.complete
}

#[allow(clippy::too_many_arguments)]
fn collect_flat_walk(
    st: &AppState,
    tenant: &TenantId,
    repr: &crate::repr::Repr,
    internal_doc: &Value,
    level: usize,
    out: &mut std::collections::BTreeMap<String, (Value, crate::repr::Repr)>,
    walk: &mut JoinWalk,
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
                if walk.seen.contains(id) {
                    continue;
                }
                if walk.budget == 0 {
                    walk.complete = false;
                    return;
                }
                walk.budget -= 1;
                if let Some(target) = st.store.get(tenant, Kind::Entity, id).ok().flatten() {
                    walk.seen.insert(id.to_owned());
                    out.insert(id.to_owned(), (target.clone(), child.clone()));
                    if level > 1 {
                        collect_flat_walk(st, tenant, &child, &target, level - 1, out, walk);
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
    // 5.7.2.4: an unknown parameter and a too-wide query are BadRequestData
    // for this request, whether or not it carries an EntityMap reference —
    // and the paged fetch below walks the whole map, locally and forwarded,
    // before the inner call would reach these same two checks.
    check_params(&params, QUERY_PARAMS)?;
    let q_ast = params.get("q").map(|q| parse_q(q)).transpose()?;
    if !qualifies_non_wide(&params, q_ast.as_ref()) {
        return Err(NgsiError::BadRequestData(
            "query needs at least one of type, attrs, q, georel (5.7.2)".into(),
        )
        .into());
    }
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
    let ids: Vec<String> = crate::entity_maps::candidate_ids(&map, &params);
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
    crate::entities::check_collation(params)?;
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
    // 5.7.2.4: a syntactically invalid context source filter is 400.
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
    // Pushdown gates: pagination per page_pushdown_allowed (no federation
    // candidates, no idPattern, no orderBy). Projection additionally excludes
    // join (linked-entity walks read page docs) and GeoJSON output.
    let (p_offset, p_limit, _) = page_params(st, params)?;
    let push_page = page_pushdown_allowed(fed.is_empty(), params);
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
        let mut complete = true;
        let held = contained_by(params);
        // 4.5.23.1 bounds the WIDTH of the retrieval per request, so one
        // allowance is spent across the whole page: minting a fresh budget per
        // payload Entity multiplied it by the page size, and a page of
        // max_limit densely linked Entities bought MAX_JOIN_LOOKUPS lookups
        // each.
        let mut budget = MAX_JOIN_LOOKUPS;
        match mode.as_str() {
            "inline" => {
                for p in &mut payload {
                    complete &=
                        inline_join_beyond(st, &tenant, &ctx, &repr, p, *level, &held, &mut budget);
                }
            }
            "flat" => {
                let mut linked = std::collections::BTreeMap::new();
                for doc in &page {
                    complete &= collect_flat_beyond(
                        st,
                        &tenant,
                        &repr,
                        doc,
                        *level,
                        &mut linked,
                        &held,
                        &mut budget,
                    );
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
        if !complete {
            warnings.push(crate::federation::warning(
                199,
                &crate::federation::alias_for(&st.host_alias, &tenant),
                "the linked entity retrieval was truncated",
            ));
        }
    }
    let mut resp = if accept == Accept::GeoJson {
        let fc = to_geojson_collection(payload, params.get("geometryProperty"));
        respond_prefer(StatusCode::OK, fc, &ctx, accept, &tenant, headers)
    } else {
        crate::negotiate::respond_list(StatusCode::OK, payload, &ctx, accept, &tenant)
    };
    attach_paging(&mut resp, count_hdr, &links);
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
    /// How many documents the store returned, before the evaluator below
    /// dropped any. With a pushed page that is the size of the SQL page —
    /// which is what a chunked walk has to step over, since `docs` undercounts
    /// it whenever idPattern (invisible to the store filter) removed rows.
    pub rows: usize,
}

/// Whether the store may be asked to project members away. Only when its
/// answer is the final answer: 5.7.2.4 applies the query, geoquery, Scope
/// query and Attribute filters after remote parts have been aggregated, so
/// with federated candidates present those filters still run over documents
/// the store would have stripped; and 4.23 orders Entities by the value of
/// the ordering member, which a projection may have removed — leaving the
/// comparator nothing to compare and the client id order.
fn proj_pushdown_allowed(fed_is_empty: bool, params: &HashMap<String, String>) -> bool {
    fed_is_empty && !params.contains_key("orderBy")
}

/// Whether the store may be asked to cut the page (ORDER BY id +
/// LIMIT/OFFSET). Only when nothing outside it still narrows or reorders the
/// match set:
///
/// * federated candidates are merged in afterwards, and 5.7.2.4 applies the
///   query, geoquery, Scope query and Attribute filters only after that
///   aggregation — so a SQL page would be cut from the wrong set;
/// * `idPattern` is not part of the store filter, so it drops rows the SQL
///   page already counted;
/// * `orderBy` orders the whole match set by the value of the ordering member
///   (4.23) before the page is cut, and that comparison order is the
///   evaluator's.
///
/// `limit=0` — the count-only shape of 6.3.10, where `count=true` is
/// mandatory — IS pushed: the store returns no rows and counts the match set,
/// which is the same count the scan derives from a materialized one, without
/// materializing 100 million documents to throw them away.
fn page_pushdown_allowed(fed_is_empty: bool, params: &HashMap<String, String>) -> bool {
    fed_is_empty && !params.contains_key("idPattern") && !params.contains_key("orderBy")
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
    // and neither can a pushed projection: the store's answer is only the
    // final answer when nothing downstream still needs the stripped members
    let proj = proj.filter(|_| proj_pushdown_allowed(fed.is_empty(), params));
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
                crate::regexcache::compile(p)
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
    let proj = proj.filter(|_| !geo_uncompiled);
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
        &antares_store::filter::EntityFilter {
            ids: ids.as_deref(),
            // 5.2.33: id takes precedence over idPattern, so the literal
            // narrows only when no id selector was given
            id_literal: if ids.is_none() {
                params
                    .get("idPattern")
                    .and_then(|p| antares_store::filter::id_pattern_literal(p))
            } else {
                None
            },
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
            page: page.map(|(offset, limit)| antares_store::filter::Page {
                offset: offset as i64,
                limit: limit as i64,
                count: params.get("count").map(String::as_str) == Some("true"),
            }),
            keep_attrs: keep_attrs.as_deref(),
            drop_attrs: drop_attrs.as_deref(),
        },
    )?;
    let decided = outcome.decided && fed.is_empty() && !geo_uncompiled;
    let paged = outcome.paged && fed.is_empty();
    let total = outcome.total.map(|t| t as usize);
    let rows = outcome.rows.len();
    let all = crate::federation::merge_candidates(outcome.rows, fed);
    // the id list is client-sized (a POST query body carries an array with no
    // count of its own) and the candidate set is store-sized, so the two are
    // never multiplied together
    let id_set: Option<std::collections::HashSet<&str>> =
        ids.as_ref().map(|v| v.iter().copied().collect());
    let mut out = Vec::new();
    for doc in all {
        let id = doc["id"].as_str().unwrap_or("");
        if let Some(ids) = &id_set {
            if !decided && !ids.contains(id) {
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
        rows,
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

// ---------- DELETE /entities/{id} (5.6.6) ----------

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
        // 5.5.7: `type` is a term, so the request's own @context expands it.
        // Under the core context the same word names a different type than
        // the client meant, and the delete then removes an Entity the
        // client's selector excluded.
        let ctx = request_context(&st.loader, &headers).await?;
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
            let seg = crate::federation::path_segment(&id);
            let fwd_q = type_selector_query(&params);
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
                        format!("{}/ngsi-ld/v1/entities/{seg}", reg.endpoint),
                        &fwd_q,
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

/// 5.6.21.3 Input data of a Purge: the Entity type selector, the identifier
/// list and id pattern, the restrictive and exclusionary Attribute-name
/// lists, the NGSI-LD Query, the GeoQuery, the Scope query and the context
/// source filter. 5.6.21.4 forwards "matching input data ... to the
/// Registration endpoint", so every one of them travels: a forward that
/// carries fewer restrictions than the client issued makes the peer execute
/// a strictly wider purge than the one requested. `local` is absent by
/// design — it selects local scope, which is what stops the forward
/// happening at all (5.5.13).
const PURGE_FORWARD_PARAMS: &[&str] = &[
    "type",
    "id",
    "idPattern",
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
];

fn forwarded_purge_query(params: &HashMap<String, String>) -> Vec<(String, String)> {
    PURGE_FORWARD_PARAMS
        .iter()
        .filter_map(|k| params.get(*k).map(|v| ((*k).to_owned(), v.clone())))
        .collect()
}

/// How many matched Entities one round of a Purge fetches and applies. The
/// match set of 5.6.21.4 is deliberately unbounded — `DELETE /entities?type=T`
/// matches every Entity of that type — so it is walked page by page and
/// applied in batches rather than materialized whole.
const PURGE_CHUNK: usize = 500;

/// Where the next Purge chunk starts, or None when the match set is
/// exhausted. `rows` = documents the store returned for this chunk (not the
/// subset that survived the evaluator: idPattern is applied after the store,
/// so a full chunk can arrive narrowed or even empty and the walk must still
/// go on — 5.6.21.4 deletes ALL matched Entities, not the first page of
/// them). `left_the_set` = how many of those rows no longer match the purge
/// query afterwards; the rows still in it have to be stepped over, or the
/// walk re-reads them forever.
///
/// Termination: every round either removes rows from the match set or
/// advances the offset by a full chunk, and both are bounded by the number of
/// stored Entities.
fn purge_next_offset(
    offset: usize,
    rows: usize,
    left_the_set: usize,
    paged: bool,
    chunk: usize,
) -> Option<usize> {
    // an unpaged answer IS the whole match set, which this round just applied
    if !paged || rows < chunk {
        return None;
    }
    Some(offset + rows.saturating_sub(left_the_set))
}

/// 5.6.21.4 "And thereafter": with no Attribute-name list, delete every
/// matched Entity found locally; with a restrictive list, delete those
/// Attributes from them; with an exclusionary list, delete all but those.
fn purge_locally(
    st: &AppState,
    tenant: &TenantId,
    params: &HashMap<String, String>,
    ctx: &antares_jsonld::Context,
    keep: &Option<Vec<String>>,
    drop: &Option<Vec<String>>,
) -> ApiResult<()> {
    let prune = keep.is_some() || drop.is_some();
    let mut offset = 0usize;
    loop {
        let batch = filter_entities_paged(
            st,
            tenant,
            params,
            ctx,
            Vec::new(),
            Some((offset, PURGE_CHUNK)),
            None,
        )?;
        let rows = batch.rows;
        let ids: Vec<String> = batch
            .docs
            .iter()
            .filter_map(|d| d["id"].as_str().map(str::to_owned))
            .collect();
        let left_the_set = if ids.is_empty() {
            0
        } else if prune {
            let mut changed = 0usize;
            st.store.batch_mutate(tenant, &ids, |_, doc| {
                let target = antares_store::stored_object(doc)?;
                let attrs: Vec<String> = target.keys().filter(|k| !is_meta(k)).cloned().collect();
                let before = target.len();
                for a in attrs {
                    let purge = match (keep, drop) {
                        (Some(keep), _) => !keep.contains(&a),
                        (_, Some(drop)) => drop.contains(&a),
                        _ => true,
                    };
                    if purge {
                        target.remove(&a);
                    }
                }
                if target.len() != before {
                    changed += 1;
                }
                Ok::<(), NgsiError>(())
            })?;
            // A prune keeps the Entity, but it may have removed the very
            // Attribute the query matched on (`attrs=speed&drop=speed`), which
            // takes it out of the match set and shifts the rest down. Whether
            // it did is only observable by reading the window again, so a round
            // that changed something re-reads it; the re-read finds those
            // Entities unchanged and steps over them.
            if changed > 0 {
                rows
            } else {
                0
            }
        } else {
            let mut gone = 0usize;
            for (id, deleted) in ids.iter().zip(st.store.batch_delete(tenant, &ids)?) {
                if deleted {
                    gone += 1;
                    mirror_delete_entity(st, tenant, id);
                }
            }
            gone
        };
        match purge_next_offset(offset, rows, left_the_set, batch.paged, PURGE_CHUNK) {
            Some(next) => offset = next,
            None => return Ok(()),
        }
    }
}

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
    // BadRequestData.
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
    let keep: Option<Vec<String>> = params
        .get("keep")
        .map(|s| s.split(',').map(|t| ctx.expand_key(t.trim())).collect());
    let drop: Option<Vec<String>> = params
        .get("drop")
        .map(|s| s.split(',').map(|t| ctx.expand_key(t.trim())).collect());
    // distributed purge (5.6.21 / 6.4.3.3). Matching and the 6.3.17/6.3.18
    // loop check come first: 508 Loop Detected is an error status, so the
    // request it answers must not have deleted a page of Entities already.
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
    purge_locally(st, &tenant, params, &ctx, &keep, &drop)?;
    if !regs.is_empty() {
        let mut parts = vec![crate::federation::Part {
            status: 204,
            detail: "purged locally".into(),
        }];
        let ctx_url = crate::federation::ctx_link_url(headers, &ctx.source);
        let query = forwarded_purge_query(params);
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
    let obj = parsed.object(NgsiError::BadRequestData(
        "fragment must be a JSON object".into(),
    ))?;
    if let Some(bid) = obj.get("id").and_then(Value::as_str) {
        if bid != id {
            return Err(NgsiError::BadRequestData("fragment id mismatch".into()).into());
        }
    }
    let mut fragment = expand_entity(
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
    // 5.6.17.3: a common observedAt timestamp to use across merged
    // Attributes, and a common language tag for merged LanguageMaps.
    let observed_at = params.get("observedAt").map(String::as_str);
    if let Some(t) = observed_at {
        if !antares_jsonld::parse_datetime(t) {
            return Err(
                NgsiError::BadRequestData("observedAt must be a DateTime (4.8)".into()).into(),
            );
        }
    }
    let lang = params.get("lang").map(String::as_str);
    apply_common_observed_at(&mut fragment, observed_at);
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
        // 5.6.17.4: the target is "an existing Entity whose id (URI), and
        // where specified type, is equivalent held locally" — the ?type
        // selector narrows it on this path exactly as on the local-only one.
        let local_exists = st
            .store
            .get(&tenant, Kind::Entity, id)?
            .is_some_and(|d| crate::attrs::matches_type_param(&d, params, &parsed.ctx));
        if (local_exists || proxies.is_empty()) && has_attrs {
            let mut local_frag = expand_entity(
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
            apply_common_observed_at(&mut local_frag, observed_at);
            let res = st.store.mutate(&tenant, Kind::Entity, id, |doc| {
                if !crate::attrs::matches_type_param(doc, params, &parsed.ctx) {
                    return Err(NgsiError::ResourceNotFound(format!(
                        "entity {id} does not match the type selector"
                    )));
                }
                let mut frag = local_frag.clone();
                apply_common_lang(doc, &mut frag, lang);
                merge_into(doc, &frag, &ts);
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
        let seg = crate::federation::path_segment(id);
        let fwd_q = type_selector_query(params);
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
                    format!("{}/ngsi-ld/v1/entities/{seg}", reg.endpoint),
                    &fwd_q,
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
        let mut frag = fragment.clone();
        apply_common_lang(doc, &mut frag, lang);
        merge_into(doc, &frag, &ts);
        Ok::<(), NgsiError>(())
    })?;
    match res {
        None => Err(NgsiError::ResourceNotFound(format!("entity {id} not found")).into()),
        Some(Err(e)) => Err(e.into()),
        Some(Ok(())) => Ok(no_content(&tenant)),
    }
}

/// 5.6.17.3: "An optional parameter indicating a common observedAt timestamp
/// to use across merged Attributes." It applies to the Attribute instances of
/// the Fragment that do not carry an observedAt of their own; a deletion
/// instance (5.5.12 NGSI-LD Null) removes the Attribute and takes none.
fn apply_common_observed_at(fragment: &mut Value, observed_at: Option<&str>) {
    let (Some(ts), Some(obj)) = (observed_at, fragment.as_object_mut()) else {
        return;
    };
    for (k, v) in obj.iter_mut() {
        if is_meta(k) {
            continue;
        }
        let Some(instances) = v.as_array_mut() else {
            continue;
        };
        for inst in instances {
            if antares_jsonld::is_deletion_instance(inst) {
                continue;
            }
            if let Some(o) = inst.as_object_mut() {
                o.entry("observedAt".to_owned())
                    .or_insert_with(|| Value::String(ts.to_owned()));
            }
        }
    }
}

/// 5.6.17.4: "If a common language tag is defined and a LanguageProperty
/// Attribute to be merged is represented as a string, the pre-existing
/// languageMap JSON object shall be preserved. The string value shall only
/// replace the value associated to the language tag key found within the
/// languageMap." The string instance is rewritten into a one-key languageMap
/// patch, which 5.5.12 then merges into the stored map key by key.
fn apply_common_lang(target: &Value, fragment: &mut Value, lang: Option<&str>) {
    let (Some(lang), Some(frag)) = (lang, fragment.as_object_mut()) else {
        return;
    };
    for (k, v) in frag.iter_mut() {
        if is_meta(k) {
            continue;
        }
        let pre_existing_langmap = target
            .get(k)
            .and_then(Value::as_array)
            .is_some_and(|insts| insts.iter().any(|i| i.get("languageMap").is_some()));
        if !pre_existing_langmap {
            continue;
        }
        let Some(instances) = v.as_array_mut() else {
            continue;
        };
        for inst in instances {
            if antares_jsonld::is_deletion_instance(inst) {
                continue;
            }
            let Some(o) = inst.as_object_mut() else {
                continue;
            };
            let Some(s) = o.get("value").and_then(Value::as_str).map(str::to_owned) else {
                continue;
            };
            o.remove("value");
            o.insert("type".into(), Value::String("LanguageProperty".into()));
            let mut map = Map::new();
            map.insert(lang.to_owned(), Value::String(s));
            o.insert("languageMap".into(), Value::Object(map));
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
        // 5.5.7 again: the selector is expanded with the request's @context,
        // not the core one. The target is judged before the body is parsed
        // (5.6.18: an unknown target is 404 before body validation), so the
        // header context is resolved on its own here.
        let ctx0 = request_context(&st.loader, &headers).await?;
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
            let obj = parsed.object(NgsiError::BadRequestData(
                "entity must be a JSON object".into(),
            ))?;
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
        let obj = parsed.object(NgsiError::BadRequestData(
            "entity must be a JSON object".into(),
        ))?;
        let expanded = expand_entity(obj, &parsed.ctx, ExpandOpts::default())?;
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
        let seg = crate::federation::path_segment(&id);
        let fwd_q = type_selector_query(&params);
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
                    format!("{}/ngsi-ld/v1/entities/{seg}", reg.endpoint),
                    &fwd_q,
                    &headers,
                    &tenant,
                    reg,
                    &ctx_url,
                    Some(frag),
                )
                .await,
            );
        }
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
/// 4.23 Entity Ordering: orderBy = `AttrName[;direction] *(, …)` with asc
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
/// 5.7.2.4 / 5.7.4.4: "If a preferred collation setting is present and it
/// does not conform to a valid ICU collation (see IETF RFC 6067 [36]) then an
/// error of type BadRequestData shall be raised." The clause names the
/// parameter's presence, not an `orderBy` that happens to consume it, so the
/// check runs on every operation that accepts `collation`.
pub fn check_collation(params: &HashMap<String, String>) -> Result<(), NgsiError> {
    match params.get("collation") {
        Some(tag) => build_collator(tag).map(|_| ()),
        None => Ok(()),
    }
}

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
        .map_err(|_| bad(format!("unsupported collation {tag:?} (4.23.3)")))
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
pub fn to_geojson_feature(entity: Value, geometry_property: Option<&String>) -> Value {
    let geom_term = geometry_property
        .cloned()
        .unwrap_or_else(|| "location".into());
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
pub fn to_geojson_collection(entities: Vec<Value>, geometry_property: Option<&String>) -> Value {
    let features: Vec<Value> = entities
        .into_iter()
        .map(|e| to_geojson_feature(e, geometry_property))
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
    let attr_iri = antares_jsonld::expand_attr_name(attr, &ctx)?;
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
        let entity = json!({
            "id": "urn:ngsi-ld:V:1", "type": "Vehicle",
            "location": [
                {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [9.0, 9.0]},
                 "datasetId": "urn:ngsi-ld:Dataset:gps"},
                {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]}}
            ],
            "speed": {"type": "Property", "value": 5}
        });
        let f = to_geojson_feature(entity.clone(), None);
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
        let f2 = to_geojson_feature(entity.clone(), Some(&"speed".to_string()));
        assert_eq!(f2["geometry"], Value::Null);
        // absent GeoProperty -> null geometry
        let f3 = to_geojson_feature(entity.clone(), Some(&"missing".to_string()));
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
        let fs = to_geojson_feature(simplified, None);
        assert_eq!(
            fs["geometry"],
            json!({"type": "Point", "coordinates": [3.0, 4.0]})
        );
        assert_eq!(fs["properties"]["speed"], 5);

        let fc = to_geojson_collection(vec![entity], None);
        assert_eq!(fc["type"], "FeatureCollection");
        assert_eq!(fc["features"].as_array().map(Vec::len), Some(1));
        assert!(
            fc["features"][0].get("@context").is_none(),
            "no per-Feature @context"
        );
        // Table 5.2.30-1: "In the case that no matches are found, features
        // will be an empty array"
        let empty = to_geojson_collection(vec![], None);
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
        // 4.23.2: under a distance ordering the GeoProperties rank first by
        // distance, and the non-GeoProperties after them BY VALUE — so the
        // ordering member has to be the same (core) one the geo entities use,
        // and there have to be two of them for their own order to mean
        // anything.
        let plain = |id: &str, v: &str| {
            json!({"id": id, "type": ["T"],
                "https://uri.etsi.org/ngsi-ld/location": [
                    {"type": "Property", "value": v}]})
        };
        let mut mixed = vec![
            plain("urn:plain-z", "zzz"),
            plain("urn:plain-a", "aaa"),
            geo("urn:g", 8.0, 40.0),
        ];
        order_entities(&mut mixed, "location;dist-asc", &p, &ctx).expect("order");
        assert_eq!(ids(&mixed), vec!["urn:g", "urn:plain-a", "urn:plain-z"]);
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

/// 4.5.23.1: "When retrieving Linked Entities, it is necessary to limit
/// retrieval to avoid cascades of an excessive length, duplicates or loops."
#[cfg(test)]
mod clause_4_5_23_bounds {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use serde_json::json;
    use tower::ServiceExt;

    fn app() -> axum::Router {
        crate::router(AppState::new("antares-test".into()))
    }

    async fn create(app: &axum::Router, body: Value) {
        let payload = body.to_string();
        let resp = app
            .clone()
            .oneshot(
                Request::post("/ngsi-ld/v1/entities")
                    .header("Content-Type", "application/json")
                    .header("Content-Length", payload.len().to_string())
                    .body(Body::from(payload))
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::CREATED, "create failed");
    }

    async fn get(app: &axum::Router, uri: &str) -> (axum::http::response::Parts, Value) {
        let resp = app
            .clone()
            .oneshot(Request::get(uri).body(Body::empty()).expect("req"))
            .await
            .expect("resp");
        let (parts, body) = resp.into_parts();
        let bytes = body.collect().await.expect("body").to_bytes();
        let json: Value = serde_json::from_slice(&bytes).expect("json");
        (parts, json)
    }

    /// 4.5.23.1/4.5.23.3: the flattened array carries the Linking Entity and
    /// its Linked Entities — a Relationship pointing back at the root is a
    /// loop, so the root shall appear exactly ONCE, not once as the Linking
    /// Entity and again as its own Linked Entity.
    #[tokio::test]
    async fn flat_join_never_repeats_the_root_entity() {
        let app = app();
        let root = "urn:ngsi-ld:Loop:root";
        let leaf = "urn:ngsi-ld:Loop:leaf";
        create(&app, json!({"id": leaf, "type": "Loop"})).await;
        create(
            &app,
            json!({"id": root, "type": "Loop",
                   "self": {"type": "Relationship", "object": root},
                   "other": {"type": "Relationship", "object": leaf}}),
        )
        .await;

        let (_, body) = get(
            &app,
            &format!("/ngsi-ld/v1/entities/{root}?join=flat&joinLevel=3"),
        )
        .await;
        let arr = match body {
            Value::Array(a) => a,
            other => vec![other],
        };
        let (mut roots, mut leaves) = (0usize, 0usize);
        for e in &arr {
            match e["id"].as_str() {
                Some(id) if id == root => roots += 1,
                Some(id) if id == leaf => leaves += 1,
                _ => {}
            }
        }
        assert_eq!(
            roots, 1,
            "the root must appear exactly once in the flattened array: {arr:?}"
        );
        assert_eq!(
            leaves, 1,
            "the genuine Linked Entity is still there once: {arr:?}"
        );
        assert_eq!(arr.len(), 2, "no other entity is in the array: {arr:?}");
    }

    /// 4.5.23.1: a cyclic graph at a high joinLevel shall not cascade — the
    /// walk stops at entities it already resolved and says so with an
    /// NGSILD-Warning (6.3.17) instead of expanding fan-out^joinLevel.
    #[tokio::test]
    async fn cyclic_inline_join_stops_instead_of_cascading() {
        let app = app();
        let ids = [
            "urn:ngsi-ld:Cyc:a",
            "urn:ngsi-ld:Cyc:b",
            "urn:ngsi-ld:Cyc:c",
        ];
        // complete graph: every entity links to every entity, itself included
        for id in ids {
            create(
                &app,
                json!({"id": id, "type": "Cyc",
                       "toA": {"type": "Relationship", "object": ids[0]},
                       "toB": {"type": "Relationship", "object": ids[1]},
                       "toC": {"type": "Relationship", "object": ids[2]}}),
            )
            .await;
        }

        let (parts, body) = get(
            &app,
            &format!("/ngsi-ld/v1/entities/{}?join=inline&joinLevel=9", ids[0]),
        )
        .await;
        assert_eq!(parts.status, StatusCode::OK);
        // 3^9 embeddings if the walk is unbounded; a handful if it is not
        let embedded = body.to_string().matches("urn:ngsi-ld:Cyc:").count();
        assert!(
            embedded < 64,
            "the cyclic walk cascaded: {embedded} entity references embedded"
        );
        assert!(
            parts.headers.get("NGSILD-Warning").is_some(),
            "a truncated Linked Entity Retrieval must be reported (6.3.17)"
        );
    }

    /// 4.5.23.1: joinLevel bounds the depth, not the width — one retrieval
    /// may only buy MAX_JOIN_LOOKUPS Linked Entity reads, and the truncation
    /// is reported back to the caller.
    #[test]
    fn wide_inline_join_stops_at_the_lookup_budget() {
        let st = AppState::new("antares-test".into());
        let tenant = TenantId::default();
        let ctx = antares_jsonld::Loader::new().core();
        let mut targets: Vec<Value> = Vec::new();
        for n in 0..MAX_JOIN_LOOKUPS + 100 {
            let id = format!("urn:ngsi-ld:Wide:{n}");
            let doc = json!({"id": &id, "type":
                ["https://uri.etsi.org/ngsi-ld/default-context/Wide"]});
            st.store
                .upsert(&tenant, Kind::Entity, &id, doc)
                .expect("seed");
            targets.push(Value::String(id));
        }
        let mut compacted = json!({"id": "urn:ngsi-ld:Wide:root", "type": "Wide",
            "links": {"type": "Relationship", "object": Value::Array(targets)}});
        let complete = inline_join(
            &st,
            &tenant,
            &ctx,
            &crate::repr::Repr::default(),
            &mut compacted,
            1,
        );
        assert!(!complete, "the budget was hit, so the walk is incomplete");
        assert_eq!(
            compacted["links"]["entity"]
                .as_array()
                .expect("entity array")
                .len(),
            MAX_JOIN_LOOKUPS,
            "no more Linked Entities than the budget are resolved"
        );
    }
}

/// 5.6.6.4 / 5.6.17.4 / 5.6.18.4 / 5.6.21.4: "matching input data is
/// forwarded to the Registration endpoint" — the forward may narrow what the
/// peer does, never widen it.
#[cfg(test)]
mod forwarded_input_data {
    use super::*;

    fn params(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    /// 5.6.21.3 lists the type selector, the id list, the id pattern, the
    /// restrictive and exclusionary Attribute-name lists, the query, the
    /// geoquery, the Scope query and the context source filter as Purge
    /// input data. Every one of them restricts the purge, so every one of
    /// them travels.
    #[test]
    fn purge_forward_carries_every_restriction_the_client_issued() {
        let p = params(&[
            ("type", "Vehicle"),
            ("id", "urn:ngsi-ld:Vehicle:A1"),
            ("idPattern", "^urn:ngsi-ld:Vehicle:"),
            ("attrs", "speed"),
            ("q", "speed>5"),
            ("georel", "near;maxDistance==100"),
            ("geometry", "Point"),
            ("coordinates", "[0,0]"),
            ("geoproperty", "location"),
            ("scopeQ", "/x"),
            ("csf", "name==p"),
            ("keep", "name"),
            ("local", "false"),
        ]);
        let q = forwarded_purge_query(&p);
        for k in [
            "type",
            "id",
            "idPattern",
            "attrs",
            "q",
            "georel",
            "geometry",
            "coordinates",
            "geoproperty",
            "scopeQ",
            "csf",
            "keep",
        ] {
            assert!(
                q.iter().any(|(a, _)| a == k),
                "{k} was dropped, so the peer executes a wider purge than the \
                 client issued: {q:?}"
            );
        }
        assert!(
            !q.iter().any(|(k, _)| k == "local"),
            "local scope is the reason a forward happens at all — it is not \
             itself forwarded (5.5.13): {q:?}"
        );
        assert_eq!(q.len(), 12, "nothing else is invented: {q:?}");
    }

    /// The exclusionary list travels on its own too — `drop=` alone must not
    /// reach the peer as a bare, entity-deleting purge.
    #[test]
    fn purge_forward_carries_drop_and_omits_absent_members() {
        let q = forwarded_purge_query(&params(&[("type", "Vehicle"), ("drop", "speed")]));
        assert!(
            q.contains(&("drop".to_owned(), "speed".to_owned())),
            "{q:?}"
        );
        assert_eq!(q.len(), 2, "absent parameters are not forwarded: {q:?}");
    }

    /// 5.6.6.3 / 5.6.17.3 / 5.6.18.3: the selector of Entity types is input
    /// data of Delete, Merge and Replace Entity. A registration may cover
    /// several types, so the peer needs the selector to reach the same
    /// verdict this broker reached locally.
    #[test]
    fn write_forwards_carry_the_type_selector() {
        assert_eq!(
            type_selector_query(&params(&[("type", "Vehicle"), ("local", "false")])),
            vec![("type".to_owned(), "Vehicle".to_owned())]
        );
        assert!(
            type_selector_query(&params(&[("local", "false")])).is_empty(),
            "no selector, nothing to forward"
        );
    }

    /// 5.7.2.4 applies q/geoquery/Scope query/Attributes only after remote
    /// parts have been aggregated, and 4.23 orders by the value of a member —
    /// neither survives a store-side projection of that member.
    #[test]
    fn projection_is_pushed_down_only_when_the_store_answer_is_final() {
        let plain = params(&[("type", "T"), ("pick", "name")]);
        assert!(
            proj_pushdown_allowed(true, &plain),
            "a purely local query keeps the pushdown"
        );
        assert!(
            !proj_pushdown_allowed(false, &plain),
            "federated candidates mean the filters run again after the merge"
        );
        assert!(
            !proj_pushdown_allowed(true, &params(&[("attrs", "name"), ("orderBy", "age")])),
            "the ordering member must survive to be compared"
        );
    }

    /// 6.3.10 makes `limit=0` legal only with `count=true` — an answer that is
    /// a count and no rows. That shape is pushed to the store; the filters the
    /// store cannot see (idPattern, 4.23 ordering, federated candidates merged
    /// per 5.7.2.4) still forfeit the page.
    #[test]
    fn the_count_only_page_is_pushed_down_but_store_blind_filters_are_not() {
        assert!(
            page_pushdown_allowed(
                true,
                &params(&[("type", "T"), ("limit", "0"), ("count", "true")])
            ),
            "a count is answered by counting, not by materializing the match set"
        );
        assert!(
            page_pushdown_allowed(true, &params(&[("type", "T"), ("limit", "10")])),
            "a plain local query keeps the pushdown"
        );
        assert!(
            !page_pushdown_allowed(true, &params(&[("type", "T"), ("idPattern", "^urn:")])),
            "idPattern is applied after the store, so it drops rows the SQL \
             page already counted"
        );
        assert!(
            !page_pushdown_allowed(true, &params(&[("type", "T"), ("orderBy", "speed")])),
            "4.23 orders the whole match set before the page is cut"
        );
        assert!(
            !page_pushdown_allowed(false, &params(&[("type", "T")])),
            "federated candidates are merged after the store answered"
        );
    }

    /// 6.3.10 count-only page (`limit=0&count=true`): the pushed shape — no
    /// rows from the store plus its pre-LIMIT count — must be the same answer
    /// the full scan builds from a materialized match set, page contents,
    /// count and Links included.
    #[test]
    fn the_count_only_page_is_the_same_answer_pushed_or_scanned() {
        let st = AppState::new("antares-test".into());
        let matches: Vec<Value> = (0..7)
            .map(|i| serde_json::json!({"id": format!("urn:ngsi-ld:T:{i}"), "type": "T"}))
            .collect();
        for extra in [vec![], vec![("offset", "3")]] {
            let mut pairs = vec![("type", "T"), ("limit", "0"), ("count", "true")];
            pairs.extend(extra.iter().copied());
            let p = params(&pairs);
            let scanned = paginate(&st, &p, matches.clone(), "/ngsi-ld/v1/entities").expect("scan");
            let pushed = paginate_pre(
                &st,
                &p,
                Vec::new(),
                "/ngsi-ld/v1/entities",
                // what the store's count(*) reports for the same query
                matches.len(),
            )
            .expect("pushed");
            assert_eq!(scanned.0, pushed.0, "page contents differ: {pairs:?}");
            assert_eq!(scanned.1, pushed.1, "count differs: {pairs:?}");
            assert_eq!(scanned.2, pushed.2, "Links differ: {pairs:?}");
            assert!(
                pushed.0.is_empty(),
                "a count-only page carries no Entity: {:?}",
                pushed.0
            );
            assert_eq!(pushed.1, Some(matches.len()), "the count is the match set");
            assert!(
                !pushed.2.iter().any(|l| l.contains("rel=\"next\"")),
                "a page of zero has no next page: {:?}",
                pushed.2
            );
        }
    }
}

/// 5.6.1 Create Entity and 5.6.21 Purge Entities, end to end over the store.
#[cfg(test)]
mod clause_5_6_1_and_5_6_21 {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use serde_json::json;
    use tower::ServiceExt;

    /// 5.6.1.5: the output is "the URI of the created Entity", returned in
    /// the Location header. An id is one path segment (RFC 3986 clause 3.3),
    /// so a `#` in it must not be able to end the segment.
    #[tokio::test]
    async fn location_header_percent_encodes_the_entity_id() {
        let app = crate::router(AppState::new("antares-test".into()));
        let id = "urn:ngsi-ld:Vehicle:A#4567";
        let payload = json!({"id": id, "type": "Vehicle"}).to_string();
        let resp = app
            .oneshot(
                Request::post("/ngsi-ld/v1/entities")
                    .header("Content-Type", "application/json")
                    .header("Content-Length", payload.len().to_string())
                    .body(Body::from(payload))
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::CREATED);
        let loc = resp
            .headers()
            .get("Location")
            .expect("Location header")
            .to_str()
            .expect("ascii");
        assert_eq!(loc, "/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:A%234567");
        assert!(
            !loc.contains('#'),
            "a raw # truncates the URL at the fragment and addresses another \
             resource: {loc}"
        );
    }

    fn seed(st: &AppState, tenant: &TenantId, n: usize) {
        for i in 0..n {
            let id = format!("urn:ngsi-ld:Purge:{i:05}");
            let doc = json!({"id": &id,
                "type": ["https://uri.etsi.org/ngsi-ld/default-context/Purge"],
                "https://uri.etsi.org/ngsi-ld/default-context/name":
                    [{"type": "Property", "value": "n"}],
                "https://uri.etsi.org/ngsi-ld/default-context/speed":
                    [{"type": "Property", "value": 1}]});
            st.store
                .upsert(tenant, Kind::Entity, &id, doc)
                .expect("seed");
        }
    }

    fn purge_params(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    /// 5.6.21.4: the implementation "shall delete all Entities that can be
    /// found locally using retrieved list of Entity ids" — all of them, not
    /// one page of them, however many rounds that takes.
    #[tokio::test]
    async fn purge_deletes_every_match_across_page_boundaries() {
        let st = AppState::new("antares-test".into());
        let tenant = TenantId::default();
        let n = PURGE_CHUNK * 2 + 7;
        seed(&st, &tenant, n);
        let resp = purge_inner(&st, &purge_params(&[("type", "Purge")]), &HeaderMap::new())
            .await
            .expect("purge");
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        assert!(
            st.store
                .list(&tenant, Kind::Entity)
                .expect("list")
                .is_empty(),
            "entities survived the purge"
        );
    }

    /// 5.6.21.4: with an exclusionary list the implementation "shall delete
    /// all but the given set of Attributes" — the Entities themselves
    /// survive, again for the whole match set and not just its first page.
    #[tokio::test]
    async fn purge_with_keep_prunes_every_match_and_deletes_no_entity() {
        let st = AppState::new("antares-test".into());
        let tenant = TenantId::default();
        let n = PURGE_CHUNK * 2 + 7;
        seed(&st, &tenant, n);
        let resp = purge_inner(
            &st,
            &purge_params(&[("type", "Purge"), ("keep", "name")]),
            &HeaderMap::new(),
        )
        .await
        .expect("purge");
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        let left = st.store.list(&tenant, Kind::Entity).expect("list");
        assert_eq!(left.len(), n, "keep= prunes attributes, not entities");
        for doc in &left {
            assert!(
                doc.get("https://uri.etsi.org/ngsi-ld/default-context/name")
                    .is_some(),
                "the kept attribute is still there: {doc}"
            );
            assert!(
                doc.get("https://uri.etsi.org/ngsi-ld/default-context/speed")
                    .is_none(),
                "every other attribute is gone: {doc}"
            );
        }
    }

    /// 5.6.21.4: "id matches the id pattern passed as a parameter" — the
    /// pattern narrows the match set, and only that set is deleted.
    #[tokio::test]
    async fn purge_deletes_the_id_pattern_matches_and_nothing_else() {
        let st = AppState::new("antares-test".into());
        let tenant = TenantId::default();
        seed(&st, &tenant, 30);
        let resp = purge_inner(
            &st,
            &purge_params(&[("type", "Purge"), ("idPattern", "^urn:ngsi-ld:Purge:0000")]),
            &HeaderMap::new(),
        )
        .await
        .expect("purge");
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        let left: Vec<String> = st
            .store
            .list(&tenant, Kind::Entity)
            .expect("list")
            .iter()
            .filter_map(|d| d["id"].as_str().map(str::to_owned))
            .collect();
        assert_eq!(
            left.len(),
            20,
            "only the ten pattern matches went: {left:?}"
        );
        assert!(
            !left
                .iter()
                .any(|id| id.starts_with("urn:ngsi-ld:Purge:0000")),
            "a pattern match survived: {left:?}"
        );
        assert!(
            left.contains(&"urn:ngsi-ld:Purge:00010".to_owned()),
            "an Entity the pattern does not match must not be purged: {left:?}"
        );
    }

    /// 5.6.21.4 deletes "all Entities that can be found locally using
    /// retrieved list of Entity ids". The retrieval is chunked, and idPattern
    /// is applied after the store — so a chunk can come back narrowed, or
    /// empty, while matches still wait behind it. The walk continues on the
    /// store's row count, never on the surviving subset.
    #[test]
    fn purge_walks_on_the_rows_the_store_returned_not_the_narrowed_subset() {
        assert_eq!(
            purge_next_offset(0, 500, 0, true, 500),
            Some(500),
            "a full chunk the pattern narrowed to nothing still has a successor"
        );
        assert_eq!(
            purge_next_offset(500, 500, 120, true, 500),
            Some(880),
            "the 380 rows the round did not remove are stepped over"
        );
        assert_ne!(
            purge_next_offset(0, 500, 0, true, 500),
            None,
            "stopping here leaves every match behind the first chunk alive"
        );
    }

    /// 5.6.21.4 against a store that really pages (memory answers every query
    /// with the whole match set, so it cannot exercise the walk): with an
    /// idPattern spread across the match set, every chunk arrives narrowed —
    /// and the purge still has to delete "all Entities that can be found
    /// locally using retrieved list of Entity ids", not just the first chunk's
    /// share. Skips without ANTARES_TEST_DATABASE_URL.
    #[cfg(feature = "postgres")]
    #[tokio::test(flavor = "multi_thread")]
    async fn purge_over_a_paging_store_deletes_every_id_pattern_match() {
        let url = match std::env::var("ANTARES_TEST_DATABASE_URL") {
            Ok(u) => u,
            Err(_) => {
                eprintln!("SKIP: ANTARES_TEST_DATABASE_URL not set");
                return;
            }
        };
        let pool = antares_sql::store::pg::connect(&url, 5)
            .await
            .expect("connect");
        let tenant = TenantId::new("purgepaging").expect("tenant");
        antares_sql::store::pg::ensure_tenant(&pool, &tenant)
            .await
            .expect("tenant row");
        let st = AppState::with_store(
            "antares-test".into(),
            std::sync::Arc::new(antares_sql::store::any::AnyStore::Pg(
                antares_sql::store::any::PgBackend::new(pool),
            )),
            "postgres",
        );
        for doc in st.store.list(&tenant, Kind::Entity).expect("list") {
            if let Some(id) = doc["id"].as_str() {
                st.store.delete(&tenant, Kind::Entity, id).expect("clean");
            }
        }
        // more than two chunks, and the pattern matches every second id — so
        // no chunk the store pages is full once the pattern filtered it
        let n = PURGE_CHUNK * 2 + 200;
        seed(&st, &tenant, n);
        // the purge must run AS the seeded tenant — with no NGSILD-Tenant
        // header it correctly purges the default tenant's (empty) match set
        // and every seeded row survives (5.5.10)
        let mut headers = HeaderMap::new();
        headers.insert("NGSILD-Tenant", "purgepaging".parse().expect("header"));
        let resp = purge_inner(
            &st,
            &purge_params(&[("type", "Purge"), ("idPattern", "[02468]$")]),
            &headers,
        )
        .await
        .expect("purge");
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        let left: Vec<String> = st
            .store
            .list(&tenant, Kind::Entity)
            .expect("list")
            .iter()
            .filter_map(|d| d["id"].as_str().map(str::to_owned))
            .collect();
        assert!(
            !left
                .iter()
                .any(|id| id.ends_with(['0', '2', '4', '6', '8'])),
            "{} of {} pattern matches survived the chunked walk",
            left.iter()
                .filter(|id| id.ends_with(['0', '2', '4', '6', '8']))
                .count(),
            n / 2
        );
        assert_eq!(
            left.len(),
            n / 2,
            "an Entity the pattern does not match must not be purged"
        );
        for id in &left {
            st.store.delete(&tenant, Kind::Entity, id).expect("clean");
        }
    }

    /// The same walk terminates: a chunk the store did not page IS the whole
    /// match set, a short chunk is the last one, and a round that deleted its
    /// whole chunk re-reads the window the deletions shifted down.
    #[test]
    fn purge_stops_when_the_match_set_is_exhausted() {
        assert_eq!(
            purge_next_offset(0, 500, 500, false, 500),
            None,
            "an unpaged answer is the whole match set"
        );
        assert_eq!(
            purge_next_offset(0, 7, 7, true, 500),
            None,
            "a chunk shorter than the page size is the last one"
        );
        assert_eq!(
            purge_next_offset(0, 500, 500, true, 500),
            Some(0),
            "everything deleted — the rest of the match set shifted to the front"
        );
    }
}

/// 5.6.17 Merge Entity and the Linked Entity Retrieval parameters of
/// Table 6.4.3.2-1, over the HTTP surface.
#[cfg(test)]
mod clause_5_6_17_and_6_4_3_2 {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use serde_json::json;
    use tower::ServiceExt;

    fn app() -> axum::Router {
        crate::router(AppState::new("antares-test".into()))
    }

    async fn create(app: &axum::Router, body: Value) {
        let payload = body.to_string();
        let resp = app
            .clone()
            .oneshot(
                Request::post("/ngsi-ld/v1/entities")
                    .header("Content-Type", "application/json")
                    .header("Content-Length", payload.len().to_string())
                    .body(Body::from(payload))
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::CREATED, "create failed");
    }

    async fn patch(app: &axum::Router, uri: &str, body: Value) -> StatusCode {
        let payload = body.to_string();
        app.clone()
            .oneshot(
                Request::patch(uri)
                    .header("Content-Type", "application/json")
                    .header("Content-Length", payload.len().to_string())
                    .body(Body::from(payload))
                    .expect("req"),
            )
            .await
            .expect("resp")
            .status()
    }

    async fn get(app: &axum::Router, uri: &str) -> Value {
        let resp = app
            .clone()
            .oneshot(Request::get(uri).body(Body::empty()).expect("req"))
            .await
            .expect("resp");
        let bytes = resp.into_body().collect().await.expect("body").to_bytes();
        serde_json::from_slice(&bytes).expect("json")
    }

    /// 5.6.17.3: "An optional parameter indicating a common observedAt
    /// timestamp to use across merged Attributes." It is the timestamp of
    /// every Attribute this merge touches; an Attribute the Fragment gives an
    /// observedAt of its own keeps that one, and an Attribute the Fragment
    /// does not mention is not a merged Attribute and is left alone.
    #[tokio::test]
    async fn merge_applies_the_common_observed_at() {
        let app = app();
        let id = "urn:ngsi-ld:Obs:1";
        create(
            &app,
            json!({"id": id, "type": "T",
                   "a": {"type": "Property", "value": 1},
                   "b": {"type": "Property", "value": 1},
                   "untouched": {"type": "Property", "value": 1,
                                 "observedAt": "2020-01-01T00:00:00Z"}}),
        )
        .await;
        let uri = format!("/ngsi-ld/v1/entities/{id}?observedAt=2026-08-17T10:00:00Z");
        assert_eq!(
            patch(
                &app,
                &uri,
                json!({"a": {"value": 2},
                       "b": {"value": 2, "observedAt": "2021-01-01T00:00:00Z"}}),
            )
            .await,
            StatusCode::NO_CONTENT
        );
        let body = get(&app, &format!("/ngsi-ld/v1/entities/{id}")).await;
        assert_eq!(
            body["a"]["observedAt"], "2026-08-17T10:00:00Z",
            "the merged Attribute takes the common timestamp: {body}"
        );
        assert_eq!(
            body["b"]["observedAt"], "2021-01-01T00:00:00Z",
            "a Fragment instance carrying its own observedAt keeps it: {body}"
        );
        assert_eq!(
            body["untouched"]["observedAt"], "2020-01-01T00:00:00Z",
            "an Attribute this merge does not touch is not restamped: {body}"
        );
    }

    /// 4.8 makes observedAt a DateTime, so a value that is not one cannot be
    /// stamped across the merged Attributes.
    #[tokio::test]
    async fn merge_rejects_an_observed_at_that_is_not_a_datetime() {
        let app = app();
        let id = "urn:ngsi-ld:Obs:2";
        create(&app, json!({"id": id, "type": "T"})).await;
        assert_eq!(
            patch(
                &app,
                &format!("/ngsi-ld/v1/entities/{id}?observedAt=yesterday"),
                json!({"a": {"type": "Property", "value": 1}}),
            )
            .await,
            StatusCode::BAD_REQUEST
        );
    }

    /// 5.6.17.4: "If a common language tag is defined and a LanguageProperty
    /// Attribute to be merged is represented as a string, the pre-existing
    /// languageMap JSON object shall be preserved. The string value shall
    /// only replace the value associated to the language tag key found
    /// within the languageMap."
    #[tokio::test]
    async fn merge_with_a_common_lang_replaces_only_that_language_key() {
        let app = app();
        let id = "urn:ngsi-ld:Lang:1";
        create(
            &app,
            json!({"id": id, "type": "T",
                   "greeting": {"type": "LanguageProperty",
                                "languageMap": {"en": "hello", "es": "adios"}}}),
        )
        .await;
        assert_eq!(
            patch(
                &app,
                &format!("/ngsi-ld/v1/entities/{id}?lang=es"),
                json!({"greeting": "hola"}),
            )
            .await,
            StatusCode::NO_CONTENT
        );
        let body = get(&app, &format!("/ngsi-ld/v1/entities/{id}")).await;
        assert_eq!(
            body["greeting"]["languageMap"],
            json!({"en": "hello", "es": "hola"}),
            "the pre-existing languageMap survives, only the tagged key moves: {body}"
        );
        assert!(
            body["greeting"].get("value").is_none(),
            "the string never lands as a plain Property value: {body}"
        );
        assert_eq!(body["greeting"]["type"], "LanguageProperty", "{body}");
    }

    /// Table 6.4.3.2-1 containedBy: "List of entity ids which have previously
    /// been encountered whilst retrieving the Entity Graph" — 4.5.23.1 keeps
    /// the walk from retrieving them a second time.
    #[tokio::test]
    async fn contained_by_ids_are_not_retrieved_again() {
        let app = app();
        let (root, held, fresh) = (
            "urn:ngsi-ld:Graph:root",
            "urn:ngsi-ld:Graph:held",
            "urn:ngsi-ld:Graph:fresh",
        );
        create(&app, json!({"id": held, "type": "G"})).await;
        create(&app, json!({"id": fresh, "type": "G"})).await;
        create(
            &app,
            json!({"id": root, "type": "G",
                   "toHeld": {"type": "Relationship", "object": held},
                   "toFresh": {"type": "Relationship", "object": fresh}}),
        )
        .await;

        let body = get(
            &app,
            &format!("/ngsi-ld/v1/entities/{root}?join=flat&joinLevel=2&containedBy={held}"),
        )
        .await;
        let arr = match body {
            Value::Array(a) => a,
            other => vec![other],
        };
        let ids: Vec<&str> = arr.iter().filter_map(|e| e["id"].as_str()).collect();
        assert!(
            !ids.contains(&held),
            "an id the client already holds is not retrieved again: {ids:?}"
        );
        assert!(
            ids.contains(&fresh),
            "the Linked Entity it does not hold is still returned: {ids:?}"
        );
        assert!(ids.contains(&root), "the Linking Entity is there: {ids:?}");
    }
}
