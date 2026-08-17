//! EntityMaps (5.14; resources 6.32, 6.34, 6.35): per-query candidate maps
//! recording which Entities — and which Context Sources — are relevant to an
//! ongoing consumption request (4.5.25, data type 5.2.39).

use crate::negotiate::*;
use crate::state::AppState;
use antares_model::{NgsiError, TenantId};
use antares_sql::store::Kind;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{json, Map, Value};
use std::collections::HashMap;

/// Per-tenant EntityMap cap (every buffer bounded); earliest-expiring evicted.
const MAX_MAPS_PER_TENANT: usize = 512;
/// Default lifetime when the client suggests none — 5.5.14: "the caching
/// strategy and expiry time … depend on implementation specific
/// configurations".
const DEFAULT_LIFETIME_SECS: i64 = 3600;
/// Ceiling on client-suggested lifetimes — 6.4.3.2-1: "the actual expiresAt
/// time of the EntityMap shall be set by the Context Broker or Context
/// Source, possibly overriding the requested duration".
const MAX_LIFETIME_SECS: i64 = 86_400;

fn dt(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.with_timezone(&chrono::Utc))
}

// ---------- storage (5.14.1.1: "internal storage, or memory") ----------

/// Fetch a live EntityMap; an expired one "cannot be accessed" (5.5.14) and
/// is pruned on touch. Maps live in the store (Kind::EntityMap) so
/// persistent modes survive restarts.
pub(crate) fn map_get(st: &AppState, tenant: &TenantId, id: &str) -> Option<Value> {
    let doc = st.store.get(tenant, Kind::EntityMap, id).ok().flatten()?;
    // 5.5.14 is a positive condition: a map is served only while a READABLE
    // expiry is still in the future. Judging "expired" instead lets a map
    // whose expiresAt is missing or unparseable outlive every ceiling.
    let live = doc
        .get("expiresAt")
        .and_then(Value::as_str)
        .and_then(dt)
        .is_some_and(|e| e > chrono::Utc::now());
    if !live {
        let _ = st.store.delete(tenant, Kind::EntityMap, id);
        return None;
    }
    Some(doc)
}

pub(crate) fn map_put(st: &AppState, tenant: &TenantId, doc: Value) {
    let Some(id) = doc.get("id").and_then(Value::as_str).map(str::to_owned) else {
        return;
    };
    let existing = st.store.list(tenant, Kind::EntityMap).unwrap_or_default();
    if existing.len() >= MAX_MAPS_PER_TENANT && !existing.iter().any(|d| d["id"] == id.as_str()) {
        // eviction order is a heuristic — earliest expiresAt string wins
        if let Some(victim) = existing
            .iter()
            .min_by(|a, b| {
                a["expiresAt"]
                    .as_str()
                    .unwrap_or("")
                    .cmp(b["expiresAt"].as_str().unwrap_or(""))
            })
            .and_then(|d| d.get("id").and_then(Value::as_str))
        {
            let _ = st.store.delete(tenant, Kind::EntityMap, victim);
        }
    }
    let updated = st
        .store
        .mutate(tenant, Kind::EntityMap, &id, |d| {
            *d = doc.clone();
            Ok::<_, std::convert::Infallible>(())
        })
        .ok()
        .flatten()
        .is_some();
    if !updated {
        let _ = st.store.create(tenant, Kind::EntityMap, &id, doc);
    }
}

pub(crate) fn map_delete(st: &AppState, tenant: &TenantId, id: &str) -> bool {
    st.store
        .delete(tenant, Kind::EntityMap, id)
        .unwrap_or(false)
}

