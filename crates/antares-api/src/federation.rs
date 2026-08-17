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
#[cfg(test)]
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
/// Table 4.20-2 associationOps: federationOps WITHOUT the EntityMap support
/// operations.
const ASSOCIATION_OPS: &[&str] = &[
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
    "retrieveContextSourceIdentity",
];
/// Table 4.20-1: every named API operation (plus the 4.20-2 group names in
/// OPERATION_GROUPS) — the legal value space of the 5.2.9 `operations`
/// member.
pub(crate) const ALL_OPERATION_NAMES: &[&str] = &[
    "createEntity",
    "updateEntity",
    "appendAttrs",
    "updateAttrs",
    "deleteAttrs",
    "deleteEntity",
    "createBatch",
    "upsertBatch",
    "updateBatch",
    "deleteBatch",
    "upsertTemporal",
    "appendAttrsTemporal",
    "deleteAttrsTemporal",
    "updateAttrInstanceTemporal",
    "deleteAttrInstanceTemporal",
    "deleteTemporal",
    "mergeEntity",
    "replaceEntity",
    "replaceAttrs",
    "mergeBatch",
    "purgeEntity",
    "retrieveEntity",
    "queryEntity",
    "queryBatch",
    "retrieveTemporal",
    "queryTemporal",
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
    "createEntityMapQueryTemporal",
    "retrieveContextSourceIdentity",
];
pub(crate) const OPERATION_GROUPS: &[&str] = &[
    "federationOps",
    "associationOps",
    "updateOps",
    "retrieveOps",
    "redirectionOps",
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
    /// EntityInfo idPattern values (5.2.8 IEEE 1003.2 regexes).
    pub ent_patterns: Vec<String>,
    /// True when any matched RegistrationInfo carries an EntityInfo with
    /// neither id nor idPattern (or no entities at all) — the registration
    /// imposes no id restriction (5.12 condition 1).
    pub ent_unrestricted: bool,
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
    /// 5.2.34 timeout: "Maximum period of time in milliseconds which may
    /// elapse before a forwarded request is assumed to have failed."
    pub timeout_ms: Option<u64>,
    /// 5.2.34 cooldown: "Minimum period of time in milliseconds which shall
    /// elapse before attempting to make a subsequent forwarded request to
    /// the same endpoint after failure."
    pub cooldown_ms: Option<u64>,
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
                || (o == "associationOps" && ASSOCIATION_OPS.contains(&op))
        })
    }
    pub fn is_proxy(&self) -> bool {
        self.mode == "exclusive" || self.mode == "redirect"
    }
    /// 4.3.6.1 ("all constraints specified in the registration shall be
    /// respected" — including Entity IDs): can this registration's
    /// EntityInfo id constraints match `id`? Patterns use regex find,
    /// mirroring `entity_info_matches` (5.12).
    pub fn can_match_id(&self, id: &str) -> bool {
        self.ent_unrestricted
            || self.ent_ids.iter().any(|i| i == id)
            || self
                .ent_patterns
                .iter()
                .any(|p| crate::regexcache::compile(p).is_ok_and(|re| re.find(id).is_some()))
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
    /// 4.20 Table 4.20-1: queryEntity/queryBatch implement 5.7.2 Query
    /// Entities — retrieveEntity implements only 5.7.1, so a query is never
    /// forwarded to a source that offers retrieveEntity alone (4.3.6.1:
    /// "Context Brokers shall respect this").
    pub fn query_op(&self) -> Option<&'static str> {
        ["queryEntity", "queryBatch"]
            .into_iter()
            .find(|op| self.supports(op))
    }
    /// 4.3.6.1: the registration's EntityInfo constraints gate which payload
    /// ITEMS a distributed write may carry — an item whose present id/type
    /// the registration does not name is not this source's data. An item
    /// without a `type` member (attribute fragments) cannot be disproven and
    /// stays covered.
    pub fn covers_item(&self, obj: &Map<String, Value>, ctx: &Context) -> bool {
        if !self.ent_ids.is_empty() || !self.ent_patterns.is_empty() {
            if let Some(id) = obj.get("id").and_then(Value::as_str) {
                if !self.can_match_id(id) {
                    return false;
                }
            }
        }
        if !self.ent_types.is_empty() {
            let types: Vec<String> = match obj.get("type") {
                Some(Value::String(t)) => vec![ctx.expand_key(t)],
                Some(Value::Array(a)) => a
                    .iter()
                    .filter_map(Value::as_str)
                    .map(|t| ctx.expand_key(t))
                    .collect(),
                _ => Vec::new(),
            };
            if !types.is_empty() && !types.iter().any(|t| self.ent_types.contains(t)) {
                return false;
            }
        }
        true
    }
}

/// Is federation active for this request? (6.3.18 local param; 5.5.13:
/// a local-scope request executes only on information available locally —
/// no Context Source Registrations are considered)
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
/// RFC 7230 sections 3.2.2 and 5.7.1: Via is a list header — senders may split the
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
/// token compared for equality, never by suffix (`ends_with`
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

/// How many `Via` elements this broker will process on one inbound request.
///
/// 6.3.18 fixes the header's purpose ("to avoid infinite loops") and Table
/// 6.3.18-2 makes its listing part of registration matching, so every
/// element is compared against every candidate registration: the chain is
/// attacker-supplied input whose length multiplies the work of the request.
/// RFC 7230 section 3.2.5 lets a recipient refuse a field longer than it is
/// willing to process. A cascade deeper than this is a loop the alias
/// pseudonyms failed to name, not a deployment.
const MAX_VIA_HOPS: usize = 32;

/// The number of `Via` elements received, counted across header fields
/// without building the token list (RFC 7230 sections 3.2.2 and 5.7.1: the
/// elements of a list header may be split over any number of field lines).
fn via_hops(headers: &HeaderMap) -> usize {
    headers
        .get_all("via")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .map(|v| v.split(',').count())
        .sum()
}

/// Does the inbound Via chain already name this broker, in this tenant?
/// (loop, 6.3.18) — `alias` is always [`alias_for`]'s tenant-qualified value.
pub fn via_loop(headers: &HeaderMap, alias: &str) -> bool {
    via_hops(headers) > MAX_VIA_HOPS || via_tokens(headers).iter().any(|t| t == alias)
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
    // A chain past [`MAX_VIA_HOPS`] is refused outright: the operation is
    // not re-forwarded and not run locally either, because a request that
    // deep is a cascade the pseudonyms failed to close.
    if via_hops(headers) > MAX_VIA_HOPS {
        regs.clear();
        return Some(loop_508(tenant));
    }
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

/// Percent-encode one client-controlled value for use as a single path
/// segment of a forwarded URL (RFC 3986 clause 3.3: a segment is made of
/// `pchar`, and `/`, `?`, `#` end it). Entity ids and attribute names arrive
/// already percent-decoded from the request path, so splicing them raw would
/// let `#` or `?` truncate the forwarded path and re-target the peer's
/// resource — `.../entities/urn:x%23/attrs/speed` would reach the peer as
/// Delete Entity (5.6.6) instead of Delete Attribute (5.6.5).
pub(crate) fn path_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        let keep = b.is_ascii_alphanumeric()
            || matches!(
                b,
                b'-' | b'.'
                    | b'_'
                    | b'~'
                    | b'!'
                    | b'$'
                    | b'&'
                    | b'\''
                    | b'('
                    | b')'
                    | b'*'
                    | b'+'
                    | b','
                    | b';'
                    | b'='
                    | b':'
                    | b'@'
            );
        if keep {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The @context URL to advertise on forwarded requests.
pub fn ctx_link_url(headers: &HeaderMap, source: &Value) -> String {
    if let Some(url) = link_context(headers) {
        return url;
    }
    match source {
        Value::String(s) => s.clone(),
        // 5.5.7/6.3.5 fidelity: an inline @context has no dereferenceable
        // URL — serialize it so forward() can embed it in the body as
        // application/ld+json instead of dropping the term mappings.
        Value::Array(a) if a.iter().any(|e| !e.is_string()) => {
            serde_json::to_string(source).unwrap_or_else(|_| antares_jsonld::CORE_CONTEXT.into())
        }
        Value::Array(a) => a
            .iter()
            .find_map(|e| e.as_str())
            .unwrap_or(antares_jsonld::CORE_CONTEXT)
            .to_owned(),
        Value::Object(_) => {
            serde_json::to_string(source).unwrap_or_else(|_| antares_jsonld::CORE_CONTEXT.into())
        }
        _ => antares_jsonld::CORE_CONTEXT.to_owned(),
    }
}

/// The registration documents of one tenant: the ONE compiled mirror when
/// wired (bus=nats), the store otherwise — narrowed there by the ids and
/// Entity Types this operation names, so a broker holding a large
/// registration set does not read all of it per distributed request.
///
/// The narrowing may only ever drop registrations [`reg_candidate`] would
/// reject anyway; it is a prefilter, never the decision. So the type
/// dimension is dropped whenever a member is a 4.17 Entity Type Selection
/// (`A|B`, `(A;B)`, `*`) rather than a single type: the index compares types
/// by equality and cannot evaluate a selection, and a narrowing that
/// mis-decides would silently lose a Context Source. A plain term is
/// expanded first — the spec carries the parameter as the client wrote it
/// (`Vehicle`), the index stores what [`reg_candidate`] compares: the IRI.
fn reg_docs(
    st: &AppState,
    tenant: &TenantId,
    spec: &crate::csource::CsrSpec,
    ctx: &Context,
) -> Vec<Value> {
    match &st.reg_mirror {
        Some(m) => m.docs(tenant.as_str()),
        None => {
            let types: Option<Vec<String>> = spec
                .types
                .as_ref()
                .filter(|ts| {
                    !ts.iter()
                        .any(|t| t.contains([',', ';', '|', '(', ')', '*']))
                })
                .map(|ts| ts.iter().map(|t| ctx.expand_key(t)).collect());
            st.store
                .matching_registrations(tenant, spec.ids.as_deref(), types.as_deref())
                .unwrap_or_default()
        }
    }
}

/// Does one stored registration take part in this operation, and through
/// which RegistrationInfos (5.12)? Every condition is decided from the
/// borrowed document — expiry, csf, datasetId, location, intervals, the Via
/// chain — so a caller that only needs the verdict ([`would_federate`])
/// stops here instead of compiling a `FedReg` per registration. Expiry is
/// filtered HERE and only here: the single yield point.
fn reg_candidate<'a>(
    doc: &'a Value,
    spec: &crate::csource::CsrSpec,
    ctx: &Context,
    seen: &[String],
) -> Option<Vec<&'a Value>> {
    if crate::csource::reg_expired(doc) {
        return None;
    }
    // 5.7.2.4/5.7.4.4/5.6.21.4: a csf gates which Context Sources
    // are considered (evaluated over the registration's own
    // Context Source Properties, 5.10.2.4 semantics).
    if let Some(csf) = &spec.csf {
        if !crate::csource::csf_matches(csf, doc, ctx) {
            return None;
        }
    }
    // 5.12 datasetId condition (should-level): both sides specifying
    // datasetId match only with a value in common; one side alone always
    // matches.
    if let Some(ds) = &spec.dataset_ids {
        if let Some(reg_ds) = doc.get("datasetId").and_then(Value::as_array) {
            if !reg_ds
                .iter()
                .filter_map(Value::as_str)
                .any(|d| ds.iter().any(|q| q == d))
            {
                return None;
            }
        }
    }
    // 5.2.9 location + 4.3.6.1: a geo-scoped registration is only consulted
    // when the query's geo filter matches its geometry; a registration
    // without `location` is unconstrained.
    if let Some(gq) = &spec.geo {
        if let Some(geom) = doc.get("location") {
            if !gq.matches_geometry(geom) {
                return None;
            }
        }
    }
    // 5.2.9: a declared observation/management interval gates temporal
    // fan-out on overlap with the temporal query; without any declared
    // interval the registration is unconstrained.
    if let Some(tq) = &spec.temporal {
        if (doc.get("observationInterval").is_some() || doc.get("managementInterval").is_some())
            && !crate::csource::temporal_interval_matches(doc, tq)
        {
            return None;
        }
    }
    // Table 6.3.18-2 / 5.2.9: this source already handled the request.
    if doc
        .get("contextSourceAlias")
        .and_then(Value::as_str)
        .is_some_and(|a| seen.iter().any(|t| t == a))
    {
        return None;
    }
    let infos = crate::csource::matching_infos(spec, doc, ctx);
    if infos.is_empty() {
        None
    } else {
        Some(infos)
    }
}

