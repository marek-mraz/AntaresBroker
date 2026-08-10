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
#[derive(Clone, Debug, Default)]
pub struct FedReg {
    /// The registration's @id — carried into `NotUpdatedDetails.registrationId`
    /// (5.2.19) and `BatchEntityError.registrationId` (5.2.17).
    pub reg_id: String,
    pub endpoint: String,
    pub mode: String, // inclusive | auxiliary | exclusive | redirect
    pub(crate) ops: Vec<String>,
    /// Expanded attribute IRIs the matched RegistrationInfo covers; None ⇒ all.
    pub attrs: Option<Vec<String>>,
    /// EntityInfo ids/types of the matched RegistrationInfo elements.
    pub ent_ids: Vec<String>,
    pub ent_types: Vec<String>,
    /// 5.2.9 `tenant`: the Tenant to specify in all requests to this Context
    /// Source. None ⇒ the requesting tenant is carried through unchanged.
    pub tenant: Option<String>,
    /// 5.2.9 `contextSourceAlias`: "a previously retrieved unique id for a
    /// registered Context Source which is used to identify loops", tenant-
    /// specific per Table 5.2.9-1. A registration whose alias is already in
    /// the inbound Via chain names a source this request has visited, so it
    /// is not a matching registration (Table 6.3.18-2).
    pub alias: Option<String>,
    /// 4.3.6.5 `contextSourceInfo` key/value pairs, conveyed as headers on
    /// every forward to this source (string values only — headers).
    pub csi: Vec<(String, String)>,
    /// 5.2.9 `localOnly` (4.3.6.4): distributed operations for this
    /// registration "will act only on data held directly by the registered
    /// Context Source itself" — every forward carries `local=true`.
    pub local_only: bool,
}

