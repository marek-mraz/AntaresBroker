// SPDX-License-Identifier: EUPL-1.2
//! /subscriptions and /csourceSubscriptions (5.8, 5.11; resources 6.10/6.11,
//! 6.12/6.13). One implementation, two store kinds — both use the
//! Subscription data type (5.2.12).

use crate::negotiate::*;
use crate::state::{now_iso, AppState};
use antares_jsonld::{parse_datetime, Context};
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

fn resource_path(kind: Kind) -> &'static str {
    match kind {
        Kind::CSourceSubscription => "csourceSubscriptions",
        _ => "subscriptions",
    }
}

/// Validate + normalize a subscription document (5.8.1). Types/attribute
/// names are expanded to IRIs; the rest is stored verbatim.
pub fn normalize_subscription(
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
        // 5.4 Fragment member removal: null and the NGSI-LD Null both delete
        // the member. Read literally, the NGSI-LD Null was stored as the
        // string it is spelled with, so the member survived carrying it.
        if is_patch && k != "id" && (v.is_null() || v.as_str() == Some("urn:ngsi-ld:null")) {
            if ["type", "notification"].contains(&k.as_str()) {
                return Err(bad(format!("cannot remove mandatory member {k} (5.8.3)")));
            }
            out.insert(k.clone(), Value::Null);
            continue;
        }
        match k.as_str() {
            "@context" | "createdAt" | "modifiedAt" | "status" => continue,
            "id" => {
                let id = v
                    .as_str()
                    .ok_or_else(|| bad("subscription id must be a string URI".into()))?;
                antares_model::EntityId::new(id)?;
                out.insert("id".into(), v.clone());
            }
            "type" => {
                if v.as_str() != Some("Subscription") {
                    return Err(bad("type must be \"Subscription\" (5.2.12)".into()));
                }
                out.insert("type".into(), v.clone());
            }
            "entities" => {
                let arr = v
                    .as_array()
                    .filter(|a| !a.is_empty())
                    .ok_or_else(|| bad("entities must be a non-empty array".into()))?;
                let mut entities = Vec::new();
                for e in arr {
                    let eo = e
                        .as_object()
                        .ok_or_else(|| bad("entities entries must be objects".into()))?;
                    let mut ne = Map::new();
                    for (ek, ev) in eo {
                        match ek.as_str() {
                            "type" => {
                                let t = ev
                                    .as_str()
                                    .filter(|t| !t.is_empty())
                                    .ok_or_else(|| bad("EntitySelector type is required".into()))?;
                                // 4.17 type-selection expressions stay raw and
                                // are evaluated at match time (046_16)
                                if t.contains(['|', ',', ';', '(']) {
                                    ne.insert("type".into(), ev.clone());
                                } else {
                                    ne.insert("type".into(), Value::String(ctx.expand_key(t)));
                                }
                            }
                            "id" => {
                                // Table 5.2.33-1: id is "String or String[]"
                                // of valid URIs
                                match ev {
                                    Value::String(id) => {
                                        antares_model::EntityId::new(id)?;
                                    }
                                    Value::Array(a) => {
                                        for i in a {
                                            let id = i.as_str().ok_or_else(|| {
                                                bad("EntitySelector id entries must be URIs (5.2.33)"
                                                    .into())
                                            })?;
                                            antares_model::EntityId::new(id)?;
                                        }
                                    }
                                    _ => return Err(bad(
                                        "EntitySelector id must be a URI string or array (5.2.33)"
                                            .into(),
                                    )),
                                }
                                ne.insert("id".into(), ev.clone());
                            }
                            "idPattern" => {
                                let p = ev
                                    .as_str()
                                    .ok_or_else(|| bad("idPattern must be a string".into()))?;
                                crate::regexcache::compile(p)
                                    .map_err(|_| bad(format!("invalid idPattern {p:?}")))?;
                                ne.insert("idPattern".into(), ev.clone());
                            }
                            _ => {
                                ne.insert(ek.clone(), ev.clone());
                            }
                        }
                    }
                    if !ne.contains_key("type") {
                        return Err(bad("EntitySelector requires type (5.2.33)".into()));
                    }
                    entities.push(Value::Object(ne));
                }
                out.insert("entities".into(), Value::Array(entities));
            }
            "watchedAttributes" => {
                let arr = v
                    .as_array()
                    .filter(|a| !a.is_empty())
                    .ok_or_else(|| bad("watchedAttributes must be a non-empty array".into()))?;
                let mut attrs = Vec::new();
                for a in arr {
                    let s = a
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .ok_or_else(|| bad("watchedAttributes entries must be strings".into()))?;
                    attrs.push(Value::String(ctx.expand_key(s)));
                }
                out.insert("watchedAttributes".into(), Value::Array(attrs));
            }
            "q" => {
                let q = v.as_str().ok_or_else(|| bad("q must be a string".into()))?;
                // Validate the string the MATCHER will parse. `conditions_match`
                // percent-decodes first (4.9, 046_05), so validating the raw
                // form would let `%28%28%28…` through create-time checks and
                // only become thousands of real parens at notification time —
                // inside a spawned task, where the parser's own limits are the
                // last line of defence.
                let decoded = crate::negotiate::percent_decode(q.as_bytes());
                antares_ql::parse_q(&decoded)?;
                out.insert("q".into(), v.clone());
            }
            "geoQ" => {
                let g = v
                    .as_object()
                    .ok_or_else(|| bad("geoQ must be an object".into()))?;
                crate::geo::GeoQuery::from_params(&antares_matcher::geo_params(g))?
                    .ok_or_else(|| bad("geoQ requires georel (5.2.13)".into()))?;
                let mut ng = g.clone();
                if let Some(gp) = g.get("geoproperty").and_then(Value::as_str) {
                    ng.insert("geoproperty".into(), Value::String(ctx.expand_key(gp)));
                }
                out.insert("geoQ".into(), Value::Object(ng));
            }
            "notification" => {
                let n = v
                    .as_object()
                    .ok_or_else(|| bad("notification must be an object (5.2.14)".into()))?;
                let mut nn = n.clone();
                // 5.2.14.2: output-only members are read-only — provided
                // ones are ignored, never stored.
                for k in [
                    "timesSent",
                    "timesFailed",
                    "lastNotification",
                    "lastSuccess",
                    "lastFailure",
                ] {
                    nn.remove(k);
                }
                if let Some(f) = n.get("format").and_then(Value::as_str) {
                    if !["normalized", "keyValues", "simplified", "concise"].contains(&f) {
                        return Err(bad(format!("invalid notification format {f:?}")));
                    }
                }
                // Table 5.2.14.1-1 p.120: "showChanges cannot be true in case
                // format is keyValues" — "simplified" is the declared synonym
                if n.get("showChanges").and_then(Value::as_bool) == Some(true)
                    && matches!(
                        n.get("format").and_then(Value::as_str),
                        Some("keyValues") | Some("simplified")
                    )
                {
                    return Err(bad(
                        "showChanges cannot be true when format is keyValues (5.2.14)".into(),
                    ));
                }
                // Table 5.2.14.1-1: join / joinLevel / sysAttrs /
                // showChanges value spaces.
                if let Some(j) = n.get("join") {
                    if !j
                        .as_str()
                        .is_some_and(|j| ["flat", "inline", "@none"].contains(&j))
                    {
                        return Err(bad(format!("invalid notification join {j:?} (5.2.14)")));
                    }
                }
                // Table 5.2.14.1-1: a positive integer. The depth it names is
                // the same Linked Entity traversal (4.5.23) a query drives, so
                // it carries the same ceiling — every notification of this
                // Subscription pays that traversal, and an unbounded level
                // makes one accepted Subscription an amplification lever.
                if let Some(jl) = n.get("joinLevel") {
                    let cap = crate::bounds::MAX_JOIN_LEVEL as u64;
                    let ok = jl.as_u64().is_some_and(|v| (1..=cap).contains(&v));
                    if !ok {
                        return Err(bad(format!(
                            "notification.joinLevel must be an integer in 1..={cap} (5.2.14)"
                        )));
                    }
                }
                for key in ["sysAttrs", "showChanges"] {
                    if n.get(key).is_some_and(|v| !v.is_boolean()) {
                        return Err(bad(format!(
                            "notification.{key} must be a boolean (5.2.14)"
                        )));
                    }
                }
                if let Some(attrs) = n.get("attributes").and_then(Value::as_array) {
                    // Table 5.2.14.1-1 p.119: "Empty array (0 length) is not
                    // allowed" — same restriction on pick and omit below
                    if attrs.is_empty() {
                        return Err(bad(
                            "notification.attributes must not be empty (5.2.14)".into()
                        ));
                    }
                    let mut na = Vec::new();
                    for a in attrs {
                        let s = a
                            .as_str()
                            .ok_or_else(|| bad("notification.attributes must be strings".into()))?;
                        // "A synonym for pick, except that id, type, scope
                        // are not allowed."
                        if ["id", "type", "scope"].contains(&s) {
                            return Err(bad(format!(
                                "notification.attributes may not name {s:?} (5.2.14)"
                            )));
                        }
                        na.push(Value::String(ctx.expand_key(s)));
                    }
                    nn.insert("attributes".into(), Value::Array(na));
                }
                for key in ["pick", "omit"] {
                    if n.get(key)
                        .and_then(Value::as_array)
                        .is_some_and(Vec::is_empty)
                    {
                        return Err(bad(format!(
                            "notification.{key} must not be empty (5.2.14)"
                        )));
                    }
                }
                let ep = n
                    .get("endpoint")
                    .and_then(Value::as_object)
                    .ok_or_else(|| bad("notification.endpoint is required (5.2.14)".into()))?;
                let uri = ep
                    .get("uri")
                    .and_then(Value::as_str)
                    .ok_or_else(|| bad("endpoint.uri is required (5.2.15)".into()))?;
                antares_model::EntityId::new(uri)
                    .map_err(|_| bad(format!("endpoint.uri is not a valid URI: {uri:?}")))?;

                let member_names = |key: &str| -> Vec<String> {
                    n.get(key)
                        .and_then(Value::as_array)
                        .map(|a| {
                            a.iter()
                                .filter_map(Value::as_str)
                                .map(str::to_owned)
                                .collect()
                        })
                        .unwrap_or_default()
                };
                let pick = member_names("pick");
                let omit = member_names("omit");
                if !pick.is_empty() && n.contains_key("attributes") {
                    return Err(bad("notification.pick and attributes are exclusive".into()));
                }
                if !omit.is_empty() && n.contains_key("attributes") {
                    return Err(bad("notification.omit and attributes are exclusive".into()));
                }
                if pick.iter().any(|p| omit.contains(p)) {
                    return Err(bad(
                        "notification.pick and omit name the same entity member".into(),
                    ));
                }
                // Table 5.2.15-1: receiverInfo/notifierInfo are
                // KeyValuePair[] — per Table 5.2.22-1 both key and value
                // are Strings, cardinality 1.
                for key in ["receiverInfo", "notifierInfo"] {
                    if let Some(arr) = ep.get(key) {
                        let ok = arr.as_array().is_some_and(|a| {
                            a.iter().all(|kv| {
                                kv.get("key").is_some_and(Value::is_string)
                                    && kv.get("value").is_some_and(Value::is_string)
                            })
                        });
                        if !ok {
                            return Err(bad(format!(
                                "endpoint.{key} entries must be {{key, value}} pairs (5.2.15/5.2.22)"
                            )));
                        }
                    }
                }
                // 6.3.8 and 6.3.9: each receiverInfo pair becomes one custom
                // header on the notification POST, and "'Key' and 'value'
                // members shall adhere to IETF RFC 7230 ... definitions
                // concerning HTTP headers". A pair that cannot be a header is
                // input the operation cannot meet (5.8.1.4), so it is refused
                // here rather than accepted into a Subscription that can only
                // ever dead-letter. notifierInfo is not headers — its own
                // binding validates it through the sink.
                if let Some(arr) = ep.get("receiverInfo").and_then(Value::as_array) {
                    for kv in arr {
                        let (k, v) = (kv["key"].as_str(), kv["value"].as_str());
                        if !k.is_some_and(is_field_name) || !v.is_some_and(is_field_value) {
                            return Err(bad(format!(
                                "endpoint.receiverInfo entry {kv} is not a valid HTTP header \
                                 (RFC 7230, 6.3.8)"
                            )));
                        }
                    }
                }
                // Table 5.2.15-1: cooldown and timeout are Numbers "Greater
                // than 0"
                for key in ["cooldown", "timeout"] {
                    if let Some(v) = ep.get(key) {
                        v.as_f64().filter(|n| *n > 0.0).ok_or_else(|| {
                            bad(format!(
                                "endpoint.{key} must be a number greater than 0 (5.2.15)"
                            ))
                        })?;
                    }
                }
                if let Some(acc) = ep.get("accept").and_then(Value::as_str) {
                    if ![
                        "application/json",
                        "application/ld+json",
                        "application/geo+json",
                    ]
                    .contains(&acc)
                    {
                        return Err(bad(format!("invalid endpoint accept {acc:?}")));
                    }
                }
                out.insert("notification".into(), Value::Object(nn));
            }
            "expiresAt" => {
                let s = v
                    .as_str()
                    .filter(|s| parse_datetime(s))
                    .ok_or_else(|| bad("expiresAt must be an ISO 8601 DateTime".into()))?;
                // 4.6.3 admits several spellings of one instant, so whether a
                // DateTime has passed cannot be read off the raw strings.
                if crate::temporal::dt_key(s) < crate::temporal::dt_key(&now_iso()) {
                    return Err(bad("expiresAt is in the past (5.8.1)".into()));
                }
                out.insert("expiresAt".into(), v.clone());
            }
            "throttling" => {
                let n = v
                    .as_f64()
                    .filter(|n| *n > 0.0)
                    .ok_or_else(|| bad("throttling must be a positive number".into()))?;
                let _ = n;
                out.insert("throttling".into(), v.clone());
            }
            "timeInterval" => {
                v.as_f64()
                    .filter(|n| *n > 0.0)
                    .ok_or_else(|| bad("timeInterval must be a positive number".into()))?;
                out.insert("timeInterval".into(), v.clone());
            }
            "isActive" => {
                if !v.is_boolean() {
                    return Err(bad("isActive must be a boolean".into()));
                }
                out.insert("isActive".into(), v.clone());
            }
            "temporalQ" => {
                // 5.2.21 TemporalQuery: timerel and timeAt are cardinality 1
                // and every member must sit in its Table 5.2.21-1 value
                // space (used by CSR subscriptions, 5.11.7).
                let tq = v.as_object().ok_or_else(|| {
                    bad("temporalQ must be a TemporalQuery object (5.2.21)".into())
                })?;
                let mut p = std::collections::HashMap::new();
                crate::temporal::temporal_q_params(tq, &mut p)?;
                crate::temporal::TemporalQ::from_params(&p, true)?;
                out.insert(k.clone(), v.clone());
            }
            // Table 5.2.12-1: "Valid notification triggers are entityCreated,
            // entityUpdated, entityDeleted, attributeCreated, attributeUpdated,
            // attributeDeleted." A trigger outside that set is accepted and
            // then matches nothing, leaving a subscription that never fires.
            "notificationTrigger" => {
                const TRIGGERS: [&str; 6] = [
                    "entityCreated",
                    "entityUpdated",
                    "entityDeleted",
                    "attributeCreated",
                    "attributeUpdated",
                    "attributeDeleted",
                ];
                let list = v.as_array().filter(|a| !a.is_empty()).ok_or_else(|| {
                    bad("notificationTrigger must be a non-empty array of strings (5.2.12)".into())
                })?;
                for t in list {
                    let t = t.as_str().ok_or_else(|| {
                        bad("notificationTrigger entries must be strings (5.2.12)".into())
                    })?;
                    if !TRIGGERS.contains(&t) {
                        return Err(bad(format!(
                            "{t} is not a valid notification trigger (5.2.12)"
                        )));
                    }
                }
                out.insert(k.clone(), v.clone());
            }
            // Table 5.2.12-1: csf is "A valid query string as per clause 4.9".
            // Unparsed here it is stored and only fails at Context Source
            // Registration matching (5.11.2.4), where it silently matches
            // nothing instead of telling the subscriber the filter is broken.
            "csf" => {
                let s = v
                    .as_str()
                    .ok_or_else(|| bad("csf must be a query string (5.2.12)".into()))?;
                antares_ql::parse_q(s)?;
                out.insert(k.clone(), v.clone());
            }
            // 5.8.6: the @context governing a subscription's notifications is
            // the @context of the creating request, held in a broker-internal
            // member. This function only ever sees client input — a create
            // body or a patch fragment — so dropping the member here stops a
            // subscriber both from seeding it and from replacing it later.
            // __via is the same class: the 6.3.18 chain comes from the Via
            // HTTP header of the creating request, never from the body.
            "__context" | "__via" => continue,
            "scopeQ" | "lang" | "subscriptionName" | "name" | "description" | "jsonldContext"
            | "ngsildConformance" | "datasetId" => {
                out.insert(k.clone(), v.clone());
            }
            // tolerant reader: keep unknown members
            _ => {
                out.insert(k.clone(), v.clone());
            }
        }
    }
    if !is_patch {
        if !out.contains_key("type") {
            return Err(bad("type must be \"Subscription\" (5.2.12)".into()));
        }
        // 5.2.12: "At least one of (a) entities or (b) watchedAttributes
        // shall be present, unless the member localOnly is set to true"
        // (local scope, 5.5.13).
        let local_only = out.get("localOnly").and_then(Value::as_bool) == Some(true);
        if !local_only && !out.contains_key("entities") && !out.contains_key("watchedAttributes") {
            return Err(bad(
                "one of entities or watchedAttributes is required (5.2.12)".into(),
            ));
        }
        if !out.contains_key("notification") {
            return Err(bad("notification is required (5.2.12)".into()));
        }
    }
    if out.contains_key("timeInterval") && out.contains_key("watchedAttributes") {
        return Err(bad(
            "timeInterval and watchedAttributes are mutually exclusive (5.2.12)".into(),
        ));
    }
    if out.contains_key("timeInterval") && out.contains_key("throttling") {
        return Err(bad(
            "timeInterval and throttling are mutually exclusive (5.2.12)".into(),
        ));
    }
    Ok(out)
}