/// Registrations matching an entity spec (5.12), compiled for forwarding.
///
/// Table 6.3.18-2 makes the inbound `Via` listing part of matching itself —
/// "the listing of previously encountered Context Sources supplied is used
/// when determining matching registrations" — so a registration whose
/// `contextSourceAlias` is already in the chain is filtered out HERE, at the
/// one place every read and write path resolves its candidates. Keeping it
/// out of the call sites is deliberate: a loop check the callers own is a
/// loop check some caller forgets.
pub fn matching_regs(
    st: &AppState,
    tenant: &TenantId,
    spec: &crate::csource::CsrSpec,
    ctx: &Context,
    headers: &HeaderMap,
) -> Vec<FedReg> {
    if via_hops(headers) > MAX_VIA_HOPS {
        return Vec::new();
    }
    let seen = via_tokens(headers);
    reg_docs(st, tenant, spec, ctx)
        .iter()
        .filter_map(|doc| {
            let infos = reg_candidate(doc, spec, ctx, &seen)?;
            let alias = doc
                .get("contextSourceAlias")
                .and_then(Value::as_str)
                .map(str::to_owned);
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
            let mut ent_patterns = Vec::new();
            let mut ent_unrestricted = false;
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
                        if let Some(p) = e.get("idPattern").and_then(Value::as_str) {
                            ent_patterns.push(p.to_owned());
                        }
                        // 5.12 condition 1: neither id nor idPattern ⇒ the
                        // element restricts by type only, never by id
                        if e.get("id").is_none() && e.get("idPattern").is_none() {
                            ent_unrestricted = true;
                        }
                        // 5.2.8: type may be a String or String[]
                        match e.get("type") {
                            Some(Value::String(t)) => ent_types.push(t.clone()),
                            Some(Value::Array(ts)) => ent_types
                                .extend(ts.iter().filter_map(Value::as_str).map(str::to_owned)),
                            _ => {}
                        }
                    }
                } else {
                    // an attributes-only RegistrationInfo imposes no id scope
                    ent_unrestricted = true;
                }
            }
            let tenant = doc.get("tenant").and_then(Value::as_str).map(str::to_owned);
            let csi = csi_of(doc);
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
                ent_patterns,
                ent_unrestricted,
                tenant,
                alias,
                csi,
                // 5.2.34: management.localOnly; the top-level spelling is
                // kept for compatibility (4.3.6.4 wording / older payloads)
                local_only: doc
                    .get("localOnly")
                    .and_then(Value::as_bool)
                    .or_else(|| {
                        doc.get("management")
                            .and_then(|m| m.get("localOnly"))
                            .and_then(Value::as_bool)
                    })
                    .unwrap_or(false),
                timeout_ms: doc
                    .get("management")
                    .and_then(|m| m.get("timeout"))
                    .and_then(Value::as_u64),
                cooldown_ms: doc
                    .get("management")
                    .and_then(|m| m.get("cooldown"))
                    .and_then(Value::as_u64),
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

/// Bounds wall on the forwarded-read path: a peer response larger than
/// ANTARES_MAX_FED_RESPONSE_BYTES is never held in memory — the part fails
/// exactly like an unparseable payload (Table 6.3.17-1, warning 111 via the
/// 2xx-with-null-body arm of `read_warning`).
async fn read_body_capped(resp: reqwest::Response) -> Value {
    let cap = *crate::bounds::MAX_FED_RESPONSE_BYTES;
    if resp.content_length().is_some_and(|l| l > cap as u64) {
        tracing::warn!("federation response over the {cap}-byte cap (declared length), skipped");
        return Value::Null;
    }
    #[cfg(not(target_arch = "wasm32"))]
    let bytes = {
        let mut resp = resp;
        let mut buf: Vec<u8> = Vec::new();
        loop {
            match resp.chunk().await {
                Ok(Some(c)) => {
                    if buf.len() + c.len() > cap {
                        tracing::warn!("federation response over the {cap}-byte cap, skipped");
                        return Value::Null;
                    }
                    buf.extend_from_slice(&c);
                }
                Ok(None) => break,
                Err(_) => return Value::Null,
            }
        }
        buf
    };
    // the browser fetch API hands the body over whole — cap after the read
    #[cfg(target_arch = "wasm32")]
    let bytes = match resp.bytes().await {
        Ok(b) if b.len() <= cap => b.to_vec(),
        _ => return Value::Null,
    };
    serde_json::from_slice(&bytes).unwrap_or(Value::Null)
}

/// 5.7.2.4: "If split entities flag is explicitly set to true or, if not
/// explicitly set, the default setting of the deployment allows split
/// entities" — this deployment's default is OFF, so only the explicit flag
/// engages the split branch.
pub(crate) fn split_entities(params: &HashMap<String, String>) -> bool {
    params.get("splitEntities").map(String::as_str) == Some("true")
}

/// 4.3.6.6: a registration carrying a jsonldContext contextSourceInfo key —
/// forwards to it are recompacted term-by-term (attrs/type/geoproperty only).
fn has_reg_context(reg: &FedReg) -> bool {
    reg.csi
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("jsonldContext"))
}