impl FedReg {
    /// 4.3.6.1: registered Context Sources "may indicate that they are only
    /// willing to respond to a limited subset of API operations. Context
    /// Brokers shall respect this, to avoid unnecessarily sending distributed
    /// operation requests which are always guaranteed to fail." Matches the
    /// registration's `operations` list (5.2.9) by name or operation group;
    /// default when absent is federationOps.
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
///
/// Table 6.4.3.2-1: for `type=*`, "local is implicitly set to true and shall
/// not be explicitly set to false" — so the wildcard alone disables forwarding.
pub fn active(params: &HashMap<String, String>) -> bool {
    if params.get("type").map(String::as_str) == Some("*") {
        return false;
    }
    params.get("local").map(String::as_str) != Some("true")
}

/// The full inbound Via chain, joined across header FIELDS.
///
/// RFC 7230 §3.2.2/§5.7.1: Via is a list header — senders may split the
/// chain over any number of `Via:` field lines, and the two forms are
/// equivalent. Reading only the first field (`headers.get`) made a loop
/// pseudonym in a later field invisible (undetected cycle) AND rebuilt the
/// outbound chain from the truncated view, deleting upstream hop history
/// that downstream brokers need for THEIR loop detection.
pub fn inbound_via(headers: &HeaderMap) -> Option<String> {
    let fields: Vec<&str> = headers
        .get_all("via")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .collect();
    if fields.is_empty() {
        None
    } else {
        Some(fields.join(", "))
    }
}

/// This broker's Via pseudonym **for one Tenant**.
///
/// Table 5.2.40-1: the alias is "a unique id for a Context Source which can
/// be used to identify loops. In the multi-tenancy use case (see clause
/// 4.14), this id **shall** be identifying a specific Tenant within a
/// registered Context Source." One static per-process alias therefore makes
/// every tenant of this broker look like the same Context Source: a request
/// in tenant B whose registration points back here for tenant A is a
/// different (source, tenant) pair, but a tenant-blind chain reads it as a
/// loop and drops the registration.
///
/// Format `{alias}~{tenant}`, and the bare alias for the default tenant —
/// mirroring 6.3.14, where the tenant header is omitted rather than sent as
/// `default`. `~` is an RFC 7230 token character that cannot occur in a
/// `TenantId` (`[A-Za-z0-9_-]{1,64}`), so the two halves never blur; the
/// broker rejects a configured `ANTARES_HOST_ALIAS` containing `~` at
/// startup, so `a~b` in the default tenant cannot collide with `a` in
/// tenant `b`.
///
/// The value is stable for the life of a deployment because peers **register**
/// it: a Context Source Registration's `contextSourceAlias` (Table 5.2.9-1) is
/// "a previously retrieved unique id" — retrieved from this broker's
/// `/info/sourceIdentity` for that tenant. Changing an alias silently breaks
/// every peer's loop detection, so treat it as a published identifier.
pub fn alias_for(host_alias: &str, tenant: &TenantId) -> String {
    if tenant.as_str() == TenantId::DEFAULT {
        host_alias.to_owned()
    } else {
        format!("{host_alias}~{}", tenant.as_str())
    }
}

/// The `received-by` pseudonyms of the inbound Via chain — the Context
/// Sources this request has already passed through (Table 6.3.18-2).
///
/// RFC 7230: `Via = 1#( received-protocol RWS received-by [ RWS comment ] )`
/// — received-by is the SECOND whitespace token of each element and is a
/// token compared for equality, never by suffix (audit V-30: `ends_with`
/// made alias `b1` match peer `sub-b1`). A malformed element with no
/// protocol falls back to its first token.
pub fn via_tokens(headers: &HeaderMap) -> Vec<String> {
    inbound_via(headers)
        .map(|v| {
            v.split(',')
                .filter_map(|t| {
                    let mut toks = t.split_whitespace();
                    let first = toks.next();
                    toks.next().or(first).map(str::to_owned)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Does the inbound Via chain already name this broker, in this tenant?
/// (loop, 6.3.18) — `alias` is always [`alias_for`]'s tenant-qualified value.
pub fn via_loop(headers: &HeaderMap, alias: &str) -> bool {
    via_tokens(headers).iter().any(|t| t == alias)
}

/// 6.3.17/6.3.18 loop handling for operations with matching registrations.
/// 508 Loop Detected is mandated ONLY "in the case of an exclusive or
/// redirect registration, where all of the data is held outside of the
/// Context Broker and held in a single registered source ... registered to
/// redirect back on to the Context Broker". Any other loop clears `regs` —
/// the Via listing "is used when determining matching registrations"
/// (Table 6.3.18-2), so the operation proceeds locally without re-forwarding.
pub fn handle_via_loop(
    headers: &HeaderMap,
    alias: &str,
    tenant: &TenantId,
    regs: &mut Vec<FedReg>,
) -> Option<Response> {
    if regs.is_empty() || !via_loop(headers, alias) {
        return None;
    }
    if regs.len() == 1 && regs[0].is_proxy() {
        return Some(loop_508(tenant));
    }
    regs.clear();
    None
}

/// One `NGSILD-Warning` header value (6.3.17), in RFC 7234 warn form:
/// `warn-code SP warn-agent SP quoted warn-text`.
pub fn warning(code: u16, alias: &str, text: &str) -> String {
    format!("{code} {alias} \"{text}\"")
}

/// Classify one forwarded-read outcome per Table 6.3.17-1. A registration
/// endpoint answering 404 with no data "should not be considered as abnormal
/// behaviour"; 503/504 means no response arrived within the timeout (199);
/// any other error status IS a received error response (299); a 2xx whose
/// payload could not be parsed as NGSI-LD is 111.
fn read_warning(status: u16, body: &Value) -> Option<(u16, &'static str)> {
    match status {
        404 => None,
        503 | 504 => Some((
            199,
            "no response was received from the registration endpoint within the timeout period",
        )),
        s if s >= 400 => Some((
            299,
            "an error response was received from the registration endpoint",
        )),
        s if (200..300).contains(&s) && body.is_null() => {
            Some((111, "the payload of the response was invalid"))
        }
        _ => None,
    }
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
///
/// Table 6.3.18-2 makes the inbound `Via` listing part of matching itself —
/// "the listing of previously encountered Context Sources supplied is used
/// when determining matching registrations" — so a registration whose
/// `contextSourceAlias` is already in the chain is filtered out HERE, at the
/// one place every read and write path resolves its candidates. Keeping it
/// out of the call sites is the §4.1 rule: a loop check the callers own is a
/// loop check some caller forgets.
pub fn matching_regs(
    st: &AppState,
    tenant: &TenantId,
    spec: &crate::csource::CsrSpec,
    ctx: &Context,
    headers: &HeaderMap,
) -> Vec<FedReg> {
    let now = crate::state::now_iso();
    let seen = via_tokens(headers);
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
            let alias = doc
                .get("contextSourceAlias")
                .and_then(Value::as_str)
                .map(str::to_owned);
            // Table 6.3.18-2 / 5.2.9: this source already handled the request.
            if alias.as_ref().is_some_and(|a| seen.iter().any(|t| t == a)) {
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
            let tenant = doc.get("tenant").and_then(Value::as_str).map(str::to_owned);
            let csi = doc
                .get("contextSourceInfo")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|kv| {
                            Some((
                                kv.get("key")?.as_str()?.to_owned(),
                                kv.get("value")?.as_str()?.to_owned(),
                            ))
                        })
                        .collect()
                })
                .unwrap_or_default();
            Some(FedReg {
                reg_id: doc
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                endpoint,
                mode,
                ops,
                attrs,
                ent_ids,
                ent_types,
                tenant,
                alias,
                csi,
                local_only: doc
                    .get("localOnly")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            })
        })
        .collect()
}

/// contextSourceInfo keys the forward must NOT copy into headers: the tenant
/// travels via the registration's own `tenant` member (4.3.6.5 "shall not be
/// part of contextSourceInfo"), connection/binding-managed headers cannot be
/// overridden ("shall be ignored"), and the 4.3.6.6 processed keys (accept,
/// contentType, jsonldContext, ngsildConformance) are TRANSFORMED by
/// `forward` (V-29) rather than passed through raw
/// would corrupt negotiation instead.
const CSI_SKIP: &[&str] = &[
    "ngsild-tenant",
    "content-length",
    "content-type",
    "host",
    "via",
    "link",
    "connection",
    "accept",
    "contenttype",
    "jsonldcontext",
    "ngsildconformance",
];

/// One forwarded request. `body` is compacted JSON (no @context member).
#[allow(clippy::too_many_arguments)] // mirrors the wire: one param per forwarded request part
pub async fn forward(
    st: &AppState,
    method: reqwest::Method,
    url: String,
    query: &[(String, String)],
    headers: &HeaderMap,
    tenant: &TenantId,
    reg: &FedReg,
    ctx_url: &str,
    mut body: Option<Value>,
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
    // 4.3.6.6 (V-29): the four contextSourceInfo keys with processing
    // semantics. Values were validated at registration time (5.9.2).
    let csi_get = |key: &str| {
        reg.csi
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    };
    // "accept": the response shall come back in this format (the read path
    // strips any body @context before expanding, so both forms import).
    let accept = csi_get("accept").unwrap_or("application/json");
    // "ngsildConformance": amend the payload to the pinned version (4.3.6.8).
    if let Some(ver) = csi_get("ngsildConformance").and_then(crate::conformance::parse_version) {
        if let Some(b) = body.as_mut() {
            crate::conformance::amend_payload(b, ver);
        }
    }
    // "jsonldContext": recompact payload and term-bearing query parameters
    // with the registered context, forward THAT context, Content-Type
    // application/json, no @context member in the payload. Entity-shaped
    // bodies only — a POST-query body has no entity terms to recompact; if
    // either context fails to load the forward degrades to the original
    // context rather than sending terms compacted against the wrong one.
    let mut link_ctx: String = ctx_url.to_owned();
    let mut query: Vec<(String, String)> = query.to_vec();
    // 4.3.6.4: "a binding-specific mechanism to request operations only on
    // the registered endpoint itself" — a localOnly registration (5.2.9)
    // must not cascade, so the forward carries the 6.3.18 local parameter.
    if reg.local_only && !query.iter().any(|(k, _)| k == "local") {
        query.push(("local".into(), "true".into()));
    }
    if let Some(reg_ctx_url) = csi_get("jsonldContext") {
        let orig = st
            .loader
            .resolve_quiet(&Value::String(ctx_url.to_owned()))
            .await;
        let target = st
            .loader
            .resolve_quiet(&Value::String(reg_ctx_url.to_owned()))
            .await;
        if let (Ok(orig), Ok(target)) = (orig, target) {
            if let Some(b) = body.as_mut() {
                let entity_shaped = b
                    .as_object()
                    .is_some_and(|o| o.get("type").and_then(Value::as_str) != Some("Query"));
                if entity_shaped {
                    if let Some(obj) = b.as_object() {
                        let opts = antares_jsonld::ExpandOpts {
                            fragment: obj.get("id").is_none(),
                            ..Default::default()
                        };
                        if let Ok(exp) = antares_jsonld::expand_entity(obj, &orig, opts) {
                            let mut re = antares_jsonld::compact::compact_entity(&exp, &target);
                            if let Some(o) = re.as_object_mut() {
                                o.remove("@context");
                            }
                            *b = re;
                        }
                    }
                }
            }
            for (k, v) in query.iter_mut() {
                if matches!(k.as_str(), "attrs" | "type" | "geoproperty") {
                    *v = v
                        .split(',')
                        .map(|t| target.compact_iri(&orig.expand_key(t.trim())))
                        .collect::<Vec<_>>()
                        .join(",");
                }
            }
            link_ctx = reg_ctx_url.to_owned();
        } else {
            tracing::warn!(
                "registered jsonldContext {reg_ctx_url} (or the request context) \
                 failed to load; forwarding with the original context"
            );
        }
    }
    let mut req = st
        .fed_http
        .request(method, &url)
        .header("Accept", accept)
        .header(
            "Link",
            format!("<{link_ctx}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\""),
        )
        .header("Via", outbound_via(headers, &alias_for(&st.host_alias, tenant)));
    if !query.is_empty() {
        req = req.query(&query);
    }
    // 5.2.9 `tenant`: requests related to this registration carry the
    // registration's Tenant; absent ⇒ the requesting tenant flows through
    // (4.3.6.4: each Tenant in a registered source is considered separately).
    let peer_tenant = reg.tenant.as_deref().unwrap_or(tenant.as_str());
    if peer_tenant != "default" {
        req = req.header("NGSILD-Tenant", peer_tenant);
    }
    // 4.3.6.5 contextSourceInfo ⇒ extra headers; the special value
    // "urn:ngsi-ld:request" copies the header from the triggering request
    // (dropped when the triggering request did not carry it).
    for (k, v) in &reg.csi {
        if CSI_SKIP.contains(&k.to_ascii_lowercase().as_str()) {
            continue;
        }
        let val = if v == "urn:ngsi-ld:request" {
            match headers.get(k.as_str()).and_then(|h| h.to_str().ok()) {
                Some(h) => h.to_owned(),
                None => continue,
            }
        } else {
            v.clone()
        };
        req = req.header(k.as_str(), val);
    }
    if let Some(mut b) = body {
        // 4.3.6.6 "contentType": provide request + @context as the MIME type
        // requires — ld+json carries the context inline. When "jsonldContext"
        // is also defined its own mandate wins ("the Content-Type of the
        // forwarded request shall be application/json").
        let want_ld = csi_get("contentType") == Some("application/ld+json")
            && csi_get("jsonldContext").is_none();
        if want_ld {
            if let Some(o) = b.as_object_mut() {
                o.insert("@context".into(), Value::String(link_ctx.clone()));
            }
            req = req.header("Content-Type", "application/ld+json");
        } else {
            req = req.header("Content-Type", "application/json");
        }
        req = req.body(serde_json::to_vec(&b).unwrap_or_default());
    }
    // N2: the whole HTTP interaction is one Send unit (http_interaction) so
    // the handler futures above stay Send on wasm32 too.
    antares_jsonld::http_interaction(async {
        // wasm has no client-level timeout — bound the forward per request
        // (mirrors the native fed_http 8 s total, §16.7); a timed-out
        // forward counts against the peer like any 5xx.
        let sent = antares_jsonld::io_deadline(req.send(), 8_000).await;
        let sent = match sent {
            Some(r) => r,
            None => {
                st.egress.record_failure(&url);
                return (504, Value::Null);
            }
        };
        match sent {
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
    })
    .await
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

/// 4.5.5.3 first step (audit V-19): "if an expiresAt DateTime is present on
/// the Attribute and the date lies in the past, it shall be discarded" —
/// BEFORE any recency comparison.
fn expired(inst: &Value, now: &str) -> bool {
    inst.get("expiresAt")
        .and_then(Value::as_str)
        .is_some_and(|e| e < now)
}

/// 4.3.6.2: "An auxiliary Context Source Registration never overrides data
/// held directly within a Context Broker. […] Context data from auxiliary
/// context sources is only included if it is supplementary."
/// Merge attributes of `add` into `base` (auxiliary sources never override —
/// base wins; otherwise conflicting instances resolve per 4.5.5.3: discard
/// past-expiresAt instances first, then most recent observedAt/modifiedAt).
pub fn merge_docs(base: &mut Value, add: &Value, aux: bool) {
    let now = crate::state::now_iso();
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
                    if expired(ai, &now) {
                        continue; // 4.5.5.3: discarded before comparison
                    }
                    let ds = ai.get("datasetId").and_then(Value::as_str);
                    match ca
                        .iter_mut()
                        .find(|ci| ci.get("datasetId").and_then(Value::as_str) == ds)
                    {
                        None => ca.push(ai.clone()),
                        Some(ci) => {
                            if expired(ci, &now) || recency(ai) > recency(ci) {
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
    warnings: &mut Vec<String>,
) -> Vec<(bool, Value)> {
    let spec = crate::csource::CsrSpec {
        ids: Some(vec![id.to_owned()]),
        ..Default::default()
    };
    let ctx_url = ctx_link_url(headers, &ctx.source);
    let mut out = Vec::new();
    for reg in matching_regs(st, tenant, &spec, ctx, headers) {
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
                    &reg,
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
                    &reg,
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
                    &reg,
                    &ctx_url,
                    Some(json!({"type": "Query", "entities": [Value::Object(sel)]})),
                )
                .await
            }
        };
        // V-14: abnormal outcomes surface as NGSILD-Warning (6.3.17) — never
        // as a failed overall response; 404-with-no-data is normal.
        if let Some((code, text)) = read_warning(status, &body) {
            warnings.push(warning(code, &alias_for(&st.host_alias, tenant), text));
        }
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
            match import_entity(c, &reg, ctx) {
                Some(doc) => out.push((reg.mode == "auxiliary", doc)),
                // received in time, but not a parseable NGSI-LD entity (111)
                None => warnings.push(warning(
                    111,
                    &alias_for(&st.host_alias, tenant),
                    "the payload of the response was invalid",
                )),
            }
        }
    }
    out
}

/// Federated query: internal docs matching a type query, per registration.
/// The `CsrSpec` a Query Entities request matches registrations against.
fn query_spec(ctx: &Context, params: &HashMap<String, String>) -> crate::csource::CsrSpec {
    let types: Option<Vec<String>> = params
        .get("type")
        .map(|s| s.split(',').map(|t| ctx.expand_key(t.trim())).collect());
    let ids: Option<Vec<String>> = params
        .get("id")
        .map(|s| s.split(',').map(str::to_owned).collect());
    crate::csource::CsrSpec {
        types,
        ids,
        ..Default::default()
    }
}

/// Will this query actually fan out to a Context Source?
///
/// 5.7.2.4 forbids ordering when "the execution of the operation is not
/// limited to the local scope", and 4.23.1 says "Sort ordering is never
/// applied to distributed operations". The subject is the *execution*, not the
/// presence of `local=true`: a query no registration matches executes locally
/// whether or not the client said so. Reading it as "local=true is mandatory
/// for orderBy" would fail ETSI's own 019_19, which orders without it.
pub fn would_federate(
    st: &AppState,
    tenant: &TenantId,
    ctx: &Context,
    params: &HashMap<String, String>,
    headers: &HeaderMap,
) -> bool {
    active(params) && !matching_regs(st, tenant, &query_spec(ctx, params), ctx, headers).is_empty()
}

pub async fn fed_query(
    st: &AppState,
    tenant: &TenantId,
    headers: &HeaderMap,
    ctx: &Context,
    params: &HashMap<String, String>,
    warnings: &mut Vec<String>,
) -> Vec<(bool, Value)> {
    let spec = query_spec(ctx, params);
    let ctx_url = ctx_link_url(headers, &ctx.source);
    let mut out = Vec::new();
    for reg in matching_regs(st, tenant, &spec, ctx, headers) {
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
                &reg,
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
                &reg,
                &ctx_url,
                None,
            )
            .await
        };
        // V-14: same NGSILD-Warning classification as fed_retrieve (6.3.17)
        if let Some((code, text)) = read_warning(status, &body) {
            warnings.push(warning(code, &alias_for(&st.host_alias, tenant), text));
        }
        if !(200..300).contains(&status) {
            continue;
        }
        if let Value::Array(a) = &body {
            for c in a {
                match import_entity(c, &reg, ctx) {
                    Some(doc) => out.push((reg.mode == "auxiliary", doc)),
                    None => warnings.push(warning(
                        111,
                        &alias_for(&st.host_alias, tenant),
                        "the payload of the response was invalid",
                    )),
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
    // 6.3.17: "the error response should be as informative as possible" — a
    // 207 that hides the succeeded halves (did my local delete happen?)
    // isn't. Parts carry no entity id, so success entries are the details.
    let success: Vec<&str> = parts
        .iter()
        .filter(|p| p.ok())
        .map(|p| p.detail.as_str())
        .collect();
    let body = json!({"success": success, "errors": errors});
    let mut resp = (
        StatusCode::MULTI_STATUS,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        axum::Json(body),
    )
        .into_response();
    echo_tenant(tenant, &mut resp);
    resp
}

/// 4.3.6.1: "It is the responsibility of the Context Broker to respect the
/// registration parameters when issuing distributed requests. […] Ultimately,
/// all constraints specified in the registration shall be respected."
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

/// 4.3.6.2: "Auxiliary distributed operations are limited to context
/// information consumption operations (see clause 5.7)" — so a write op
/// only ever considers non-auxiliary matching registrations.
pub fn write_regs(
    st: &AppState,
    tenant: &TenantId,
    spec: &crate::csource::CsrSpec,
    ctx: &Context,
    params: &HashMap<String, String>,
    headers: &HeaderMap,
) -> Vec<FedReg> {
    if !active(params) {
        return Vec::new();
    }
    matching_regs(st, tenant, spec, ctx, headers)
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
    reg: &FedReg,
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
        reg,
        ctx_url,
        body,
    )
    .await;
    let detail = format!("distributed operation to {url} returned {status}");
    // 6.3.17 p.278: for a proxied (exclusive/redirect) source the error
    // vocabulary is fixed — 508 loop, 504 timeout, 404 not found, and
    // "502 Bad Gateway — if the single forwarded request fails for any other
    // reason such as the Context Broker itself having insufficient access
    // rights". 404/504/508 pass through, 409 keeps AlreadyExists semantics
    // (the peer speaks NGSI-LD) and a 207 stays the partial verdict it is;
    // every other failure — auth-class 401/403, a peer's 500/503, a 400 on
    // the inter-broker request — surfaces as 502. The original status stays
    // in `detail` for diagnosis.
    let status = if reg.is_proxy()
        && !(200..300).contains(&status)
        && !matches!(status, 207 | 404 | 409 | 504 | 508)
    {
        502
    } else {
        status
    };
    Part { status, detail }
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
                reg,
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn hdrs(via: Option<&str>) -> HeaderMap {
        let mut h = HeaderMap::new();
        if let Some(v) = via {
            h.insert("via", v.parse().expect("via"));
        }
        h
    }

    /// RFC 7230 received-by is a TOKEN compared for equality (audit V-30):
    /// `ends_with` made alias `b1` match pseudonym `sub-b1` (spurious loop)
    /// and could never catch the converse.
    #[test]
    fn via_loop_compares_tokens_not_suffixes() {
        assert!(via_loop(&hdrs(Some("1.1 b1")), "b1"));
        assert!(via_loop(&hdrs(Some("1.1 b2, 1.1 b1")), "b1"));
        assert!(via_loop(&hdrs(Some("HTTP/1.1 b1")), "b1"));
        assert!(
            !via_loop(&hdrs(Some("1.1 sub-b1")), "b1"),
            "suffix must not match — this was the V-30 false positive"
        );
        assert!(!via_loop(&hdrs(Some("1.1 b10")), "b1"));
        assert!(!via_loop(&hdrs(None), "b1"));
        // malformed element carrying only a pseudonym still detects
        assert!(via_loop(&hdrs(Some("b1")), "b1"));
    }

    fn reg(mode: &str) -> FedReg {
        FedReg {
            reg_id: "urn:ngsi-ld:ContextSourceRegistration:test".into(),
            endpoint: "http://peer:9090".into(),
            mode: mode.into(),
            ops: vec!["federationOps".into()],
            attrs: None,
            ent_ids: vec![],
            ent_types: vec![],
            tenant: None,
            alias: None,
            csi: vec![],
            local_only: false,
        }
    }

    /// Table 5.2.40-1: "In the multi-tenancy use case (see clause 4.14), this
    /// id shall be identifying a specific Tenant within a registered Context
    /// Source." One alias for every tenant of a broker makes cross-tenant
    /// federation inside that broker look like a loop.
    #[test]
    fn alias_identifies_the_tenant_not_just_the_broker() {
        let tenant = |s: &str| antares_model::TenantId::new(s).expect("tenant");
        // the default tenant keeps the bare alias — 6.3.14's own convention
        // (the header is omitted, not sent as "default"), and the wire format
        // every single-tenant peer already registered
        assert_eq!(alias_for("antares1", &tenant("default")), "antares1");
        assert_eq!(alias_for("antares1", &tenant("zvolen")), "antares1~zvolen");
        // a chain naming this broker in ANOTHER tenant is not a loop: the
        // registration points at a different (source, tenant) pair
        let h = hdrs(Some("1.1 antares1~zvolen"));
        assert!(via_loop(&h, &alias_for("antares1", &tenant("zvolen"))));
        assert!(!via_loop(
            &h,
            &alias_for("antares1", &tenant("banskabystrica"))
        ));
        assert!(
            !via_loop(&h, &alias_for("antares1", &tenant("default"))),
            "the default tenant of this broker is its own Context Source"
        );
        // `~` cannot occur in a TenantId and is rejected in a configured
        // alias, so the two halves can never blur into each other
        assert!(antares_model::TenantId::new("a~b").is_err());
    }

    /// Table 6.3.18-2: "the listing of previously encountered Context Sources
    /// supplied is used when determining matching registrations", and 5.2.9
    /// gives a registration the peer's `contextSourceAlias` "which is used to
    /// identify loops". A source already in the chain is therefore not a
    /// match — and the tenant-specific alias keeps that per (source, tenant).
    #[test]
    fn registered_alias_in_the_via_chain_is_not_a_matching_registration() {
        let st = AppState::new("me".into());
        let tenant = antares_model::TenantId::new("default").expect("tenant");
        let ctx = st.loader.core();
        for (id, alias) in [
            ("urn:ngsi-ld:ContextSourceRegistration:visited", "peer1"),
            ("urn:ngsi-ld:ContextSourceRegistration:fresh", "peer2"),
            ("urn:ngsi-ld:ContextSourceRegistration:anon", ""),
        ] {
            let mut doc = json!({
                "id": id,
                "type": "ContextSourceRegistration",
                "endpoint": "http://peer:9090",
                "information": [{"entities": [{"type": "https://uri.etsi.org/ngsi-ld/default-context/Vehicle"}]}],
            });
            if !alias.is_empty() {
                doc["contextSourceAlias"] = json!(alias);
            }
            st.store
                .create(&tenant, Kind::Registration, id, doc)
                .expect("seed registration");
        }
        let spec = crate::csource::CsrSpec {
            types: Some(vec![
                "https://uri.etsi.org/ngsi-ld/default-context/Vehicle".into()
            ]),
            ..Default::default()
        };
        let all = matching_regs(&st, &tenant, &spec, &ctx, &hdrs(None));
        assert_eq!(all.len(), 3, "no Via ⇒ every registration matches");
        let via = hdrs(Some("1.1 peer1"));
        let left = matching_regs(&st, &tenant, &spec, &ctx, &via);
        let ids: Vec<&str> = left.iter().map(|r| r.reg_id.as_str()).collect();
        assert!(
            !ids.contains(&"urn:ngsi-ld:ContextSourceRegistration:visited"),
            "a source already in the Via chain must not match"
        );
        assert_eq!(
            ids.len(),
            2,
            "an unvisited peer and a registration with no alias still match"
        );
    }

    /// 6.3.17 p.278 scopes 508 to "an exclusive or redirect registration,
    /// where all of the data is held ... in a single registered source";
    /// any other loop clears the forward set and proceeds locally
    /// (Table 6.3.18-2: the Via listing amends registration matching).
    #[test]
    fn loop_508_only_for_a_single_proxy_registration() {
        let t = antares_model::TenantId::new("default").expect("tenant");
        let h = hdrs(Some("1.1 me"));
        // single exclusive source looping back → 508
        let mut regs = vec![reg("exclusive")];
        assert!(handle_via_loop(&h, "me", &t, &mut regs).is_some());
        let mut regs = vec![reg("redirect")];
        assert!(handle_via_loop(&h, "me", &t, &mut regs).is_some());
        // inclusive loop → no 508, forwards cleared, local execution proceeds
        let mut regs = vec![reg("inclusive")];
        assert!(handle_via_loop(&h, "me", &t, &mut regs).is_none());
        assert!(regs.is_empty(), "looping forwards must be dropped");
        // a mixed set is not "a single registered source"
        let mut regs = vec![reg("exclusive"), reg("inclusive")];
        assert!(handle_via_loop(&h, "me", &t, &mut regs).is_none());
        assert!(regs.is_empty());
        // no loop → untouched
        let mut regs = vec![reg("exclusive")];
        assert!(handle_via_loop(&hdrs(None), "me", &t, &mut regs).is_none());
        assert_eq!(regs.len(), 1);
    }

    /// 4.3.6.4 / 5.2.9 localOnly: "distributed operations associated to this
    /// Context Source Registration will act only on data held directly by
    /// the registered Context Source itself" — the flag must survive
    /// registration compilation so every forward can carry local=true.
    #[test]
    fn local_only_survives_registration_compilation() {
        let st = AppState::new("me".into());
        let tenant = antares_model::TenantId::new("default").expect("tenant");
        let ctx = st.loader.core();
        for (id, local_only) in [
            ("urn:ngsi-ld:ContextSourceRegistration:lo", true),
            ("urn:ngsi-ld:ContextSourceRegistration:casc", false),
        ] {
            let mut doc = json!({
                "id": id,
                "type": "ContextSourceRegistration",
                "endpoint": "http://peer:9090",
                "information": [{"entities": [{"type": "https://uri.etsi.org/ngsi-ld/default-context/Vehicle"}]}],
            });
            if local_only {
                doc["localOnly"] = json!(true);
            }
            st.store
                .create(&tenant, Kind::Registration, id, doc)
                .expect("seed registration");
        }
        let spec = crate::csource::CsrSpec {
            types: Some(vec![
                "https://uri.etsi.org/ngsi-ld/default-context/Vehicle".into()
            ]),
            ..Default::default()
        };
        let regs = matching_regs(&st, &tenant, &spec, &ctx, &HeaderMap::new());
        let lo = |id: &str| {
            regs.iter()
                .find(|r| r.reg_id == id)
                .expect("compiled")
                .local_only
        };
        assert!(lo("urn:ngsi-ld:ContextSourceRegistration:lo"));
        assert!(!lo("urn:ngsi-ld:ContextSourceRegistration:casc"));
    }

    /// 4.3.6.2: "An auxiliary Context Source Registration never overrides
    /// data held directly within a Context Broker" — supplementary attributes
    /// are included, conflicting ones lose to the base regardless of recency.
    #[test]
    fn auxiliary_merge_supplements_but_never_overrides() {
        let attr = "https://uri.etsi.org/ngsi-ld/default-context/speed";
        let extra = "https://uri.etsi.org/ngsi-ld/default-context/color";
        let mut base = json!({
            "id": "urn:x", "type": ["T"],
            attr: [{"type": "Property", "value": 1, "modifiedAt": "2020-01-01T00:00:00Z"}]
        });
        // aux instance is FRESHER and would win a 4.5.5.3 recency merge —
        // auxiliary mode must still lose the conflict, yet supplement `color`
        let add = json!({
            "id": "urn:x", "type": ["T"],
            attr: [{"type": "Property", "value": 2, "modifiedAt": "2026-01-01T00:00:00Z"}],
            extra: [{"type": "Property", "value": "red"}]
        });
        merge_docs(&mut base, &add, true);
        assert_eq!(base[attr][0]["value"], 1, "aux must not override local");
        assert_eq!(base[extra][0]["value"], "red", "aux supplement is included");
        // the same add as a non-aux inclusive source DOES win on recency
        let mut base2 = json!({
            "id": "urn:x", "type": ["T"],
            attr: [{"type": "Property", "value": 1, "modifiedAt": "2020-01-01T00:00:00Z"}]
        });
        merge_docs(&mut base2, &add, false);
        assert_eq!(base2[attr][0]["value"], 2);
    }

    /// 4.3.6.2: "Auxiliary distributed operations are limited to context
    /// information consumption operations" — write_regs must drop a matching
    /// auxiliary registration while keeping an inclusive one.
    #[test]
    fn write_regs_exclude_auxiliary_registrations() {
        let st = AppState::new("me".into());
        let tenant = antares_model::TenantId::new("default").expect("tenant");
        let ctx = st.loader.core();
        for (id, mode) in [
            ("urn:ngsi-ld:ContextSourceRegistration:aux", "auxiliary"),
            ("urn:ngsi-ld:ContextSourceRegistration:inc", "inclusive"),
        ] {
            let doc = json!({
                "id": id,
                "type": "ContextSourceRegistration",
                "mode": mode,
                "operations": ["redirectionOps"],
                "endpoint": "http://peer:9090",
                "information": [{"entities": [{"type": "https://uri.etsi.org/ngsi-ld/default-context/Vehicle"}]}],
            });
            st.store
                .create(&tenant, Kind::Registration, id, doc)
                .expect("seed registration");
        }
        let spec = crate::csource::CsrSpec {
            types: Some(vec![
                "https://uri.etsi.org/ngsi-ld/default-context/Vehicle".into()
            ]),
            ..Default::default()
        };
        let regs = write_regs(
            &st,
            &tenant,
            &spec,
            &ctx,
            &HashMap::new(),
            &HeaderMap::new(),
        );
        let ids: Vec<&str> = regs.iter().map(|r| r.reg_id.as_str()).collect();
        assert_eq!(ids, vec!["urn:ngsi-ld:ContextSourceRegistration:inc"]);
    }

    /// 4.3.6.1: "Context Brokers shall respect" a Context Source's declared
    /// operations subset — explicit operation names and operation groups
    /// (5.2.9) both gate; anything outside the list must not be forwarded.
    #[test]
    fn operations_subset_gates_forwarding() {
        let mut r = reg("inclusive"); // ops = ["federationOps"], the 5.2.9 default
        assert!(r.supports("queryEntity"));
        assert!(r.supports("createSubscription"));
        assert!(
            !r.supports("createEntity"),
            "federationOps carries no provision operations"
        );
        r.ops = vec!["updateOps".into()];
        assert!(r.supports("updateAttrs"));
        assert!(!r.supports("queryEntity"));
        r.ops = vec!["createEntity".into()];
        assert!(r.supports("createEntity"), "explicit op name matches");
        assert!(!r.supports("deleteEntity"));
        r.ops = vec![];
        assert!(!r.supports("queryEntity"), "empty subset forwards nothing");
    }

    /// 4.3.6.1: "all constraints specified in the registration shall be
    /// respected" — a forwarded fragment is reduced to the attributes the
    /// RegistrationInfo covers; when nothing covered remains there is no
    /// forward at all (None).
    #[test]
    fn forwarded_fragment_reduced_to_registration_scope() {
        let st = AppState::new("me".into());
        let ctx = st.loader.core();
        let speed = "https://uri.etsi.org/ngsi-ld/default-context/speed";
        let mut r = reg("inclusive");
        r.attrs = Some(vec![speed.into()]);
        let obj = json!({
            "id": "urn:x", "type": "Vehicle",
            "speed": {"type": "Property", "value": 3},
            "color": {"type": "Property", "value": "red"},
            "@context": "https://example.org/ctx.jsonld"
        });
        let out = reduce_to_scope(obj.as_object().expect("obj"), &r, &ctx).expect("covered");
        let out = out.as_object().expect("out");
        assert!(out.contains_key("speed"));
        assert!(out.contains_key("id") && out.contains_key("type"));
        assert!(
            !out.contains_key("color"),
            "uncovered attribute must be dropped"
        );
        assert!(!out.contains_key("@context"));
        // nothing covered ⇒ no forwarded fragment
        let only_color = json!({
            "id": "urn:x", "type": "Vehicle",
            "color": {"type": "Property", "value": "red"}
        });
        assert!(reduce_to_scope(only_color.as_object().expect("obj"), &r, &ctx).is_none());
        // an unscoped registration (attrs: None) passes everything but @context
        r.attrs = None;
        let full = reduce_to_scope(obj.as_object().expect("obj"), &r, &ctx).expect("all");
        let full = full.as_object().expect("full");
        assert!(full.contains_key("color") && !full.contains_key("@context"));
    }

    /// 4.5.5.3 p.60 (audit V-19): "if an expiresAt DateTime is present on the
    /// Attribute and the date lies in the past, it shall be discarded" —
    /// BEFORE the observedAt/modifiedAt recency comparison.
    #[test]
    fn merge_discards_expired_instances_before_recency() {
        let attr = "https://uri.etsi.org/ngsi-ld/default-context/speed";
        let mut base = json!({
            "id": "urn:x", "type": ["T"],
            attr: [{"type": "Property", "value": 1, "modifiedAt": "2026-01-01T00:00:00Z"}]
        });
        // fresher instance, but expired → must NOT win
        let add = json!({
            "id": "urn:x", "type": ["T"],
            attr: [{"type": "Property", "value": 2,
                    "modifiedAt": "2026-06-01T00:00:00Z",
                    "expiresAt": "2020-01-01T00:00:00Z"}]
        });
        merge_docs(&mut base, &add, false);
        assert_eq!(base[attr][0]["value"], 1, "expired instance was discarded");
        // an expired BASE instance loses to a live remote one even if fresher
        let mut base2 = json!({
            "id": "urn:x", "type": ["T"],
            attr: [{"type": "Property", "value": 1,
                    "modifiedAt": "2026-06-01T00:00:00Z",
                    "expiresAt": "2020-01-01T00:00:00Z"}]
        });
        let add2 = json!({
            "id": "urn:x", "type": ["T"],
            attr: [{"type": "Property", "value": 2, "modifiedAt": "2026-01-01T00:00:00Z"}]
        });
        merge_docs(&mut base2, &add2, false);
        assert_eq!(base2[attr][0]["value"], 2, "live instance replaces expired");
    }
}
