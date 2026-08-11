//! /subscriptions and /csourceSubscriptions (5.8, 5.11; resources 6.10/6.11,
//! 6.12/6.13). One implementation, two store kinds — both use the
//! Subscription data type (5.2.12).

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
    let mut out = Map::new();
    for (k, v) in doc {
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
                                let id = ev
                                    .as_str()
                                    .ok_or_else(|| bad("EntitySelector id must be a URI".into()))?;
                                antares_model::EntityId::new(id)?;
                                ne.insert("id".into(), ev.clone());
                            }
                            "idPattern" => {
                                let p = ev
                                    .as_str()
                                    .ok_or_else(|| bad("idPattern must be a string".into()))?;
                                regex::Regex::new(p)
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
                let mut params: HashMap<String, String> = HashMap::new();
                for k in ["georel", "geometry", "geoproperty"] {
                    if let Some(s) = g.get(k).and_then(Value::as_str) {
                        params.insert(k.into(), s.to_owned());
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
                crate::geo::GeoQuery::from_params(&params)?
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
                let scheme = uri.split(':').next().unwrap_or("");
                // G3: a scheme with no registered sink is 422 at creation.
                #[cfg(feature = "mqtt")]
                let supported = ["http", "https", "mqtt", "mqtts"];
                #[cfg(not(feature = "mqtt"))]
                let supported = ["http", "https"];
                if !supported.contains(&scheme) {
                    return Err(NgsiError::OperationNotSupported(format!(
                        "unsupported endpoint scheme {scheme:?}"
                    )));
                }
                #[cfg(feature = "mqtt")]
                if scheme.starts_with("mqtt") {
                    // 7.2: endpoint URI shape and Table 7.2-1 params validate
                    // at creation, not at first delivery.
                    antares_notifier::mqtt::MqttEndpoint::parse(uri)?;
                    let pairs = ep
                        .get("notifierInfo")
                        .and_then(Value::as_array)
                        .map(|ni| {
                            ni.iter()
                                .filter_map(|kv| {
                                    Some((kv.get("key")?.as_str()?, kv.get("value")?.as_str()?))
                                })
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    antares_notifier::mqtt::MqttParams::from_notifier_info(pairs)?;
                }
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
                // Table 5.2.15-1: cooldown and timeout are Numbers "Greater
                // than 0" (V-16/V-17)
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
                if s < now_iso().as_str() {
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
            "scopeQ"
            | "lang"
            | "subscriptionName"
            | "name"
            | "description"
            | "notificationTrigger"
            | "temporalQ"
            | "csf"
            | "jsonldContext"
            | "ngsildConformance"
            | "datasetId" => {
                out.insert(k.clone(), v.clone());
            }
            // tolerant reader: keep unknown members (§15.1)
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
            "__context" => continue,
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
        .is_some_and(|e| e < now_iso().as_str());
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
/// dereferenceable @context — invalid value ⇒ 400, unresolvable ⇒ 503.
async fn check_jsonld_context(st: &AppState, norm: &Map<String, Value>) -> Result<(), ApiError> {
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
    st.loader.resolve(v).await?;
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
    let obj = parsed
        .value
        .as_object()
        .ok_or_else(|| NgsiError::BadRequestData("subscription must be a JSON object".into()))?;
    let mut norm = normalize_subscription(obj, &parsed.ctx, false)?;
    check_jsonld_context(st, &norm).await?;
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
    // notification @context = the creating request's context (5.8.6; §8.3
    // stores it as its own column) — internal member, stripped on output.
    norm.insert("__context".into(), parsed.ctx.source.clone());
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
                        "body": {"@context": parsed.ctx.source.clone()},
                    }),
                )?;
                st.loader
                    .put_local(url.clone(), parsed.ctx.source.clone())
                    .await;
                norm.insert("jsonldContext".into(), Value::String(url));
            }
        }
    }
    let doc = Value::Object(norm);
    if !st.store.create(&tenant, kind, &id, doc.clone())? {
        return Err(NgsiError::AlreadyExists(format!("subscription {id} already exists")).into());
    }
    if kind == Kind::Subscription {
        st.sub_changed(&tenant, &id, Some(&doc));
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
    let sys = params
        .get("options")
        .is_some_and(|o| o.split(',').any(|s| s.trim() == "sysAttrs"));
    let payload = present_subscription(&doc, &ctx, sys, kind == Kind::CSourceSubscription);
    Ok(respond(StatusCode::OK, payload, &ctx, accept, &tenant))
}

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
    let all = st.store.list(&tenant, kind)?;
    let (page, count_hdr, links) = crate::entities::paginate_accept(
        st,
        params,
        all,
        &format!("/ngsi-ld/v1/{}", resource_path(kind)),
        accept,
    )?;
    let sys = params
        .get("options")
        .is_some_and(|o| o.split(',').any(|s| s.trim() == "sysAttrs"));
    let payload: Vec<Value> = page
        .iter()
        .map(|d| present_subscription(d, &ctx, sys, kind == Kind::CSourceSubscription))
        .collect();
    let mut resp = crate::negotiate::respond_list(StatusCode::OK, payload, &ctx, accept, &tenant);
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
    let obj = parsed
        .value
        .as_object()
        .ok_or_else(|| NgsiError::BadRequestData("fragment must be a JSON object".into()))?;
    if let Some(bid) = obj.get("id").and_then(Value::as_str) {
        if bid != id {
            return Err(NgsiError::BadRequestData("fragment id mismatch".into()).into());
        }
    }
    let norm = normalize_subscription(obj, &parsed.ctx, true)?;
    check_jsonld_context(st, &norm).await?;
    let ts = now_iso();
    let res = st.store.mutate(&tenant, kind, id, |doc| {
        let target = doc.as_object_mut().expect("subscription object");
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
            if kind == Kind::Subscription && st.sub_sync.is_some() {
                let doc = st.store.get(&tenant, kind, id)?;
                st.sub_changed(&tenant, id, doc.as_ref());
            }
            Ok(no_content(&tenant))
        }
    }
}

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
        if kind == Kind::Subscription {
            st.sub_changed(&tenant, id, None);
        }
        Ok(no_content(&tenant))
    } else {
        Err(NgsiError::ResourceNotFound(format!("subscription {id} not found")).into())
    }
}

// axum route fns

macro_rules! route4 {
    ($create:ident, $retrieve:ident, $list:ident, $update:ident, $delete:ident, $kind:expr) => {
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
        pub async fn $list(
            State(st): State<AppState>,
            CleanParams(params): CleanParams,
            headers: HeaderMap,
        ) -> Response {
            list(&st, $kind, &params, &headers)
                .await
                .unwrap_or_else(|e| e.into_response())
        }
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
    Kind::Subscription
);
route4!(
    create_csource_subscription,
    retrieve_csource_subscription,
    query_csource_subscriptions,
    update_csource_subscription,
    delete_csource_subscription,
    Kind::CSourceSubscription
);

#[cfg(test)]
mod tests {
    use super::*;
    use antares_jsonld::Loader;
    use serde_json::json;

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
    /// keyValues" (audit V-20). "simplified" is the table's declared synonym.
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
    /// notification.attributes / pick / omit (audit V-18).
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
}