/// 4.3.6.1 fan-out: forwards to matching registrations run concurrently —
/// the clause fixes the merge order (4.5.5 non-aux before aux), never the
/// request order, and cross-source result ordering does not exist (the
/// `ordering` parameter is a 400 outside local scope, 5.7.2.4). Results
/// return in registration order so warning and merge processing stay
/// deterministic. Concurrency per request is bounded by
/// bounds::MAX_FED_FANOUT.
async fn fan_out<I, T, F, Fut>(items: Vec<I>, per_item: F) -> Vec<T>
where
    F: FnMut(I) -> Fut,
    Fut: std::future::Future<Output = T>,
{
    use futures_util::StreamExt;
    futures_util::stream::iter(items.into_iter().map(per_item))
        .buffered(*crate::bounds::MAX_FED_FANOUT)
        .collect()
        .await
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
    reg: &FedReg,
    ctx_url: &str,
    mut body: Option<Value>,
) -> (u16, Value, Vec<String>) {
    // 6.3.17: NGSILD-Warning values received from the peer are returned to
    // the caller — abnormal behaviour detected downstream in a cascade
    // (4.3.6.4) must surface on the aggregated response, not vanish here.
    // One policy for every outbound class — scheme allowlist,
    // private-range deny, per-destination circuit breaker.
    if let Err(e) = st.egress.check_url(&url).await {
        tracing::warn!("federation forward to {url} refused: {e}");
        return (502, Value::Null, Vec::new());
    }
    if st.egress.is_open(&url) {
        tracing::debug!("federation forward to {url} short-circuited (breaker open)");
        return (503, Value::Null, Vec::new());
    }
    // 5.2.34 cooldown (per REGISTRATION, distinct from the host:port
    // breaker): inside the declared window "a timeout error response for
    // the registration is automatically returned" — the source is not
    // contacted.
    if let Some(cd) = reg.cooldown_ms {
        if st
            .egress
            .reg_in_cooldown(&crate::egress::reg_key(tenant.as_str(), &reg.reg_id), cd)
        {
            return (504, Value::Null, Vec::new());
        }
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
    // An inline @context (serialized JSON from ctx_link_url) cannot travel
    // as a Link header — it is embedded in the body below (5.5.7/6.3.5).
    let inline_ctx: Option<Value> = if link_ctx.starts_with('[') || link_ctx.starts_with('{') {
        serde_json::from_str(&link_ctx).ok()
    } else {
        None
    };
    let mut req = st
        .fed_http
        .request(method, &url)
        .header("Accept", accept)
        .header(
            "Via",
            outbound_via(headers, &alias_for(&st.host_alias, tenant)),
        );
    if inline_ctx.is_none() {
        req = req.header(
            "Link",
            format!("<{link_ctx}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\""),
        );
    }
    if !query.is_empty() {
        req = req.query(&query);
    }
    // 4.14: "the Tenant information from the Context Source Registration has
    // to be used. If no Tenant information is present in the Context Source
    // Registration, no Tenant information is to be used and thus the default
    // Tenant is targeted" — the requesting tenant never flows through; 6.3.14
    // omits the header for the default Tenant.
    if let Some(peer_tenant) = reg.tenant.as_deref().filter(|t| *t != "default") {
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
        if let Some(ic) = &inline_ctx {
            // inline request @context: the only lossless carrier is the
            // body itself, as application/ld+json (5.5.7/6.3.5).
            if let Some(o) = b.as_object_mut() {
                o.insert("@context".into(), ic.clone());
            }
            req = req.header("Content-Type", "application/ld+json");
        } else if want_ld {
            if let Some(o) = b.as_object_mut() {
                o.insert("@context".into(), Value::String(link_ctx.clone()));
            }
            req = req.header("Content-Type", "application/ld+json");
        } else {
            req = req.header("Content-Type", "application/json");
        }
        req = req.body(serde_json::to_vec(&b).unwrap_or_default());
    }
    // The whole HTTP interaction is one Send unit (http_interaction) so
    // the handler futures above stay Send on wasm32 too.
    antares_jsonld::http_interaction(async {
        // wasm has no client-level timeout — bound the forward per request
        // (mirrors the native fed_http 8 s total); a timed-out
        // forward is the only failure class that feeds the breaker.
        // 5.2.34 timeout bounds the forward below the 8 s ceiling.
        // Natively io_deadline is a passthrough (the client owns the 8 s
        // default), so the per-registration budget rides on the request.
        let deadline: u32 = reg.timeout_ms.map_or(8_000, |t| t.min(8_000) as u32);
        #[cfg(not(target_arch = "wasm32"))]
        let req = req.timeout(std::time::Duration::from_millis(deadline as u64));
        let sent = antares_jsonld::io_deadline(req.send(), deadline).await;
        let sent = match sent {
            Some(r) => r,
            None => {
                st.egress.record_failure(&url);
                if reg.cooldown_ms.is_some() {
                    st.reg_cooldown_stamp(tenant, &reg.reg_id, false);
                }
                return (504, Value::Null, Vec::new());
            }
        };
        match sent {
            Ok(resp) => {
                // Any response — even 5xx — proves the peer answers within
                // its own response time: no deadline cost, so no breaker
                // state. Only TIMEOUT-class failures trip; a
                // responding-but-erroring peer must keep being attempted,
                // else unrelated registrations sharing its host:port starve.
                let status = resp.status().as_u16();
                st.egress.record_success(&url);
                if reg.cooldown_ms.is_some() {
                    // 5.2.9 Table 5.2.9-2 failure definition: any response
                    // code other than 2xx
                    st.reg_cooldown_stamp(tenant, &reg.reg_id, (200..300).contains(&status));
                }
                let peer_warnings: Vec<String> = resp
                    .headers()
                    .get_all("NGSILD-Warning")
                    .iter()
                    .filter_map(|v| v.to_str().ok().map(str::to_owned))
                    .collect();
                let body = read_body_capped(resp).await;
                (status, body, peer_warnings)
            }
            Err(e) if e.is_timeout() => {
                st.egress.record_failure(&url);
                if reg.cooldown_ms.is_some() {
                    st.reg_cooldown_stamp(tenant, &reg.reg_id, false);
                }
                (504, Value::Null, Vec::new())
            }
            Err(_) => {
                // connect refused/reset: fails in milliseconds — no deadline cost;
                // clearing avoids stale suppression of a restarted peer.
                // 503 (not 502): NO HTTP response was received, so the read
                // path classifies it under Table 6.3.17-1 code 199 ("No
                // response was received from the registration endpoint"),
                // never 299 ("An error response ... was received").
                st.egress.record_success(&url);
                if reg.cooldown_ms.is_some() {
                    st.reg_cooldown_stamp(tenant, &reg.reg_id, false);
                }
                (503, Value::Null, Vec::new())
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
        // The scope narrows ATTRIBUTES (5.2.9 `attrs`); the entity-level
        // members stay, or 4.5.5.3 would read the missing expiresAt as
        // "absent from a received version" and drop the local one.
        if [
            "id",
            "type",
            "scope",
            "createdAt",
            "modifiedAt",
            "expiresAt",
            "deletedAt",
        ]
        .contains(&k.as_str())
            || scope.iter().any(|s| s == k)
        {
            out.insert(k.clone(), v.clone());
        }
    }
    Some(Value::Object(out))
}

/// Expand + registration-scope-filter one remote temporal entity (5.7.3.4).
/// Instances of one datasetId legally repeat in a Temporal Evolution, so
/// expansion runs in temporal mode.
fn import_temporal(remote: &Value, reg: &FedReg, ctx: &Context) -> Option<Value> {
    let mut obj = remote.as_object()?.clone();
    obj.remove("@context");
    let expanded = antares_jsonld::expand_entity(
        &obj,
        ctx,
        antares_jsonld::ExpandOpts {
            sys: true,
            temporal: true,
            // 4.5.7: deletion instances (value urn:ngsi-ld:null +
            // deletedAt) are part of a Temporal Evolution — a remote
            // tombstone must import, not be dropped as an invalid payload.
            allow_null: true,
            ..Default::default()
        },
    )
    .ok()?;
    let Some(scope) = &reg.attrs else {
        return Some(expanded);
    };
    let mut out = Map::new();
    for (k, v) in expanded.as_object()? {
        if [
            "id",
            "type",
            "scope",
            "createdAt",
            "modifiedAt",
            "expiresAt",
            "deletedAt",
        ]
        .contains(&k.as_str())
            || scope.iter().any(|s| s == k)
        {
            out.insert(k.clone(), v.clone());
        }
    }
    Some(Value::Object(out))
}

/// 5.7.3.4: forward Retrieve Temporal Evolution to matching registrations
/// that support the retrieveTemporal operation; registrations without it
/// are not contacted. Returns (auxiliary, expanded doc) pairs for the
/// caller's 4.5.5 merge.
/// 4.20: does a raw registration document support `op`? Same group tables
/// as FedReg::supports; default when the operations member is absent is
/// federationOps (5.2.9).
pub(crate) fn doc_supports(reg: &Value, op: &str) -> bool {
    fed_reg_of(reg.get("id").and_then(Value::as_str).unwrap_or(""), reg).supports(op)
}

/// A minimal FedReg view of a raw registration document — enough for
/// `forward` (endpoint/tenant/csi/alias) and `supports`.
/// 4.3.6.6 contextSourceInfo, as the key/value pairs a forward applies.
fn csi_of(reg: &Value) -> Vec<(String, String)> {
    reg.get("contextSourceInfo")
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
        .unwrap_or_default()
}

pub(crate) fn fed_reg_of(reg_id: &str, reg: &Value) -> FedReg {
    let endpoint = reg
        .get("endpoint")
        .and_then(Value::as_str)
        .map(|e| {
            let e = e.trim_end_matches('/');
            e.strip_suffix("/ngsi-ld/v1").unwrap_or(e).to_owned()
        })
        .unwrap_or_default();
    FedReg {
        reg_id: reg_id.to_owned(),
        endpoint,
        mode: reg
            .get("mode")
            .and_then(Value::as_str)
            .unwrap_or("inclusive")
            .to_owned(),
        ops: reg
            .get("operations")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_else(|| vec!["federationOps".into()]),
        attrs: None,
        ent_ids: Vec::new(),
        ent_types: Vec::new(),
        ent_patterns: Vec::new(),
        // minimal view: no EntityInfo scope loaded ⇒ never narrow by it
        ent_unrestricted: true,
        tenant: reg.get("tenant").and_then(Value::as_str).map(str::to_owned),
        alias: reg
            .get("contextSourceAlias")
            .and_then(Value::as_str)
            .map(str::to_owned),
        // 4.3.6.6: the registered headers (auth among them) travel with every
        // forward, including the subscription operations that build their
        // registration view from this function.
        csi: csi_of(reg),
        local_only: false,
        timeout_ms: reg
            .get("management")
            .and_then(|m| m.get("timeout"))
            .and_then(Value::as_u64),
        cooldown_ms: reg
            .get("management")
            .and_then(|m| m.get("cooldown"))
            .and_then(Value::as_u64),
    }
}

/// 5.7.1.4 / 5.7.3.4: with an EntityMap in use, "only the retrieved Entity
/// Map shall be used to determine which Context Source Registrations match
/// the Entity ID" — a registration not listed in the entry does not match;
/// "the location of the linked EntityMap shall be passed as part of any
/// forwarded request" (conveyed as an extra header on the forward).
fn map_gate(mut reg: FedReg, map: Option<&Value>, id: &str) -> Option<FedReg> {
    let Some(m) = map else { return Some(reg) };
    let listed = m
        .get("entityMap")
        .and_then(|e| e.get(id))
        .and_then(Value::as_array)
        .is_some_and(|a| a.iter().any(|v| v.as_str() == Some(reg.reg_id.as_str())));
    if !listed {
        return None;
    }
    if let Some(mid) = m
        .get("linkedMaps")
        .and_then(|l| l.get(&reg.reg_id))
        .and_then(Value::as_str)
    {
        reg.csi.push(("NGSILD-EntityMap".into(), mid.to_owned()));
    }
    Some(reg)
}

#[allow(clippy::too_many_arguments)] // mirrors the wire: one param per forwarded request part
pub async fn fed_retrieve_temporal(
    st: &AppState,
    tenant: &TenantId,
    headers: &HeaderMap,
    ctx: &Context,
    id: &str,
    params: &HashMap<String, String>,
    map: Option<&Value>,
    warnings: &mut Vec<String>,
) -> Vec<(bool, Value)> {
    let spec = crate::csource::CsrSpec {
        ids: Some(vec![id.to_owned()]),
        temporal: crate::temporal::TemporalQ::from_params(params, false)
            .ok()
            .flatten(),
        ..Default::default()
    };
    let ctx_url = ctx_link_url(headers, &ctx.source);
    let ctx_url = &ctx_url;
    let regs: Vec<FedReg> = matching_regs(st, tenant, &spec, ctx, headers)
        .into_iter()
        // 5.7.3.4: a live EntityMap in use is the ONLY source of matching
        // registrations; its linked map location travels with the forward.
        .filter_map(|reg| map_gate(reg, map, id))
        .filter(|reg| reg.supports("retrieveTemporal"))
        .collect();
    let fetched = fan_out(regs, move |reg| async move {
        // the temporal window travels with the forward; sysAttrs for the
        // 4.5.5.3 recency arbitration
        let mut query: Vec<(String, String)> = vec![("options".into(), "sysAttrs".into())];
        for k in ["timerel", "timeAt", "endTimeAt", "timeproperty", "lastN"] {
            if let Some(v) = params.get(k) {
                query.push((k.into(), v.clone()));
            }
        }
        if let Some(scope) = &reg.attrs {
            let names: Vec<String> = scope.iter().map(|a| ctx.compact_iri(a)).collect();
            query.push(("attrs".into(), names.join(",")));
        }
        let (status, body, peer_warns) = forward(
            st,
            reqwest::Method::GET,
            format!(
                "{}/ngsi-ld/v1/temporal/entities/{}",
                reg.endpoint,
                path_segment(id)
            ),
            &query,
            headers,
            tenant,
            &reg,
            ctx_url,
            None,
        )
        .await;
        (reg, status, body, peer_warns)
    })
    .await;
    let mut out = Vec::new();
    for (reg, status, body, peer_warns) in fetched {
        warnings.extend(peer_warns);
        if let Some((code, text)) = read_warning(status, &body) {
            warnings.push(warning(code, &alias_for(&st.host_alias, tenant), text));
        }
        if !(200..300).contains(&status) {
            continue;
        }
        if body.get("id").and_then(Value::as_str) != Some(id) {
            continue;
        }
        match import_temporal(&body, &reg, ctx) {
            Some(doc) => out.push((reg.mode == "auxiliary", doc)),
            None => warnings.push(warning(
                111,
                &alias_for(&st.host_alias, tenant),
                "the payload of the response was invalid",
            )),
        }
    }
    out
}

/// The DateTime 4.5.5.3 arbitrates on, as a comparable key: 4.6.3 admits the
/// same instant with or without a fraction, so a raw string compare would
/// rank `…:01.500Z` below `…:01Z`.
fn recency(inst: &Value) -> String {
    crate::temporal::dt_key(
        inst.get("observedAt")
            .or_else(|| inst.get("modifiedAt"))
            .and_then(Value::as_str)
            .unwrap_or(""),
    )
}

/// 4.5.5.2 Processing of Conflicting Transient Entities: for each received
/// Entity version with an entity-level expiresAt, add it as a non-reified
/// expiresAt on every Attribute instance that lacks one, and cap any
/// Attribute-level expiresAt further in the future to the entity's (earlier)
/// DateTime.
fn push_down_expires(doc: &mut Value) {
    let Some(o) = doc.as_object_mut() else { return };
    let Some(exp) = o.get("expiresAt").and_then(Value::as_str).map(String::from) else {
        return;
    };
    for (k, v) in o.iter_mut() {
        if matches!(
            k.as_str(),
            "id" | "type" | "scope" | "expiresAt" | "createdAt" | "modifiedAt" | "deletedAt"
        ) {
            continue;
        }
        let Some(instances) = v.as_array_mut() else {
            continue;
        };
        for inst in instances.iter_mut().filter_map(Value::as_object_mut) {
            match inst.get("expiresAt").and_then(Value::as_str) {
                Some(ae) if crate::temporal::dt_key(ae) <= crate::temporal::dt_key(&exp) => {}
                _ => {
                    inst.insert("expiresAt".into(), Value::String(exp.clone()));
                }
            }
        }
    }
}

/// 4.5.5.3 first step: "if an expiresAt DateTime is present on
/// the Attribute and the date lies in the past, it shall be discarded" —
/// BEFORE any recency comparison.
fn expired(inst: &Value, now: &str) -> bool {
    inst.get("expiresAt")
        .and_then(Value::as_str)
        .is_some_and(|e| crate::temporal::dt_key(e) < crate::temporal::dt_key(now))
}

/// 4.3.6.2: "An auxiliary Context Source Registration never overrides data
/// held directly within a Context Broker. […] Context data from auxiliary
/// context sources is only included if it is supplementary."
/// Merge attributes of `add` into `base` (auxiliary sources never override —
/// base wins; otherwise conflicting instances resolve per 4.5.5.3: discard
/// past-expiresAt instances first, then most recent observedAt/modifiedAt).
pub fn merge_docs(base: &mut Value, add: &Value, aux: bool) {
    let now = crate::state::now_iso();
    let mut add = add.clone();
    push_down_expires(&mut add);
    let Some(bo) = base.as_object_mut() else {
        return;
    };
    let Some(ao) = add.as_object() else { return };
    // 4.5.5.3: entity-level expiresAt — "missing from at least one version of
    // the Entity received" → removed; present in all versions → the DateTime
    // furthest in the future. 4.3.6.2 keeps auxiliary versions out of this:
    // they never override data the broker holds directly, and removing or
    // extending the lifetime is an override.
    if !aux {
        match (
            bo.get("expiresAt").and_then(Value::as_str),
            ao.get("expiresAt").and_then(Value::as_str),
        ) {
            (Some(b), Some(a)) => {
                if crate::temporal::dt_key(a) > crate::temporal::dt_key(b) {
                    bo.insert("expiresAt".into(), Value::String(a.to_owned()));
                }
            }
            _ => {
                bo.remove("expiresAt");
            }
        }
    }
    for (k, v) in ao {
        if k == "expiresAt" {
            continue; // resolved above
        }
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
#[allow(clippy::too_many_arguments)] // mirrors the wire: one param per forwarded request part
pub async fn fed_retrieve(
    st: &AppState,
    tenant: &TenantId,
    headers: &HeaderMap,
    ctx: &Context,
    id: &str,
    map: Option<&Value>,
    except_reg: Option<&str>,
    warnings: &mut Vec<String>,
) -> Vec<(bool, Value)> {
    let spec = crate::csource::CsrSpec {
        ids: Some(vec![id.to_owned()]),
        ..Default::default()
    };
    let ctx_url = ctx_link_url(headers, &ctx.source);
    let ctx_url = &ctx_url;
    let regs: Vec<FedReg> = matching_regs(st, tenant, &spec, ctx, headers)
        .into_iter()
        // 5.8.6 splitEntities merge: "except for the one from which the
        // Notification has been received"
        .filter(|reg| !except_reg.is_some_and(|x| x == reg.reg_id))
        // 5.7.1.4: a live EntityMap in use is the ONLY source of matching
        // registrations; its linked map location travels with the forward.
        .filter_map(|reg| map_gate(reg, map, id))
        .filter(|reg| reg.read_op().is_some())
        .collect();
    let fetched = fan_out(regs, move |reg| async move {
        let Some(op) = reg.read_op() else {
            return (reg, 0, Value::Null, Vec::new());
        };
        // sysAttrs on every forwarded read: conflicting instances resolve by
        // most recent observedAt/modifiedAt (4.5.5.3) — without the remote
        // modifiedAt the winner would be arrival order, i.e. indeterminate.
        let mut query: Vec<(String, String)> = vec![("options".into(), "sysAttrs".into())];
        if let Some(scope) = &reg.attrs {
            let names: Vec<String> = scope.iter().map(|a| ctx.compact_iri(a)).collect();
            query.push(("attrs".into(), names.join(",")));
        }
        let (status, body, peer_warns) = match op {
            "retrieveEntity" => {
                forward(
                    st,
                    reqwest::Method::GET,
                    format!("{}/ngsi-ld/v1/entities/{}", reg.endpoint, path_segment(id)),
                    &query,
                    headers,
                    tenant,
                    &reg,
                    ctx_url,
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
                    ctx_url,
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
                    ctx_url,
                    Some(json!({"type": "Query", "entities": [Value::Object(sel)]})),
                )
                .await
            }
        };
        (reg, status, body, peer_warns)
    })
    .await;
    let mut out = Vec::new();
    for (reg, status, body, peer_warns) in fetched {
        warnings.extend(peer_warns);
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
    // 4.17: the parameter is ONE Entity Type Selection, so it travels whole —
    // splitting it on commas turned a conjunction like (A;B) into two terms
    // that match nothing, and no registration was consulted for it.
    let types: Option<Vec<String>> = params.get("type").cloned().map(|s| vec![s]);
    let ids: Option<Vec<String>> = params
        .get("id")
        .map(|s| s.split(',').map(str::to_owned).collect());
    crate::csource::CsrSpec {
        types,
        ids,
        // 5.12: "the id pattern (if present)" is part of the query-side
        // Entity specification matched against EntityInfo elements
        id_pattern: params.get("idPattern").cloned(),
        // 5.12 attribute conditions: the "list of Attribute names (if
        // present)" gates which RegistrationInfos match
        attrs: params
            .get("attrs")
            .map(|s| s.split(',').map(|a| ctx.expand_key(a.trim())).collect()),
        // 5.12 datasetId condition (should-level): disjoint sets don't match
        dataset_ids: params
            .get("datasetId")
            .map(|s| s.split(',').map(|d| d.trim().to_owned()).collect()),
        csf: params.get("csf").and_then(|c| antares_ql::parse_q(c).ok()),
        geo: crate::geo::GeoQuery::from_params(params).ok().flatten(),
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
    if !active(params) || via_hops(headers) > MAX_VIA_HOPS {
        return false;
    }
    // the verdict only — no forwarding set is compiled for it
    let spec = query_spec(ctx, params);
    let seen = via_tokens(headers);
    reg_docs(st, tenant, &spec, ctx)
        .iter()
        .any(|doc| reg_candidate(doc, &spec, ctx, &seen).is_some())
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
    let ctx_url = &ctx_url;
    let regs: Vec<FedReg> = matching_regs(st, tenant, &spec, ctx, headers)
        .into_iter()
        .filter(|r| r.query_op().is_some())
        .collect();
    let fetched = fan_out(regs, move |reg| async move {
        let Some(op) = reg.query_op() else {
            return (reg, 0, Value::Null, Vec::new());
        };
        // The forwarded selection is decided ONCE and then rendered either as
        // query parameters (Query Entities, 5.7.2) or as a Query body
        // (5.2.23) — the two must ask the peer the same question.
        //
        // 4.3.6.1: the forwarded id list carries only ids this registration
        // can match — never the full client list.
        let ids: Option<String> = match params.get("id") {
            Some(ids) => {
                let keep: Vec<&str> = ids
                    .split(',')
                    .filter(|i| reg.can_match_id(i.trim()))
                    .collect();
                (!keep.is_empty()).then(|| keep.join(","))
            }
            // the registration is scoped to exact ids only — ask for those
            None if !reg.ent_unrestricted
                && reg.ent_patterns.is_empty()
                && !reg.ent_ids.is_empty() =>
            {
                Some(reg.ent_ids.join(","))
            }
            None => None,
        };
        let attrs: Option<String> = match &reg.attrs {
            Some(scope) => Some(
                scope
                    .iter()
                    .map(|a| ctx.compact_iri(a))
                    .collect::<Vec<String>>()
                    .join(","),
            ),
            None => params.get("attrs").cloned(),
        };
        // 5.7.2.4: with split entities the filters "shall be removed
        // before forwarding" and re-applied on the aggregate (which the
        // local re-check always does); otherwise the request is forwarded
        // WITH its filters, so the peer returns its filtered subset
        // instead of everything. A registered jsonldContext (4.3.6.6)
        // recompacts only attrs/type/geoproperty — q/scopeQ terms cannot
        // be recompacted, so push-down is skipped there rather than
        // filtering at the remote against the wrong terms.
        let push_filters = !split_entities(params) && !has_reg_context(&reg);
        let filter = |k: &str| {
            push_filters
                .then(|| params.get(k))
                .flatten()
                .map(String::to_owned)
        };
        let (status, body, peer_warns) = if op == "queryBatch" {
            let mut sel = Map::new();
            if let Some(t) = params.get("type") {
                sel.insert("type".into(), Value::String(t.clone()));
            }
            match &ids {
                // 5.2.33: `id` is one URI or an array of them, and it takes
                // precedence over idPattern — so a pattern only travels when
                // no id list survived the narrowing.
                Some(list) if list.contains(',') => {
                    let arr: Vec<Value> =
                        list.split(',').map(|i| Value::String(i.into())).collect();
                    sel.insert("id".into(), Value::Array(arr));
                }
                Some(one) => {
                    sel.insert("id".into(), Value::String(one.clone()));
                }
                None => {
                    if let Some(p) = params.get("idPattern") {
                        sel.insert("idPattern".into(), Value::String(p.clone()));
                    }
                }
            }
            let mut q_body = Map::new();
            q_body.insert("type".into(), Value::String("Query".into()));
            if !sel.is_empty() {
                q_body.insert("entities".into(), json!([Value::Object(sel)]));
            }
            if let Some(a) = &attrs {
                let list: Vec<Value> = a.split(',').map(|n| Value::String(n.into())).collect();
                q_body.insert("attrs".into(), Value::Array(list));
            }
            if let Some(q) = filter("q") {
                q_body.insert("q".into(), Value::String(q));
            }
            if let Some(s) = filter("scopeQ") {
                q_body.insert("scopeQ".into(), Value::String(s));
            }
            let mut geo = Map::new();
            for k in ["georel", "geometry", "coordinates", "geoproperty"] {
                if let Some(v) = filter(k) {
                    // 5.2.13 GeoQuery carries coordinates as the GeoJSON
                    // value, not as the query-string spelling of it.
                    let parsed = if k == "coordinates" {
                        serde_json::from_str(&v).unwrap_or(Value::String(v))
                    } else {
                        Value::String(v)
                    };
                    geo.insert(k.into(), parsed);
                }
            }
            if !geo.is_empty() {
                q_body.insert("geoQ".into(), Value::Object(geo));
            }
            forward(
                st,
                reqwest::Method::POST,
                format!("{}/ngsi-ld/v1/entityOperations/query", reg.endpoint),
                &[("options".into(), "sysAttrs".into())],
                headers,
                tenant,
                &reg,
                ctx_url,
                Some(Value::Object(q_body)),
            )
            .await
        } else {
            let mut query: Vec<(String, String)> = vec![("options".into(), "sysAttrs".into())];
            if let Some(t) = params.get("type") {
                query.push(("type".into(), t.clone()));
            }
            if let Some(list) = &ids {
                query.push(("id".into(), list.clone()));
            }
            if let Some(a) = &attrs {
                query.push(("attrs".into(), a.clone()));
            }
            for k in [
                "q",
                "georel",
                "geometry",
                "coordinates",
                "geoproperty",
                "scopeQ",
            ] {
                if let Some(v) = filter(k) {
                    query.push((k.into(), v));
                }
            }
            forward(
                st,
                reqwest::Method::GET,
                format!("{}/ngsi-ld/v1/entities", reg.endpoint),
                &query,
                headers,
                tenant,
                &reg,
                ctx_url,
                None,
            )
            .await
        };
        (reg, status, body, peer_warns)
    })
    .await;
    let mut out = Vec::new();
    for (reg, status, body, peer_warns) in fetched {
        warnings.extend(peer_warns);
        // V-14: same NGSILD-Warning classification as fed_retrieve (6.3.17)
        if let Some((code, text)) = read_warning(status, &body) {
            warnings.push(warning(code, &alias_for(&st.host_alias, tenant), text));
        }
        if !(200..300).contains(&status) {
            continue;
        }
        if let Value::Array(a) = &body {
            // A source only speaks for the entities its registration covers:
            // an id outside that scope is dropped rather than merged, or a
            // peer could overwrite unrelated local attributes on recency.
            for c in a.iter().filter(|c| {
                c.get("id")
                    .and_then(Value::as_str)
                    .is_some_and(|i| reg.can_match_id(i))
            }) {
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

/// 5.14.4.4 / 5.14.5.4: forward EntityMap creation to matching registrations
/// that support `op` (createEntityMapQueryEntity / …QueryTemporal, 4.20);
/// with split entities in play the value/geo/scope filters are removed
/// before forwarding. Returns (registration id, returned EntityMap) pairs —
/// the caller merges them into the local map's entityMap/linkedMaps.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn fed_entity_maps(
    st: &AppState,
    tenant: &TenantId,
    headers: &HeaderMap,
    ctx: &Context,
    params: &HashMap<String, String>,
    split: bool,
    op: &str,
    path: &str,
) -> Vec<(String, Value)> {
    let spec = query_spec(ctx, params);
    let ctx_url = ctx_link_url(headers, &ctx.source);
    let ctx_url = &ctx_url;
    let regs: Vec<FedReg> = matching_regs(st, tenant, &spec, ctx, headers)
        .into_iter()
        .filter(|reg| reg.supports(op))
        .collect();
    let fetched = fan_out(regs, move |reg| async move {
        let mut query: Vec<(String, String)> = Vec::new();
        for k in [
            "id",
            "idPattern",
            "type",
            "timerel",
            "timeAt",
            "endTimeAt",
            "timeproperty",
            "lastN",
        ] {
            if let Some(v) = params.get(k) {
                query.push((k.to_owned(), v.clone()));
            }
        }
        if !split {
            for k in [
                "attrs",
                "q",
                "georel",
                "geometry",
                "coordinates",
                "geoproperty",
                "scopeQ",
                "lang",
            ] {
                if let Some(v) = params.get(k) {
                    query.push((k.to_owned(), v.clone()));
                }
            }
        }
        let (status, body, _) = forward(
            st,
            reqwest::Method::GET,
            format!("{}/ngsi-ld/v1/{path}", reg.endpoint),
            &query,
            headers,
            tenant,
            &reg,
            ctx_url,
            None,
        )
        .await;
        (reg, status, body)
    })
    .await;
    let mut out = Vec::new();
    for (reg, status, body) in fetched {
        if (200..300).contains(&status) && body.get("entityMap").is_some() {
            out.push((reg.reg_id.clone(), body));
        }
    }
    out
}

/// 5.7.4.4: forward the temporal query to matching registrations that
/// support the queryTemporal operation; registrations without it are not
/// contacted. Returns (auxiliary, expanded doc) pairs.
pub async fn fed_query_temporal(
    st: &AppState,
    tenant: &TenantId,
    headers: &HeaderMap,
    ctx: &Context,
    params: &HashMap<String, String>,
    warnings: &mut Vec<String>,
) -> Vec<(bool, Value)> {
    let mut spec = query_spec(ctx, params);
    spec.temporal = crate::temporal::TemporalQ::from_params(params, false)
        .ok()
        .flatten();
    let ctx_url = ctx_link_url(headers, &ctx.source);
    let ctx_url = &ctx_url;
    let regs: Vec<FedReg> = matching_regs(st, tenant, &spec, ctx, headers)
        .into_iter()
        .filter(|reg| reg.supports("queryTemporal"))
        .collect();
    let fetched = fan_out(regs, move |reg| async move {
        let mut query: Vec<(String, String)> = vec![("options".into(), "sysAttrs".into())];
        for k in [
            "type",
            "id",
            "idPattern",
            "timerel",
            "timeAt",
            "endTimeAt",
            "timeproperty",
            "lastN",
        ] {
            if let Some(v) = params.get(k) {
                query.push((k.into(), v.clone()));
            }
        }
        // 5.7.4.4 mirrors 5.7.2.4: with split entities the value/geo/scope
        // filters are stripped from the forward and applied on the
        // aggregate; a registered jsonldContext cannot recompact q/scopeQ
        // terms, so push-down is skipped there too.
        if !split_entities(params) && !has_reg_context(&reg) {
            for k in [
                "q",
                "georel",
                "geometry",
                "coordinates",
                "geoproperty",
                "scopeQ",
            ] {
                if let Some(v) = params.get(k) {
                    query.push((k.into(), v.clone()));
                }
            }
        }
        if let Some(scope) = &reg.attrs {
            let names: Vec<String> = scope.iter().map(|a| ctx.compact_iri(a)).collect();
            query.push(("attrs".into(), names.join(",")));
        } else if let Some(a) = params.get("attrs") {
            query.push(("attrs".into(), a.clone()));
        }
        let (status, body, peer_warns) = forward(
            st,
            reqwest::Method::GET,
            format!("{}/ngsi-ld/v1/temporal/entities", reg.endpoint),
            &query,
            headers,
            tenant,
            &reg,
            ctx_url,
            None,
        )
        .await;
        (reg, status, body, peer_warns)
    })
    .await;
    let mut out = Vec::new();
    for (reg, status, body, peer_warns) in fetched {
        warnings.extend(peer_warns);
        if let Some((code, text)) = read_warning(status, &body) {
            warnings.push(warning(code, &alias_for(&st.host_alias, tenant), text));
        }
        if !(200..300).contains(&status) {
            continue;
        }
        if let Value::Array(a) = &body {
            // Same scope gate as the non-temporal query fan-out.
            for c in a.iter().filter(|c| {
                c.get("id")
                    .and_then(Value::as_str)
                    .is_some_and(|i| reg.can_match_id(i))
            }) {
                match import_temporal(c, &reg, ctx) {
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
            // 5.6.1.4/5.6.2…: an unsupported-operation part is the Conflict
            // error type; an entity-exists part stays AlreadyExists.
            409 if p.detail.contains("does not accept") => ("Conflict", "Conflict"),
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
    // an item outside the registration's EntityInfo ids/types is not this
    // source's data at all — nothing of it may be forwarded there
    if !reg.covers_item(obj, ctx) {
        return None;
    }
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
    let (status, _, _) = forward(
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
    // 6.3.17: "the error response should be as informative as possible" —
    // informative about the operation, not about the deployment. The part is
    // named by its Context Source Registration id (5.2.9), which the client
    // can already read from /csourceRegistrations; the registered endpoint
    // is internal topology and stays in the log, so that a client able to
    // provoke a partial failure cannot enumerate the peers.
    tracing::debug!("distributed operation to {url} returned {status}");
    let detail = format!(
        "distributed operation to registration {} returned {status}",
        reg.reg_id
    );
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
        // 5.6.2.4 (and the sibling attribute operations): a proxy-mode
        // registration not supporting the operation is an error of type
        // Conflict and is never contacted; an inclusive one is simply not
        // forwarded.
        if !reg.supports(op) {
            if reg.is_proxy() {
                parts.push(conflict_part(op));
            } else {
                // status 0 = "not forwarded" sentinel: keeps the parts list
                // 1:1 with regs (combine_attr_parts zips them) without
                // counting as success or failure.
                parts.push(Part {
                    status: 0,
                    detail: format!("not forwarded: {op} not supported"),
                });
            }
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
        // a proxy only owns the attribute if this ITEM is within its
        // EntityInfo constraints (4.3.6.1) — a type-scoped registration
        // must not strip attributes from an unrelated entity
        if proxies
            .iter()
            .any(|r| r.covers_item(obj, ctx) && r.covers_attr(&iri))
        {
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

    /// 4.3.6.6: contextSourceInfo carries the headers a source needs to
    /// answer at all (an API key, say). The minimal registration view built
    /// for the subscription operations dropped them, so every forwarded
    /// subscription create, update and delete went out unauthenticated.
    #[test]
    fn the_minimal_registration_view_keeps_context_source_info() {
        let doc = json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:csi",
            "endpoint": "http://peer:9090",
            "contextSourceInfo": [
                {"key": "X-API-Key", "value": "s3cret"},
                {"key": "jsonldContext", "value": "https://example.org/ctx.jsonld"}
            ]
        });
        let reg = fed_reg_of("urn:ngsi-ld:ContextSourceRegistration:csi", &doc);
        assert_eq!(
            reg.csi,
            vec![
                ("X-API-Key".to_owned(), "s3cret".to_owned()),
                (
                    "jsonldContext".to_owned(),
                    "https://example.org/ctx.jsonld".to_owned()
                ),
            ]
        );
    }

    /// 4.17: `type` is one Entity Type Selection. Splitting it on commas
    /// destroys a conjunction, and the shared evaluator (which csource tests
    /// against real registrations) never sees the expression the client sent.
    #[test]
    fn the_type_selection_reaches_registration_matching_whole() {
        let ctx = antares_jsonld::Loader::new().core();
        let mut params = HashMap::new();
        params.insert("type".to_owned(), "(Home;Vehicle),Building".to_owned());
        let spec = query_spec(&ctx, &params);
        assert_eq!(
            spec.types,
            Some(vec!["(Home;Vehicle),Building".to_owned()]),
            "the selection travels as one expression"
        );
    }

    /// RFC 3986 clause 3.3: `#`, `?` and `/` end a path segment, so a client
    /// id carrying one would re-target the peer's resource. The characters an
    /// NGSI-LD id legitimately uses (`urn:`, `:`, `-`) must survive unchanged
    /// or every forward would address a different entity than the client did.
    #[test]
    fn path_segment_encodes_what_would_end_the_segment() {
        assert_eq!(
            path_segment("urn:ngsi-ld:Vehicle:A4567-W"),
            "urn:ngsi-ld:Vehicle:A4567-W"
        );
        assert_eq!(path_segment("urn:x#"), "urn:x%23");
        assert_eq!(path_segment("urn:x?q=1"), "urn:x%3Fq=1");
        assert_eq!(path_segment("a/b"), "a%2Fb");
        // an id already carrying a percent must not decode twice at the peer
        assert_eq!(path_segment("a%2Fb"), "a%252Fb");
        // `.` is unreserved, so dot segments pass through unchanged here —
        // they are refused at the door instead (EntityId::new, check_attr_name)
        assert_eq!(path_segment(".."), "..");
        // non-ASCII is percent-encoded per its UTF-8 bytes
        assert_eq!(path_segment("é"), "%C3%A9");
    }

    /// RFC 7230 received-by is a TOKEN compared for equality:
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
            ent_patterns: vec![],
            ent_unrestricted: false,
            tenant: None,
            alias: None,
            csi: vec![],
            local_only: false,
            timeout_ms: None,
            cooldown_ms: None,
        }
    }

    /// 4.3.6.1/5.12: an idPattern-scoped registration gates payload items
    /// exactly like an exact-id one — a foreign-razidlo item is not this
    /// source's data; an id-less fragment cannot be disproven.
    #[test]
    fn covers_item_honours_entityinfo_id_patterns() {
        let mut r = reg("redirect");
        r.ent_patterns = vec!["^urn:ngsi-ld:V:sk_bb:.*$".into()];
        let st = AppState::new("me".into());
        let ctx = st.loader.core();
        let item = |id: Option<&str>| {
            let mut m = Map::new();
            if let Some(id) = id {
                m.insert("id".into(), Value::String(id.into()));
            }
            m
        };
        assert!(r.covers_item(&item(Some("urn:ngsi-ld:V:sk_bb:1")), &ctx));
        assert!(
            !r.covers_item(&item(Some("urn:ngsi-ld:V:sk_po:1")), &ctx),
            "a foreign-razidlo item must not be covered"
        );
        assert!(
            r.covers_item(&item(None), &ctx),
            "id-less fragments stay covered"
        );
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

    /// Table 5.2.34-1: management.localOnly — "distributed operations
    /// associated to this Context Source Registration will act only on data
    /// held directly by the registered Context Source itself".
    #[test]
    fn management_local_only_survives_registration_compilation() {
        let st = AppState::new("me".into());
        let tenant = antares_model::TenantId::new("default").expect("tenant");
        let ctx = st.loader.core();
        let id = "urn:ngsi-ld:ContextSourceRegistration:mgmt-lo";
        let doc = json!({
            "id": id,
            "type": "ContextSourceRegistration",
            "endpoint": "http://peer:9090",
            "information": [{"entities": [{"type": "https://uri.etsi.org/ngsi-ld/default-context/Vehicle"}]}],
            "management": {"localOnly": true}
        });
        st.store
            .create(&tenant, Kind::Registration, id, doc)
            .expect("seed registration");
        let spec = crate::csource::CsrSpec {
            types: Some(vec![
                "https://uri.etsi.org/ngsi-ld/default-context/Vehicle".into()
            ]),
            ..Default::default()
        };
        let regs = matching_regs(&st, &tenant, &spec, &ctx, &HeaderMap::new());
        assert!(
            regs.iter()
                .find(|r| r.reg_id == id)
                .expect("compiled")
                .local_only,
            "management.localOnly must compile into the forward flag"
        );
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

    /// 4.20: retrieveEntity implements only 5.7.1 — a source offering it
    /// alone is never a query target; queryEntity/queryBatch are.
    #[test]
    fn query_op_requires_query_support() {
        let reg =
            |ops: &[&str]| fed_reg_of("urn:r", &json!({"endpoint": "http://x", "operations": ops}));
        assert_eq!(reg(&["retrieveEntity"]).query_op(), None);
        assert_eq!(reg(&["queryEntity"]).query_op(), Some("queryEntity"));
        assert_eq!(reg(&["queryBatch"]).query_op(), Some("queryBatch"));
        assert_eq!(reg(&["federationOps"]).query_op(), Some("queryEntity"));
        assert_eq!(reg(&["retrieveOps"]).query_op(), Some("queryEntity"));
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

    /// 4.5.5.3 p.60: "if an expiresAt DateTime is present on the
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

    /// 4.5.5.3: entity-level expiresAt — missing from at least one received
    /// version → removed; present in all versions → furthest in the future.
    #[test]
    fn merge_entity_expires_at_intersection_and_max() {
        let mut base = json!({"id": "urn:x", "type": ["T"], "expiresAt": "2030-01-01T00:00:00Z"});
        let add = json!({"id": "urn:x", "type": ["T"], "expiresAt": "2031-01-01T00:00:00Z"});
        merge_docs(&mut base, &add, false);
        assert_eq!(base["expiresAt"], "2031-01-01T00:00:00Z");
        // one version without expiresAt → removed
        let add2 = json!({"id": "urn:x", "type": ["T"]});
        merge_docs(&mut base, &add2, false);
        assert!(base.get("expiresAt").is_none(), "expiresAt must be removed");
        // and never re-introduced by a later version that has one
        let add3 = json!({"id": "urn:x", "type": ["T"], "expiresAt": "2032-01-01T00:00:00Z"});
        merge_docs(&mut base, &add3, false);
        assert!(base.get("expiresAt").is_none());
    }

    /// 4.3.6.2: "An auxiliary Context Source Registration never overrides
    /// data held directly within a Context Broker." The entity-level
    /// expiresAt reconciliation is part of that data, so an auxiliary
    /// version can neither remove it nor push it further out.
    #[test]
    fn auxiliary_merge_never_touches_entity_expires_at() {
        let mut base = json!({"id": "urn:x", "type": ["T"], "expiresAt": "2030-01-01T00:00:00Z"});
        let aux_without = json!({"id": "urn:x", "type": ["T"]});
        merge_docs(&mut base, &aux_without, true);
        assert_eq!(
            base["expiresAt"], "2030-01-01T00:00:00Z",
            "an auxiliary version lacking expiresAt must not remove the broker's own"
        );
        let aux_later = json!({"id": "urn:x", "type": ["T"], "expiresAt": "2031-01-01T00:00:00Z"});
        merge_docs(&mut base, &aux_later, true);
        assert_eq!(
            base["expiresAt"], "2030-01-01T00:00:00Z",
            "an auxiliary version must not extend the broker's own expiresAt"
        );
    }

    /// 4.5.5.3 arbitrates on the most recent DateTime, and 4.6.3 lets the
    /// same instant be written with or without a fraction — so the winner
    /// must be chosen on the instant, never on the spelling.
    #[test]
    fn recency_arbitrates_on_the_instant_not_the_spelling() {
        let attr = "https://uri.etsi.org/ngsi-ld/default-context/speed";
        let mut base = json!({
            "id": "urn:x", "type": ["T"],
            attr: [{"type": "Property", "value": 1, "observedAt": "2026-01-01T00:00:01Z"}]
        });
        let add = json!({
            "id": "urn:x", "type": ["T"],
            attr: [{"type": "Property", "value": 2, "observedAt": "2026-01-01T00:00:01.500Z"}]
        });
        merge_docs(&mut base, &add, false);
        assert_eq!(
            base[attr][0]["value"], 2,
            "the later instant wins even though its string sorts lower"
        );
        // and the converse: a fraction that is EARLIER must not win
        let older = json!({
            "id": "urn:x", "type": ["T"],
            attr: [{"type": "Property", "value": 3, "observedAt": "2026-01-01T00:00:01.250Z"}]
        });
        merge_docs(&mut base, &older, false);
        assert_eq!(base[attr][0]["value"], 2, "an earlier instant must not win");
    }

    /// 4.5.5.2/4.5.7: the registration-scope filter narrows ATTRIBUTES, so
    /// the entity-level lifetime members must cross it — dropping expiresAt
    /// here made the 4.5.5.3 reconciliation delete the local one.
    #[test]
    fn scoped_import_keeps_the_entity_level_lifetime_members() {
        let speed = "https://uri.etsi.org/ngsi-ld/default-context/speed";
        let reg = FedReg {
            attrs: Some(vec![speed.to_owned()]),
            ..FedReg::default()
        };
        let remote = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "expiresAt": "2030-01-01T00:00:00Z",
            "speed": {"type": "Property", "value": 1},
            "brandName": {"type": "Property", "value": "x"}
        });
        let ctx = antares_jsonld::Loader::new().core();
        let imported = import_entity(&remote, &reg, &ctx).expect("import");
        assert_eq!(
            imported["expiresAt"], "2030-01-01T00:00:00Z",
            "entity-level expiresAt must survive the scope filter"
        );
        assert!(
            imported
                .get("https://uri.etsi.org/ngsi-ld/default-context/brandName")
                .is_none(),
            "an out-of-scope attribute must not be imported"
        );
    }

    /// 4.5.5.2: a received version's entity-level expiresAt is pushed onto
    /// each Attribute instance — added where absent, capped where the
    /// Attribute's own expiresAt lies further in the future.
    #[test]
    fn merge_pushes_entity_expires_at_onto_attributes() {
        let attr = "https://uri.etsi.org/ngsi-ld/default-context/speed";
        let mut base = json!({"id": "urn:x", "type": ["T"]});
        let add = json!({
            "id": "urn:x", "type": ["T"],
            "expiresAt": "2030-01-01T00:00:00Z",
            attr: [
                {"type": "Property", "value": 1},
                {"type": "Property", "value": 2, "datasetId": "urn:ngsi-ld:Dataset:1",
                 "expiresAt": "2035-01-01T00:00:00Z"},
                {"type": "Property", "value": 3, "datasetId": "urn:ngsi-ld:Dataset:2",
                 "expiresAt": "2029-01-01T00:00:00Z"}
            ]
        });
        merge_docs(&mut base, &add, false);
        let inst = base[attr].as_array().expect("attr array");
        assert_eq!(inst[0]["expiresAt"], "2030-01-01T00:00:00Z", "added");
        assert_eq!(inst[1]["expiresAt"], "2030-01-01T00:00:00Z", "capped");
        assert_eq!(inst[2]["expiresAt"], "2029-01-01T00:00:00Z", "earlier kept");
    }

    /// 6.3.17: "the error response should be as informative as possible" —
    /// informative about the OPERATION, not about the deployment. The part
    /// that failed is identified by its Context Source Registration id
    /// (5.2.9), which the client can retrieve from /csourceRegistrations;
    /// the registration `endpoint` is not part of any client-facing payload,
    /// and a client able to provoke a partial failure must not be able to
    /// enumerate the address of every registered Context Source.
    #[tokio::test]
    async fn partial_failure_detail_omits_the_peer_endpoint() {
        use std::io::{Read, Write};
        std::env::set_var("ANTARES_EGRESS_ALLOW_PRIVATE", "true");
        // a Context Source that refuses every forwarded write
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { continue };
                let mut buf = [0u8; 4096];
                let _ = s.read(&mut buf);
                let _ = s.write_all(
                    b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
                );
            }
        });
        let st = AppState::new("me".into());
        let tenant = antares_model::TenantId::new("default").expect("tenant");
        let mut r = reg("inclusive");
        r.endpoint = format!("http://127.0.0.1:{port}");
        let part = forward_part(
            &st,
            reqwest::Method::POST,
            format!("{}/ngsi-ld/v1/entities", r.endpoint),
            &[],
            &HeaderMap::new(),
            &tenant,
            &r,
            antares_jsonld::CORE_CONTEXT,
            Some(json!({"id": "urn:ngsi-ld:V:1", "type": "Vehicle"})),
        )
        .await;
        assert_eq!(part.status, 400, "the peer refused the write");
        // one failed forward + one succeeded local part ⇒ 207 Multi-Status
        let local = Part {
            status: 204,
            detail: "local write applied".into(),
        };
        let resp = combine(
            vec![local, part],
            StatusCode::NO_CONTENT.into_response(),
            &tenant,
        );
        assert_eq!(resp.status(), StatusCode::MULTI_STATUS);
        let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
            .await
            .expect("body");
        let body = String::from_utf8_lossy(&bytes).into_owned();
        assert!(
            !body.contains(&format!("127.0.0.1:{port}")),
            "the peer host:port must never reach the client, got {body}"
        );
        assert!(
            body.contains(&r.reg_id),
            "the registration id is the client-safe identifier, got {body}"
        );
    }

    /// 6.3.18: the Via header exists "to avoid infinite loops", and Table
    /// 6.3.18-2 makes its listing part of registration matching — every
    /// element is compared against every candidate registration. A chain
    /// longer than any real cascade is therefore both a loop symptom and a
    /// work amplifier, and is refused before the registrations are read.
    #[test]
    fn via_chain_beyond_the_hop_ceiling_is_refused() {
        let chain = |n: usize| {
            (0..n)
                .map(|i| format!("1.1 hop{i}"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let t = antares_model::TenantId::new("default").expect("tenant");
        let over = hdrs(Some(&chain(MAX_VIA_HOPS + 1)));
        assert!(
            via_loop(&over, "me"),
            "a chain past the ceiling is treated as a loop"
        );
        let mut regs = vec![reg("inclusive")];
        let resp = handle_via_loop(&over, "me", &t, &mut regs)
            .expect("an over-long Via chain must be refused");
        assert_eq!(resp.status(), StatusCode::LOOP_DETECTED);
        // and no registration is consulted: the candidate set is empty
        // without a single registration document being examined
        let st = AppState::new("me".into());
        let ctx = st.loader.core();
        let id = "urn:ngsi-ld:ContextSourceRegistration:hops";
        st.store
            .create(
                &t,
                Kind::Registration,
                id,
                json!({
                    "id": id,
                    "type": "ContextSourceRegistration",
                    "endpoint": "http://peer:9090",
                    "information": [{"entities": [{"type": "https://uri.etsi.org/ngsi-ld/default-context/Vehicle"}]}],
                }),
            )
            .expect("seed registration");
        let spec = crate::csource::CsrSpec {
            types: Some(vec![
                "https://uri.etsi.org/ngsi-ld/default-context/Vehicle".into()
            ]),
            ..Default::default()
        };
        assert!(
            matching_regs(&st, &t, &spec, &ctx, &over).is_empty(),
            "no registration is matched past the hop ceiling"
        );
        // at the ceiling the chain is still processed normally
        let at = hdrs(Some(&chain(MAX_VIA_HOPS)));
        assert!(!via_loop(&at, "me"));
        assert!(handle_via_loop(&at, "me", &t, &mut vec![reg("inclusive")]).is_none());
        assert_eq!(matching_regs(&st, &t, &spec, &ctx, &at).len(), 1);
    }

    /// 5.7.2.4/4.23.1: `would_federate` only answers WHETHER a query leaves
    /// the local scope, so it never compiles a forwarding set — but its
    /// verdict must stay identical to the set's emptiness for every shape
    /// that gates matching (type, id, the Via chain, local scope).
    #[test]
    fn would_federate_agrees_with_the_compiled_forward_set() {
        let st = AppState::new("me".into());
        let t = antares_model::TenantId::new("default").expect("tenant");
        let ctx = st.loader.core();
        let id = "urn:ngsi-ld:ContextSourceRegistration:wf";
        st.store
            .create(
                &t,
                Kind::Registration,
                id,
                json!({
                    "id": id,
                    "type": "ContextSourceRegistration",
                    "endpoint": "http://peer:9090",
                    "contextSourceAlias": "peer1",
                    "information": [{"entities": [{"type": "https://uri.etsi.org/ngsi-ld/default-context/Vehicle"}]}],
                }),
            )
            .expect("seed registration");
        let params = |kv: &[(&str, &str)]| {
            kv.iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect::<HashMap<String, String>>()
        };
        for (p, headers, expected) in [
            (params(&[("type", "Vehicle")]), hdrs(None), true),
            (params(&[("type", "Parking")]), hdrs(None), false),
            (
                params(&[("type", "Vehicle"), ("local", "true")]),
                hdrs(None),
                false,
            ),
            (
                params(&[("type", "Vehicle")]),
                hdrs(Some("1.1 peer1")),
                false,
            ),
            (
                params(&[("type", "Vehicle")]),
                hdrs(Some("1.1 other")),
                true,
            ),
        ] {
            assert_eq!(
                would_federate(&st, &t, &ctx, &p, &headers),
                expected,
                "would_federate verdict for {p:?}"
            );
            assert_eq!(
                active(&p)
                    && !matching_regs(&st, &t, &query_spec(&ctx, &p), &ctx, &headers).is_empty(),
                expected,
                "the compiled forward set must agree for {p:?}"
            );
        }
    }
}

#[cfg(test)]
mod clause_4_20 {
    use super::*;

    fn reg(ops: &[&str]) -> FedReg {
        FedReg {
            ops: ops.iter().map(|s| (*s).to_owned()).collect(),
            ..FedReg::default()
        }
    }

    /// Table 4.20-2: associationOps is federationOps WITHOUT the EntityMap
    /// support operations (and without createEntityMapQueryTemporal, which is
    /// in neither group).
    #[test]
    fn association_ops_exclude_the_entity_map_operations() {
        let r = reg(&["associationOps"]);
        for op in [
            "retrieveEntity",
            "queryEntity",
            "deleteSubscription",
            "retrieveContextSourceIdentity",
        ] {
            assert!(r.supports(op), "{op} is in associationOps");
        }
        for op in [
            "retrieveEntityMap",
            "updateEntityMap",
            "deleteEntityMap",
            "createEntityMapQueryEntity",
        ] {
            assert!(
                !r.supports(op),
                "{op} is NOT in associationOps (Table 4.20-2)"
            );
        }
    }

    /// Table 4.20-1/2: individual names match themselves; groups match their
    /// members; nothing matches createEntityMapQueryTemporal except itself.
    #[test]
    fn groups_and_individual_names() {
        assert!(reg(&["federationOps"]).supports("retrieveEntityMap"));
        assert!(reg(&["redirectionOps"]).supports("purgeEntity"));
        assert!(!reg(&["redirectionOps"]).supports("createSubscription"));
        assert!(reg(&["updateOps"]).supports("replaceAttrs"));
        assert!(!reg(&["updateOps"]).supports("deleteEntity"));
        assert!(reg(&["retrieveOps"]).supports("queryEntity"));
        assert!(!reg(&["retrieveOps"]).supports("retrieveTemporal"));
        assert!(reg(&["createEntityMapQueryTemporal"]).supports("createEntityMapQueryTemporal"));
        for group in ["federationOps", "associationOps", "redirectionOps"] {
            assert!(
                !reg(&[group]).supports("createEntityMapQueryTemporal"),
                "{group} does not include createEntityMapQueryTemporal"
            );
        }
    }
}
