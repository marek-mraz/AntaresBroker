//! Distributed operations (4.3.6, 5.12 matching, 6.3.17–6.3.19).
//!
//! Registration modes: inclusive (local + forward), auxiliary (read-only
//! supplement, local wins), exclusive/redirect (proxied — registered data is
//! never held locally). Forwarded requests carry `Via: 1.1 <hostAlias>` and
//! the request @context as a Link header; bodies travel as application/json
//! without an inline @context.

use crate::negotiate::*;
use crate::state::AppState;
use antares_jsonld::Context;
use antares_model::TenantId;
use antares_sql::store::Kind;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{json, Map, Value};
use std::collections::HashMap;

const FEDERATION_OPS: &[&str] = &[
    "retrieveEntity",
    "queryEntity",
    "queryBatch",
    "retrieveEntityTypes",
    "retrieveEntityTypeDetails",
    "retrieveEntityTypeInfo",
    "retrieveAttrTypes",
    "retrieveAttrTypeDetails",
    "retrieveAttrTypeInfo",
    "createSubscription",
    "updateSubscription",
    "retrieveSubscription",
    "querySubscription",
    "deleteSubscription",
    "retrieveEntityMap",
    "updateEntityMap",
    "deleteEntityMap",
    "createEntityMapQueryEntity",
    "retrieveContextSourceIdentity",
];
const REDIRECTION_OPS: &[&str] = &[
    "createEntity",
    "updateEntity",
    "appendAttrs",
    "updateAttrs",
    "deleteAttrs",
    "deleteEntity",
    "mergeEntity",
    "replaceEntity",
    "replaceAttrs",
    "retrieveEntity",
    "queryEntity",
    "purgeEntity",
    "retrieveEntityTypes",
    "retrieveEntityTypeDetails",
    "retrieveEntityTypeInfo",
    "retrieveAttrTypes",
    "retrieveAttrTypeDetails",
    "retrieveAttrTypeInfo",
    "retrieveEntityMap",
    "updateEntityMap",
    "deleteEntityMap",
    "createEntityMapQueryEntity",
    "retrieveContextSourceIdentity",
];
const UPDATE_OPS: &[&str] = &[
    "updateEntity",
    "updateAttrs",
    "replaceEntity",
    "replaceAttrs",
];
const RETRIEVE_OPS: &[&str] = &["retrieveEntity", "queryEntity"];

/// One matching registration, compiled for forwarding.
#[derive(Clone, Debug)]
pub struct FedReg {
    pub endpoint: String,
    pub mode: String, // inclusive | auxiliary | exclusive | redirect
    ops: Vec<String>,
    /// Expanded attribute IRIs the matched RegistrationInfo covers; None ⇒ all.
    pub attrs: Option<Vec<String>>,
    /// EntityInfo ids/types of the matched RegistrationInfo elements.
    pub ent_ids: Vec<String>,
    pub ent_types: Vec<String>,
}

impl FedReg {
    pub fn supports(&self, op: &str) -> bool {
        self.ops.iter().any(|o| {
            o == op
                || (o == "federationOps" && FEDERATION_OPS.contains(&op))
                || (o == "redirectionOps" && REDIRECTION_OPS.contains(&op))
                || (o == "updateOps" && UPDATE_OPS.contains(&op))
                || (o == "retrieveOps" && RETRIEVE_OPS.contains(&op))
                || (o == "associationOps" && FEDERATION_OPS.contains(&op))
        })
    }
    pub fn is_proxy(&self) -> bool {
        self.mode == "exclusive" || self.mode == "redirect"
    }
    /// Does this registration cover the given expanded attribute IRI?
    pub fn covers_attr(&self, iri: &str) -> bool {
        self.attrs
            .as_ref()
            .is_none_or(|a| a.iter().any(|x| x == iri))
    }
    pub fn read_op(&self) -> Option<&'static str> {
        ["retrieveEntity", "queryEntity", "queryBatch"]
            .into_iter()
            .find(|op| self.supports(op))
    }
}

/// Is federation active for this request? (6.3.18 local param)
pub fn active(params: &HashMap<String, String>) -> bool {
    params.get("local").map(String::as_str) != Some("true")
}