/// Output shaping: compact IRIs, add status (5.8.3).
pub fn present_subscription(doc: &Value, ctx: &Context, sys_attrs: bool, csource: bool) -> Value {
    let Some(obj) = doc.as_object() else {
        return doc.clone();
    };
    let mut out = Map::new();
    for (k, v) in obj {
        match k.as_str() {
            "__context" | "__via" => continue,
            "createdAt" | "modifiedAt" if !sys_attrs => continue,
            "entities" => {
                let entities: Vec<Value> = v
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .map(|e| {
                        let mut ne = e.as_object().cloned().unwrap_or_default();
                        if let Some(t) = ne.get("type").and_then(Value::as_str) {
                            ne.insert("type".into(), Value::String(ctx.compact_iri(t)));
                        }
                        Value::Object(ne)
                    })
                    .collect();
                out.insert("entities".into(), Value::Array(entities));
            }
            "watchedAttributes" => {
                let attrs: Vec<Value> = v
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .filter_map(Value::as_str)
                    .map(|a| Value::String(ctx.compact_iri(a)))
                    .collect();
                out.insert("watchedAttributes".into(), Value::Array(attrs));
            }
            "notification" => {
                let mut n = v.as_object().cloned().unwrap_or_default();
                if let Some(attrs) = n.get("attributes").and_then(Value::as_array) {
                    let na: Vec<Value> = attrs
                        .iter()
                        .filter_map(Value::as_str)
                        .map(|a| Value::String(ctx.compact_iri(a)))
                        .collect();
                    n.insert("attributes".into(), Value::Array(na));
                }
                out.insert("notification".into(), Value::Object(n));
            }
            "geoQ" => {
                let mut g = v.as_object().cloned().unwrap_or_default();
                if let Some(gp) = g.get("geoproperty").and_then(Value::as_str) {
                    g.insert("geoproperty".into(), Value::String(ctx.compact_iri(gp)));
                }
                out.insert("geoQ".into(), Value::Object(g));
            }
            _ => {
                out.insert(k.clone(), v.clone());
            }
        }
    }
    // default notificationTrigger surfaced on output (5.2.12; 028_06) —
    // entity subscriptions only, csource subs have no such default (5.11)
    if !csource && !out.contains_key("notificationTrigger") && !out.contains_key("timeInterval") {
        out.insert(
            "notificationTrigger".into(),
            serde_json::json!(["attributeCreated", "attributeUpdated"]),
        );
    }
    // status (5.2.12 output): active | paused | expired
    let expired = obj
        .get("expiresAt")
        .and_then(Value::as_str)
        .is_some_and(|e| crate::temporal::dt_key(e) < crate::temporal::dt_key(&now_iso()));
    let paused = obj.get("isActive") == Some(&Value::Bool(false));
    let status = if expired {
        "expired"
    } else if paused {
        "paused"
    } else if obj.get("status").and_then(Value::as_str) == Some("failed") {
        "failed" // 5.8.6 / 5.11.7 delivery-failure status
    } else {
        "active"
    };
    out.insert("status".into(), Value::String(status.into()));
    Value::Object(out)
}