/// Parse an ISO 8601 duration (entityMapLifetime, Table 6.4.3.2-1) to whole
/// seconds; years/months are approximated (365/30 days), fractions rejected.
pub(crate) fn iso8601_secs(s: &str) -> Option<i64> {
    let rest = s.strip_prefix('P')?;
    if rest.is_empty() {
        return None;
    }
    let (date, time) = match rest.split_once('T') {
        Some((d, t)) => (d, Some(t)),
        None => (rest, None),
    };
    let mut secs: i64 = 0;
    let mut num = String::new();
    for c in date.chars() {
        if c.is_ascii_digit() {
            num.push(c);
        } else {
            let n: i64 = num.parse().ok()?;
            num.clear();
            secs = secs.checked_add(n.checked_mul(match c {
                'Y' => 31_536_000,
                'M' => 2_592_000,
                'W' => 604_800,
                'D' => 86_400,
                _ => return None,
            })?)?;
        }
    }
    if !num.is_empty() {
        return None; // trailing digits without a designator
    }
    if let Some(t) = time {
        if t.is_empty() {
            return None;
        }
        for c in t.chars() {
            if c.is_ascii_digit() {
                num.push(c);
            } else {
                let n: i64 = num.parse().ok()?;
                num.clear();
                secs = secs.checked_add(n.checked_mul(match c {
                    'H' => 3600,
                    'M' => 60,
                    'S' => 1,
                    _ => return None,
                })?)?;
            }
        }
        if !num.is_empty() {
            return None;
        }
    }
    Some(secs)
}