pub fn inbound_via(headers: &HeaderMap) -> Option<String> {
    headers
        .get("via")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

/// Does the inbound Via chain already name this broker? (loop, 6.3.18)
pub fn via_loop(headers: &HeaderMap, alias: &str) -> bool {
    inbound_via(headers).is_some_and(|v| v.split(',').any(|t| t.trim().ends_with(alias)))
}

fn outbound_via(headers: &HeaderMap, alias: &str) -> String {
    match inbound_via(headers) {
        Some(v) => format!("{v}, 1.1 {alias}"),
        None => format!("1.1 {alias}"),
    }
}

/// The @context URL to advertise on forwarded requests.
pub fn ctx_link_url(headers: &HeaderMap, source: &Value) -> String {
    if let Some(url) = link_context(headers) {
        return url;
    }
    match source {
        Value::String(s) => s.clone(),
        Value::Array(a) => a
            .iter()
            .find_map(|e| e.as_str())
            .unwrap_or(antares_jsonld::CORE_CONTEXT)
            .to_owned(),
        _ => antares_jsonld::CORE_CONTEXT.to_owned(),
    }
}

/// Registrations matching an entity spec (5.12), compiled for forwarding.
pub fn matching_regs(
    st: &AppState,
    tenant: &TenantId,
    spec: &crate::csource::CsrSpec,
    ctx: &Context,
) -> Vec<FedReg> {
    let now = crate::state::now_iso();
    // F5: the ONE compiled mirror when wired (bus=nats), the store otherwise.
    // Expiry is filtered HERE and only here — the single yield point (§4.1).
    let regs = match &st.reg_mirror {
        Some(m) => m.docs(tenant.as_str()),
        None => st
            .store
            .list(tenant, Kind::Registration)
            .unwrap_or_default(),
    };
    regs.into_iter()
        .filter_map(|doc| {
            if doc
                .get("expiresAt")
                .and_then(Value::as_str)
                .is_some_and(|e| e < now.as_str())
            {
                return None;
            }
            let infos = crate::csource::matching_infos(spec, &doc, ctx);
            if infos.is_empty() {
                return None;
            }
            let endpoint = doc
                .get("endpoint")
                .and_then(Value::as_str)?
                .trim_end_matches('/');
            // registrations may name the API root itself (…/ngsi-ld/v1) —
            // normalize so forward URLs never double the prefix (IOP fixtures)
            let endpoint = endpoint
                .strip_suffix("/ngsi-ld/v1")
                .unwrap_or(endpoint)
                .to_owned();
            let mode = doc
                .get("mode")
                .and_then(Value::as_str)
                .unwrap_or("inclusive")
                .to_owned();
            let ops = doc
                .get("operations")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_else(|| vec!["federationOps".into()]);
            let mut attrs: Option<Vec<String>> = Some(Vec::new());
            let mut ent_ids = Vec::new();
            let mut ent_types = Vec::new();
            for info in &infos {
                let props = info.get("propertyNames").and_then(Value::as_array);
                let rels = info.get("relationshipNames").and_then(Value::as_array);
                if props.is_none() && rels.is_none() {
                    attrs = None; // an unscoped info covers everything
                } else if let Some(list) = &mut attrs {
                    for src in [props, rels].into_iter().flatten() {
                        list.extend(src.iter().filter_map(Value::as_str).map(str::to_owned));
                    }
                }
                if let Some(es) = info.get("entities").and_then(Value::as_array) {
                    for e in es {
                        if let Some(i) = e.get("id").and_then(Value::as_str) {
                            ent_ids.push(i.to_owned());
                        }
                        if let Some(t) = e.get("type").and_then(Value::as_str) {
                            ent_types.push(t.to_owned());
                        }
                    }
                }
            }
            Some(FedReg {
                endpoint,
                mode,
                ops,
                attrs,
                ent_ids,
                ent_types,
            })
        })
        .collect()
}

/// One forwarded request. `body` is compacted JSON (no @context member).
#[allow(clippy::too_many_arguments)] // mirrors the wire: one param per forwarded request part
pub async fn forward(
    st: &AppState,
    method: reqwest::Method,
    url: String,
    query: &[(String, String)],
    headers: &HeaderMap,
    tenant: &TenantId,
    ctx_url: &str,
    body: Option<Value>,
) -> (u16, Value) {
    // I4: one policy for every outbound class — scheme allowlist,
    // private-range deny, per-destination circuit breaker (§16.4/§16.7).
    if let Err(e) = st.egress.check_url(&url).await {
        tracing::warn!("federation forward to {url} refused: {e}");
        return (502, Value::Null);
    }
    if st.egress.is_open(&url) {
        tracing::debug!("federation forward to {url} short-circuited (breaker open)");
        return (503, Value::Null);
    }
    let mut req = st
        .fed_http
        .request(method, &url)
        .header("Accept", "application/json")
        .header(
            "Link",
            format!("<{ctx_url}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\""),
        )
        .header("Via", outbound_via(headers, &st.host_alias));
    if !query.is_empty() {
        req = req.query(query);
    }
    if tenant.as_str() != "default" {
        req = req.header("NGSILD-Tenant", tenant.as_str());
    }
    if let Some(b) = body {
        req = req
            .header("Content-Type", "application/json")
            .body(serde_json::to_vec(&b).unwrap_or_default());
    }
    match req.send().await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            // 5xx from a peer counts against it too — a peer answering 500
            // on every forward is as dead as one that times out.
            if status >= 500 {
                st.egress.record_failure(&url);
            } else {
                st.egress.record_success(&url);
            }
            let body = resp.json::<Value>().await.unwrap_or(Value::Null);
            (status, body)
        }
        Err(e) if e.is_timeout() => {
            st.egress.record_failure(&url);
            (504, Value::Null)
        }
        Err(_) => {
            st.egress.record_failure(&url);
            (502, Value::Null)
        }
    }
}