// ---------- handlers (parameterized by Kind) ----------

/// Validate a subscription's jsonldContext member (5.2.12): must be a
/// dereferenceable @context — invalid value ⇒ 400, unresolvable ⇒ 504.
/// 5.8.1.4 and 5.8.2.4: the notification endpoint has to be one this
/// deployment can deliver to. The sink registered for the URI's scheme
/// (6.3.8, and 7.2 for the optional MQTT binding) validates the endpoint's
/// own syntax and its `notifierInfo`; a scheme no sink serves is input data
/// that does not meet the requirements of the operation — BadRequestData,
/// never a fall-through to the HTTP binding. A fragment that carries no
/// endpoint leaves the stored one in place and has nothing to check.
fn check_endpoint(st: &AppState, norm: &Map<String, Value>) -> Result<(), NgsiError> {
    let Some(ep) = norm
        .get("notification")
        .and_then(|n| n.get("endpoint"))
        .and_then(Value::as_object)
    else {
        return Ok(());
    };
    let Some(uri) = ep.get("uri").and_then(Value::as_str) else {
        return Ok(());
    };
    let notifier_info = ep
        .get("notifierInfo")
        .and_then(Value::as_array)
        .map(|ni| {
            ni.iter()
                .filter_map(|kv| Some((kv.get("key")?.as_str()?, kv.get("value")?.as_str()?)))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    st.sinks.require(uri, &notifier_info)
}

/// 5.2.12 `jsonldContext`: the @context a Notification of this Subscription
/// is compacted against, so the member is dereferenced here rather than at
/// first delivery — a shape that is not a URL or an array of URLs is 400,
/// one that does not resolve is 504.
///
/// Resolution is Tenant-scoped (5.5.10): a Hosted @context belongs to the
/// Tenant that stored it (5.13.1), and resolving the URL outside that Tenant
/// would compact every Notification of this Subscription against another
/// Tenant's term mappings. For any other Tenant the URL is as absent as one
/// that never existed.
/// RFC 7230 `field-name`: a `token`, one or more `tchar`.
pub(crate) fn is_field_name(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
}

/// RFC 7230 `field-value`: visible ASCII, space and horizontal tab, with no
/// leading or trailing whitespace. Empty is legal; `obs-text` and the
/// deprecated `obs-fold` are not generated, so a byte outside that set — a
/// bare CR or LF above all — makes the pair unsendable as a header.
pub(crate) fn is_field_value(s: &str) -> bool {
    !s.starts_with([' ', '\t'])
        && !s.ends_with([' ', '\t'])
        && s.bytes().all(|b| b == b'\t' || (0x20..=0x7e).contains(&b))
}

async fn check_jsonld_context(
    st: &AppState,
    tenant: &TenantId,
    norm: &Map<String, Value>,
) -> Result<(), ApiError> {
    let Some(v) = norm.get("jsonldContext") else {
        return Ok(());
    };
    let is_url = |s: &str| s.starts_with("http://") || s.starts_with("https://");
    let ok_shape = match v {
        Value::String(s) => is_url(s),
        Value::Array(a) => a.iter().all(|e| e.as_str().is_some_and(is_url)),
        _ => false,
    };
    if !ok_shape {
        return Err(NgsiError::BadRequestData(format!(
            "jsonldContext is not a valid @context reference: {v}"
        ))
        .into());
    }
    st.loader.resolve_for(tenant, v).await?;
    Ok(())
}

pub async fn create(
    st: &AppState,
    kind: Kind,
    params: &HashMap<String, String>,
    headers: &HeaderMap,
    body: &[u8],
) -> ApiResult<Response> {
    let tenant = tenant_from(headers)?;
    check_params(params, &["local"])?;
    let parsed = parse_body(&st.loader, headers, body, BodyKind::Standard).await?;
    let obj = parsed.object(NgsiError::BadRequestData(
        "subscription must be a JSON object".into(),
    ))?;
    let mut norm = normalize_subscription(obj, &parsed.ctx, false)?;
    check_endpoint(st, &norm)?;
    check_jsonld_context(st, &tenant, &norm).await?;
    let id = match norm.get("id").and_then(Value::as_str) {
        Some(id) => id.to_owned(),
        None => {
            let id = format!("urn:ngsi-ld:Subscription:{}", uuid::Uuid::new_v4());
            norm.insert("id".into(), Value::String(id.clone()));
            id
        }
    };
    let ts = now_iso();
    norm.insert("createdAt".into(), Value::String(ts.clone()));
    norm.insert("modifiedAt".into(), Value::String(ts.clone()));
    // notification @context = the creating request's context (5.8.6),
    // stored as its own column — internal member, stripped on output.
    norm.insert("__context".into(), parsed.ctx.source.clone());
    // 6.3.17/6.3.18: a Subscription arriving as a forwarded copy (5.8.1.4)
    // carries the Via chain of the brokers it has passed through — kept on
    // the stored document so the distributed half can extend the chain
    // outbound and refuse to re-forward a copy that has looped back.
    if let Some(via) = crate::federation::inbound_via(headers) {
        norm.insert("__via".into(), Value::String(via));
    }
    // Array @context (>1 entry): the broker must host it at its own URL as an
    // ImplicitlyCreated @context, surfaced via jsonldContext (5.13.1, 050_03)
    if !norm.contains_key("jsonldContext") {
        if let Value::Array(a) = &parsed.ctx.source {
            if a.len() > 1 {
                let local_id = uuid::Uuid::new_v4().to_string();
                let url = format!("{}/{local_id}", crate::contexts::base_url(headers));
                st.store.context_put(
                    &local_id,
                    serde_json::json!({
                        "url": url,
                        "localId": local_id,
                        "kind": "ImplicitlyCreated",
                        "createdAt": ts,
                        // owned by the tenant whose subscription created it
                        "owner": tenant.as_str(),
                        "body": {"@context": parsed.ctx.source.clone()},
                    }),
                )?;
                st.loader
                    .put_local_for(&tenant, url.clone(), parsed.ctx.source.clone())
                    .await;
                norm.insert("jsonldContext".into(), Value::String(url));
            }
        }
    }
    let doc = Value::Object(norm);
    if !st.store.create(&tenant, kind, &id, doc.clone())? {
        return Err(NgsiError::AlreadyExists(format!("subscription {id} already exists")).into());
    }
    st.sub_changed(&tenant, kind, &id, Some(&doc));
    if kind == Kind::Subscription {
        // 5.8.1.4: a distributed Subscription creates its internal Context
        // Source Registration Subscription (consumer half)
        crate::distsub::on_subscription_created(st, &tenant, &doc);
    }
    if kind == Kind::CSourceSubscription {
        // initial CSourceNotification with all matching registrations (5.11.2.4)
        let (st2, t2, id2) = (st.clone(), tenant.clone(), id.clone());
        crate::spawn(async move {
            crate::notify::csource_initial(&st2, &t2, &id2).await;
        });
    }
    Ok(created(
        format!("/ngsi-ld/v1/{}/{id}", resource_path(kind)),
        &tenant,
    ))
}

/// 5.8.3 Retrieve Subscription: invalid URI 400, unknown id 404, else the
/// 5.2.12 subscription document (status/timesSent are output members).
pub async fn retrieve(
    st: &AppState,
    kind: Kind,
    id: &str,
    params: &HashMap<String, String>,
    headers: &HeaderMap,
) -> ApiResult<Response> {
    let tenant = tenant_from(headers)?;
    antares_model::EntityId::new(id)
        .map_err(|_| NgsiError::BadRequestData(format!("invalid subscription id {id:?}")))?;
    check_params(params, &["options", "format", "sysAttrs", "local"])?;
    let accept = parse_accept(headers)?;
    let ctx = request_context(&st.loader, headers).await?;
    let doc = st
        .store
        .get(&tenant, kind, id)?
        .ok_or_else(|| NgsiError::ResourceNotFound(format!("subscription {id} not found")))?;
    let sys = sys_attrs_asked(params);
    let payload = present_subscription(&doc, &ctx, sys, kind == Kind::CSourceSubscription);
    Ok(respond(StatusCode::OK, payload, &ctx, accept, &tenant))
}

/// 5.8.4 Query Subscriptions: list with 5.5.9 pagination; each element a
/// 5.2.12 subscription document.
pub async fn list(
    st: &AppState,
    kind: Kind,
    params: &HashMap<String, String>,
    headers: &HeaderMap,
) -> ApiResult<Response> {
    let tenant = tenant_from(headers)?;
    check_params(
        params,
        &["limit", "offset", "count", "options", "format", "local"],
    )?;
    let accept = parse_accept(headers)?;
    let ctx = request_context(&st.loader, headers).await?;
    // 5.5.9.1: "only up to a maximum of L NGSI-LD Elements are RETRIEVED
    // and returned". 5.8.4 takes no filter parameters — `check_params`
    // above is the whole list — so the tenant IS the match set and the
    // window is the store's to apply. Reading the tenant and slicing it
    // here made a tenant at the document ceiling unable to list at all,
    // because that read is the one carrying the ceiling for client queries.
    let (offset, limit, _) = crate::entities::page_params(st, params)?;
    let (page_docs, total) = st.store.list_slice(&tenant, kind, offset, limit)?;
    let (page, count_hdr, links) = crate::entities::paginate_pre_accept(
        st,
        params,
        page_docs,
        &format!("/ngsi-ld/v1/{}", resource_path(kind)),
        accept,
        total,
    )?;
    let sys = sys_attrs_asked(params);
    let payload: Vec<Value> = page
        .iter()
        .map(|d| present_subscription(d, &ctx, sys, kind == Kind::CSourceSubscription))
        .collect();
    let mut resp = crate::negotiate::respond_list(StatusCode::OK, payload, &ctx, accept, &tenant);
    attach_paging(&mut resp, count_hdr, &links);
    Ok(resp)
}

/// 5.8.2 Update Subscription: invalid URI 400, unknown id 404, fragment
/// validated per 5.5.4 + 5.2.12 (past expiresAt 400), jsonldContext
/// unavailable -> LdContextNotAvailable / invalid -> 400, modify per 5.5.8;
/// the 5.8.2.4 status table falls out of the computed status
/// (isActive/expiresAt) on read.
pub async fn update(
    st: &AppState,
    kind: Kind,
    id: &str,
    params: &HashMap<String, String>,
    headers: &HeaderMap,
    body: &[u8],
) -> ApiResult<Response> {
    let tenant = tenant_from(headers)?;
    antares_model::EntityId::new(id)
        .map_err(|_| NgsiError::BadRequestData(format!("invalid subscription id {id:?}")))?;
    check_params(params, &["local"])?;
    let parsed = parse_body(&st.loader, headers, body, BodyKind::MergePatch).await?;
    let obj = parsed.object(NgsiError::BadRequestData(
        "fragment must be a JSON object".into(),
    ))?;
    if let Some(bid) = obj.get("id").and_then(Value::as_str) {
        if bid != id {
            return Err(NgsiError::BadRequestData("fragment id mismatch".into()).into());
        }
    }
    let norm = normalize_subscription(obj, &parsed.ctx, true)?;
    check_endpoint(st, &norm)?;
    check_jsonld_context(st, &tenant, &norm).await?;
    let ts = now_iso();
    let res = st.store.mutate(&tenant, kind, id, |doc| {
        let target = antares_store::stored_object(doc)?;
        crate::apply_doc_fragment(target, &norm, &ts);
        Ok::<(), NgsiError>(())
    })?;
    match res {
        None => Err(NgsiError::ResourceNotFound(format!("subscription {id} not found")).into()),
        Some(Err(e)) => Err(e.into()),
        Some(Ok(())) => {
            if kind == Kind::CSourceSubscription {
                // 5.11.3.4: after update, notify with all currently matching
                let (st2, t2, id2) = (st.clone(), tenant.clone(), id.to_owned());
                crate::spawn(async move {
                    crate::notify::csource_initial(&st2, &t2, &id2).await;
                });
            }
            if st.sub_sync.is_some() {
                let doc = st.store.get(&tenant, kind, id)?;
                st.sub_changed(&tenant, kind, id, doc.as_ref());
            }
            if kind == Kind::Subscription {
                // 5.8.2.4: the CSR subscription and the mapped remote
                // subscriptions follow the update (5.11.3)
                crate::distsub::on_subscription_updated(st, &tenant, id);
            }
            Ok(no_content(&tenant))
        }
    }
}

/// 5.8.5 Delete Subscription: invalid URI 400, unknown id 404, 204 on
/// success and no further notifications (sub_changed drops the mirror).
pub async fn delete(
    st: &AppState,
    kind: Kind,
    id: &str,
    params: &HashMap<String, String>,
    headers: &HeaderMap,
) -> ApiResult<Response> {
    let tenant = tenant_from(headers)?;
    antares_model::EntityId::new(id)
        .map_err(|_| NgsiError::BadRequestData(format!("invalid subscription id {id:?}")))?;
    check_params(params, &["local"])?;
    if st.store.delete(&tenant, kind, id)? {
        st.sub_changed(&tenant, kind, id, None);
        if kind == Kind::Subscription {
            // 5.8.5.4: forward the delete to every mapped Context Source
            // and drop the internal CSR subscription (5.11.6)
            crate::distsub::on_subscription_deleted(st, &tenant, id);
        }
        Ok(no_content(&tenant))
    } else {
        Err(NgsiError::ResourceNotFound(format!("subscription {id} not found")).into())
    }
}

// axum route fns

macro_rules! route4 {
    ($create:ident, $retrieve:ident, $list:ident, $update:ident, $delete:ident, $kind:expr, $c:literal) => {
        #[doc = concat!("HTTP handler for ", $c, " Create: the axum seam over [`create`].")]
        pub async fn $create(
            State(st): State<AppState>,
            CleanParams(params): CleanParams,
            headers: HeaderMap,
            body: Bytes,
        ) -> Response {
            create(&st, $kind, &params, &headers, &body)
                .await
                .unwrap_or_else(|e| e.into_response())
        }
        #[doc = concat!("HTTP handler for ", $c, " Retrieve: the axum seam over [`retrieve`].")]
        pub async fn $retrieve(
            State(st): State<AppState>,
            Path(id): Path<String>,
            CleanParams(params): CleanParams,
            headers: HeaderMap,
        ) -> Response {
            retrieve(&st, $kind, &id, &params, &headers)
                .await
                .unwrap_or_else(|e| e.into_response())
        }
        #[doc = concat!("HTTP handler for ", $c, " Query: the axum seam over [`list`].")]
        pub async fn $list(
            State(st): State<AppState>,
            CleanParams(params): CleanParams,
            headers: HeaderMap,
        ) -> Response {
            list(&st, $kind, &params, &headers)
                .await
                .unwrap_or_else(|e| e.into_response())
        }
        #[doc = concat!("HTTP handler for ", $c, " Update: the axum seam over [`update`].")]
        pub async fn $update(
            State(st): State<AppState>,
            Path(id): Path<String>,
            CleanParams(params): CleanParams,
            headers: HeaderMap,
            body: Bytes,
        ) -> Response {
            update(&st, $kind, &id, &params, &headers, &body)
                .await
                .unwrap_or_else(|e| e.into_response())
        }
        #[doc = concat!("HTTP handler for ", $c, " Delete: the axum seam over [`delete`].")]
        pub async fn $delete(
            State(st): State<AppState>,
            Path(id): Path<String>,
            CleanParams(params): CleanParams,
            headers: HeaderMap,
        ) -> Response {
            delete(&st, $kind, &id, &params, &headers)
                .await
                .unwrap_or_else(|e| e.into_response())
        }
    };
}

route4!(
    create_subscription,
    retrieve_subscription,
    query_subscriptions,
    update_subscription,
    delete_subscription,
    Kind::Subscription,
    "5.8 Subscription"
);
route4!(
    create_csource_subscription,
    retrieve_csource_subscription,
    query_csource_subscriptions,
    update_csource_subscription,
    delete_csource_subscription,
    Kind::CSourceSubscription,
    "5.11 Context Source Registration Subscription"
);

#[cfg(test)]
mod tests {
    use super::*;
    use antares_jsonld::Loader;
    use serde_json::json;

    /// 5.5.4: "urn:ngsi-ld:null" as a first-level member value is
    /// BadRequestData on create; in a patch fragment it is the removal form.
    #[test]
    fn clause_5_5_4_first_level_null_in_subscription() {
        let ctx = Loader::new().core();
        let doc = json!({
            "type": "Subscription",
            "entities": [{"type": "Building"}],
            "description": "urn:ngsi-ld:null",
            "notification": {"endpoint": {"uri": "http://localhost:1111/notify"}}
        });
        assert!(
            normalize_subscription(doc.as_object().unwrap(), &ctx, false).is_err(),
            "create with a first-level null URN must be rejected"
        );
        // patch fragment: 5.4 removal semantics — the member is marked for
        // deletion, never stored carrying the string it is spelled with
        let patch = json!({"description": "urn:ngsi-ld:null"});
        let n = normalize_subscription(patch.as_object().unwrap(), &ctx, true).expect("fragment");
        assert_eq!(
            n["description"],
            Value::Null,
            "the NGSI-LD Null removes the member, like a JSON null"
        );
        // and a mandatory member cannot be removed at all (5.8.3)
        for k in ["type", "notification"] {
            let patch = json!({ k: "urn:ngsi-ld:null" });
            assert!(
                normalize_subscription(patch.as_object().unwrap(), &ctx, true).is_err(),
                "removing the mandatory member {k} must be refused"
            );
        }
    }

    #[test]
    fn validates_subscription() {
        let ctx = Loader::new().core();
        let doc = json!({
            "id": "urn:ngsi-ld:Subscription:1",
            "type": "Subscription",
            "entities": [{"type": "Building"}],
            "notification": {"endpoint": {"uri": "http://localhost:1111/notify"}}
        });
        let n = normalize_subscription(doc.as_object().unwrap(), &ctx, false).expect("valid");
        assert_eq!(
            n["entities"][0]["type"],
            "https://uri.etsi.org/ngsi-ld/default-context/Building"
        );

        let missing_notification = json!({
            "type": "Subscription",
            "entities": [{"type": "Building"}]
        });
        assert!(
            normalize_subscription(missing_notification.as_object().unwrap(), &ctx, false).is_err()
        );

        let past_expiry = json!({
            "type": "Subscription",
            "entities": [{"type": "Building"}],
            "expiresAt": "2020-01-01T00:00:00Z",
            "notification": {"endpoint": {"uri": "http://localhost:1111/notify"}}
        });
        assert!(normalize_subscription(past_expiry.as_object().unwrap(), &ctx, false).is_err());
    }

    /// Table 5.2.14.1-1 p.120: "showChanges cannot be true in case format is
    /// keyValues". "simplified" is the table's declared synonym.
    #[test]
    fn show_changes_with_key_values_is_rejected() {
        let ctx = Loader::new().core();
        let mk = |format: &str, show: bool| {
            json!({
                "type": "Subscription",
                "entities": [{"type": "Building"}],
                "notification": {
                    "format": format,
                    "showChanges": show,
                    "endpoint": {"uri": "http://localhost:1111/notify"}
                }
            })
        };
        for f in ["keyValues", "simplified"] {
            assert!(
                normalize_subscription(mk(f, true).as_object().unwrap(), &ctx, false).is_err(),
                "showChanges+{f} must be rejected"
            );
            assert!(
                normalize_subscription(mk(f, false).as_object().unwrap(), &ctx, false).is_ok(),
                "{f} without showChanges is fine"
            );
        }
        assert!(
            normalize_subscription(mk("normalized", true).as_object().unwrap(), &ctx, false)
                .is_ok(),
            "showChanges+normalized is fine"
        );
    }

    /// Table 5.2.14.1-1 p.119: "Empty array (0 length) is not allowed" on
    /// notification.attributes / pick / omit.
    #[test]
    fn empty_projection_arrays_are_rejected() {
        let ctx = Loader::new().core();
        for key in ["attributes", "pick", "omit"] {
            let doc = json!({
                "type": "Subscription",
                "entities": [{"type": "Building"}],
                "notification": {
                    key: [],
                    "endpoint": {"uri": "http://localhost:1111/notify"}
                }
            });
            assert!(
                normalize_subscription(doc.as_object().unwrap(), &ctx, false).is_err(),
                "empty notification.{key} must be rejected"
            );
        }
    }

    // ---------- shared fixtures ----------

    /// The minimal valid 5.2.12 Subscription, with `extra` merged over it.
    fn sub(extra: Value) -> Value {
        let mut base = json!({
            "type": "Subscription",
            "entities": [{"type": "Building"}],
            "notification": {"endpoint": {"uri": "http://localhost:1111/notify"}}
        });
        let obj = base.as_object_mut().expect("object");
        for (k, v) in extra.as_object().expect("object") {
            obj.insert(k.clone(), v.clone());
        }
        base
    }

    fn norm(doc: &Value) -> Result<Map<String, Value>, NgsiError> {
        normalize_subscription(
            doc.as_object().expect("object"),
            &Loader::new().core(),
            false,
        )
    }

    fn frag(doc: &Value) -> Result<Map<String, Value>, NgsiError> {
        normalize_subscription(
            doc.as_object().expect("object"),
            &Loader::new().core(),
            true,
        )
    }

    // ---------- 5.2.12 read-only and internal members ----------

    /// 5.8.6: the @context governing a Subscription's notifications is the
    /// @context of the creating request, kept in a broker-internal member.
    /// A Context Subscriber can neither set it (create) nor replace it
    /// (update), and it appears in no served representation — 5.8.3 and
    /// 5.8.4 serve the 5.2.12 data type, which has no such member.
    #[test]
    fn clause_5_8_6_internal_context_member_is_client_proof() {
        let hostile = json!({"__context": "http://attacker.invalid/ctx.jsonld",
                             "__via": "1.1 forged-alias"});
        let created = norm(&sub(hostile.clone())).expect("valid subscription");
        assert!(
            !created.contains_key("__context"),
            "a client-supplied __context must not reach storage: {created:?}"
        );
        assert!(
            !created.contains_key("__via"),
            "a body member must not forge the Via chain — the chain comes \
             from the HTTP header only: {created:?}"
        );
        let patched = frag(&hostile).expect("valid fragment");
        assert!(
            !patched.contains_key("__context"),
            "a patch fragment must not replace the notification @context: {patched:?}"
        );
        assert!(
            !patched.contains_key("__via"),
            "a patch fragment must not rewrite the stored Via chain: {patched:?}"
        );
        let ctx = Loader::new().core();
        let stored = json!({
            "id": "urn:ngsi-ld:Subscription:ctx",
            "type": "Subscription",
            "entities": [{"type": "Building"}],
            "notification": {"endpoint": {"uri": "http://localhost:1111/notify"}},
            "__context": "https://example.org/private-ctx.jsonld",
            "__via": "1.1 upstream-broker",
        });
        for csource in [false, true] {
            for sys in [false, true] {
                let out = present_subscription(&stored, &ctx, sys, csource);
                assert!(
                    !out.to_string().contains("__context"),
                    "served representation leaked the internal @context member: {out}"
                );
                assert!(
                    !out.to_string().contains("private-ctx"),
                    "served representation leaked the internal @context value: {out}"
                );
                assert!(
                    !out.to_string().contains("__via")
                        && !out.to_string().contains("upstream-broker"),
                    "5.8.3/5.8.4 serve the 5.2.12 data type, which has no Via member: {out}"
                );
            }
        }
    }

    /// Table 5.2.12-2 and 5.2.14.2: read-only members "shall not be provided
    /// by Context Subscribers. In the event that they are provided (in
    /// update or create operations) NGSI-LD implementations shall ignore
    /// them."
    #[test]
    fn clause_5_2_12_read_only_members_are_ignored() {
        let doc = sub(json!({
            "status": "active",
            "createdAt": "2020-01-01T00:00:00Z",
            "modifiedAt": "2020-01-01T00:00:00Z",
            "notification": {
                "endpoint": {"uri": "http://localhost:1111/notify"},
                "timesSent": 42,
                "lastNotification": "2020-01-01T00:00:00Z",
                "lastSuccess": "2020-01-01T00:00:00Z",
                "lastFailure": "2020-01-01T00:00:00Z",
            }
        }));
        for is_patch in [false, true] {
            let n = normalize_subscription(
                doc.as_object().expect("object"),
                &Loader::new().core(),
                is_patch,
            )
            .expect("valid");
            for k in ["status", "createdAt", "modifiedAt"] {
                assert!(
                    !n.contains_key(k),
                    "{k} must not persist (patch={is_patch})"
                );
            }
            let notif = n["notification"].as_object().expect("notification");
            for k in [
                "timesSent",
                "lastNotification",
                "lastSuccess",
                "lastFailure",
            ] {
                assert!(
                    !notif.contains_key(k),
                    "notification.{k} must not persist (patch={is_patch})"
                );
            }
        }
    }

    // ---------- 5.2.12 value spaces ----------

    /// Table 5.2.12-1: "Valid notification triggers are entityCreated,
    /// entityUpdated, entityDeleted, attributeCreated, attributeUpdated,
    /// attributeDeleted". A trigger outside that set would leave a
    /// subscription that silently never fires.
    #[test]
    fn clause_5_2_12_notification_trigger_value_space() {
        for good in [
            json!(["entityCreated"]),
            json!(["entityUpdated", "entityDeleted"]),
            json!(["attributeCreated", "attributeUpdated", "attributeDeleted"]),
        ] {
            assert!(
                norm(&sub(json!({ "notificationTrigger": good }))).is_ok(),
                "{good} must be accepted"
            );
        }
        for bad in [
            json!(["entityChanged"]),
            json!(["attributeCreated", "nope"]),
            json!("entityCreated"),
            json!([1]),
            json!({}),
        ] {
            assert!(
                norm(&sub(json!({ "notificationTrigger": bad }))).is_err(),
                "{bad} must be rejected"
            );
        }
    }

    /// Table 5.2.12-1: csf is "A valid query string as per clause 4.9". An
    /// unparseable filter is otherwise stored and only fails at Context
    /// Source Registration matching time (5.11.2.4), where it silently
    /// matches nothing.
    #[test]
    fn clause_5_2_12_csf_must_be_a_valid_query() {
        assert!(norm(&sub(json!({"csf": "endpoint==\"http://a/x\""}))).is_ok());
        for bad in [json!("(("), json!("a==(("), json!(5), json!(["a==1"])] {
            assert!(
                norm(&sub(json!({ "csf": bad }))).is_err(),
                "{bad} must be rejected"
            );
        }
    }

    /// Table 5.2.12-1: throttling and timeInterval are Numbers "Greater
    /// than 0"; throttling allows fractional values, and neither accepts a
    /// stringified number. The table sets no lower bound on `timeInterval`
    /// beyond that, so a sub-second one is a legal Subscription and is
    /// rejected by nothing.
    #[test]
    fn clause_5_2_12_throttling_and_time_interval_value_space() {
        assert!(norm(&sub(json!({"throttling": 0.5}))).is_ok());
        assert!(norm(&sub(json!({"timeInterval": 5}))).is_ok());
        assert!(norm(&sub(json!({"timeInterval": 0.5}))).is_ok());
        for bad in [json!(0), json!(-1), json!("5"), json!(true), json!(null)] {
            assert!(
                norm(&sub(json!({ "throttling": bad }))).is_err(),
                "throttling {bad} must be rejected"
            );
            assert!(
                norm(&sub(json!({ "timeInterval": bad }))).is_err(),
                "timeInterval {bad} must be rejected"
            );
        }
    }

    /// Table 5.2.12-1: expiresAt is a 4.6.3 DateTime, and 5.8.1.4/5.8.2.4
    /// reject one "referring to a DateTime in the past". 4.6.3 admits both
    /// fraction separators, so the past/future decision must be taken on the
    /// instant, not on the raw string.
    #[test]
    fn clause_5_2_12_expires_at_boundary() {
        assert!(norm(&sub(json!({"expiresAt": "2099-01-01T00:00:00Z"}))).is_ok());
        // comma is the other legal fraction separator (4.6.3); comparing the
        // raw strings would place ',' before now_iso()'s '.' and reject it
        assert!(
            norm(&sub(json!({"expiresAt": "2099-01-01T00:00:00,500Z"}))).is_ok(),
            "a comma fraction separator is legal (4.6.3)"
        );
        let soon = (chrono::Utc::now() + chrono::Duration::seconds(30))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        assert!(
            norm(&sub(json!({ "expiresAt": soon }))).is_ok(),
            "a whole-second DateTime 30s ahead is in the future: {soon}"
        );
        for bad in [
            json!("2020-01-01T00:00:00Z"),
            json!("tomorrow"),
            json!("2099-01-01"),
            json!("2099-01-01T00:00:00+05:00"),
            json!(1234),
        ] {
            assert!(
                norm(&sub(json!({ "expiresAt": bad }))).is_err(),
                "expiresAt {bad} must be rejected"
            );
        }
    }

    /// Table 5.2.12-1: watchedAttributes is a String[] of Attribute names,
    /// "Empty array (0 length) is not allowed"; names expand per 5.5.7.
    #[test]
    fn clause_5_2_12_watched_attributes_contract() {
        let n = norm(&sub(json!({"watchedAttributes": ["temperature"]}))).expect("valid");
        assert_eq!(
            n["watchedAttributes"][0],
            "https://uri.etsi.org/ngsi-ld/default-context/temperature"
        );
        for bad in [json!([]), json!("temperature"), json!([1]), json!([""])] {
            assert!(
                norm(&sub(json!({ "watchedAttributes": bad }))).is_err(),
                "watchedAttributes {bad} must be rejected"
            );
        }
    }

    /// Table 5.2.12-1: q is "A valid query string as per clause 4.9". The
    /// matcher percent-decodes before parsing (4.9), so the DECODED form is
    /// what must be validated here — otherwise an encoded paren bomb passes
    /// creation and only unfolds inside the notification task.
    #[test]
    fn clause_5_2_12_q_is_validated_in_its_decoded_form() {
        assert!(norm(&sub(json!({"q": "temperature>20"}))).is_ok());
        assert!(norm(&sub(json!({"q": "(("}))).is_err());
        assert!(
            norm(&sub(json!({"q": "%28%28%28"}))).is_err(),
            "a percent-encoded unbalanced expression must be rejected at creation"
        );
        assert!(norm(&sub(json!({"q": 5}))).is_err());
    }

    /// Table 5.2.12-1: "At least one of (a) entities or (b)
    /// watchedAttributes shall be present, unless the member localOnly is
    /// set to true"; timeInterval excludes watchedAttributes and throttling.
    #[test]
    fn clause_5_2_12_mutual_exclusions_and_local_only() {
        let bare = json!({
            "type": "Subscription",
            "notification": {"endpoint": {"uri": "http://localhost:1111/notify"}}
        });
        assert!(
            norm(&bare).is_err(),
            "neither entities nor watchedAttributes"
        );
        let mut local = bare.clone();
        local["localOnly"] = json!(true);
        assert!(
            norm(&local).is_ok(),
            "localOnly=true waives the rule (5.5.13)"
        );
        assert!(norm(&sub(json!({
            "timeInterval": 5,
            "watchedAttributes": ["temperature"]
        })))
        .is_err());
        assert!(norm(&sub(json!({"timeInterval": 5, "throttling": 5}))).is_err());
        // a fragment carries no mandatory members of its own (5.8.2.4)
        assert!(frag(&json!({"isActive": false})).is_ok());
    }

    /// Table 5.2.33-1 EntitySelector: type is required, id is "String or
    /// String[]" of valid URIs, idPattern is a regular expression; 4.17
    /// type-selection expressions stay unexpanded.
    #[test]
    fn clause_5_2_33_entity_selector_contract() {
        let n = norm(&sub(json!({"entities": [{"type": "Building|Room"}]}))).expect("valid");
        assert_eq!(
            n["entities"][0]["type"], "Building|Room",
            "a 4.17 type-selection expression is evaluated at match time"
        );
        let n = norm(&sub(json!({
            "entities": [{"type": "Building", "id": ["urn:ngsi-ld:B:1", "urn:ngsi-ld:B:2"]}]
        })))
        .expect("valid");
        assert_eq!(n["entities"][0]["id"][1], "urn:ngsi-ld:B:2");
        for bad in [
            json!([]),
            json!("Building"),
            json!([{"id": "urn:ngsi-ld:B:1"}]),
            json!([{"type": ""}]),
            json!([{"type": "Building", "id": "not a uri"}]),
            json!([{"type": "Building", "id": ["urn:ngsi-ld:B:1", 7]}]),
            json!([{"type": "Building", "idPattern": "["}]),
            // 21 bytes, a 16 MiB automaton: above the compile ceiling the
            // pattern is refused here rather than compiled per event
            json!([{"type": "Building", "idPattern": r"(?:\p{Any}{100}){100}"}]),
            json!([{"type": "Building", "idPattern": 7}]),
            json!(["Building"]),
        ] {
            assert!(
                norm(&sub(json!({ "entities": bad }))).is_err(),
                "entities {bad} must be rejected"
            );
        }
    }

    // ---------- 5.2.14 / 5.2.15 notification parameters ----------

    /// Table 5.2.15-1 Endpoint: uri is mandatory and a valid URI; cooldown
    /// and timeout are Numbers "Greater than 0"; accept is one of the three
    /// media types. Which schemes are deliverable is the sink registry's
    /// question, answered by `check_endpoint` where the state is in hand.
    #[test]
    fn clause_5_2_15_endpoint_contract() {
        let mk = |ep: Value| sub(json!({"notification": {"endpoint": ep}}));
        assert!(
            norm(&sub(json!({"notification": {}}))).is_err(),
            "endpoint required"
        );
        assert!(norm(&mk(json!({}))).is_err(), "endpoint.uri required");
        assert!(norm(&mk(json!({"uri": "no-scheme"}))).is_err());
        assert!(norm(&mk(json!({"uri": "http://a/x\r\nX: y"}))).is_err());
        for key in ["cooldown", "timeout"] {
            for bad in [json!(0), json!(-1), json!("5")] {
                assert!(
                    norm(&mk(json!({"uri": "http://a/x", key: bad}))).is_err(),
                    "endpoint.{key} {bad} must be rejected"
                );
            }
            assert!(norm(&mk(json!({"uri": "http://a/x", key: 1.5}))).is_ok());
        }
        for key in ["receiverInfo", "notifierInfo"] {
            for bad in [
                json!({}),
                json!([{"key": "k"}]),
                json!([{"key": 1, "value": "v"}]),
            ] {
                assert!(
                    norm(&mk(json!({"uri": "http://a/x", key: bad}))).is_err(),
                    "endpoint.{key} {bad} must be rejected"
                );
            }
            assert!(norm(&mk(
                json!({"uri": "http://a/x", key: [{"key": "k", "value": "v"}]})
            ))
            .is_ok());
        }
        // 6.3.8/6.3.9: each receiverInfo pair becomes one custom HTTP header,
        // and "'Key' and 'value' members shall adhere to IETF RFC 7230 ...
        // definitions concerning HTTP headers" — so a pair that cannot be a
        // header is input the operation cannot meet, not a delivery that fails
        // later.
        for bad in [
            json!([{"key": "", "value": "v"}]),
            json!([{"key": "Bad Key", "value": "v"}]),
            json!([{"key": "X:Y", "value": "v"}]),
            json!([{"key": "X\r\nInjected", "value": "v"}]),
            json!([{"key": "X", "value": "a\r\nInjected: 1"}]),
            json!([{"key": "X", "value": "tab\u{7f}del"}]),
            json!([{"key": "X", "value": " leading"}]),
            json!([{"key": "X", "value": "trailing "}]),
        ] {
            assert!(
                norm(&mk(json!({"uri": "http://a/x", "receiverInfo": bad}))).is_err(),
                "receiverInfo {bad} must be rejected"
            );
        }
        for ok in [
            json!([{"key": "Authorization", "value": "Bearer t"}]),
            json!([{"key": "X-Custom_1!", "value": ""}]),
            json!([{"key": "Prefer", "value": "body=json"}]),
            json!([{"key": "X", "value": "a\tb"}]),
        ] {
            assert!(
                norm(&mk(json!({"uri": "http://a/x", "receiverInfo": ok}))).is_ok(),
                "receiverInfo {ok} must be accepted"
            );
        }
        assert!(norm(&mk(json!({"uri": "http://a/x", "accept": "text/html"}))).is_err());
        assert!(norm(&mk(
            json!({"uri": "http://a/x", "accept": "application/geo+json"})
        ))
        .is_ok());
    }

    /// Table 5.2.14.1-1 NotificationParams: format value space, join /
    /// joinLevel, boolean sysAttrs/showChanges, and the pick/omit/attributes
    /// exclusivity — "A synonym for pick, except that id, type, scope are
    /// not allowed."
    #[test]
    fn clause_5_2_14_notification_params_contract() {
        let mk = |extra: Value| {
            let mut n = json!({"endpoint": {"uri": "http://localhost:1111/notify"}});
            let o = n.as_object_mut().expect("object");
            for (k, v) in extra.as_object().expect("object") {
                o.insert(k.clone(), v.clone());
            }
            sub(json!({ "notification": n }))
        };
        assert!(
            norm(&sub(json!({"notification": []}))).is_err(),
            "not an object"
        );
        assert!(norm(&mk(json!({"format": "verbose"}))).is_err());
        for f in ["normalized", "keyValues", "simplified", "concise"] {
            assert!(norm(&mk(json!({ "format": f }))).is_ok(), "{f}");
        }
        for bad in [json!("nested"), json!(1), json!("")] {
            assert!(norm(&mk(json!({ "join": bad }))).is_err(), "join {bad}");
        }
        for good in ["flat", "inline", "@none"] {
            assert!(norm(&mk(json!({ "join": good }))).is_ok(), "{good}");
        }
        // Table 5.2.14.1-1 restricts joinLevel to a positive integer, and the
        // depth it names is the same Linked Entity traversal the query
        // parameter drives — so the ceiling this deployment publishes as
        // maxJoinLevel bounds it on both surfaces, not on the query alone.
        let cap = crate::bounds::MAX_JOIN_LEVEL;
        for bad in [
            json!(0),
            json!(-1),
            json!("2"),
            json!(1.5),
            json!(cap + 1),
            json!(u64::MAX),
        ] {
            assert!(
                norm(&mk(json!({ "joinLevel": bad }))).is_err(),
                "joinLevel {bad}"
            );
        }
        assert!(norm(&mk(json!({"joinLevel": 1}))).is_ok());
        assert!(norm(&mk(json!({ "joinLevel": cap }))).is_ok(), "at the cap");
        for key in ["sysAttrs", "showChanges"] {
            assert!(
                norm(&mk(json!({ key: "true" }))).is_err(),
                "{key} must be a boolean"
            );
            assert!(norm(&mk(json!({ key: true }))).is_ok());
        }
        for name in ["id", "type", "scope"] {
            assert!(
                norm(&mk(json!({"attributes": [name]}))).is_err(),
                "notification.attributes may not name {name}"
            );
        }
        assert!(norm(&mk(json!({"attributes": [1]}))).is_err());
        assert!(norm(&mk(json!({"attributes": ["a"], "pick": ["b"]}))).is_err());
        assert!(norm(&mk(json!({"attributes": ["a"], "omit": ["b"]}))).is_err());
        assert!(norm(&mk(json!({"pick": ["a"], "omit": ["a"]}))).is_err());
        assert!(norm(&mk(json!({"pick": ["a"], "omit": ["b"]}))).is_ok());
        // attribute names expand per 5.5.7
        let n = norm(&mk(json!({"attributes": ["temperature"]}))).expect("valid");
        assert_eq!(
            n["notification"]["attributes"][0],
            "https://uri.etsi.org/ngsi-ld/default-context/temperature"
        );
    }

    /// Table 5.2.13-1 GeoQuery: georel is mandatory and the geoproperty
    /// name expands per 5.5.7.
    #[test]
    fn clause_5_2_13_geo_q_contract() {
        let ok = json!({
            "georel": "near;maxDistance==2000",
            "geometry": "Point",
            "coordinates": [-8.5, 41.2],
            "geoproperty": "location"
        });
        let n = norm(&sub(json!({ "geoQ": ok }))).expect("valid");
        assert_eq!(
            n["geoQ"]["geoproperty"],
            "https://uri.etsi.org/ngsi-ld/location"
        );
        for bad in [
            json!("near"),
            json!({"geometry": "Point", "coordinates": [1, 2]}),
            json!({"georel": "sideways", "geometry": "Point", "coordinates": [1, 2]}),
        ] {
            assert!(
                norm(&sub(json!({ "geoQ": bad }))).is_err(),
                "geoQ {bad} must be rejected"
            );
        }
    }

    /// 5.2.21 TemporalQuery, used by Context Source Registration
    /// Subscriptions (5.11): timerel and timeAt are cardinality 1.
    #[test]
    fn clause_5_2_21_temporal_q_contract() {
        assert!(norm(&sub(json!({
            "temporalQ": {"timerel": "after", "timeAt": "2020-01-01T00:00:00Z"}
        })))
        .is_ok());
        for bad in [
            json!("after"),
            json!({"timerel": "after"}),
            json!({"timerel": "sideways", "timeAt": "2020-01-01T00:00:00Z"}),
            json!({"timerel": "after", "timeAt": "yesterday"}),
        ] {
            assert!(
                norm(&sub(json!({ "temporalQ": bad }))).is_err(),
                "temporalQ {bad} must be rejected"
            );
        }
    }

    // ---------- 5.8.3 / 5.8.4 presentation ----------

    /// 5.2.12 Table 5.2.12-2: status is "Provided by the system"; 5.8.2.4
    /// fixes its value space to active | paused | expired. The default
    /// notificationTrigger is surfaced for entity Subscriptions only, and
    /// createdAt/modifiedAt stay behind the sysAttrs gate.
    #[test]
    fn clause_5_8_3_presented_subscription_shape() {
        let ctx = Loader::new().core();
        let base = json!({
            "id": "urn:ngsi-ld:Subscription:p1",
            "type": "Subscription",
            "entities": [{"type": "https://uri.etsi.org/ngsi-ld/default-context/Building"}],
            "watchedAttributes": ["https://uri.etsi.org/ngsi-ld/default-context/temperature"],
            "notification": {
                "endpoint": {"uri": "http://localhost:1111/notify"},
                "attributes": ["https://uri.etsi.org/ngsi-ld/default-context/temperature"]
            },
            "geoQ": {"georel": "near;maxDistance==1", "geoproperty": "https://uri.etsi.org/ngsi-ld/location"},
            "createdAt": "2020-01-01T00:00:00Z",
            "modifiedAt": "2020-01-01T00:00:00Z",
        });
        let out = present_subscription(&base, &ctx, false, false);
        assert_eq!(out["status"], "active");
        assert_eq!(out["entities"][0]["type"], "Building");
        assert_eq!(out["watchedAttributes"][0], "temperature");
        assert_eq!(out["notification"]["attributes"][0], "temperature");
        assert_eq!(out["geoQ"]["geoproperty"], "location");
        assert!(
            out.get("createdAt").is_none() && out.get("modifiedAt").is_none(),
            "sysAttrs are gated (6.3.11): {out}"
        );
        assert_eq!(
            out["notificationTrigger"],
            json!(["attributeCreated", "attributeUpdated"])
        );
        let sys = present_subscription(&base, &ctx, true, false);
        assert_eq!(sys["createdAt"], "2020-01-01T00:00:00Z");
        // a Context Source Registration Subscription has no such default
        let cs = present_subscription(&base, &ctx, false, true);
        assert!(cs.get("notificationTrigger").is_none(), "{cs}");

        let mut paused = base.clone();
        paused["isActive"] = json!(false);
        assert_eq!(
            present_subscription(&paused, &ctx, false, false)["status"],
            "paused"
        );
        let mut expired = base.clone();
        expired["expiresAt"] = json!("2020-01-01T00:00:00Z");
        expired["isActive"] = json!(false);
        assert_eq!(
            present_subscription(&expired, &ctx, false, false)["status"],
            "expired",
            "expiry wins over paused (5.8.2.4)"
        );
        let mut periodic = base.clone();
        periodic["timeInterval"] = json!(30);
        assert!(
            present_subscription(&periodic, &ctx, false, false)
                .get("notificationTrigger")
                .is_none(),
            "a periodic subscription has no attribute triggers"
        );
    }
}
