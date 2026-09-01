// SPDX-License-Identifier: EUPL-1.2
//! /csourceRegistrations (5.9, 5.10; resources 6.8/6.9).

use crate::negotiate::*;
use crate::state::{now_iso, AppState};
use antares_jsonld::{parse_datetime, Context};
use antares_model::operations::{OPERATION_GROUPS, OPERATION_NAMES};
use antares_model::{NgsiError, TenantId};
use antares_store::CurrentStateDriverExt;
use antares_store::Kind;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{Map, Value};
use std::collections::HashMap;

use crate::negotiate::CleanParams;

/// Validate + normalize a CSourceRegistration (5.2.9): types and attribute
/// names inside `information` expand to IRIs.
/// Cardinality caps on a CSourceRegistration. Generous against any real
/// federation topology (a tenant is sized at 1000+ registrations, not one
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
    // 5.5.4: first-level member nulls are only legal in fragments (patch)
    if !is_patch {
        antares_jsonld::reject_first_level_nulls(doc)?;
    }
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
                // The csource_index explosion is |entities| ×
                // (|propertyNames| + |relationshipNames|) PER information
                // element, materialised in memory before any SQL runs. Under
                // only the 4 MiB body cap that is ~10^10 objects — an OOM from
                // one request. Cardinality is capped at the validation
                // boundary, where the error is a 400 and not a dead pod:
                // there is no query here to be too complex, and 5.9.2.4 gives
                // BadRequestData for a registration whose content is refused.
                if arr.len() > MAX_INFORMATION {
                    return Err(bad(format!(
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
                                return Err(bad(format!(
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
                                                crate::regexcache::compile(p).map_err(|_| {
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
                                // 5.2.10: "Empty array is not allowed"
                                let names =
                                    iv.as_array().filter(|a| !a.is_empty()).ok_or_else(|| {
                                        bad(format!("{ik} must be a non-empty array (5.2.10)"))
                                    })?;
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
                // the instant decides, not the spelling: now_iso always
                // carries 3 fraction digits, a client's expiresAt 0 to 6
                if crate::temporal::dt_key(s) < crate::temporal::dt_key(&now_iso()) {
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
                    // Table 5.2.22-1: value is a String, cardinality 1.
                    let Some(value) = kv.get("value").filter(|v| v.is_string()) else {
                        return Err(bad(
                            "contextSourceInfo entries must be {key, value} pairs of Strings (5.2.22)"
                                .into(),
                        ));
                    };
                    // 6.3.19: "Key and value members shall adhere to IETF
                    // RFC 7230 definitions concerning HTTP headers". The pair
                    // becomes a header on every forward, so the transport's
                    // own RFC 7230 parsers are the judge — a name or a value
                    // they refuse can only fail later, at a forward whose
                    // error names no registration.
                    if !crate::subscriptions::is_field_name(key) {
                        return Err(bad(format!(
                            "contextSourceInfo key {key:?} is not an RFC 7230 header name (6.3.19)"
                        )));
                    }
                    if !value
                        .as_str()
                        .is_some_and(crate::subscriptions::is_field_value)
                    {
                        return Err(bad(format!(
                            "contextSourceInfo value for {key:?} is not an RFC 7230 header value \
                             (6.3.19)"
                        )));
                    }
                    // 4.3.6.6: the four processed keys have constrained
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
            // Table 5.2.9-1: operations entries "are limited to the named
            // API operations and named operation groups (see clause 4.20)".
            "operations" => {
                let arr = v
                    .as_array()
                    .filter(|a| !a.is_empty())
                    .ok_or_else(|| bad("operations must be a non-empty array (5.2.9)".into()))?;
                for op in arr {
                    let name = op.as_str().unwrap_or_default();
                    if !OPERATION_NAMES.contains(&name) && !OPERATION_GROUPS.contains(&name) {
                        return Err(bad(format!(
                            "unknown operation {name:?} — entries are limited to the \
                             4.20 names and groups (5.2.9)"
                        )));
                    }
                }
                out.insert("operations".into(), v.clone());
            }
            // 4.3.6.4 / 5.2.9: localOnly is a Boolean.
            "localOnly" => {
                if !v.is_boolean() {
                    return Err(bad("localOnly must be a boolean (5.2.9)".into()));
                }
                out.insert("localOnly".into(), v.clone());
            }
            // Table 5.2.9-1: a non-empty RFC 7230 pseudonym token.
            "contextSourceAlias" => {
                let a = v
                    .as_str()
                    .filter(|a| crate::subscriptions::is_field_name(a))
                    .ok_or_else(|| {
                        bad(
                            "contextSourceAlias must be a non-empty RFC 7230 pseudonym token \
                             (5.2.9)"
                                .into(),
                        )
                    })?;
                out.insert("contextSourceAlias".into(), Value::String(a.to_owned()));
            }
            // Table 5.2.9-1: non-empty strings.
            "description" | "registrationName" => {
                if v.as_str().is_none_or(str::is_empty) {
                    return Err(bad(format!("{k} must be a non-empty string (5.2.9)")));
                }
                out.insert(k.clone(), v.clone());
            }
            // Table 5.2.9-1: valid URIs, "@none" for the default instances.
            "datasetId" => {
                let arr = v
                    .as_array()
                    .ok_or_else(|| bad("datasetId must be an array of URIs (5.2.9)".into()))?;
                for d in arr {
                    let d = d.as_str().unwrap_or_default();
                    if d != "@none" && antares_model::EntityId::new(d).is_err() {
                        return Err(bad(format!("datasetId entry {d:?} is not a URI (5.2.9)")));
                    }
                }
                out.insert("datasetId".into(), v.clone());
            }
            // Table 5.2.9-1: scope(s) per the 4.18 grammar.
            "scope" => {
                let all_valid = match v {
                    Value::String(s) => antares_jsonld::valid_scope_value(s),
                    Value::Array(a) => a
                        .iter()
                        .all(|s| s.as_str().is_some_and(antares_jsonld::valid_scope_value)),
                    _ => false,
                };
                if !all_valid {
                    return Err(bad("scope violates the 4.18 grammar (5.2.9)".into()));
                }
                out.insert("scope".into(), v.clone());
            }
            // Table 5.2.9-1: GeoJSON geometries per 4.7.
            "location" | "observationSpace" | "operationSpace" => {
                let ok = v
                    .as_object()
                    .and_then(|o| Some((o.get("type")?.as_str()?, o.get("coordinates")?)))
                    .is_some_and(|(t, c)| crate::geo::parse_ref_geometry(t, c).is_ok());
                if !ok {
                    return Err(bad(format!("{k} must be a 4.7 GeoJSON geometry (5.2.9)")));
                }
                out.insert(k.clone(), v.clone());
            }
            // Table 5.2.34-1 (RegistrationManagementInfo): cacheDuration an
            // ISO 8601 duration, cooldown/timeout numbers greater than 0,
            // localOnly a boolean.
            "management" => {
                let m = v.as_object().ok_or_else(|| {
                    bad("management must be a RegistrationManagementInfo object (5.2.34)".into())
                })?;
                if let Some(d) = m.get("cacheDuration") {
                    if !d.as_str().is_some_and(valid_iso8601_duration) {
                        return Err(bad(
                            "management cacheDuration must be an ISO 8601 duration (5.2.34)".into(),
                        ));
                    }
                }
                for key in ["cooldown", "timeout"] {
                    if let Some(n) = m.get(key) {
                        if !n.as_f64().is_some_and(|n| n > 0.0) {
                            return Err(bad(format!(
                                "management {key} must be a number greater than 0 (5.2.34)"
                            )));
                        }
                    }
                }
                if let Some(l) = m.get("localOnly") {
                    if !l.is_boolean() {
                        return Err(bad("management localOnly must be a boolean (5.2.34)".into()));
                    }
                }
                out.insert("management".into(), v.clone());
            }
            // Table 5.2.9-1: an ISO 8601 duration.
            "refreshRate" => {
                let ok = v.as_str().is_some_and(valid_iso8601_duration);
                if !ok {
                    return Err(bad(
                        "refreshRate must be an ISO 8601 duration (5.2.9)".into()
                    ));
                }
                out.insert("refreshRate".into(), v.clone());
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
                // tolerant reader: keep unknown members
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

/// ISO 8601 duration (5.2.9 refreshRate): `P[nY][nM][nW][nD][T[nH][nM][nS]]`,
/// at least one component, digits (fraction allowed in seconds).
fn valid_iso8601_duration(s: &str) -> bool {
    let Some(rest) = s.strip_prefix('P') else {
        return false;
    };
    if rest.is_empty() {
        return false;
    }
    let (date, time) = match rest.split_once('T') {
        Some((d, t)) => (d, Some(t)),
        None => (rest, None),
    };
    let take = |part: &str, units: &[char]| -> bool {
        let mut p = part;
        let mut any = false;
        for u in units {
            if let Some(i) = p.find(*u) {
                let num = &p[..i];
                if num.is_empty()
                    || !num
                        .bytes()
                        .all(|b| b.is_ascii_digit() || b == b'.' || b == b',')
                {
                    return false;
                }
                any = true;
                p = &p[i + 1..];
            }
        }
        p.is_empty() && (any || part.is_empty())
    };
    let date_ok = take(date, &['Y', 'M', 'W', 'D']);
    match time {
        None => date_ok && !date.is_empty(),
        Some(t) => date_ok && !t.is_empty() && take(t, &['H', 'M', 'S']),
    }
}

/// 5.9.2.4: an auxiliary registration may only offer "retrieveOps",
/// "retrieveEntity" or "queryEntity" (or a combination thereof) — enforced
/// when the operations member is present (absent = deployment default).
fn validate_auxiliary_ops(doc: &Map<String, Value>) -> Result<(), NgsiError> {
    if doc.get("mode").and_then(Value::as_str) != Some("auxiliary") {
        return Ok(());
    }
    let Some(ops) = doc.get("operations").and_then(Value::as_array) else {
        return Ok(());
    };
    let allowed = ["retrieveOps", "retrieveEntity", "queryEntity"];
    if let Some(bad_op) = ops
        .iter()
        .filter_map(Value::as_str)
        .find(|o| !allowed.contains(o))
    {
        return Err(NgsiError::BadRequestData(format!(
            "auxiliary registration operations are limited to \
             retrieveOps/retrieveEntity/queryEntity — {bad_op:?} is not allowed (5.9.2.4)"
        )));
    }
    Ok(())
}

/// 5.9.2.4 registration-vs-entity conflicts. Exclusive: "If an Entity
/// already exists for the supplied Entity ID (URI) and the existing Entity
/// contains any of the Attributes defined in the registration, an error of
/// type Conflict shall be raised." Redirect: "If an existing Entity already
/// matches the Context Source Registration, an error of type Conflict shall
/// be raised."
///
/// Read shape, not read volume: this runs under the process-wide
/// registration write lock, so a fold of the tenant here stalls every other
/// registration write on the broker, and a whole-tenant query above the
/// store's row ceiling (5.5.6) would refuse a registration create that has
/// no TooManyResults to raise. An EntityInfo that names a concrete id is
/// answered by reading that Entity — the only shape an exclusive
/// registration has, since 4.3.6.3 requires an entity id. Everything else
/// (an `idPattern`, or a type alone) is answered by walking the tenant a
/// page at a time, asking every EntityInfo about each Entity as it arrives
/// rather than re-reading the tenant per EntityInfo.
fn check_entity_conflict(
    st: &AppState,
    tenant: &antares_model::TenantId,
    doc: &Map<String, Value>,
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
    for info in infos {
        let attrs: Vec<String> = ["propertyNames", "relationshipNames"]
            .iter()
            .flat_map(|k| info.get(*k).and_then(Value::as_array).into_iter().flatten())
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        let ents = info
            .get("entities")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        if ents.is_empty() {
            continue;
        }
        // Every selector of this RegistrationInfo, its idPattern compiled
        // once: an Entity is read at most once and asked about all of them.
        let wants: Vec<_> = ents
            .iter()
            .map(|e| {
                (
                    e.get("id").and_then(Value::as_str),
                    // 5.2.8: an EntityInfo type is a String or a String[]
                    ei_types(e),
                    e.get("idPattern")
                        .and_then(Value::as_str)
                        .and_then(|p| crate::regexcache::compile(p).ok()),
                )
            })
            .collect();
        let hit = |existing: &Value| -> Option<NgsiError> {
            let eid = existing.get("id").and_then(Value::as_str).unwrap_or("");
            for (want_id, want_types, pattern) in &wants {
                let id_hit = match (want_id, pattern) {
                    (Some(w), _) => *w == eid,
                    (None, Some(re)) => re.is_match(eid),
                    (None, None) => true,
                };
                if !id_hit {
                    continue;
                }
                if !want_types.is_empty() {
                    let matches_type =
                        existing
                            .get("type")
                            .and_then(Value::as_array)
                            .is_some_and(|ts| {
                                ts.iter()
                                    .filter_map(Value::as_str)
                                    .any(|x| want_types.contains(&x))
                            });
                    if !matches_type {
                        continue;
                    }
                }
                let conflict = match mode {
                    // exclusive names concrete Attributes (4.3.6.3) — only
                    // an entity already carrying one of them conflicts
                    "exclusive" => attrs.iter().any(|a| existing.get(a).is_some()),
                    // redirect: any matching entity conflicts
                    _ => true,
                };
                if conflict {
                    return Some(NgsiError::Conflict(format!(
                        "existing entity {eid} conflicts with the {mode} registration (5.9.2.4)"
                    )));
                }
            }
            None
        };

        let ids: Vec<&str> = ents
            .iter()
            .filter_map(|e| e.get("id").and_then(Value::as_str))
            .collect();
        if ids.len() == ents.len() && ents.iter().all(|e| e.get("idPattern").is_none()) {
            // Every selector names one Entity, so the read is those Entities
            // and nothing else — bounded by the registration, not by the
            // tenant, whatever the tenant holds.
            for id in &ids {
                if let Some(existing) = st.store.get(tenant, Kind::Entity, id)? {
                    if let Some(conflict) = hit(&existing) {
                        return Err(conflict);
                    }
                }
            }
        } else {
            // A pattern (or a type alone) can only be answered by the
            // Entities of the tenant. The walk stops at the first conflict.
            // ponytail: O(tenant) reads under the registration write lock for
            // a pattern selector; narrow the walk to the id range of
            // `filter::id_pattern_literal` when every selector of the
            // RegistrationInfo carries an anchored literal.
            walk_docs(st, tenant, Kind::Entity, |existing| match hit(&existing) {
                Some(conflict) => Err(conflict),
                None => Ok(()),
            })?;
        }
    }
    Ok(())
}

/// 5.9.2.4: a registration whose expiresAt has been reached counts as
/// deleted — lazily filtered on every read/match path (dt_key so fraction
/// spellings cannot misorder, 4.11).
pub fn reg_expired(doc: &Value) -> bool {
    doc.get("expiresAt")
        .and_then(Value::as_str)
        .is_some_and(|e| {
            crate::temporal::dt_key(e) < crate::temporal::dt_key(&crate::state::now_iso())
        })
}

/// The stored registration for `id`, with 5.9.2.4's deletion applied: "If
/// expiresAt is a date and time in the future, implementations shall delete
/// the Registration when this point in time is reached." The sweep is lazy
/// — "final deletion will always lag the expiresAt timestamp" — so the first
/// operation to name an expired registration performs the deletion and then
/// sees what every later one sees. A read raises ResourceNotFound (5.9.3.4,
/// 5.9.4.4) and a create takes the id back, because 5.9.2.4 raises
/// AlreadyExists only for a registration that exists. Unlike a Subscription
/// (5.8.6), a Registration has no `status` member that keeps an expired one
/// visible.
fn take_live_registration(
    st: &AppState,
    tenant: &TenantId,
    id: &str,
) -> Result<Option<Value>, NgsiError> {
    match st.store.get(tenant, Kind::Registration, id)? {
        Some(doc) if reg_expired(&doc) => {
            st.store.delete(tenant, Kind::Registration, id)?;
            Ok(None)
        }
        live => Ok(live),
    }
}

/// 4.3.6.3 Proxied Registrations: "An exclusive registration shall always
/// relate to specific Attributes found on a single Entity. Thus, the
/// registration shall define both: an entity id (i.e. an id pattern or Entity
/// type defining a group of entities is not supported for exclusive
/// registrations) `[and]` Attributes."
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
    // Fail closed: treating a lookup failure as "no conflicts" would admit a
    // second exclusive registration for the same scope.
    walk_docs(st, tenant, Kind::Registration, |other| {
        let other = &other;
        if other.get("id").and_then(Value::as_str) == self_id {
            return Ok(());
        }
        if reg_expired(other) {
            return Ok(());
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
            return Ok(());
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
                dataset_ids: None,
                csf: None,
                geo: None,
                temporal: None,
            };
            if csr_matches(&spec, other, ctx) {
                let oid = other.get("id").and_then(Value::as_str).unwrap_or("?");
                return Err(NgsiError::AlreadyExists(format!(
                    "proxied registration overlaps {oid} for the same combination of \
                     Entity ID and Attributes (4.3.6.3)"
                )));
            }
        }
        Ok(())
    })
}

/// The 5.9.2.4 conflict rules ("if an exclusive or redirect Context Source
/// Registration already matches … an error of type Conflict shall be
/// raised") are decided by reading the registration set, and the create or
/// update that follows writes it. Read and write are separate store
/// operations, so without this lock two requests can each observe a
/// conflict-free set and both land, leaving the two exclusive registrations
/// for one Entity ID and Attribute the clause forbids. Every registration
/// write holds it for the whole check-then-write sequence.
///
/// Async on purpose: the Postgres driver answers a store call by parking
/// the calling thread on the runtime's I/O driver, and a waiter blocked in
/// a plain `Mutex::lock` would hold a runtime worker hostage; once the
/// worker that owns the I/O driver is among the waiters, the holder never
/// gets its query result and the whole broker stops accepting.
static REGISTRATION_WRITE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn registration_write_lock() -> tokio::sync::MutexGuard<'static, ()> {
    REGISTRATION_WRITE.lock().await
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
        validate_auxiliary_ops(&norm)?;
        let doc = {
            let _serialized = registration_write_lock().await;
            take_live_registration(&st, &tenant, &id)?;
            check_entity_conflict(&st, &tenant, &norm)?;
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
            doc
        };
        st.reg_changed(&tenant, &id, Some(&doc));
        crate::notify::csource_fanout(&st, &tenant, None, Some(doc)).await;
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
            .filter(|d| !reg_expired(d))
            .ok_or_else(|| NgsiError::ResourceNotFound(format!("registration {id} not found")))?;
        let sys = sys_attrs_asked(&params);
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
                crate::regexcache::compile(p)
                    .map(|_| p.clone())
                    .map_err(|_| bad(format!("invalid idPattern {p:?}")))
            })
            .transpose()?;
        // Table 6.8.3.2-1: `type` is a "Selection of Entity Types as per
        // clause 4.17", i.e. ONE expression — splitting it on ',' and
        // expanding the fragments mangles every selector that uses ';' or
        // parentheses. entity_info_matches evaluates it whole.
        spec.types = params.get("type").cloned().map(|s| vec![s]);
        let mut attrs: Vec<String> = params
            .get("attrs")
            .map(|s| s.split(',').map(|t| ctx.expand_key(t.trim())).collect())
            .unwrap_or_default();
        // attributes referenced in q / geoQ count as query projection
        // attributes for matching (5.10.2.4)
        // `CleanParams` percent-decodes every value once, at the extractor
        // (6.3.1). Decoding again here reads escapes that are part of the
        // value, so a `q` legitimately containing `%22` became one carrying a
        // bare quote and 4.9's parser refused a legal query. `csf` below
        // takes the extractor's form, and so does this.
        if let Some(q) = params.get("q") {
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
        // 5.10.2.4: csf is a 4.9 query over Context Source Properties
        let csf = params
            .get("csf")
            .map(|c| antares_ql::parse_q(c))
            .transpose()?;
        let scope_q = params.get("scopeQ").cloned();
        // temporal query: validate + interval presence rules (5.10.2.4)
        let temporal =
            crate::temporal::TemporalQ::from_params(&params, false)?.filter(|t| t.timerel != "any");
        // 5.10.2.4 fixes the order: run the query returning the
        // registrations that "meet all the applicable conditions", THEN
        // "Pagination logic shall be in place as mandated by clause 5.5.9".
        // Filter first, page second — so the window 5.8.4 pushes into the
        // store cannot be pushed here, where it would cut the page out of
        // the stored rows and serve registrations the query never matched.
        //
        // What CAN be bounded is the read. Walking the tenant in pages keeps
        // the peak at one page plus the matches, and moves the ceiling from
        // what the tenant STORES to what the query MATCHES; 5.5.6 licenses
        // TooManyResults for "a query operation ... producing so many
        // results that can potentially exhaust client or server resources",
        // which is a statement about the result and not about the store.
        let keep = |doc: &Value| {
            if reg_expired(doc) {
                return false;
            }
            let has_interval =
                doc.get("observationInterval").is_some() || doc.get("managementInterval").is_some();
            match &temporal {
                None if has_interval => return false,
                Some(tq) if !temporal_interval_matches(doc, tq) => return false,
                _ => {}
            }
            // 5.10.2.4: csf vs Context Source Properties, Scope query vs
            // the registration scope, geoquery vs its location
            if let Some(csf) = &csf {
                if !csf_matches(csf, doc, &ctx) {
                    return false;
                }
            }
            if let Some(sq) = &scope_q {
                if !crate::scope_matches(sq, doc) {
                    return false;
                }
            }
            if let Some(g) = &geo {
                match doc.get("location") {
                    Some(geom) if g.matches_geometry(geom) => {}
                    _ => return false,
                }
            }
            csr_matches(&spec, doc, &ctx)
        };
        let matches = collect_matching(&st, &tenant, keep, *crate::bounds::MAX_FOLD_DOCS)?;
        let (page, count_hdr, links) = crate::entities::paginate_accept(
            &st,
            &params,
            matches,
            "/ngsi-ld/v1/csourceRegistrations",
            accept,
        )?;
        let sys = sys_attrs_asked(&params);
        let payload: Vec<Value> = page
            .iter()
            .map(|d| present_registration(d, &ctx, sys))
            .collect();
        let mut resp =
            crate::negotiate::respond_list(StatusCode::OK, payload, &ctx, accept, &tenant);
        attach_paging(&mut resp, count_hdr, &links);
        Ok::<_, ApiError>(resp)
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

/// Registrations per page of the 5.10.2.4 walk: the peak transient
/// allocation on top of the match set.
const SCAN_PAGE: usize = 1_000;

/// Visit every document of one kind in one tenant, a page at a time. An
/// `Err` from `visit` ends the walk and is the walk's own result, which is
/// how the 5.9.2.4 conflict checks stop at the document they conflict with.
///
/// The whole-tenant `list` and `query_entities` carry the row ceiling meant
/// for client queries (5.5.6). Every caller below must see EVERY document —
/// to answer a query over the registrations, to refuse a second exclusive
/// registration for the same scope, or to find the Entity a redirect
/// registration would shadow — so that ceiling refused them outright once a
/// tenant held more than it, whatever the read narrowed to and however few
/// conflicts existed. A page bounds the allocation by construction and
/// carries no ceiling, so a large tenant costs time here rather than a
/// permanent 403.
pub(crate) fn walk_docs(
    st: &AppState,
    tenant: &antares_model::TenantId,
    kind: Kind,
    mut visit: impl FnMut(Value) -> Result<(), NgsiError>,
) -> Result<(), NgsiError> {
    let mut after: Option<String> = None;
    loop {
        let page = st
            .store
            .list_page(tenant, kind, after.as_deref(), SCAN_PAGE)?;
        let short = page.len() < SCAN_PAGE;
        let before = after.clone();
        for doc in page {
            if let Some(id) = doc.get("id").and_then(Value::as_str) {
                after = Some(id.to_owned());
            }
            visit(doc)?;
        }
        // A short page ends the walk, and so does a cursor that did not
        // move: only a document carrying an `id` advances it, so a full page
        // without one would otherwise be re-read forever.
        if short || after == before {
            break;
        }
    }
    Ok(())
}

/// Every registration of one tenant that `keep` accepts.
/// Every registration of one tenant that `keep` accepts, up to `ceiling`.
///
/// 5.10.2.4 filters before it pages, so the page cannot be pushed into the
/// store and the whole match set is held at once. A broker is built for
/// 100 000+ registrations per tenant, so "the whole match set" is a number a
/// client picks with one `type=` — and 5.5.6 gives the answer for "a query
/// operation … producing so many results that can potentially exhaust client
/// or server resources": TooManyResults, rather than the memory.
fn collect_matching(
    st: &AppState,
    tenant: &antares_model::TenantId,
    keep: impl Fn(&Value) -> bool,
    ceiling: usize,
) -> Result<Vec<Value>, NgsiError> {
    let mut matches = Vec::new();
    walk_docs(st, tenant, Kind::Registration, |doc| {
        if keep(&doc) {
            if matches.len() == ceiling {
                return Err(NgsiError::TooManyResults(format!(
                    "the query matches more than {ceiling} registrations — narrow it (5.5.6)"
                )));
            }
            matches.push(doc);
        }
        Ok(())
    })?;
    Ok(matches)
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
pub(crate) fn temporal_interval_matches(doc: &Value, tq: &crate::temporal::TemporalQ) -> bool {
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
    /// 5.12 datasetId condition (should-level): with both the request and
    /// the CSourceRegistration specifying datasetId, they match only with
    /// "at least one value in common"; one side alone always matches.
    pub dataset_ids: Option<Vec<String>>,
    /// 4.9 Context Source Filter: with a csf present, only registrations
    /// whose Context Source Properties match it are considered (query,
    /// temporal query, purge, entityMaps — 5.7.2.4/5.7.4.4/5.6.21.4).
    pub csf: Option<antares_ql::QNode>,
    /// 5.2.9 location ("Location for which the Context Source may be able
    /// to provide information") + 4.3.6.1: a geo query is only distributed
    /// to registrations whose location geometry matches it.
    pub geo: Option<crate::geo::GeoQuery>,
    /// 5.2.9 observationInterval/managementInterval: "matched against the
    /// observationInterval for overlap" — a temporal read is only
    /// distributed to registrations whose declared interval overlaps the
    /// temporal query; a registration declaring NO interval stays
    /// unconstrained (both members are optional).
    pub temporal: Option<crate::temporal::TemporalQ>,
}

/// 5.2.8: EntityInfo type is a String or String[] — yield every named type.
fn ei_types(ei: &Value) -> Vec<&str> {
    match ei.get("type") {
        Some(Value::String(s)) => vec![s.as_str()],
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    }
}

/// 5.12: does an EntityInfo element match the entity specification?
fn entity_info_matches(spec: &CsrSpec, ei: &Value, ctx: &Context) -> bool {
    if let Some(types) = &spec.types {
        let its = ei_types(ei);
        // EntityInfo without a type restricts only by id/idPattern. Each spec
        // entry is a 4.17 Entity Type Selection over the WHOLE declared type
        // list (a conjunction needs every named type present); a plain
        // expanded IRI is the one-term case of the same evaluation.
        if !its.is_empty()
            && !types.iter().any(|t| {
                its.contains(&t.as_str()) || crate::entities::type_selection_matches(t, &its, ctx)
            })
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
            if let Ok(re) = crate::regexcache::compile(p) {
                if ids.iter().any(|i| re.find(i).is_some()) {
                    return true;
                }
            }
        }
    }
    if let Some(qp) = &spec.id_pattern {
        if let Some(rid) = ei_id {
            if crate::regexcache::compile(qp).is_ok_and(|re| re.find(rid).is_some()) {
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

/// 5.10.2.4: the context source filter (csf, 4.9) evaluates over the
/// registration document's own Context Source Properties — its members are
/// wrapped as Property instances so the shared 4.9 evaluator applies.
pub(crate) fn csf_matches(csf: &antares_ql::QNode, reg: &Value, ctx: &Context) -> bool {
    let Some(obj) = reg.as_object() else {
        return false;
    };
    let mut pseudo = Map::new();
    pseudo.insert("id".into(), obj.get("id").cloned().unwrap_or(Value::Null));
    pseudo.insert(
        "type".into(),
        serde_json::json!(["ContextSourceRegistration"]),
    );
    for (k, v) in obj {
        if ["id", "type", "information", "createdAt", "modifiedAt"].contains(&k.as_str()) {
            continue;
        }
        // a Context Source Property stored in attribute form (5.2.9) is an
        // instance already — only bare scalars need the Property wrap
        let inst = match v {
            Value::Array(_) => v.clone(),
            Value::Object(o) if o.contains_key("value") || o.contains_key("object") => {
                serde_json::json!([v])
            }
            _ => serde_json::json!([{"type": "Property", "value": v}]),
        };
        pseudo.insert(ctx.expand_key(k), inst);
    }
    crate::qeval::eval_q(csf, &Value::Object(pseudo), ctx, &|_| None)
}

pub fn csr_matches(spec: &CsrSpec, doc: &Value, ctx: &Context) -> bool {
    !matching_infos(spec, doc, ctx).is_empty()
}

/// Full 5.11.2.4 match of a registration against a csource subscription:
/// 5.12 entity/attr matching + temporal interval rules + geoQ vs the
/// registration's own `location`.
///
/// Not the same rule as `antares_matcher::selector_match`, which it shares a
/// signature with: that one asks whether an ENTITY satisfies a
/// subscription's `entities` selector, this one asks whether a REGISTRATION
/// does — and a registration carries its selector inside `information`, has
/// an observation/management interval a latest-information subscription must
/// not match, and answers `geoQ` from its own `location` rather than from a
/// GeoProperty of the data. The part that IS the same rule is the selector
/// walk, and it is not written twice: `spec_for_subscription` turns the
/// subscription into a `CsrSpec` and `csr_matches` walks it.
pub fn csr_matches_subscription(sub: &Value, reg: &Value, ctx: &Context) -> bool {
    // An expired registration is no longer a Context Source: it must not be
    // reported as newlyMatching, nor receive a forwarded subscription copy.
    if reg_expired(reg) {
        return false;
    }
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
        if let Ok(Some(gq)) = crate::geo::GeoQuery::from_params(&antares_matcher::geo_params(g)) {
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
    // 5.11.2.4: csf vs the registration's Context Source Properties, scopeQ
    // vs its scope property
    if let Some(csf) = sub.get("csf").and_then(Value::as_str) {
        match antares_ql::parse_q(csf) {
            Ok(ast) if csf_matches(&ast, reg, ctx) => {}
            _ => return false,
        }
    }
    if let Some(sq) = sub.get("scopeQ").and_then(Value::as_str) {
        if !crate::scope_matches(sq, reg) {
            return false;
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

/// 5.9.3 Update Context Source Registration: invalid URI 400, unknown 404,
/// 5.2.9 fragment merge per 5.5.8 with every mode rule re-checked on the
/// post-merge document (4.3.6.3 exclusive shape, auxiliary ops limit,
/// entity/registration conflicts).
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
        let obj = parsed.object(NgsiError::BadRequestData(
            "fragment must be a JSON object".into(),
        ))?;
        let norm = normalize_registration(obj, &parsed.ctx, true)?;
        let ts = now_iso();
        // The 5.9.3.4 re-checks below read the registration set that the
        // mutate then writes — the pair is atomic or a concurrent write can
        // invalidate the checks between them.
        let (before, res) = {
            let _serialized = registration_write_lock().await;
            let before = take_live_registration(&st, &tenant, &id)?;
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
                // 5.9.3.4: the mode-specific rules apply to the merged document
                validate_auxiliary_ops(&merged)?;
                check_entity_conflict(&st, &tenant, &merged)?;
                check_proxied_overlap(&st, &tenant, &merged, Some(&id), &parsed.ctx)?;
            }
            let res = st.store.mutate(&tenant, Kind::Registration, &id, |doc| {
                let Some(target) = doc.as_object_mut() else {
                    return Err(NgsiError::InternalError(
                        "stored registration is not a JSON object".into(),
                    ));
                };
                crate::apply_doc_fragment(target, &norm, &ts);
                Ok::<(), NgsiError>(())
            })?;
            (before, res)
        };
        match res {
            None => Err(NgsiError::ResourceNotFound(format!("registration {id} not found")).into()),
            Some(Err(e)) => Err(ApiError::from(e)),
            Some(Ok(())) => {
                let after = st.store.get(&tenant, Kind::Registration, &id)?;
                st.reg_changed(&tenant, &id, after.as_ref());
                crate::notify::csource_fanout(&st, &tenant, before, after).await;
                Ok(no_content(&tenant))
            }
        }
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

/// 5.9.4 Delete Context Source Registration: invalid URI 400, unknown id
/// 404, 204 on removal (registry mirror + csource subscriptions refresh).
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
        let before = take_live_registration(&st, &tenant, &id)?;
        if st.store.delete(&tenant, Kind::Registration, &id)? {
            st.reg_changed(&tenant, &id, None);
            crate::notify::csource_fanout(&st, &tenant, before, None).await;
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
mod clause_4_20 {
    use super::*;
    use antares_jsonld::Loader;
    use serde_json::json;

    fn reg_with(ops: Value) -> Result<Map<String, Value>, NgsiError> {
        let ctx = Loader::new().core();
        let doc = json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:ops",
            "type": "ContextSourceRegistration",
            "endpoint": "http://peer:9090",
            "information": [{"entities": [{"type": "Building"}]}],
            "operations": ops
        });
        normalize_registration(doc.as_object().expect("object"), &ctx, false)
    }

    /// Table 5.2.9-1: `operations` entries "are limited to the named API
    /// operations and named operation groups (see clause 4.20)". Every name
    /// of Table 4.20-1 and every group of Table 4.20-2 is accepted, and
    /// nothing else is — including a name that only looks like one.
    #[test]
    fn only_the_4_20_vocabulary_is_accepted() {
        for op in OPERATION_NAMES.iter().chain(OPERATION_GROUPS) {
            assert!(reg_with(json!([op])).is_ok(), "{op} is a 4.20 name");
        }
        assert!(
            reg_with(json!(OPERATION_NAMES.to_vec())).is_ok(),
            "the whole vocabulary at once is legal"
        );
        for bad in [
            "notARealOp",
            "createentity",
            "createEntity ",
            "federationops",
            "queryEntities",
            "",
        ] {
            let e = reg_with(json!([bad])).expect_err("outside the vocabulary");
            assert!(
                matches!(e, NgsiError::BadRequestData(_)),
                "{bad} must be BadRequestData, got {e:?}"
            );
        }
        // 5.2.9: the member is present-or-absent, never an empty list
        assert!(reg_with(json!([])).is_err(), "empty operations");
        assert!(reg_with(json!("federationOps")).is_err(), "not an array");
    }
}

#[cfg(test)]
mod clause_5_11_2 {
    use super::*;
    use antares_jsonld::Loader;
    use serde_json::json;

    /// 5.11.2.4: a csource subscription's csf matches the registration's
    /// own Context Source Properties, and its scopeQ matches the
    /// registration scope.
    #[test]
    fn csource_subscription_csf_and_scope_matching() {
        let ctx = Loader::new().core();
        let reg_a = json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:sub-a",
            "type": "ContextSourceRegistration",
            "information": [{"entities": [{"type": "Building"}]}],
            "endpoint": "http://a.example.com",
            "scope": "/Madrid/Centro"
        });
        let reg_b = json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:sub-b",
            "type": "ContextSourceRegistration",
            "information": [{"entities": [{"type": "Building"}]}],
            "endpoint": "http://b.example.com",
            "scope": "/Berlin"
        });
        let sub_csf = json!({
            "entities": [{"type": "Building"}],
            "csf": "endpoint==\"http://a.example.com\""
        });
        assert!(csr_matches_subscription(&sub_csf, &reg_a, &ctx));
        assert!(
            !csr_matches_subscription(&sub_csf, &reg_b, &ctx),
            "csf must exclude the other endpoint"
        );
        let sub_scope = json!({
            "entities": [{"type": "Building"}],
            "scopeQ": "/Madrid/#"
        });
        assert!(csr_matches_subscription(&sub_scope, &reg_a, &ctx));
        assert!(
            !csr_matches_subscription(&sub_scope, &reg_b, &ctx),
            "scopeQ must exclude /Berlin"
        );
    }
}

#[cfg(test)]
mod csi_tests {
    use super::*;
    use antares_jsonld::Loader;
    use serde_json::json;

    /// 5.5.4: "urn:ngsi-ld:null" as a first-level member value is
    /// BadRequestData on create; on patch it is the Fragment removal form.
    #[test]
    fn clause_5_5_4_first_level_null_in_registration() {
        let ctx = Loader::new().core();
        let doc = json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:n1",
            "type": "ContextSourceRegistration",
            "endpoint": "http://peer:9090",
            "description": "urn:ngsi-ld:null",
            "information": [{"entities": [{"type": "Building"}]}]
        });
        assert!(
            normalize_registration(doc.as_object().unwrap(), &ctx, false).is_err(),
            "create with a first-level null URN must be rejected"
        );
        // patch: the same member is a removal fragment (stored as Null)
        let patch = json!({"description": "urn:ngsi-ld:null"});
        let out = normalize_registration(patch.as_object().unwrap(), &ctx, true)
            .expect("patch fragment null is legal");
        assert!(out["description"].is_null());
    }

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

    /// 5.2.8 Table 5.2.8-1 — an EntityInfo `type` is "String or String[]" —
    /// applied to the 5.9.2.4 redirect rule: only an entity that matches the
    /// registered Entity type conflicts, whichever spelling was registered.
    #[test]
    fn clause_5_9_2_4_redirect_conflict_honours_the_array_form_entity_type() {
        let st = crate::state::AppState::new("me".into());
        let tenant = antares_model::TenantId::new("default").expect("tenant");
        let ctx = st.loader.core();
        let mut building = Map::new();
        building.insert("id".into(), json!("urn:ngsi-ld:Building:b1"));
        building.insert("type".into(), json!([ctx.expand_key("Building")]));
        st.store
            .create(
                &tenant,
                Kind::Entity,
                "urn:ngsi-ld:Building:b1",
                Value::Object(building),
            )
            .expect("seed building");
        let reg = |ty: Value| {
            let doc = json!({
                "id": "urn:ngsi-ld:ContextSourceRegistration:red1",
                "type": "ContextSourceRegistration",
                "endpoint": "http://peer:9090",
                "mode": "redirect",
                "information": [{"entities": [{"type": ty}]}]
            });
            normalize_registration(doc.as_object().expect("object"), &ctx, false).expect("valid")
        };
        let err = |ty: Value| match check_entity_conflict(&st, &tenant, &reg(ty)) {
            Err(NgsiError::Conflict(m)) => Some(m),
            Err(other) => panic!("unexpected error {other:?}"),
            Ok(()) => None,
        };
        assert_eq!(
            err(json!(["Vehicle"])),
            None,
            "a redirect for Vehicle must not conflict with a Building"
        );
        assert_eq!(err(json!("Vehicle")), None, "same, in the string spelling");
        let hit = err(json!(["Building"])).expect("the Building conflicts");
        assert!(
            hit.contains("urn:ngsi-ld:Building:b1"),
            "the conflict names the existing entity: {hit}"
        );
        assert!(err(json!("Building")).is_some(), "same, string spelling");
    }

    /// 5.9.2.4 redirect: "If an existing Entity already matches the
    /// `Context Source Registration`, an error of type Conflict shall be
    /// raised." An EntityInfo may identify its Entities by `idPattern`
    /// alone (5.2.8) — a predicate no store can decide — and a
    /// RegistrationInfo may carry several EntityInfo entries, so each
    /// Entity read is asked about all of them.
    #[test]
    fn clause_5_9_2_4_redirect_conflict_matches_an_id_pattern() {
        let st = crate::state::AppState::new("me".into());
        let tenant = antares_model::TenantId::new("default").expect("tenant");
        let ctx = st.loader.core();
        for (id, ty) in [
            ("urn:ngsi-ld:Vehicle:v1", "Vehicle"),
            ("urn:ngsi-ld:Device:d1", "Device"),
        ] {
            let mut e = Map::new();
            e.insert("id".into(), json!(id));
            e.insert("type".into(), json!([ctx.expand_key(ty)]));
            st.store
                .create(&tenant, Kind::Entity, id, Value::Object(e))
                .expect("seed");
        }
        let err = |ents: Value| {
            let doc = json!({
                "id": "urn:ngsi-ld:ContextSourceRegistration:pat1",
                "type": "ContextSourceRegistration",
                "endpoint": "http://peer:9090",
                "mode": "redirect",
                "information": [{"entities": ents}]
            });
            let norm = normalize_registration(doc.as_object().expect("object"), &ctx, false)
                .expect("valid");
            match check_entity_conflict(&st, &tenant, &norm) {
                Err(NgsiError::Conflict(m)) => Some(m),
                Err(other) => panic!("unexpected error {other:?}"),
                Ok(()) => None,
            }
        };
        assert_eq!(
            err(json!([{"idPattern": "^urn:ngsi-ld:Nothing:.*"}])),
            None,
            "a pattern no entity matches does not conflict"
        );
        let hit = err(json!([{"idPattern": "^urn:ngsi-ld:Vehicle:.*"}]))
            .expect("the Vehicle matches the pattern");
        assert!(hit.contains("urn:ngsi-ld:Vehicle:v1"), "{hit}");
        assert_eq!(
            err(json!([{"idPattern": "^urn:ngsi-ld:Vehicle:.*", "type": "Device"}])),
            None,
            "the pattern and the type must both hold: the Vehicle is not a Device"
        );
        // The second EntityInfo is the one that matches: every selector of
        // the RegistrationInfo is asked about every Entity, not only the first.
        let hit = err(json!([
            {"idPattern": "^urn:ngsi-ld:Nothing:.*"},
            {"idPattern": "^urn:ngsi-ld:Device:.*"}
        ]))
        .expect("the Device matches the second EntityInfo");
        assert!(hit.contains("urn:ngsi-ld:Device:d1"), "{hit}");
    }

    /// The Entities of the tenant are read a page at a time (a whole-tenant
    /// read is refused above the store's row ceiling, 5.5.6, and this check
    /// has no TooManyResults to raise), so a conflict that sits beyond the
    /// first page must still be found.
    #[test]
    fn clause_5_9_2_4_a_conflict_past_the_first_page_is_found() {
        let st = crate::state::AppState::new("me".into());
        let tenant = antares_model::TenantId::new("default").expect("tenant");
        let ctx = st.loader.core();
        let device = ctx.expand_key("Device");
        for i in 0..SCAN_PAGE {
            let id = format!("urn:ngsi-ld:Device:{i:06}");
            let mut e = Map::new();
            e.insert("id".into(), json!(id));
            e.insert("type".into(), json!([device]));
            st.store
                .create(&tenant, Kind::Entity, &id, Value::Object(e))
                .expect("seed");
        }
        // sorts after every Device above, so it is only reached on page two
        let mut zebra = Map::new();
        zebra.insert("id".into(), json!("urn:ngsi-ld:Zebra:z1"));
        zebra.insert("type".into(), json!([ctx.expand_key("Zebra")]));
        st.store
            .create(
                &tenant,
                Kind::Entity,
                "urn:ngsi-ld:Zebra:z1",
                Value::Object(zebra),
            )
            .expect("seed");
        let doc = json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:pat2",
            "type": "ContextSourceRegistration",
            "endpoint": "http://peer:9090",
            "mode": "redirect",
            "information": [{"entities": [{"idPattern": "^urn:ngsi-ld:Zebra:.*"}]}]
        });
        let norm =
            normalize_registration(doc.as_object().expect("object"), &ctx, false).expect("valid");
        match check_entity_conflict(&st, &tenant, &norm) {
            Err(NgsiError::Conflict(m)) => assert!(m.contains("urn:ngsi-ld:Zebra:z1"), "{m}"),
            other => panic!("the conflict on page two was missed: {other:?}"),
        }
    }

    /// 5.9.2.4: an exclusive registration conflicts with an existing Entity
    /// only when "the existing Entity contains any of the Attributes defined
    /// in the registration".
    #[test]
    fn clause_5_9_2_4_exclusive_conflict_needs_a_registered_attribute() {
        let st = crate::state::AppState::new("me".into());
        let tenant = antares_model::TenantId::new("default").expect("tenant");
        let ctx = st.loader.core();
        let mut vehicle = Map::new();
        vehicle.insert("id".into(), json!("urn:ngsi-ld:Vehicle:v1"));
        vehicle.insert("type".into(), json!([ctx.expand_key("Vehicle")]));
        vehicle.insert(ctx.expand_key("color"), json!([{"type": "Property"}]));
        st.store
            .create(
                &tenant,
                Kind::Entity,
                "urn:ngsi-ld:Vehicle:v1",
                Value::Object(vehicle),
            )
            .expect("seed vehicle");
        let reg = |attr: &str, id: &str| {
            let doc = json!({
                "id": "urn:ngsi-ld:ContextSourceRegistration:exc1",
                "type": "ContextSourceRegistration",
                "endpoint": "http://peer:9090",
                "mode": "exclusive",
                "information": [{
                    "entities": [{"id": id, "type": "Vehicle"}],
                    "propertyNames": [attr]
                }]
            });
            normalize_registration(doc.as_object().expect("object"), &ctx, false).expect("valid")
        };
        assert!(
            check_entity_conflict(&st, &tenant, &reg("speed", "urn:ngsi-ld:Vehicle:v1")).is_ok(),
            "the entity carries no speed Attribute"
        );
        assert!(
            check_entity_conflict(&st, &tenant, &reg("color", "urn:ngsi-ld:Vehicle:v2")).is_ok(),
            "another entity id is not this entity"
        );
        assert!(
            check_entity_conflict(&st, &tenant, &reg("color", "urn:ngsi-ld:Vehicle:v1")).is_err(),
            "the entity carries the registered color Attribute"
        );
    }

    /// 6.8.3.2 Table 6.8.3.2-1: the `type` parameter of Query Context Source
    /// Registrations is "Selection of Entity Types as per clause 4.17" — a
    /// selection expression, not a comma-separated list of terms.
    #[tokio::test]
    async fn clause_4_17_type_selection_queries_registrations() {
        let st = crate::state::AppState::new("me".into());
        let tenant = antares_model::TenantId::new("default").expect("tenant");
        let ctx = st.loader.core();
        let seed = |id: &str, ty: Value| {
            let doc = json!({
                "id": id,
                "type": "ContextSourceRegistration",
                "endpoint": "http://peer:9090",
                "information": [{"entities": [{"type": ty}]}]
            });
            let norm =
                normalize_registration(doc.as_object().expect("object"), &ctx, false).expect("ok");
            st.store
                .create(&tenant, Kind::Registration, id, Value::Object(norm))
                .expect("seed");
        };
        seed(
            "urn:ngsi-ld:ContextSourceRegistration:both",
            json!(["Home", "Vehicle"]),
        );
        seed("urn:ngsi-ld:ContextSourceRegistration:home", json!("Home"));
        let ids = |sel: &str| {
            let st = st.clone();
            let sel = sel.to_owned();
            async move {
                let params = HashMap::from([("type".to_owned(), sel)]);
                let resp =
                    query_registrations(State(st), CleanParams(params), HeaderMap::new()).await;
                assert_eq!(resp.status(), StatusCode::OK);
                let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
                    .await
                    .expect("body");
                let body: Value = serde_json::from_slice(&bytes).expect("json list");
                body.as_array()
                    .expect("array")
                    .iter()
                    .filter_map(|r| r.get("id").and_then(Value::as_str).map(str::to_owned))
                    .collect::<Vec<String>>()
            }
        };
        let conj = ids("(Home;Vehicle)").await;
        assert!(
            conj.contains(&"urn:ngsi-ld:ContextSourceRegistration:both".to_owned()),
            "a registration declaring both types matches the conjunction: {conj:?}"
        );
        assert!(
            !conj.contains(&"urn:ngsi-ld:ContextSourceRegistration:home".to_owned()),
            "a registration declaring only Home must not match: {conj:?}"
        );
        let alt = ids("Vehicle,Home").await;
        assert_eq!(alt.len(), 2, "a comma list is a disjunction: {alt:?}");
        let none = ids("Parking").await;
        assert!(
            none.is_empty(),
            "no registration declares Parking: {none:?}"
        );
    }

    /// 5.9.2.4: "If expiresAt is a date and time in the past, an error of
    /// type BadRequestData shall be raised" — the comparison is over the
    /// instant, not over the spelling, so an expiresAt written with fewer
    /// sub-second digits than the server's own timestamp is still past.
    #[test]
    fn clause_5_9_2_4_past_expires_at_whatever_the_fraction_spelling() {
        let ctx = Loader::new().core();
        for _ in 0..8 {
            let now = now_iso();
            if now.len() < 24 || &now[20..23] == "000" {
                continue; // no sub-second gap to compare against
            }
            let past = format!("{}Z", &now[..19]); // same second, no fraction
            let doc = json!({
                "id": "urn:ngsi-ld:ContextSourceRegistration:exp1",
                "type": "ContextSourceRegistration",
                "endpoint": "http://peer:9090",
                "expiresAt": past,
                "information": [{"entities": [{"type": "Vehicle"}]}]
            });
            assert!(
                normalize_registration(doc.as_object().expect("object"), &ctx, false).is_err(),
                "expiresAt {past} is in the past of {now}"
            );
        }
    }

    /// The registration body's cardinality caps are the only bound on the
    /// index rows one create materialises: the last accepted size and the
    /// first rejected one both have to hold.
    #[test]
    fn registration_cardinality_caps_hold_at_the_edge() {
        let ctx = Loader::new().core();
        let info = |n: usize| {
            json!({"entities": [{"type": "Vehicle"}], "propertyNames":
            (0..n).map(|i| Value::String(format!("p{i}"))).collect::<Vec<_>>()})
        };
        let mk = |information: Value| {
            let doc = json!({
                "id": "urn:ngsi-ld:ContextSourceRegistration:cap1",
                "type": "ContextSourceRegistration",
                "endpoint": "http://peer:9090",
                "information": information
            });
            normalize_registration(doc.as_object().expect("object"), &ctx, false)
        };
        let many = |n: usize| Value::Array((0..n).map(|_| info(1)).collect());
        assert!(mk(many(MAX_INFORMATION)).is_ok(), "the cap itself is legal");
        let over = mk(many(MAX_INFORMATION + 1)).expect_err("over the cap");
        assert!(matches!(over, NgsiError::BadRequestData(_)), "{over:?}");
        // the status is what the client sees, and 403 would tell it to
        // narrow a query it never sent
        assert_eq!(over.status(), 400);
        assert!(mk(json!([info(MAX_INFO_MEMBERS)])).is_ok());
        assert!(matches!(
            mk(json!([info(MAX_INFO_MEMBERS + 1)])),
            Err(NgsiError::BadRequestData(_))
        ));
        let entities = |n: usize| {
            json!([{"entities": (0..n)
                .map(|i| json!({"id": format!("urn:ngsi-ld:Vehicle:v{i}"), "type": "Vehicle"}))
                .collect::<Vec<_>>()}])
        };
        assert!(mk(entities(MAX_INFO_MEMBERS)).is_ok());
        assert!(matches!(
            mk(entities(MAX_INFO_MEMBERS + 1)),
            Err(NgsiError::BadRequestData(_))
        ));
        let rels = |n: usize| {
            json!([{"entities": [{"type": "Vehicle"}], "relationshipNames":
                (0..n).map(|i| Value::String(format!("r{i}"))).collect::<Vec<_>>()}])
        };
        assert!(mk(rels(MAX_INFO_MEMBERS)).is_ok());
        assert!(matches!(
            mk(rels(MAX_INFO_MEMBERS + 1)),
            Err(NgsiError::BadRequestData(_))
        ));
    }

    /// 4.3.6.6: the four processed contextSourceInfo keys have
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

    /// 5.5.6 + 5.10.2.4: the registration query filters before it pages, so
    /// every match is held at once. A tenant at the 100 000-registration
    /// target answers one `type=` selector with every one of them, which is
    /// the "so many results that can potentially exhaust … server resources"
    /// 5.5.6 names — the query is refused at the ceiling instead of building
    /// the answer.
    #[test]
    fn clause_5_5_6_a_registration_query_stops_at_the_fold_ceiling() {
        let st = AppState::new("antares-csr-ceiling".into());
        let tenant = TenantId::default();
        for i in 0..5 {
            let id = format!("urn:ngsi-ld:ContextSourceRegistration:ceil{i}");
            st.store
                .create(
                    &tenant,
                    Kind::Registration,
                    &id,
                    json!({
                        "id": id,
                        "type": "ContextSourceRegistration",
                        "endpoint": "http://peer:9090",
                        "information": [{"entities": [{"type": "Building"}]}],
                    }),
                )
                .expect("store the registration");
        }
        let all = collect_matching(&st, &tenant, |_| true, 100).expect("under the ceiling");
        assert_eq!(all.len(), 5, "every registration matches");
        let err = collect_matching(&st, &tenant, |_| true, 2)
            .expect_err("a match set over the ceiling is refused");
        assert!(
            matches!(err, NgsiError::TooManyResults(_)),
            "5.5.6 names TooManyResults, got {err:?}"
        );
        // the ceiling counts MATCHES, not documents walked: a narrow query
        // over the same tenant still answers
        let narrow = collect_matching(
            &st,
            &tenant,
            |d| d["id"] == json!("urn:ngsi-ld:ContextSourceRegistration:ceil3"),
            2,
        )
        .expect("one match is under any ceiling");
        assert_eq!(narrow.len(), 1);
    }

    /// 6.3.19: "Key and value members shall adhere to IETF RFC 7230 …
    /// definitions concerning HTTP headers". A pair that is not a header is
    /// refused where 5.9.2.4 refuses registration content, not carried until
    /// the first forward — where the request cannot be built at all, and the
    /// registration that caused it is nowhere in the failure.
    #[test]
    fn clause_6_3_19_a_pair_that_is_not_a_header_is_refused() {
        let ctx = Loader::new().core();
        let mk = |key: &str, value: &str| {
            json!({
                "id": "urn:ngsi-ld:ContextSourceRegistration:csi2",
                "type": "ContextSourceRegistration",
                "endpoint": "http://peer:9090",
                "information": [{"entities": [{"type": "Building"}]}],
                "contextSourceInfo": [{"key": key, "value": value}]
            })
        };
        let ok = |key: &str, value: &str| {
            normalize_registration(mk(key, value).as_object().expect("object"), &ctx, false).is_ok()
        };
        // RFC 7230 field-name is a token: no separators, no space, no CTL
        for key in [
            "X-Injected\r\nX-Second",
            "X Injected",
            "X:Injected",
            "",
            "X-Inj\u{0000}ected",
            "Über-Header",
        ] {
            assert!(!ok(key, "value"), "{key:?} is not an RFC 7230 field-name");
        }
        // RFC 7230 field-value carries no CR, LF or NUL
        for value in ["a\r\nX-Injected: 1", "a\nb", "a\rb", "a\u{0000}b"] {
            assert!(
                !ok("X-Custom", value),
                "{value:?} is not an RFC 7230 field-value"
            );
        }
        // and the shapes a real registration uses stay accepted
        assert!(ok("X-Custom", "urn:ngsi-ld:request"));
        assert!(ok("X-Api-Key", "a b c"));
        assert!(ok("Authorization", "Bearer abc.def-ghi_jkl"));
    }
}

#[cfg(test)]
mod concurrent_create_5_9_2_4 {
    use super::*;

    fn exclusive_body(n: usize) -> Bytes {
        Bytes::from(format!(
            r#"{{"id":"urn:ngsi-ld:ContextSourceRegistration:race{n}",
                 "type":"ContextSourceRegistration",
                 "endpoint":"http://peer:9090",
                 "mode":"exclusive",
                 "information":[{{"entities":[{{"id":"urn:ngsi-ld:Vehicle:race",
                                                "type":"Vehicle"}}],
                                  "propertyNames":["speed"]}}]}}"#
        ))
    }

    fn json_headers() -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(
            axum::http::header::CONTENT_TYPE,
            "application/json".parse().expect("header value"),
        );
        h
    }

    /// 5.9.2.4: "If an exclusive or redirect Context Source Registration
    /// already matches against the Entity ID (URI) and any of the Attributes
    /// defined in the registration, an error of type Conflict shall be
    /// raised." Requests racing each other must not both slip past that
    /// check: whatever the interleaving, one registration is stored and
    /// every other request gets the Conflict.
    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn concurrent_exclusive_creates_store_exactly_one_registration() {
        let tenant = antares_model::TenantId::new("default").expect("tenant");
        for round in 0..25 {
            let st = crate::state::AppState::new("me".into());
            let gate = std::sync::Arc::new(tokio::sync::Barrier::new(8));
            let mut tasks = Vec::new();
            for n in 0..8 {
                let (st, gate) = (st.clone(), gate.clone());
                tasks.push(tokio::spawn(async move {
                    gate.wait().await;
                    create_registration(
                        State(st),
                        CleanParams(HashMap::new()),
                        json_headers(),
                        exclusive_body(n),
                    )
                    .await
                    .status()
                }));
            }
            let mut created = 0;
            for t in tasks {
                match t.await.expect("task") {
                    StatusCode::CREATED => created += 1,
                    StatusCode::CONFLICT => {}
                    other => panic!("round {round}: unexpected status {other}"),
                }
            }
            let stored = st
                .store
                .list(&tenant, Kind::Registration)
                .expect("registrations");
            assert_eq!(
                stored.len(),
                1,
                "round {round}: 5.9.2.4 forbids a second exclusive registration \
                 for the same Entity ID and Attribute"
            );
            assert_eq!(created, 1, "round {round}: exactly one create may succeed");
        }
    }

    /// 5.9.3.4 applies the same 5.9.2.4 mode rules to the merged document,
    /// so two patches that each flip an inclusive registration to exclusive
    /// over one Entity ID and Attribute may not both take effect.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_patches_to_exclusive_leave_one_exclusive_registration() {
        let tenant = antares_model::TenantId::new("default").expect("tenant");
        for round in 0..25 {
            let st = crate::state::AppState::new("me".into());
            for n in 0..4 {
                let body = Bytes::from(
                    String::from_utf8_lossy(&exclusive_body(n))
                        .replace(r#""mode":"exclusive","#, ""),
                );
                let status = create_registration(
                    State(st.clone()),
                    CleanParams(HashMap::new()),
                    json_headers(),
                    body,
                )
                .await
                .status();
                assert_eq!(status, StatusCode::CREATED, "round {round}: seed {n}");
            }
            let gate = std::sync::Arc::new(tokio::sync::Barrier::new(4));
            let mut tasks = Vec::new();
            for n in 0..4 {
                let (st, gate) = (st.clone(), gate.clone());
                tasks.push(tokio::spawn(async move {
                    gate.wait().await;
                    update_registration(
                        State(st),
                        Path(format!("urn:ngsi-ld:ContextSourceRegistration:race{n}")),
                        CleanParams(HashMap::new()),
                        json_headers(),
                        Bytes::from(r#"{"mode":"exclusive"}"#),
                    )
                    .await
                    .status()
                }));
            }
            let mut patched = 0;
            for t in tasks {
                match t.await.expect("task") {
                    StatusCode::NO_CONTENT => patched += 1,
                    StatusCode::CONFLICT => {}
                    other => panic!("round {round}: unexpected status {other}"),
                }
            }
            let exclusive = st
                .store
                .list(&tenant, Kind::Registration)
                .expect("registrations")
                .iter()
                .filter(|r| r.get("mode").and_then(Value::as_str) == Some("exclusive"))
                .count();
            assert_eq!(
                exclusive, 1,
                "round {round}: 5.9.2.4 forbids a second exclusive registration \
                 for the same Entity ID and Attribute"
            );
            assert_eq!(patched, 1, "round {round}: exactly one patch may succeed");
        }
    }
}
