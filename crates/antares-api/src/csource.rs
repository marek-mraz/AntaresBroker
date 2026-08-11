//! /csourceRegistrations (5.9, 5.10; resources 6.8/6.9).

use crate::negotiate::*;
use crate::state::{now_iso, AppState};
use antares_jsonld::{parse_datetime, Context};
use antares_model::NgsiError;
use antares_sql::store::Kind;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{Map, Value};
use std::collections::HashMap;

use crate::negotiate::CleanParams;

/// Validate + normalize a CSourceRegistration (5.2.9): types and attribute
/// names inside `information` expand to IRIs.
/// §16.3 cardinality caps on a CSourceRegistration. Generous against any real
/// federation topology (§16.7 sizes a tenant at 1000+ registrations, not one
/// registration at 1000+ selectors) and small enough that the worst case is
/// MAX_INFORMATION × MAX_INFO_MEMBERS² index rows, not 10^10.
const MAX_INFORMATION: usize = 128;
const MAX_INFO_MEMBERS: usize = 128;

pub fn normalize_registration(
    doc: &Map<String, Value>,
    ctx: &Context,
    is_patch: bool,
) -> Result<Map<String, Value>, NgsiError> {
    let bad = |m: String| NgsiError::BadRequestData(m);
    let mut out = Map::new();
    for (k, v) in doc {
        // NGSI-LD Fragment member removal (5.4): null / NGSI-LD Null
        if is_patch && k != "id" && (v.is_null() || v.as_str() == Some("urn:ngsi-ld:null")) {
            if ["type", "information", "endpoint"].contains(&k.as_str()) {
                return Err(bad(format!("cannot remove mandatory member {k} (5.9.3)")));
            }
            out.insert(k.clone(), Value::Null);
            continue;
        }
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
                        "type must be \"ContextSourceRegistration\" (5.2.9)".into()
                    ));
                }
                out.insert("type".into(), v.clone());
            }
            "information" => {
                let arr = v
                    .as_array()
                    .filter(|a| !a.is_empty())
                    .ok_or_else(|| bad("information must be a non-empty array (5.2.9)".into()))?;
                // §16.3: the csource_index explosion is |entities| ×
                // (|propertyNames| + |relationshipNames|) PER information
                // element, materialised in memory before any SQL runs. Under
                // only the 4 MiB body cap that is ~10^10 objects — an OOM from
                // one request. Cardinality is capped at the validation
                // boundary, where the error is a 400 and not a dead pod.
                if arr.len() > MAX_INFORMATION {
                    return Err(NgsiError::TooComplexQuery(format!(
                        "information has {} entries (limit {MAX_INFORMATION})",
                        arr.len()
                    )));
                }
                let mut infos = Vec::new();
                for info in arr {
                    let io = info
                        .as_object()
                        .ok_or_else(|| bad("information entries must be objects".into()))?;
                    for key in ["entities", "propertyNames", "relationshipNames"] {
                        if let Some(n) = io.get(key).and_then(Value::as_array).map(Vec::len) {
                            if n > MAX_INFO_MEMBERS {
                                return Err(NgsiError::TooComplexQuery(format!(
                                    "information.{key} has {n} entries (limit {MAX_INFO_MEMBERS})"
                                )));
                            }
                        }
                    }
                    let mut ni = Map::new();
                    for (ik, iv) in io {
                        match ik.as_str() {
                            "entities" => {
                                let es =
                                    iv.as_array().filter(|a| !a.is_empty()).ok_or_else(|| {
                                        bad("entities must be a non-empty array".into())
                                    })?;
                                let mut nes = Vec::new();
                                for e in es {
                                    let eo = e.as_object().ok_or_else(|| {
                                        bad("entities entries must be objects".into())
                                    })?;
                                    let mut ne = Map::new();
                                    for (ek, ev) in eo {
                                        match ek.as_str() {
                                            // 5.2.8: type is "String or String[]" — both forms legal.
                                            "type" => {
                                                let expand_one =
                                                    |t: &Value| -> Result<Value, NgsiError> {
                                                        let t = t.as_str().filter(|t| !t.is_empty()).ok_or_else(|| {
                                                        bad("EntityInfo type must be a non-empty string (5.2.8)".into())
                                                    })?;
                                                        Ok(Value::String(ctx.expand_key(t)))
                                                    };
                                                let expanded = match ev {
                                                    Value::Array(ts) if !ts.is_empty() => Value::Array(
                                                        ts.iter().map(expand_one).collect::<Result<_, _>>()?,
                                                    ),
                                                    Value::Array(_) => {
                                                        return Err(bad(
                                                            "EntityInfo type array must not be empty (5.2.8)".into(),
                                                        ))
                                                    }
                                                    other => expand_one(other)?,
                                                };
                                                ne.insert("type".into(), expanded);
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
                                    // type is optional in EntityInfo when an
                                    // id/idPattern identifies the entities
                                    if !ne.contains_key("type")
                                        && !ne.contains_key("id")
                                        && !ne.contains_key("idPattern")
                                    {
                                        return Err(bad(
                                            "EntityInfo requires type, id or idPattern (5.2.8)"
                                                .into(),
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
                                    let s = n.as_str().ok_or_else(|| {
                                        bad(format!("{ik} entries must be strings"))
                                    })?;
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
            "mode" => {
                let m = v
                    .as_str()
                    .filter(|m| ["inclusive", "auxiliary", "exclusive", "redirect"].contains(m))
                    .ok_or_else(|| {
                        bad(
                            "mode must be inclusive, auxiliary, exclusive or redirect (5.2.9)"
                                .into(),
                        )
                    })?;
                out.insert("mode".into(), Value::String(m.to_owned()));
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
            // 5.2.9 `tenant`: the Tenant to use in all requests to this
            // source — validated with the same rules as the header (4.14).
            "tenant" => {
                let t = v
                    .as_str()
                    .ok_or_else(|| bad("tenant must be a string (5.2.9)".into()))?;
                antares_model::TenantId::new(t)?;
                out.insert("tenant".into(), v.clone());
            }
            // 4.3.6.5: KeyValuePair[] conveyed when contacting the source.
            "contextSourceInfo" => {
                let arr = v
                    .as_array()
                    .ok_or_else(|| bad("contextSourceInfo must be an array (5.2.9)".into()))?;
                for kv in arr {
                    let Some(key) = kv.get("key").and_then(Value::as_str) else {
                        return Err(bad(
                            "contextSourceInfo entries must be {key, value} pairs (5.2.22)".into(),
                        ));
                    };
                    let Some(value) = kv.get("value") else {
                        return Err(bad(
                            "contextSourceInfo entries must be {key, value} pairs (5.2.22)".into(),
                        ));
                    };
                    // 4.3.6.6 (V-29): the four processed keys have constrained
                    // value spaces — reject bad ones at registration, not at
                    // first forward
                    let sval = value.as_str();
                    match key.to_ascii_lowercase().as_str() {
                        "accept" | "contenttype" => {
                            if !matches!(sval, Some("application/json" | "application/ld+json")) {
                                return Err(bad(format!(
                                    "contextSourceInfo {key} must be application/json or \
                                     application/ld+json (4.3.6.6)"
                                )));
                            }
                        }
                        "jsonldcontext" => {
                            if sval.is_none_or(|s| antares_model::EntityId::new(s).is_err()) {
                                return Err(bad(
                                    "contextSourceInfo jsonldContext must be a URL (4.3.6.6)"
                                        .into(),
                                ));
                            }
                        }
                        "ngsildconformance"
                            if sval
                                .is_none_or(|s| crate::conformance::parse_version(s).is_none()) =>
                        {
                            return Err(bad(
                                "contextSourceInfo ngsildConformance must be major.minor \
                                     (4.3.6.6)"
                                    .into(),
                            ));
                        }
                        _ => {}
                    }
                }
                out.insert("contextSourceInfo".into(), v.clone());
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
            return Err(bad(
                "type must be \"ContextSourceRegistration\" (5.2.9)".into()
            ));
        }
        if !out.contains_key("endpoint") {
            return Err(bad("endpoint is required (5.2.9)".into()));
        }
        if !out.contains_key("information") {
            return Err(bad("information is required (5.2.9)".into()));
        }
        validate_exclusive(&out)?;
    }
    Ok(out)
}

/// 4.3.6.3 Proxied Registrations: "An exclusive registration shall always
/// relate to specific Attributes found on a single Entity. Thus, the
/// registration shall define both: an entity id (i.e. an id pattern or Entity
/// type defining a group of entities is not supported for exclusive
/// registrations) [and] Attributes."
pub fn validate_exclusive(doc: &Map<String, Value>) -> Result<(), NgsiError> {
    if doc.get("mode").and_then(Value::as_str) != Some("exclusive") {
        return Ok(());
    }
    let bad = |m: &str| NgsiError::BadRequestData(format!("{m} (4.3.6.3)"));
    let infos = doc
        .get("information")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    for info in infos {
        let has_attrs = ["propertyNames", "relationshipNames"].iter().any(|k| {
            info.get(*k)
                .and_then(Value::as_array)
                .is_some_and(|a| !a.is_empty())
        });
        if !has_attrs {
            return Err(bad("an exclusive registration shall define Attributes"));
        }
        let ids_only = info
            .get("entities")
            .and_then(Value::as_array)
            .is_some_and(|es| {
                es.iter().all(|e| {
                    e.get("id").and_then(Value::as_str).is_some() && e.get("idPattern").is_none()
                })
            });
        if !ids_only {
            return Err(bad(
                "an exclusive registration shall name an entity id — an id pattern or \
                 Entity type defining a group of entities is not supported",
            ));
        }
    }
    Ok(())
}

/// 4.3.6.3: "Once an exclusive Context Source Registration has been created,
/// no further exclusive or redirect Context Source Registrations can be
/// created for that same combination of Entity ID and Attributes" — and per
/// 5.9.2, registering an exclusive Context Source when "an exclusive or
/// redirect Context Source Registration already matches against the Entity ID
/// (URI) and any of the Attributes defined in the registration" raises a
/// Conflict (409; Table 6.3.2-1 defines no Conflict type, so it travels as
/// AlreadyExists — the project's standing 409 mapping). Redirect overlapping
/// redirect stays legal: "operations are distributed to all registered
/// Context Sources".
pub fn check_proxied_overlap(
    st: &AppState,
    tenant: &antares_model::TenantId,
    doc: &Map<String, Value>,
    self_id: Option<&str>,
    ctx: &Context,
) -> Result<(), NgsiError> {
    let mode = doc
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("inclusive");
    if mode != "exclusive" && mode != "redirect" {
        return Ok(());
    }
    let infos = doc
        .get("information")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let now = now_iso();
    let existing = st
        .store
        .list(tenant, Kind::Registration)
        .unwrap_or_default();
    for other in &existing {
        if other.get("id").and_then(Value::as_str) == self_id {
            continue;
        }
        if other
            .get("expiresAt")
            .and_then(Value::as_str)
            .is_some_and(|e| e < now.as_str())
        {
            continue;
        }
        let omode = other
            .get("mode")
            .and_then(Value::as_str)
            .unwrap_or("inclusive");
        // new exclusive × existing proxied; new redirect × existing exclusive
        let guarded = match mode {
            "exclusive" => omode == "exclusive" || omode == "redirect",
            _ => omode == "exclusive",
        };
        if !guarded {
            continue;
        }
        for info in infos {
            let attrs: Vec<String> = ["propertyNames", "relationshipNames"]
                .iter()
                .filter_map(|k| info.get(*k).and_then(Value::as_array))
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect();
            let empty = Vec::new();
            let entities = info
                .get("entities")
                .and_then(Value::as_array)
                .unwrap_or(&empty);
            let ids: Vec<String> = entities
                .iter()
                .filter_map(|e| e.get("id").and_then(Value::as_str))
                .map(str::to_owned)
                .collect();
            let types: Vec<String> = entities
                .iter()
                .flat_map(ei_types)
                .map(str::to_owned)
                .collect();
            let spec = CsrSpec {
                types: (!types.is_empty()).then_some(types),
                ids: (!ids.is_empty()).then_some(ids),
                id_pattern: entities
                    .iter()
                    .find_map(|e| e.get("idPattern").and_then(Value::as_str))
                    .map(str::to_owned),
                attrs: (!attrs.is_empty()).then_some(attrs),
            };
            if csr_matches(&spec, other, ctx) {
                let oid = other.get("id").and_then(Value::as_str).unwrap_or("?");
                return Err(NgsiError::AlreadyExists(format!(
                    "proxied registration overlaps {oid} for the same combination of \
                     Entity ID and Attributes (4.3.6.3)"
                )));
            }
        }
    }
    Ok(())
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
                                    match ne.get("type") {
                                        Some(Value::String(t)) => {
                                            let c = ctx.compact_iri(t);
                                            ne.insert("type".into(), Value::String(c));
                                        }
                                        Some(Value::Array(ts)) => {
                                            let cs: Vec<Value> = ts
                                                .iter()
                                                .filter_map(Value::as_str)
                                                .map(|t| Value::String(ctx.compact_iri(t)))
                                                .collect();
                                            ne.insert("type".into(), Value::Array(cs));
                                        }
                                        _ => {}
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
    CleanParams(params): CleanParams,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        check_params(&params, &["local"])?;
        let parsed = parse_body(&st.loader, &headers, &body, BodyKind::Standard).await?;
        let obj = parsed.value.as_object().ok_or_else(|| {
            NgsiError::BadRequestData("registration must be a JSON object".into())
        })?;
        let mut norm = normalize_registration(obj, &parsed.ctx, false)?;
        let id = match norm.get("id").and_then(Value::as_str) {
            Some(id) => id.to_owned(),
            None => {
                let id = format!(
                    "urn:ngsi-ld:ContextSourceRegistration:{}",
                    uuid::Uuid::new_v4()
                );
                norm.insert("id".into(), Value::String(id.clone()));
                id
            }
        };
        check_proxied_overlap(&st, &tenant, &norm, None, &parsed.ctx)?;
        let ts = now_iso();
        norm.insert("createdAt".into(), Value::String(ts.clone()));
        norm.insert("modifiedAt".into(), Value::String(ts));
        let doc = Value::Object(norm);
        if !st
            .store
            .create(&tenant, Kind::Registration, &id, doc.clone())?
        {
            return Err(
                NgsiError::AlreadyExists(format!("registration {id} already exists")).into(),
            );
        }
        st.reg_changed(&tenant, &id, Some(&doc));
        {
            // Prepare in the request path (ordering), spawn only the send
            // (the ack must not block on the receiver).
            let jobs = crate::notify::prepare_csource_jobs(&st, &tenant, None, Some(doc)).await;
            let (st2, t2) = (st.clone(), tenant.clone());
            crate::spawn(async move {
                crate::notify::send_csource_jobs(&st2, &t2, jobs).await;
            });
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
    CleanParams(params): CleanParams,
    headers: HeaderMap,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        antares_model::EntityId::new(&id)
            .map_err(|_| NgsiError::BadRequestData(format!("invalid registration id {id:?}")))?;
        check_params(&params, &["options", "format", "local"])?;
        let accept = parse_accept(&headers)?;
        let ctx = request_context(&st.loader, &headers).await?;
        let doc = st
            .store
            .get(&tenant, Kind::Registration, &id)?
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
    CleanParams(params): CleanParams,
    headers: HeaderMap,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        check_params(
            &params,
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
                "timeproperty",
                "timerel",
                "timeAt",
                "endTimeAt",
                "csf",
                "limit",
                "offset",
                "count",
                "options",
                "format",
                "local",
                "scopeQ",
            ],
        )?;
        let accept = parse_accept(&headers)?;
        let ctx = request_context(&st.loader, &headers).await?;
        let bad = |m: String| NgsiError::BadRequestData(m);
        let mut spec = CsrSpec::default();
        if let Some(s) = params.get("id") {
            let mut ids = Vec::new();
            for i in s.split(',') {
                antares_model::EntityId::new(i)
                    .map_err(|_| bad(format!("invalid id in list: {i:?}")))?;
                ids.push(i.to_owned());
            }
            spec.ids = Some(ids);
        }
        spec.id_pattern = params
            .get("idPattern")
            .map(|p| {
                regex::Regex::new(p)
                    .map(|_| p.clone())
                    .map_err(|_| bad(format!("invalid idPattern {p:?}")))
            })
            .transpose()?;
        spec.types = params
            .get("type")
            .map(|s| s.split(',').map(|t| ctx.expand_key(t.trim())).collect());
        let mut attrs: Vec<String> = params
            .get("attrs")
            .map(|s| s.split(',').map(|t| ctx.expand_key(t.trim())).collect())
            .unwrap_or_default();
        // attributes referenced in q / geoQ count as query projection
        // attributes for matching (5.10.2.4)
        let q = params
            .get("q")
            .map(|s| crate::negotiate::percent_decode(s.as_bytes()));
        if let Some(q) = &q {
            let ast = antares_ql::parse_q(q)?;
            let mut roots = Vec::new();
            q_attr_roots(&ast, &mut roots);
            attrs.extend(roots.into_iter().map(|r| ctx.expand_key(&r)));
        }
        let geo = crate::geo::GeoQuery::from_params(&params)?;
        if let Some(g) = &geo {
            attrs.push(ctx.expand_key(&g.geoproperty));
        }
        if !attrs.is_empty() {
            spec.attrs = Some(attrs);
        }
        // 5.10.2.4: a discriminating input is required, else too wide
        // (the suite additionally accepts id-only queries — 037_10_01)
        if spec.types.is_none()
            && spec.attrs.is_none()
            && spec.ids.is_none()
            && spec.id_pattern.is_none()
        {
            return Err(bad(
                "query too wide: one of type, attrs, q or geo query is required (5.10.2.4)".into(),
            )
            .into());
        }
        // temporal query: validate + interval presence rules (5.10.2.4)
        let temporal =
            crate::temporal::TemporalQ::from_params(&params, false)?.filter(|t| t.timerel != "any");
        let all = st.store.list(&tenant, Kind::Registration)?;
        let matches: Vec<Value> = all
            .into_iter()
            .filter(|doc| {
                let has_interval = doc.get("observationInterval").is_some()
                    || doc.get("managementInterval").is_some();
                match &temporal {
                    None if has_interval => return false,
                    Some(tq) if !temporal_interval_matches(doc, tq) => return false,
                    _ => {}
                }
                csr_matches(&spec, doc, &ctx)
            })
            .collect();
        let (page, count_hdr, links) = crate::entities::paginate_accept(
            &st,
            &params,
            matches,
            "/ngsi-ld/v1/csourceRegistrations",
            accept,
        )?;
        let sys = params
            .get("options")
            .is_some_and(|o| o.split(',').any(|s| s.trim() == "sysAttrs"));
        let payload: Vec<Value> = page
            .iter()
            .map(|d| present_registration(d, &ctx, sys))
            .collect();
        let mut resp =
            crate::negotiate::respond_list(StatusCode::OK, payload, &ctx, accept, &tenant);
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

/// Root attribute names referenced by a q= expression (5.10.2.4: they count
/// as query projection attributes for RegistrationInfo matching).
fn q_attr_roots(node: &antares_ql::QNode, out: &mut Vec<String>) {
    use antares_ql::QNode::*;
    match node {
        And(v) | Or(v) => v.iter().for_each(|n| q_attr_roots(n, out)),
        Cmp { path, .. } | Exists { path, .. } => {
            if let Some(r) = path.top() {
                out.push(r.to_owned());
            }
        }
    }
}

/// 5.10.2.4 temporal matching against observationInterval/managementInterval.
fn temporal_interval_matches(doc: &Value, tq: &crate::temporal::TemporalQ) -> bool {
    let key = if tq.timeproperty == "observedAt" {
        "observationInterval"
    } else {
        "managementInterval"
    };
    let Some(iv) = doc.get(key).and_then(Value::as_object) else {
        return false; // relevant interval not present ⇒ no match
    };
    // 4.11 comparison on the canonical key — equal instants in different
    // 4.6.3 fraction spellings must hit the bounds exactly.
    let dt = crate::temporal::dt_key;
    let start = dt(iv.get("startAt").and_then(Value::as_str).unwrap_or(""));
    let end = iv.get("endAt").and_then(Value::as_str).map(dt); // open-ended when absent
    match tq.timerel.as_str() {
        // interval contains times before/after timeAt (037_09, 047_10/11)
        "before" => start < dt(&tq.time_at),
        "after" => end.is_none_or(|e| e > dt(&tq.time_at)),
        "between" => {
            // overlap between [timeAt, endTimeAt] and the interval
            let qe = dt(tq.end_time_at.as_deref().unwrap_or(&tq.time_at));
            dt(&tq.time_at) <= end.unwrap_or_else(|| "9999".into()) && qe >= start
        }
        _ => true,
    }
}

/// The entity/attribute specification matched against registrations (5.12).
#[derive(Default)]
pub struct CsrSpec {
    /// Expanded type IRIs (or raw 4.17 selector expressions).
    pub types: Option<Vec<String>>,
    pub ids: Option<Vec<String>>,
    pub id_pattern: Option<String>,
    /// Expanded attribute IRIs.
    pub attrs: Option<Vec<String>>,
}

/// 5.2.8: EntityInfo type is a String or String[] — yield every named type.
fn ei_types(ei: &Value) -> Vec<&str> {
    match ei.get("type") {
        Some(Value::String(s)) => vec![s.as_str()],
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    }
}

fn type_matches(sel: &str, info_type: &str, ctx: &Context) -> bool {
    if sel.contains(['|', ',', ';', '(']) {
        crate::entities::type_selection_matches(sel, &[info_type], ctx)
    } else {
        sel == info_type
    }
}

/// 5.12: does an EntityInfo element match the entity specification?
fn entity_info_matches(spec: &CsrSpec, ei: &Value, ctx: &Context) -> bool {
    if let Some(types) = &spec.types {
        let its = ei_types(ei);
        // EntityInfo without a type restricts only by id/idPattern
        if !its.is_empty()
            && !types
                .iter()
                .any(|t| its.iter().any(|it| type_matches(t, it, ctx)))
        {
            return false;
        }
    }
    let ei_id = ei.get("id").and_then(Value::as_str);
    let ei_pat = ei.get("idPattern").and_then(Value::as_str);
    if ei_id.is_none() && ei_pat.is_none() {
        return true;
    }
    if let Some(ids) = &spec.ids {
        if let Some(rid) = ei_id {
            if ids.iter().any(|i| i == rid) {
                return true;
            }
        }
        if let Some(p) = ei_pat {
            if let Ok(re) = regex::Regex::new(p) {
                if ids.iter().any(|i| re.find(i).is_some()) {
                    return true;
                }
            }
        }
    }
    if let Some(qp) = &spec.id_pattern {
        if let Some(rid) = ei_id {
            if regex::Regex::new(qp).is_ok_and(|re| re.find(rid).is_some()) {
                return true;
            }
        }
        if ei_pat.is_some() {
            return true; // both patterns present ⇒ assumed compatible (5.12)
        }
    }
    // no id restriction given by the query side ⇒ EntityInfo id restrictions
    // don't exclude it when the type matched
    spec.ids.is_none() && spec.id_pattern.is_none()
}

fn attrs_match_info(attrs: &Option<Vec<String>>, info: &Value) -> bool {
    let Some(attrs) = attrs else { return true };
    if attrs.is_empty() {
        return true;
    }
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
}

/// 5.12: the RegistrationInfo elements of `doc.information` that match `spec`.
pub fn matching_infos<'a>(spec: &CsrSpec, doc: &'a Value, ctx: &Context) -> Vec<&'a Value> {
    let Some(infos) = doc.get("information").and_then(Value::as_array) else {
        return Vec::new();
    };
    infos
        .iter()
        .filter(|info| {
            let entity_ok = match info.get("entities").and_then(Value::as_array) {
                None => true,
                Some(es) => es.iter().any(|ei| entity_info_matches(spec, ei, ctx)),
            };
            entity_ok && attrs_match_info(&spec.attrs, info)
        })
        .collect()
}

pub fn csr_matches(spec: &CsrSpec, doc: &Value, ctx: &Context) -> bool {
    !matching_infos(spec, doc, ctx).is_empty()
}

/// Full 5.11.2.4 match of a registration against a csource subscription:
/// 5.12 entity/attr matching + temporal interval rules + geoQ vs the
/// registration's own `location`.
pub fn csr_matches_subscription(sub: &Value, reg: &Value, ctx: &Context) -> bool {
    let spec = spec_for_subscription(sub);
    if !csr_matches(&spec, reg, ctx) {
        return false;
    }
    let has_interval =
        reg.get("observationInterval").is_some() || reg.get("managementInterval").is_some();
    match sub.get("temporalQ").and_then(Value::as_object) {
        None => {
            if has_interval {
                return false; // latest-information sources only (5.11.2.4)
            }
        }
        Some(tq) => {
            let mut params: HashMap<String, String> = HashMap::new();
            for k in ["timerel", "timeAt", "endTimeAt", "timeproperty"] {
                if let Some(s) = tq.get(k).and_then(Value::as_str) {
                    params.insert(k.into(), s.into());
                }
            }
            if let Ok(Some(t)) = crate::temporal::TemporalQ::from_params(&params, false) {
                if t.timerel != "any" && !temporal_interval_matches(reg, &t) {
                    return false;
                }
            }
        }
    }
    if let Some(g) = sub.get("geoQ").and_then(Value::as_object) {
        let mut params: HashMap<String, String> = HashMap::new();
        for k in ["georel", "geometry", "geoproperty"] {
            if let Some(s) = g.get(k).and_then(Value::as_str) {
                params.insert(k.into(), s.into());
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
        if let Ok(Some(gq)) = crate::geo::GeoQuery::from_params(&params) {
            match reg.get("location") {
                Some(geom) => {
                    if !gq.matches_geometry(geom) {
                        return false;
                    }
                }
                None => return false,
            }
        }
    }
    true
}

/// Build the 5.12 spec for a csource subscription (5.11.2.4): entities
/// selectors + watchedAttributes ∪ notification.attributes.
pub fn spec_for_subscription(sub: &Value) -> CsrSpec {
    let mut spec = CsrSpec::default();
    if let Some(es) = sub.get("entities").and_then(Value::as_array) {
        let mut types = Vec::new();
        let mut ids = Vec::new();
        for e in es {
            if let Some(t) = e.get("type").and_then(Value::as_str) {
                types.push(t.to_owned());
            }
            if let Some(i) = e.get("id").and_then(Value::as_str) {
                ids.push(i.to_owned());
            }
            if spec.id_pattern.is_none() {
                spec.id_pattern = e
                    .get("idPattern")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
            }
        }
        if !types.is_empty() {
            spec.types = Some(types);
        }
        if !ids.is_empty() {
            spec.ids = Some(ids);
        }
    }
    let mut attrs: Vec<String> = sub
        .get("watchedAttributes")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    if let Some(na) = sub
        .get("notification")
        .and_then(|n| n.get("attributes"))
        .and_then(Value::as_array)
    {
        attrs.extend(na.iter().filter_map(Value::as_str).map(str::to_owned));
    }
    if !attrs.is_empty() {
        spec.attrs = Some(attrs);
    }
    spec
}

pub async fn update_registration(
    State(st): State<AppState>,
    Path(id): Path<String>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        antares_model::EntityId::new(&id)
            .map_err(|_| NgsiError::BadRequestData(format!("invalid registration id {id:?}")))?;
        check_params(&params, &["local"])?;
        let parsed = parse_body(&st.loader, &headers, &body, BodyKind::MergePatch).await?;
        let obj = parsed
            .value
            .as_object()
            .ok_or_else(|| NgsiError::BadRequestData("fragment must be a JSON object".into()))?;
        let norm = normalize_registration(obj, &parsed.ctx, true)?;
        let ts = now_iso();
        let before = st.store.get(&tenant, Kind::Registration, &id)?;
        if let Some(prev) = before.as_ref().and_then(Value::as_object) {
            // validate the post-merge document (4.3.6.3) BEFORE mutating:
            // a patch may flip the mode or rewrite information
            let mut merged = prev.clone();
            for (k, v) in &norm {
                if k == "id" {
                    continue;
                }
                if v.is_null() {
                    merged.remove(k);
                } else {
                    merged.insert(k.clone(), v.clone());
                }
            }
            validate_exclusive(&merged)?;
            check_proxied_overlap(&st, &tenant, &merged, Some(&id), &parsed.ctx)?;
        }
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
        })?;
        match res {
            None => Err(NgsiError::ResourceNotFound(format!("registration {id} not found")).into()),
            Some(Err(e)) => Err(ApiError::from(e)),
            Some(Ok(())) => {
                let after = st.store.get(&tenant, Kind::Registration, &id)?;
                st.reg_changed(&tenant, &id, after.as_ref());
                let jobs = crate::notify::prepare_csource_jobs(&st, &tenant, before, after).await;
                let (st2, t2) = (st.clone(), tenant.clone());
                crate::spawn(async move {
                    crate::notify::send_csource_jobs(&st2, &t2, jobs).await;
                });
                Ok(no_content(&tenant))
            }
        }
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

pub async fn delete_registration(
    State(st): State<AppState>,
    Path(id): Path<String>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        antares_model::EntityId::new(&id)
            .map_err(|_| NgsiError::BadRequestData(format!("invalid registration id {id:?}")))?;
        check_params(&params, &["local"])?;
        let before = st.store.get(&tenant, Kind::Registration, &id)?;
        if st.store.delete(&tenant, Kind::Registration, &id)? {
            st.reg_changed(&tenant, &id, None);
            let jobs = crate::notify::prepare_csource_jobs(&st, &tenant, before, None).await;
            let (st2, t2) = (st.clone(), tenant.clone());
            crate::spawn(async move {
                crate::notify::send_csource_jobs(&st2, &t2, jobs).await;
            });
            Ok(no_content(&tenant))
        } else {
            Err::<Response, ApiError>(
                NgsiError::ResourceNotFound(format!("registration {id} not found")).into(),
            )
        }
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

#[cfg(test)]
mod csi_tests {
    use super::*;
    use antares_jsonld::Loader;
    use serde_json::json;

    /// 4.3.6.3: "the registration shall define both: an entity id (i.e. an id
    /// pattern or Entity type defining a group of entities is not supported
    /// for exclusive registrations) [and] Attributes."
    #[test]
    fn exclusive_registration_requires_entity_id_and_attributes() {
        let ctx = Loader::new().core();
        let mk = |mode: &str, info: Value| {
            json!({
                "id": "urn:ngsi-ld:ContextSourceRegistration:x1",
                "type": "ContextSourceRegistration",
                "endpoint": "http://peer:9090",
                "mode": mode,
                "information": [info]
            })
        };
        let norm = |mode: &str, info: Value| {
            normalize_registration(mk(mode, info).as_object().unwrap(), &ctx, false)
        };
        let full = json!({
            "entities": [{"id": "urn:ngsi-ld:Vehicle:v1", "type": "Vehicle"}],
            "propertyNames": ["speed"]
        });
        assert!(norm("exclusive", full.clone()).is_ok());
        assert!(
            norm(
                "exclusive",
                json!({"entities": [{"id": "urn:ngsi-ld:Vehicle:v1", "type": "Vehicle"}]})
            )
            .is_err(),
            "exclusive without Attributes"
        );
        assert!(
            norm(
                "exclusive",
                json!({"entities": [{"type": "Vehicle"}], "propertyNames": ["speed"]})
            )
            .is_err(),
            "exclusive with a type-only entity group"
        );
        assert!(
            norm(
                "exclusive",
                json!({"entities": [{"idPattern": ".*", "type": "Vehicle"}],
                       "propertyNames": ["speed"]})
            )
            .is_err(),
            "exclusive with an id pattern"
        );
        assert!(
            norm("redirect", json!({"entities": [{"type": "Vehicle"}]})).is_ok(),
            "redirect may register a whole type without attributes (4.3.6.3)"
        );
        assert!(
            norm("sideways", full).is_err(),
            "mode outside the 5.2.9 enum"
        );
    }

    /// 4.3.6.3: "Once an exclusive Context Source Registration has been
    /// created, no further exclusive or redirect Context Source Registrations
    /// can be created for that same combination of Entity ID and Attributes"
    /// — while redirect × redirect overlap stays legal.
    #[test]
    fn proxied_overlap_with_an_exclusive_registration_conflicts() {
        let st = crate::state::AppState::new("me".into());
        let tenant = antares_model::TenantId::new("default").expect("tenant");
        let ctx = st.loader.core();
        let mk = |id: &str, mode: &str, attr: &str| {
            let doc = json!({
                "id": id,
                "type": "ContextSourceRegistration",
                "endpoint": "http://peer:9090",
                "mode": mode,
                "information": [{
                    "entities": [{"id": "urn:ngsi-ld:Vehicle:v1", "type": "Vehicle"}],
                    "propertyNames": [attr]
                }]
            });
            normalize_registration(doc.as_object().unwrap(), &ctx, false).expect("valid reg")
        };
        let seeded = mk(
            "urn:ngsi-ld:ContextSourceRegistration:e1",
            "exclusive",
            "speed",
        );
        st.store
            .create(
                &tenant,
                Kind::Registration,
                "urn:ngsi-ld:ContextSourceRegistration:e1",
                Value::Object(seeded),
            )
            .expect("seed");
        let overlap_exc = mk(
            "urn:ngsi-ld:ContextSourceRegistration:e2",
            "exclusive",
            "speed",
        );
        assert!(
            check_proxied_overlap(&st, &tenant, &overlap_exc, None, &ctx).is_err(),
            "second exclusive for the same (id, attr)"
        );
        let overlap_red = mk(
            "urn:ngsi-ld:ContextSourceRegistration:r1",
            "redirect",
            "speed",
        );
        assert!(
            check_proxied_overlap(&st, &tenant, &overlap_red, None, &ctx).is_err(),
            "redirect after an exclusive for the same combination"
        );
        let other_attr = mk(
            "urn:ngsi-ld:ContextSourceRegistration:e3",
            "exclusive",
            "color",
        );
        assert!(
            check_proxied_overlap(&st, &tenant, &other_attr, None, &ctx).is_ok(),
            "disjoint attribute is a different combination"
        );
        // the registration itself is not its own conflict (update path)
        let self_doc = mk(
            "urn:ngsi-ld:ContextSourceRegistration:e1",
            "exclusive",
            "speed",
        );
        assert!(check_proxied_overlap(
            &st,
            &tenant,
            &self_doc,
            Some("urn:ngsi-ld:ContextSourceRegistration:e1"),
            &ctx
        )
        .is_ok());
        // redirect × redirect overlap is explicitly legal
        let r2 = mk(
            "urn:ngsi-ld:ContextSourceRegistration:r2",
            "redirect",
            "color",
        );
        st.store
            .create(
                &tenant,
                Kind::Registration,
                "urn:ngsi-ld:ContextSourceRegistration:r2",
                Value::Object(r2),
            )
            .expect("seed redirect");
        let r3 = mk(
            "urn:ngsi-ld:ContextSourceRegistration:r3",
            "redirect",
            "color",
        );
        assert!(check_proxied_overlap(&st, &tenant, &r3, None, &ctx).is_ok());
    }

    /// 4.3.6.6 (audit V-29): the four processed contextSourceInfo keys have
    /// constrained value spaces, checked at registration time.
    #[test]
    fn context_source_info_reserved_keys_are_validated() {
        let ctx = Loader::new().core();
        let mk = |key: &str, value: &str| {
            json!({
                "id": "urn:ngsi-ld:ContextSourceRegistration:csi1",
                "type": "ContextSourceRegistration",
                "endpoint": "http://peer:9090",
                "information": [{"entities": [{"type": "Building"}]}],
                "contextSourceInfo": [{"key": key, "value": value}]
            })
        };
        let ok = |key: &str, value: &str| {
            normalize_registration(mk(key, value).as_object().unwrap(), &ctx, false).is_ok()
        };
        assert!(ok("accept", "application/json"));
        assert!(ok("contentType", "application/ld+json"));
        assert!(!ok("accept", "text/html"), "MIME outside 4.3.6.6's list");
        assert!(!ok("contentType", "application/geo+json"));
        assert!(ok("jsonldContext", "https://example.org/ctx.jsonld"));
        assert!(!ok("jsonldContext", "not a url"));
        assert!(ok("ngsildConformance", "1.6"));
        assert!(!ok("ngsildConformance", "latest"));
        // ordinary custom keys stay free-form
        assert!(ok("Authorization", "Bearer abc"));
    }
}