/// The expiresAt the broker assigns (5.2.39): now + suggested lifetime,
/// bounded by the broker's ceiling; the default applies when none is given.
fn expires_at(params: &HashMap<String, String>) -> Result<String, NgsiError> {
    let secs = match params.get("entityMapLifetime") {
        Some(d) => iso8601_secs(d)
            .ok_or_else(|| {
                NgsiError::BadRequestData(format!(
                    "entityMapLifetime is not an ISO 8601 duration: {d:?}"
                ))
            })?
            // A zero or negative suggestion would answer 201 with a map that
            // 5.5.14 already forbids anyone from accessing, so the broker
            // floor applies as well as the ceiling.
            .clamp(1, MAX_LIFETIME_SECS),
        None => DEFAULT_LIFETIME_SECS,
    };
    Ok((chrono::Utc::now() + chrono::Duration::seconds(secs))
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

// ---------- 5.14.1 / 5.14.2 / 5.14.3: /entityMaps/{id} (6.32) ----------

fn map_id_check(id: &str) -> Result<(), NgsiError> {
    antares_model::EntityId::new(id)
        .map(|_| ())
        .map_err(|_| NgsiError::BadRequestData(format!("EntityMap id is not a valid URI: {id:?}")))
}

/// 5.14.1.4 Retrieve EntityMap: invalid-URI id → 400 BadRequestData, unknown
/// id → 404 ResourceNotFound, else the 5.2.39 JSON-LD object.
pub async fn retrieve_entity_map(
    State(st): State<AppState>,
    Path(id): Path<String>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        check_params(&params, &["local"])?;
        map_id_check(&id)?;
        let accept = parse_accept(&headers)?;
        let ctx = request_context(&st.loader, &headers).await?;
        let doc = map_get(&st, &tenant, &id)
            .ok_or_else(|| NgsiError::ResourceNotFound(format!("EntityMap {id} not found")))?;
        Ok::<_, ApiError>(respond(StatusCode::OK, doc, &ctx, accept, &tenant))
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

/// 5.14.2.4 Update EntityMap: partial update of the target EntityMap;
/// output-only members (entityMap, linkedMaps — 5.2.39) are ignored, and per
/// 5.5.14 other components may only update the expiry timestamp.
pub async fn update_entity_map(
    State(st): State<AppState>,
    Path(id): Path<String>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        check_params(&params, &["local"])?;
        map_id_check(&id)?;
        let frag: Value = serde_json::from_slice(&body)
            .map_err(|e| NgsiError::InvalidRequest(format!("body is not valid JSON: {e}")))?;
        let obj = frag.as_object().ok_or_else(|| {
            NgsiError::BadRequestData("EntityMap fragment must be a JSON object".into())
        })?;
        let mut doc = map_get(&st, &tenant, &id)
            .ok_or_else(|| NgsiError::ResourceNotFound(format!("EntityMap {id} not found")))?;
        if let Some(e) = obj.get("expiresAt") {
            let s = e.as_str().filter(|s| dt(s).is_some()).ok_or_else(|| {
                NgsiError::BadRequestData("expiresAt must be a DateTime (4.6.3)".into())
            })?;
            doc["expiresAt"] = json!(s);
        }
        map_put(&st, &tenant, doc);
        Ok::<_, ApiError>(no_content(&tenant))
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

/// 5.14.3.4 Delete EntityMap: invalid-URI id → 400, unknown id → 404, else
/// the EntityMap is removed from storage/memory (204).
pub async fn delete_entity_map(
    State(st): State<AppState>,
    Path(id): Path<String>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        check_params(&params, &["local"])?;
        map_id_check(&id)?;
        if !map_delete(&st, &tenant, &id) {
            return Err(NgsiError::ResourceNotFound(format!("EntityMap {id} not found")).into());
        }
        Ok::<_, ApiError>(no_content(&tenant))
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

// ---------- 5.14.4: Create EntityMap for Query Entities (6.34) ----------

fn allowed_create_params() -> Vec<&'static str> {
    let mut v = crate::entities::QUERY_PARAMS.to_vec();
    v.extend(["entityMapLifetime", "splitEntities"]);
    v
}

/// 5.14.4.4: run the (split-reduced when applicable) local query and record
/// each matching id under the "@none" local marker; forward to matching
/// registrations supporting createEntityMapQueryEntity and merge each
/// returned EntityMap (ids → registration id, linkedMaps → remote map id);
/// store the local EntityMap and return it.
pub(crate) async fn build_query_map(
    st: &AppState,
    tenant: &TenantId,
    headers: &HeaderMap,
    ctx: &antares_jsonld::Context,
    params: &HashMap<String, String>,
) -> ApiResult<Value> {
    let q_ast = params
        .get("q")
        .map(|q| antares_ql::parse_q(q))
        .transpose()?;
    // 5.14.4.4 a-e: too wide query
    if !crate::entities::qualifies_non_wide(params, q_ast.as_ref()) {
        return Err(NgsiError::BadRequestData(
            "EntityMap query needs at least one of type, attrs, q, georel, or local=true \
             (5.14.4.4 — too wide query)"
                .into(),
        )
        .into());
    }
    // 5.14.4.4: invalid entity ids / csf are BadRequestData
    if let Some(ids) = params.get("id") {
        for id in ids.split(',') {
            antares_model::EntityId::new(id.trim())?;
        }
    }
    if let Some(csf) = params.get("csf") {
        antares_ql::parse_q(csf)?;
    }
    let local_scope = params.get("local").map(String::as_str) == Some("true");
    let split = params.get("splitEntities").map(String::as_str) == Some("true");
    // Split entities: only id/type/idPattern narrow the local candidate set —
    // value/geo/scope filters cannot be judged on a fragment (5.14.4.4).
    let eff: HashMap<String, String> = if split && !local_scope {
        params
            .iter()
            .filter(|(k, _)| ["id", "idPattern", "type", "local"].contains(&k.as_str()))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    } else {
        params.clone()
    };
    let local_docs = crate::entities::filter_entities(st, tenant, &eff, ctx)?;
    let mut emap = Map::new();
    for d in &local_docs {
        if let Some(id) = d.get("id").and_then(Value::as_str) {
            // "@none" refers to an Entity held locally (5.2.39)
            emap.insert(id.to_owned(), json!(["@none"]));
        }
    }
    let mut linked = Map::new();
    if !local_scope {
        for (reg_id, remote) in crate::federation::fed_entity_maps(
            st,
            tenant,
            headers,
            ctx,
            params,
            split,
            "createEntityMapQueryEntity",
            "entityMaps",
        )
        .await
        {
            if let Some(obj) = remote.get("entityMap").and_then(Value::as_object) {
                for eid in obj.keys() {
                    if let Some(a) = emap
                        .entry(eid.clone())
                        .or_insert_with(|| json!([]))
                        .as_array_mut()
                    {
                        a.push(json!(reg_id.clone()));
                    }
                }
            }
            if let Some(mid) = remote.get("id").and_then(Value::as_str) {
                linked.insert(reg_id, json!(mid));
            }
        }
    }
    let doc = json!({
        "id": format!("urn:ngsi-ld:entitymap:{}", uuid::Uuid::new_v4()),
        "type": "EntityMap",
        "expiresAt": expires_at(params)?,
        "entityMap": Value::Object(emap),
        "linkedMaps": Value::Object(linked),
    });
    map_put(st, tenant, doc.clone());
    Ok(doc)
}

/// 5.7.1.4 / 5.7.3.4: the EntityMap created for a single-Entity retrieve —
/// its one entry lists "@none" when Attribute data is held locally plus
/// every matching Context Source Registration supporting the retrieve
/// operation ("only the retrieved Entity Map shall be used to determine
/// which Context Source Registrations match the Entity ID").
#[allow(clippy::too_many_arguments)] // one param per 5.7.1.4 input
pub(crate) fn build_retrieve_map(
    st: &AppState,
    tenant: &TenantId,
    ctx: &antares_jsonld::Context,
    headers: &HeaderMap,
    id: &str,
    params: &HashMap<String, String>,
    temporal: bool,
    local_held: bool,
) -> Result<Value, NgsiError> {
    let mut srcs: Vec<Value> = Vec::new();
    if local_held {
        srcs.push(json!("@none"));
    }
    if crate::federation::active(params) {
        let spec = crate::csource::CsrSpec {
            ids: Some(vec![id.to_owned()]),
            ..Default::default()
        };
        for reg in crate::federation::matching_regs(st, tenant, &spec, ctx, headers) {
            let ok = if temporal {
                reg.supports("retrieveTemporal")
            } else {
                reg.read_op().is_some()
            };
            if ok {
                srcs.push(json!(reg.reg_id));
            }
        }
    }
    let mut emap = Map::new();
    if !srcs.is_empty() {
        emap.insert(id.to_owned(), Value::Array(srcs));
    }
    let doc = json!({
        "id": format!("urn:ngsi-ld:entitymap:{}", uuid::Uuid::new_v4()),
        "type": "EntityMap",
        "expiresAt": expires_at(params)?,
        "entityMap": Value::Object(emap),
        "linkedMaps": {},
    });
    map_put(st, tenant, doc.clone());
    Ok(doc)
}

/// 201 + the EntityMap body + the NGSILD-EntityMap header carrying the
/// resource URI of the created map (6.34.3.1 / 6.35.3.1).
fn created_response(
    doc: Value,
    ctx: &antares_jsonld::Context,
    accept: Accept,
    tenant: &TenantId,
) -> Response {
    let uri = format!(
        "/ngsi-ld/v1/entityMaps/{}",
        doc.get("id").and_then(Value::as_str).unwrap_or_default()
    );
    let mut resp = respond(StatusCode::CREATED, doc, ctx, accept, tenant);
    if let Ok(v) = uri.parse() {
        resp.headers_mut().insert("NGSILD-EntityMap", v);
    }
    resp
}

/// GET /entityMaps — Create EntityMap for Query Entities (6.34.3.1).
pub async fn create_entity_map(
    State(st): State<AppState>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        check_params(&params, &allowed_create_params())?;
        let accept = parse_accept(&headers)?;
        let ctx = request_context(&st.loader, &headers).await?;
        let doc = build_query_map(&st, &tenant, &headers, &ctx, &params).await?;
        Ok::<_, ApiError>(created_response(doc, &ctx, accept, &tenant))
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

/// POST /entityMaps — the 5.2.23 Query-object form (6.34.3.2).
pub async fn create_entity_map_post(
    State(st): State<AppState>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        let q: Value = serde_json::from_slice(&body)
            .map_err(|e| NgsiError::InvalidRequest(format!("body is not valid JSON: {e}")))?;
        if q.get("type").and_then(Value::as_str) != Some("Query") {
            return Err(NgsiError::BadRequestData("body type must be Query".into()).into());
        }
        let qo = q
            .as_object()
            .ok_or_else(|| NgsiError::BadRequestData("query body must be an object".into()))?;
        let mut vp: HashMap<String, String> = params.clone();
        crate::batch::query_doc_params(qo, false, &mut vp)?;
        check_params(&vp, &allowed_create_params())?;
        let accept = parse_accept(&headers)?;
        let ctx = request_context(&st.loader, &headers).await?;
        let doc = build_query_map(&st, &tenant, &headers, &ctx, &vp).await?;
        Ok::<_, ApiError>(created_response(doc, &ctx, accept, &tenant))
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

// ------ 5.14.5: Create EntityMap for Query Temporal Evolution (6.35) ------

/// 5.14.5.4: temporal query required; the S1–S4 candidate selection is the
/// temporal query pipeline itself (5.7.4.4) run unpaged — the ids of its
/// result set form the EntityMap; the createEntityMapQueryTemporal
/// registrations are then merged like 5.14.4.
/// Known ceiling: candidate ids are read from the internal 5.7.4 response capped
/// at max_limit; raise the cap if temporal sets outgrow it.
pub(crate) async fn build_temporal_map(
    st: &AppState,
    tenant: &TenantId,
    headers: &HeaderMap,
    ctx: &antares_jsonld::Context,
    params: &HashMap<String, String>,
) -> ApiResult<Value> {
    if !params.contains_key("timerel") {
        return Err(NgsiError::BadRequestData(
            "a temporal query is required to create a temporal EntityMap (5.14.5.4)".into(),
        )
        .into());
    }
    let local_scope = params.get("local").map(String::as_str) == Some("true");
    let split = params.get("splitEntities").map(String::as_str) == Some("true");
    let mut eff: HashMap<String, String> = if split && !local_scope {
        params
            .iter()
            .filter(|(k, _)| {
                [
                    "id",
                    "idPattern",
                    "type",
                    "local",
                    "timerel",
                    "timeAt",
                    "endTimeAt",
                    "timeproperty",
                ]
                .contains(&k.as_str())
            })
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    } else {
        params.clone()
    };
    for k in [
        "entityMap",
        "entityMapLifetime",
        "splitEntities",
        "offset",
        "count",
    ] {
        eff.remove(k);
    }
    eff.insert("limit".into(), st.max_limit.to_string());
    // Box::pin: build_temporal_map is reachable from query_temporal_inner
    // (entityMap=true), so this recursive edge needs indirection.
    let resp = Box::pin(crate::temporal::query_temporal_inner(st, &eff, headers)).await?;
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .map_err(|e| NgsiError::InternalError(format!("temporal candidate read: {e}")))?;
    let candidates: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Array(vec![]));
    let mut emap = Map::new();
    if let Some(arr) = candidates.as_array() {
        for d in arr {
            if let Some(id) = d.get("id").and_then(Value::as_str) {
                emap.insert(id.to_owned(), json!(["@none"]));
            }
        }
    }
    let mut linked = Map::new();
    if !local_scope {
        for (reg_id, remote) in crate::federation::fed_entity_maps(
            st,
            tenant,
            headers,
            ctx,
            params,
            split,
            "createEntityMapQueryTemporal",
            "temporal/entityMaps",
        )
        .await
        {
            if let Some(obj) = remote.get("entityMap").and_then(Value::as_object) {
                for eid in obj.keys() {
                    if let Some(a) = emap
                        .entry(eid.clone())
                        .or_insert_with(|| json!([]))
                        .as_array_mut()
                    {
                        a.push(json!(reg_id.clone()));
                    }
                }
            }
            if let Some(mid) = remote.get("id").and_then(Value::as_str) {
                linked.insert(reg_id, json!(mid));
            }
        }
    }
    let doc = json!({
        "id": format!("urn:ngsi-ld:entitymap:{}", uuid::Uuid::new_v4()),
        "type": "EntityMap",
        "expiresAt": expires_at(params)?,
        "entityMap": Value::Object(emap),
        "linkedMaps": Value::Object(linked),
    });
    map_put(st, tenant, doc.clone());
    Ok(doc)
}

/// GET /temporal/entityMaps — Create EntityMap for Query Temporal Evolution
/// of Entities (6.35.3.1).
pub async fn create_temporal_entity_map(
    State(st): State<AppState>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        let accept = parse_accept(&headers)?;
        let ctx = request_context(&st.loader, &headers).await?;
        let doc = build_temporal_map(&st, &tenant, &headers, &ctx, &params).await?;
        Ok::<_, ApiError>(created_response(doc, &ctx, accept, &tenant))
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

/// POST /temporal/entityMaps — the Query-object form (6.35.3.2).
pub async fn create_temporal_entity_map_post(
    State(st): State<AppState>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        let q: Value = serde_json::from_slice(&body)
            .map_err(|e| NgsiError::InvalidRequest(format!("body is not valid JSON: {e}")))?;
        if q.get("type").and_then(Value::as_str) != Some("Query") {
            return Err(NgsiError::BadRequestData("body type must be Query".into()).into());
        }
        let qo = q
            .as_object()
            .ok_or_else(|| NgsiError::BadRequestData("query body must be an object".into()))?;
        let mut vp: HashMap<String, String> = params.clone();
        crate::batch::query_doc_params(qo, true, &mut vp)?;
        let accept = parse_accept(&headers)?;
        let ctx = request_context(&st.loader, &headers).await?;
        let doc = build_temporal_map(&st, &tenant, &headers, &ctx, &vp).await?;
        Ok::<_, ApiError>(created_response(doc, &ctx, accept, &tenant))
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Table 6.4.3.2-1: entityMapLifetime is an ISO 8601 duration.
    #[test]
    fn clause_5_14_4_lifetime_parse() {
        assert_eq!(iso8601_secs("PT1H"), Some(3600));
        assert_eq!(iso8601_secs("PT90S"), Some(90));
        assert_eq!(iso8601_secs("P1DT2H3M4S"), Some(93784));
        assert_eq!(iso8601_secs("P2W"), Some(1_209_600));
        // invalid shapes are rejected (→ 400 at the handler)
        for bad in ["", "P", "PT", "1H", "PT1X", "PT1.5S", "PT1"] {
            assert_eq!(iso8601_secs(bad), None, "{bad:?}");
        }
    }

    /// Table 6.4.3.2-1: entityMapLifetime arrives on the query string, so
    /// the parser is attacker-facing — every hostile shape must return None
    /// (a 400) rather than panic, wrap or saturate.
    #[test]
    fn clause_5_14_4_lifetime_hostile_inputs() {
        for bad in [
            "P-1D",                    // negative component
            "-P1D",                    // negative duration
            "P+1D",                    // signed component
            "p1d",                     // lower case designators
            " PT1H",                   // leading whitespace
            "PT1H ",                   // trailing whitespace
            "P1DT",                    // empty time part
            "PT99999999999999999999S", // digit run past i64
            "P9999999999999Y",         // multiplication overflow
            "P92233720368547758S",     // addition overflow after scaling
            "PT1H1",                   // trailing digits, no designator
            "P١D",                     // non-ASCII digit
            "P1D\u{0}",                // embedded NUL
            "PT,5S",                   // comma fraction
            "P1S",                     // time designator in the date part
            "PT1D",                    // date designator in the time part
        ] {
            assert_eq!(iso8601_secs(bad), None, "{bad:?} must not parse");
        }
        // the whole i64 range is walked without panicking
        assert_eq!(iso8601_secs(&format!("PT{}S", i64::MAX)), Some(i64::MAX));
        assert_eq!(iso8601_secs(&format!("PT{}S", u64::MAX)), None);
        assert_eq!(iso8601_secs("PT0S"), Some(0));
    }

    fn params(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    /// 6.4.3.2-1: "the actual expiresAt time of the EntityMap shall be set by
    /// the Context Broker or Context Source, possibly overriding the
    /// requested duration" — the client suggestion is bounded above by the
    /// broker ceiling and below by a lifetime the map can actually be used
    /// for; an unparseable duration is BadRequestData.
    #[test]
    fn clause_5_14_4_expires_at_is_broker_bounded() {
        let now = chrono::Utc::now();
        let at = |p: &[(&str, &str)]| {
            let s = expires_at(&params(p)).expect("expiry");
            dt(&s).expect("RFC 3339 expiry")
        };
        let default = at(&[]);
        assert!(
            (default - now).num_seconds() >= DEFAULT_LIFETIME_SECS - 5
                && (default - now).num_seconds() <= DEFAULT_LIFETIME_SECS + 5,
            "no suggestion → the default lifetime"
        );
        let capped = at(&[("entityMapLifetime", "P30D")]);
        assert!(
            (capped - now).num_seconds() <= MAX_LIFETIME_SECS,
            "a client cannot exceed the broker ceiling"
        );
        let zero = at(&[("entityMapLifetime", "PT0S")]);
        assert!(
            zero > now,
            "a zero lifetime would return 201 for a map that is already \
             unusable (5.5.14): {zero}"
        );
        match expires_at(&params(&[("entityMapLifetime", "yesterday")])) {
            Err(NgsiError::BadRequestData(_)) => {}
            other => panic!("an invalid duration must be BadRequestData: {other:?}"),
        }
    }

    /// 5.14: EntityMaps are per-tenant resources — an EntityMap created
    /// under one tenant is invisible and undeletable from another (4.14
    /// multi-tenancy: "an NGSI-LD system shall behave as if the tenants were
    /// separate systems").
    #[test]
    fn clause_5_14_maps_are_tenant_scoped() {
        let st = AppState::new("antares-em-unit".into());
        let a = TenantId::new("alpha").expect("tenant");
        let b = TenantId::new("beta").expect("tenant");
        let id = "urn:ngsi-ld:entitymap:t1";
        map_put(&st, &a, live_map(id));
        assert!(map_get(&st, &a, id).is_some());
        assert!(
            map_get(&st, &b, id).is_none(),
            "another tenant must not read the map"
        );
        assert!(
            !map_delete(&st, &b, id),
            "another tenant must not delete the map"
        );
        assert!(
            map_get(&st, &a, id).is_some(),
            "the owner still has its map"
        );
        assert!(map_delete(&st, &a, id));
        assert!(map_get(&st, &a, id).is_none());
    }

    fn live_map(id: &str) -> Value {
        json!({
            "id": id,
            "type": "EntityMap",
            "expiresAt": (chrono::Utc::now() + chrono::Duration::seconds(600))
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            "entityMap": {},
            "linkedMaps": {},
        })
    }

    /// 5.5.14: an expired EntityMap "cannot be accessed" — it is never
    /// served, and a map whose expiry cannot be read is treated the same way
    /// rather than living forever.
    #[test]
    fn clause_5_5_14_expired_maps_are_never_served() {
        let st = AppState::new("antares-em-exp".into());
        let t = TenantId::default();
        let mut past = live_map("urn:ngsi-ld:entitymap:past");
        past["expiresAt"] = json!("2020-01-01T00:00:00.000Z");
        map_put(&st, &t, past);
        assert!(map_get(&st, &t, "urn:ngsi-ld:entitymap:past").is_none());
        assert!(
            st.store
                .get(&t, Kind::EntityMap, "urn:ngsi-ld:entitymap:past")
                .expect("store")
                .is_none(),
            "an expired map is pruned on touch"
        );
        for (id, expiry) in [
            ("urn:ngsi-ld:entitymap:none", None),
            ("urn:ngsi-ld:entitymap:junk", Some(json!("whenever"))),
            ("urn:ngsi-ld:entitymap:num", Some(json!(0))),
        ] {
            let mut doc = live_map(id);
            match expiry {
                Some(v) => doc["expiresAt"] = v,
                None => {
                    doc.as_object_mut().expect("object").remove("expiresAt");
                }
            }
            map_put(&st, &t, doc);
            assert!(
                map_get(&st, &t, id).is_none(),
                "{id} has no readable expiry and must not be served"
            );
        }
    }

    /// 5.14.1.1 storage: every buffer is bounded — the per-tenant EntityMap
    /// registry has a ceiling, and filling it evicts rather than growing.
    #[test]
    fn clause_5_14_1_map_registry_is_bounded() {
        let st = AppState::new("antares-em-cap".into());
        let t = TenantId::default();
        for i in 0..MAX_MAPS_PER_TENANT + 8 {
            let mut doc = live_map(&format!("urn:ngsi-ld:entitymap:{i:04}"));
            // earliest expiry first, so the eviction victim is deterministic
            doc["expiresAt"] = json!(format!("2099-01-01T00:00:{:02}.000Z", i % 60));
            map_put(&st, &t, doc);
            assert!(
                st.store.list(&t, Kind::EntityMap).expect("list").len() <= MAX_MAPS_PER_TENANT,
                "the registry exceeded its ceiling at {i}"
            );
        }
        // re-storing a known id is an update, never an eviction
        let before = st.store.list(&t, Kind::EntityMap).expect("list").len();
        let known = st.store.list(&t, Kind::EntityMap).expect("list")[0]["id"]
            .as_str()
            .expect("id")
            .to_owned();
        map_put(&st, &t, live_map(&known));
        assert_eq!(
            st.store.list(&t, Kind::EntityMap).expect("list").len(),
            before
        );
    }

    /// 5.14.1.4 / 5.14.3.4: "If the EntityMap id is not present or it is not
    /// a valid URI, then an error of type BadRequestData shall be raised."
    #[test]
    fn clause_5_14_1_map_id_must_be_a_uri() {
        assert!(map_id_check("urn:ngsi-ld:entitymap:1").is_ok());
        for bad in [
            "",
            "entitymap",
            "urn:ngsi-ld:entity map:1",
            "urn:ngsi-ld:entitymap:1\r\nX: y",
            ":nostem",
            "urn:",
        ] {
            match map_id_check(bad) {
                Err(NgsiError::BadRequestData(_)) => {}
                other => panic!("{bad:?} must be BadRequestData, got {other:?}"),
            }
        }
    }
}