/// Expand + registration-scope-filter one remote compacted entity.
pub fn import_entity(remote: &Value, reg: &FedReg, ctx: &Context) -> Option<Value> {
    let mut obj = remote.as_object()?.clone();
    obj.remove("@context");
    let expanded = antares_jsonld::expand_entity(
        &obj,
        ctx,
        antares_jsonld::ExpandOpts {
            sys: true,
            ..Default::default()
        },
    )
    .ok()?;
    let Some(scope) = &reg.attrs else {
        return Some(expanded);
    };
    let mut out = Map::new();
    for (k, v) in expanded.as_object()? {
        if ["id", "type", "scope", "createdAt", "modifiedAt"].contains(&k.as_str())
            || scope.iter().any(|s| s == k)
        {
            out.insert(k.clone(), v.clone());
        }
    }
    Some(Value::Object(out))
}

fn recency(inst: &Value) -> &str {
    inst.get("observedAt")
        .or_else(|| inst.get("modifiedAt"))
        .and_then(Value::as_str)
        .unwrap_or("")
}

/// Merge attributes of `add` into `base` (auxiliary sources never override —
/// base wins; otherwise conflicting instances resolve by most recent
/// observedAt/modifiedAt per 4.5.5.3).
pub fn merge_docs(base: &mut Value, add: &Value, aux: bool) {
    let Some(bo) = base.as_object_mut() else {
        return;
    };
    let Some(ao) = add.as_object() else { return };
    for (k, v) in ao {
        match bo.get_mut(k) {
            None => {
                bo.insert(k.clone(), v.clone());
            }
            Some(cur) if !aux && !matches!(k.as_str(), "id" | "type" | "scope") => {
                let (Some(ca), Some(aa)) = (cur.as_array_mut(), v.as_array()) else {
                    continue;
                };
                for ai in aa {
                    let ds = ai.get("datasetId").and_then(Value::as_str);
                    match ca
                        .iter_mut()
                        .find(|ci| ci.get("datasetId").and_then(Value::as_str) == ds)
                    {
                        None => ca.push(ai.clone()),
                        Some(ci) => {
                            if recency(ai) > recency(ci) {
                                *ci = ai.clone();
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

// ---------- distributed reads ----------

/// Federated retrieve: internal docs from every matching registration,
/// (aux, doc) pairs so callers can order the merge.
pub async fn fed_retrieve(
    st: &AppState,
    tenant: &TenantId,
    headers: &HeaderMap,
    ctx: &Context,
    id: &str,
) -> Vec<(bool, Value)> {
    let spec = crate::csource::CsrSpec {
        ids: Some(vec![id.to_owned()]),
        ..Default::default()
    };
    let ctx_url = ctx_link_url(headers, &ctx.source);
    let mut out = Vec::new();
    for reg in matching_regs(st, tenant, &spec, ctx) {
        let Some(op) = reg.read_op() else { continue };
        // sysAttrs on every forwarded read: conflicting instances resolve by
        // most recent observedAt/modifiedAt (4.5.5.3) — without the remote
        // modifiedAt the winner would be arrival order, i.e. indeterminate.
        let mut query: Vec<(String, String)> = vec![("options".into(), "sysAttrs".into())];
        if let Some(scope) = &reg.attrs {
            let names: Vec<String> = scope.iter().map(|a| ctx.compact_iri(a)).collect();
            query.push(("attrs".into(), names.join(",")));
        }
        let (status, body) = match op {
            "retrieveEntity" => {
                forward(
                    st,
                    reqwest::Method::GET,
                    format!("{}/ngsi-ld/v1/entities/{id}", reg.endpoint),
                    &query,
                    headers,
                    tenant,
                    &ctx_url,
                    None,
                )
                .await
            }
            "queryEntity" => {
                let t = reg
                    .ent_types
                    .first()
                    .map(|t| ctx.compact_iri(t))
                    .unwrap_or_else(|| "*".into());
                query.push(("type".into(), t));
                query.push(("id".into(), id.to_owned()));
                forward(
                    st,
                    reqwest::Method::GET,
                    format!("{}/ngsi-ld/v1/entities", reg.endpoint),
                    &query,
                    headers,
                    tenant,
                    &ctx_url,
                    None,
                )
                .await
            }
            _ => {
                // queryBatch
                let mut sel = Map::new();
                if let Some(t) = reg.ent_types.first() {
                    sel.insert("type".into(), Value::String(ctx.compact_iri(t)));
                }
                sel.insert("id".into(), Value::String(id.to_owned()));
                forward(
                    st,
                    reqwest::Method::POST,
                    format!("{}/ngsi-ld/v1/entityOperations/query", reg.endpoint),
                    &query,
                    headers,
                    tenant,
                    &ctx_url,
                    Some(json!({"type": "Query", "entities": [Value::Object(sel)]})),
                )
                .await
            }
        };
        if !(200..300).contains(&status) {
            continue;
        }
        let candidates: Vec<&Value> = match &body {
            Value::Array(a) => a.iter().collect(),
            Value::Object(_) => vec![&body],
            _ => continue,
        };
        for c in candidates {
            if c.get("id").and_then(Value::as_str) != Some(id) {
                continue;
            }
            if let Some(doc) = import_entity(c, &reg, ctx) {
                out.push((reg.mode == "auxiliary", doc));
            }
        }
    }
    out
}

/// Federated query: internal docs matching a type query, per registration.
pub async fn fed_query(
    st: &AppState,
    tenant: &TenantId,
    headers: &HeaderMap,
    ctx: &Context,
    params: &HashMap<String, String>,
) -> Vec<(bool, Value)> {
    let types: Option<Vec<String>> = params
        .get("type")
        .map(|s| s.split(',').map(|t| ctx.expand_key(t.trim())).collect());
    let ids: Option<Vec<String>> = params
        .get("id")
        .map(|s| s.split(',').map(str::to_owned).collect());
    let spec = crate::csource::CsrSpec {
        types,
        ids,
        ..Default::default()
    };
    let ctx_url = ctx_link_url(headers, &ctx.source);
    let mut out = Vec::new();
    for reg in matching_regs(st, tenant, &spec, ctx) {
        let Some(op) = reg.read_op() else { continue };
        let (status, body) = if op == "queryBatch" && !reg.supports("queryEntity") {
            let mut sel = Map::new();
            if let Some(t) = params.get("type") {
                sel.insert("type".into(), Value::String(t.clone()));
            }
            if let Some(id) = reg.ent_ids.first() {
                sel.insert("id".into(), Value::String(id.clone()));
            }
            forward(
                st,
                reqwest::Method::POST,
                format!("{}/ngsi-ld/v1/entityOperations/query", reg.endpoint),
                &[("options".into(), "sysAttrs".into())],
                headers,
                tenant,
                &ctx_url,
                Some(json!({"type": "Query", "entities": [Value::Object(sel)]})),
            )
            .await
        } else {
            let mut query: Vec<(String, String)> = vec![("options".into(), "sysAttrs".into())];
            if let Some(t) = params.get("type") {
                query.push(("type".into(), t.clone()));
            }
            if let Some(id) = reg.ent_ids.first() {
                query.push(("id".into(), id.clone()));
            } else if let Some(ids) = params.get("id") {
                query.push(("id".into(), ids.clone()));
            }
            if let Some(scope) = &reg.attrs {
                let names: Vec<String> = scope.iter().map(|a| ctx.compact_iri(a)).collect();
                query.push(("attrs".into(), names.join(",")));
            } else if let Some(a) = params.get("attrs") {
                query.push(("attrs".into(), a.clone()));
            }
            forward(
                st,
                reqwest::Method::GET,
                format!("{}/ngsi-ld/v1/entities", reg.endpoint),
                &query,
                headers,
                tenant,
                &ctx_url,
                None,
            )
            .await
        };
        if !(200..300).contains(&status) {
            continue;
        }
        if let Value::Array(a) = &body {
            for c in a {
                if let Some(doc) = import_entity(c, &reg, ctx) {
                    out.push((reg.mode == "auxiliary", doc));
                }
            }
        }
    }
    out
}

/// Merge federated docs into a local candidate set (keyed by id). Local docs
/// win; non-aux remote attrs merge before aux ones.
pub fn merge_candidates(local: Vec<Value>, fed: Vec<(bool, Value)>) -> Vec<Value> {
    let mut order: Vec<String> = Vec::new();
    let mut by_id: HashMap<String, Value> = HashMap::new();
    for doc in local {
        if let Some(id) = doc.get("id").and_then(Value::as_str) {
            order.push(id.to_owned());
            by_id.insert(id.to_owned(), doc);
        }
    }
    for aux_pass in [false, true] {
        for (aux, doc) in &fed {
            if *aux != aux_pass {
                continue;
            }
            let Some(id) = doc.get("id").and_then(Value::as_str) else {
                continue;
            };
            match by_id.get_mut(id) {
                Some(base) => merge_docs(base, doc, *aux),
                None => {
                    order.push(id.to_owned());
                    by_id.insert(id.to_owned(), doc.clone());
                }
            }
        }
    }
    order
        .into_iter()
        .filter_map(|id| by_id.remove(&id))
        .collect()
}

// ---------- distributed writes ----------

/// Outcome of one part of a distributed write.
pub struct Part {
    pub status: u16,
    pub detail: String,
}

impl Part {
    pub fn ok(&self) -> bool {
        // 207 from a forwarded source is a partial failure, not a success
        (200..300).contains(&self.status) && self.status != 207
    }
}

/// Combine local + forwarded parts (6.3.17/6.4.3.1): all-success ⇒ `ok`,
/// single failing part ⇒ its own error, mixed ⇒ 207 Multi-Status.
pub fn combine(parts: Vec<Part>, ok: Response, tenant: &TenantId) -> Response {
    if parts.iter().all(Part::ok) {
        return ok;
    }
    if parts.len() == 1 {
        let p = &parts[0];
        let status = StatusCode::from_u16(p.status).unwrap_or(StatusCode::BAD_GATEWAY);
        let (etype, title) = match p.status {
            409 => ("AlreadyExists", "Conflict"),
            404 => ("ResourceNotFound", "Not Found"),
            _ => ("InternalError", "Error"),
        };
        let body = json!({
            "type": format!("https://uri.etsi.org/ngsi-ld/errors/{etype}"),
            "title": title,
            "detail": p.detail,
            "status": p.status,
        });
        let mut resp = (
            status,
            [(axum::http::header::CONTENT_TYPE, "application/json")],
            axum::Json(body),
        )
            .into_response();
        echo_tenant(tenant, &mut resp);
        return resp;
    }
    let errors: Vec<Value> = parts
        .iter()
        .filter(|p| !p.ok())
        .map(|p| {
            json!({
                "error": {
                    "status": p.status,
                    "type": "https://uri.etsi.org/ngsi-ld/errors/InternalError",
                    "title": "distributed operation failed",
                    "detail": p.detail,
                }
            })
        })
        .collect();
    let body = json!({"success": [], "errors": errors});
    let mut resp = (
        StatusCode::MULTI_STATUS,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        axum::Json(body),
    )
        .into_response();
    echo_tenant(tenant, &mut resp);
    resp
}

/// Reduce a compacted entity/fragment to the members a registration covers
/// (plus id/type); returns None if no attribute member remains.
pub fn reduce_to_scope(obj: &Map<String, Value>, reg: &FedReg, ctx: &Context) -> Option<Value> {
    let Some(_) = &reg.attrs else {
        let mut full = obj.clone();
        full.remove("@context");
        return Some(Value::Object(full));
    };
    let mut out = Map::new();
    let mut any = false;
    for (k, v) in obj {
        if k == "@context" {
            continue;
        }
        if ["id", "type", "scope"].contains(&k.as_str()) {
            out.insert(k.clone(), v.clone());
            continue;
        }
        if reg.covers_attr(&ctx.expand_key(k)) {
            out.insert(k.clone(), v.clone());
            any = true;
        }
    }
    any.then_some(Value::Object(out))
}

/// 508 Loop Detected (6.3.17): the inbound Via chain already names us and a
/// registration would forward the operation right back.
pub fn loop_508(tenant: &TenantId) -> Response {
    let body = json!({
        "type": "https://uri.etsi.org/ngsi-ld/errors/InternalError",
        "title": "Loop Detected",
        "detail": "the Via chain already contains this broker",
        "status": 508,
    });
    let mut resp = (
        StatusCode::LOOP_DETECTED,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        axum::Json(body),
    )
        .into_response();
    echo_tenant(tenant, &mut resp);
    resp
}

/// Matching non-auxiliary registrations for a write op.
pub fn write_regs(
    st: &AppState,
    tenant: &TenantId,
    spec: &crate::csource::CsrSpec,
    ctx: &Context,
    params: &HashMap<String, String>,
) -> Vec<FedReg> {
    if !active(params) {
        return Vec::new();
    }
    matching_regs(st, tenant, spec, ctx)
        .into_iter()
        .filter(|r| r.mode != "auxiliary")
        .collect()
}

/// Execute one forwarded write and turn it into a Part.
#[allow(clippy::too_many_arguments)]
pub async fn forward_part(
    st: &AppState,
    method: reqwest::Method,
    url: String,
    query: &[(String, String)],
    headers: &HeaderMap,
    tenant: &TenantId,
    ctx_url: &str,
    body: Option<Value>,
) -> Part {
    let (status, _) = forward(
        st,
        method,
        url.clone(),
        query,
        headers,
        tenant,
        ctx_url,
        body,
    )
    .await;
    Part {
        status,
        detail: format!("distributed operation to {url} returned {status}"),
    }
}

/// Forward one attribute-level write to every matching registration.
#[allow(clippy::too_many_arguments)]
pub async fn fed_attr_parts(
    st: &AppState,
    headers: &HeaderMap,
    tenant: &TenantId,
    ctx_source: &Value,
    regs: &[FedReg],
    op: &str,
    method: reqwest::Method,
    path: &str,
    query: &[(String, String)],
    body: Option<Value>,
) -> Vec<Part> {
    let ctx_url = ctx_link_url(headers, ctx_source);
    let mut parts = Vec::new();
    for reg in regs {
        if reg.mode == "exclusive" && !reg.supports(op) {
            parts.push(conflict_part(op));
            continue;
        }
        parts.push(
            forward_part(
                st,
                method.clone(),
                format!("{}/ngsi-ld/v1{path}", reg.endpoint),
                query,
                headers,
                tenant,
                &ctx_url,
                body.clone(),
            )
            .await,
        );
    }
    parts
}

/// Conflict part for an exclusive registration that does not accept the op.
pub fn conflict_part(op: &str) -> Part {
    Part {
        status: 409,
        detail: format!("exclusive registration does not accept {op}"),
    }
}

/// Remove proxy-covered attributes from an EXPANDED fragment so the local
/// write never stores exclusively/redirect-registered data (4.3.6.3).
pub fn strip_covered_expanded(fragment: &Value, regs: &[FedReg]) -> Value {
    let mut f = fragment.clone();
    if let Some(o) = f.as_object_mut() {
        let covered: Vec<String> = o
            .keys()
            .filter(|k| {
                !matches!(
                    k.as_str(),
                    "id" | "type" | "scope" | "createdAt" | "modifiedAt"
                ) && regs.iter().any(|r| r.is_proxy() && r.covers_attr(k))
            })
            .cloned()
            .collect();
        for k in covered {
            o.remove(&k);
        }
    }
    f
}

/// Strip the members proxied registrations cover from a compacted object;
/// returns (remainder, had_attrs_left).
pub fn strip_proxied(
    obj: &Map<String, Value>,
    proxies: &[&FedReg],
    ctx: &Context,
) -> (Map<String, Value>, bool) {
    let mut out = Map::new();
    let mut any_attr = false;
    for (k, v) in obj {
        if k == "@context" {
            continue;
        }
        if ["id", "type", "scope"].contains(&k.as_str()) {
            out.insert(k.clone(), v.clone());
            continue;
        }
        let iri = ctx.expand_key(k);
        if proxies.iter().any(|r| r.covers_attr(&iri)) {
            continue;
        }
        out.insert(k.clone(), v.clone());
        any_attr = true;
    }
    (out, any_attr)
}
