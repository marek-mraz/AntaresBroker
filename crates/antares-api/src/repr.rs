// SPDX-License-Identifier: EUPL-1.2
//! Representation transforms (6.3.7, 4.5.4, concise, sysAttrs, attrs
//! projection, lang filter) — applied on the INTERNAL expanded form before
//! compaction.

use crate::state::AppState;
use antares_jsonld::{compact_entity, compact_entity_shallow, Context};
use antares_model::NgsiError;
use antares_model::{is_meta, TenantId};
use antares_store::Kind;
use serde_json::{Map, Value};
use std::collections::HashMap;

#[derive(Debug, Default, Clone)]
pub struct Repr {
    pub sys_attrs: bool,
    pub key_values: bool,
    pub concise: bool,
    /// expanded attribute IRIs to project (attrs=): entity meta stays.
    pub attrs: Option<Vec<String>>,
    /// pick= (4.21): STRICT projection — core members (id/type/scope/…) only
    /// survive when explicitly picked. Nodes may carry nested selections for
    /// linked entities.
    pub pick: Option<Vec<ProjNode>>,
    /// omit= (4.21): nodes WITHOUT children omit their head; nodes WITH
    /// children only constrain the linked entity below that head.
    pub omit: Option<Vec<ProjNode>>,
    pub lang: Option<String>,
    /// datasetId= instance filter; entry "@none" selects default instances.
    pub dataset_id: Option<Vec<String>>,
}

pub fn parse_repr(params: &HashMap<String, String>, ctx: &Context) -> Result<Repr, NgsiError> {
    let mut r = Repr::default();
    let mut format: Option<String> = None;
    if let Some(opts) = params.get("options") {
        for o in opts.split(',') {
            match o.trim() {
                "sysAttrs" => r.sys_attrs = true,
                "keyValues" | "simplified" => {
                    format.get_or_insert("simplified".into());
                }
                "concise" => {
                    format.get_or_insert("concise".into());
                }
                "normalized" => {
                    format.get_or_insert("normalized".into());
                }
                _ => {
                    return Err(NgsiError::InvalidRequest(format!(
                        "unsupported options value {o:?}"
                    )))
                }
            };
        }
    }
    // format wins over options on conflict (6.3.7)
    if let Some(f) = params.get("format") {
        match f.as_str() {
            "normalized" | "concise" | "simplified" | "keyValues" => {
                format = Some(if f == "keyValues" {
                    "simplified".into()
                } else {
                    f.clone()
                })
            }
            _ => {
                return Err(NgsiError::InvalidRequest(format!(
                    "unsupported format value {f:?}"
                )))
            }
        }
    }
    match format.as_deref() {
        Some("simplified") => r.key_values = true,
        Some("concise") => r.concise = true,
        _ => {}
    }
    check_projection_exclusive(params)?;
    if let Some(a) = params.get("attrs") {
        let mut list = Vec::new();
        for t in a.split(',') {
            let t = t.trim();
            if t.is_empty() || ENTITY_META.contains(&t) {
                return Err(NgsiError::BadRequestData(format!(
                    "invalid attribute name {t:?} in attrs"
                )));
            }
            list.push(ctx.expand_key(t));
        }
        r.attrs = Some(list);
    }
    if let Some(pck) = params.get("pick") {
        r.pick = Some(parse_projection(pck, ctx)?);
    }
    if let Some(o) = params.get("omit") {
        r.omit = Some(parse_projection(o, ctx)?);
    }
    r.lang = params.get("lang").cloned();
    r.dataset_id = params
        .get("datasetId")
        .map(|s| s.split(',').map(|d| d.trim().to_owned()).collect());
    Ok(r)
}

/// Narrow a representation by a policy decision (ADR-0020).
///
/// `omit` is appended to whatever the request asked to omit: removing more
/// members can only remove more. `pick` intersects with the request's,
/// because a pick that added a name would serve a member the request did
/// not ask for and the engine's narrowing would have widened the answer.
///
/// The Entity frame survives a policy pick. 6.5.3.1 makes `pick` reduce an
/// Entity "down to only contain the listed Entity members", and a client
/// that wants `id` back names it; but the engine is restricting what may be
/// seen rather than choosing a representation, and 5.2.4 makes `id` and
/// `type` the Entity — a document without them is not one.
pub fn narrow_projection(
    pick: &mut Option<Vec<ProjNode>>,
    omit: &mut Option<Vec<ProjNode>>,
    f: &crate::policy::Filter,
    ctx: &Context,
) -> Result<(), NgsiError> {
    if !f.omit.is_empty() {
        let mut nodes = policy_nodes(&f.omit);
        match omit {
            Some(own) => own.append(&mut nodes),
            None => *omit = Some(nodes),
        }
    }
    if !f.pick.is_empty() {
        let allowed = policy_nodes(&f.pick);
        let mut kept = match pick.take() {
            None => allowed,
            Some(own) => own
                .into_iter()
                .filter(|n| allowed.iter().any(|a| a.iri == n.iri))
                .collect(),
        };
        for frame in parse_projection("id,type", ctx)? {
            if !kept.iter().any(|n| n.raw == frame.raw) {
                kept.push(frame);
            }
        }
        *pick = Some(kept);
    }
    Ok(())
}

/// The same `pick`/`omit` names in the form an already-compacted document
/// carries them. The query path folds a policy projection into the
/// request's own representation, where the names are IRIs; a notification
/// is compacted by `notify::build_data` long before the seam sees it, so
/// there the names have to travel the other way — parsed against the same
/// `@context` the document was compacted with, then compacted back. An
/// engine writes one rule set either way, and a rule written as an IRI
/// (which is what ADR-0020 asks of an engine) removes the member it names
/// rather than silently matching nothing.
pub fn compacted_filter(f: &crate::policy::Filter, ctx: &Context) -> crate::policy::Filter {
    let names = |raw: &[String]| -> Vec<String> {
        policy_nodes(raw)
            .iter()
            .map(|n| ctx.compact_iri(&n.iri))
            .collect()
    };
    crate::policy::Filter {
        pick: names(&f.pick),
        omit: names(&f.omit),
        ..f.clone()
    }
}

/// The `@context` a policy name is read in. Deliberately NOT the request's:
/// [`antares_jsonld::Context::expand_key`] consults the term map before it
/// decides a name is already an IRI, so a caller that binds the term a rule
/// names — or binds the rule's own IRI as a term, which its own inline
/// `@context` is enough to do — would move the rule off its target and walk
/// out of the narrowing. A deployment's rule means the same thing whatever
/// the caller sends. ADR-0020 asks an engine to write its rules as IRIs and
/// those pass through unchanged; a short name is read here the way a name a
/// request does not define is read anywhere else.
static POLICY_CONTEXT: std::sync::LazyLock<Context> =
    std::sync::LazyLock::new(antares_jsonld::core_context);

/// A policy's `pick`/`omit` names as projection nodes, expanded against
/// [`POLICY_CONTEXT`].
///
/// Deliberately NOT the 4.21 parser: 6.5.3.1 lets those members name `"id"`,
/// `"type"`, `"scope"` or one projected Attribute, and ADR-0020 asks an
/// engine to write its rules against IRIs — while 4.21 reads a dot as the
/// sub-attribute path separator, so `https://uri.etsi.org/…/colour` through
/// that grammar becomes the member `https://uri` and removes nothing. An
/// engine names one member; it is expanded, and both forms of the name are
/// kept so the projection matches a document whichever form its keys are in.
fn policy_nodes(names: &[String]) -> Vec<ProjNode> {
    names
        .iter()
        .map(|n| ProjNode {
            raw: n.clone(),
            iri: POLICY_CONTEXT.expand_key(n),
            children: None,
        })
        .collect()
}

/// Maximum `{…}` selection depth of a projection tree — the number of
/// Linked Entity hops it implies (5.7.1.4: must not exceed joinLevel).
pub fn proj_depth(nodes: &[ProjNode]) -> usize {
    nodes
        .iter()
        .map(|n| match &n.children {
            Some(c) => 1 + proj_depth(c),
            None => 0,
        })
        .max()
        .unwrap_or(0)
}

/// The entity-level members of 4.5.1: everything an Entity carries that is
/// NOT an Attribute. Every layer that has to tell the two apart reads this
/// one list — `attrs`/`pick`/`omit` validation and projection here, the
/// notification diff and tombstone in `notify`, the 4.3.6.8 amendment in
/// `conformance`, the registration-scope narrowing in `federation`. A layer
/// with its own copy is a layer that will disagree with the others about
/// what an attribute is.
pub(crate) const ENTITY_META: &[&str] = &[
    "id",
    "type",
    "scope",
    "createdAt",
    "modifiedAt",
    "deletedAt",
    "expiresAt",
    "@context",
];

/// One node of a 4.21 attribute-projection expression; `children` carries a
/// nested `{…}` selection (applied to linked entities on join).
#[derive(Debug, Clone)]
pub struct ProjNode {
    pub raw: String,
    pub iri: String,
    pub children: Option<Vec<ProjNode>>,
}

/// Parse + validate a pick=/omit= value (4.21) into a projection tree.
pub(crate) fn parse_projection(s: &str, ctx: &Context) -> Result<Vec<ProjNode>, NgsiError> {
    let bad = || NgsiError::BadRequestData(format!("invalid attribute projection {s:?} (4.21)"));
    // Each `{…}` level is one Linked Entity hop (5.7.1.4), so a selection
    // deeper than the joinLevel ceiling can never be satisfied. Bounding it
    // here, before the recursive descent below, is what keeps the recursion
    // finite: pick=/omit= reach this parser as plain STRINGS — from the URI,
    // or from inside a query body, where the JSON nesting wall never sees
    // their braces.
    if s.is_empty()
        || s.matches('{').count() != s.matches('}').count()
        || crate::bounds::json_depth(s.as_bytes()) > crate::bounds::MAX_JOIN_LEVEL
        || !s
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_,.:{}#/%-+@|".contains(c))
    {
        return Err(bad());
    }
    fn split_top(s: &str) -> Option<Vec<&str>> {
        let mut out = Vec::new();
        let mut depth = 0usize;
        let mut start = 0usize;
        for (i, c) in s.char_indices() {
            match c {
                '{' => depth += 1,
                '}' => depth = depth.checked_sub(1)?,
                // 4.21 orOp = | / , — both split at the same depth
                ',' | '|' if depth == 0 => {
                    out.push(&s[start..i]);
                    start = i + 1;
                }
                _ => {}
            }
        }
        out.push(&s[start..]);
        Some(out)
    }
    fn parse_nodes(s: &str, ctx: &Context) -> Result<Vec<ProjNode>, NgsiError> {
        let bad = |m: &str| NgsiError::BadRequestData(format!("invalid attribute projection: {m}"));
        let parts = split_top(s).ok_or_else(|| bad("unbalanced braces"))?;
        let mut out: Vec<ProjNode> = Vec::new();
        for t in parts {
            let t = t.trim();
            if t.is_empty() {
                return Err(bad("empty projection member"));
            }
            let (head_part, children) = match t.find('{') {
                Some(i) => {
                    let inner = t[i + 1..]
                        .strip_suffix('}')
                        .ok_or_else(|| bad("unclosed brace"))?;
                    (&t[..i], Some(parse_nodes(inner, ctx)?))
                }
                None => (t, None),
            };
            let head = head_part.split('.').next().unwrap_or(head_part);
            if head.is_empty() || head_part.split('.').any(str::is_empty) {
                return Err(bad("empty path segment"));
            }
            if !head
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphanumeric() || "_:@#".contains(c))
            {
                return Err(bad("projection member starts with a special character"));
            }
            if out.iter().any(|n| n.raw == head) {
                return Err(bad("duplicate projection member"));
            }
            out.push(ProjNode {
                raw: head.to_owned(),
                iri: ctx.expand_key(head),
                children,
            });
        }
        Ok(out)
    }
    parse_nodes(s, ctx)
}

/// Apply the representation to an internal doc, producing a new internal doc
/// 4.21 Projections: "pick, omit and attrs are mutually exclusive" — the one
/// reading, so an operation cannot accept a combination another rejects.
pub fn check_projection_exclusive(params: &HashMap<String, String>) -> Result<(), NgsiError> {
    let excl = ["pick", "omit", "attrs"]
        .iter()
        .filter(|k| params.contains_key(**k))
        .count();
    if excl > 1 {
        return Err(NgsiError::BadRequestData(
            "pick, omit and attrs are mutually exclusive (4.21)".into(),
        ));
    }
    Ok(())
}

/// 4.21: does a core Entity member (id, type, scope, the system temporal
/// Properties) survive the projection? `pick` constrains core members
/// strictly — only what is named survives; `omit` drops a named member only
/// when the node carries no children, because a node with children
/// constrains the linked Entity below the head, not the head itself. The
/// current-state and temporal representations project core members by the
/// same rule and differ only below it.
pub fn meta_projected(pick: Option<&[ProjNode]>, omit: Option<&[ProjNode]>, k: &str) -> bool {
    if let Some(pick) = pick {
        if !pick.iter().any(|n| n.raw == *k) {
            return false;
        }
    }
    if let Some(omit) = omit {
        if omit.iter().any(|n| n.raw == *k && n.children.is_none()) {
            return false;
        }
    }
    true
}

/// ready for compaction.
pub fn apply(doc: &Value, r: &Repr) -> Value {
    let Some(obj) = doc.as_object() else {
        return doc.clone();
    };
    let mut out = Map::new();
    for (k, v) in obj {
        let is_meta = ENTITY_META.contains(&k.as_str());
        if is_meta {
            match k.as_str() {
                // 6.3.11 Table 6.3.11-1: expiresAt is a system temporal
                // attribute — included only when options=sysAttrs.
                "createdAt" | "modifiedAt" | "expiresAt" if !r.sys_attrs => continue,
                _ => {}
            }
            if !meta_projected(r.pick.as_deref(), r.omit.as_deref(), k) {
                continue;
            }
            out.insert(k.clone(), v.clone());
            continue;
        }
        if let Some(keep) = &r.attrs {
            if !keep.contains(k) {
                continue;
            }
        }
        if let Some(pick) = &r.pick {
            if !pick.iter().any(|n| n.iri == *k) {
                continue;
            }
        }
        if let Some(drop) = &r.omit {
            if drop.iter().any(|n| n.iri == *k && n.children.is_none()) {
                continue;
            }
        }
        let raw: Vec<Value> = v.as_array().cloned().unwrap_or_else(|| vec![v.clone()]);
        let kept: Vec<&Value> = raw
            .iter()
            .filter(|inst| match (&r.dataset_id, inst.get("datasetId")) {
                (None, _) => true,
                (Some(want), Some(Value::String(have))) => want.iter().any(|w| w == have),
                (Some(want), None) => want.iter().any(|w| w == "@none"),
                _ => false,
            })
            .collect();
        let mut instances: Vec<Value> = kept
            .iter()
            .map(|inst| transform_instance(inst, r))
            .collect();
        if instances.is_empty() {
            continue;
        }
        if r.key_values {
            if instances.len() == 1 {
                // 4.5.4: a lone instance simplifies to its bare value
                out.extend(instances.pop().map(|one| (k.clone(), one)));
            } else {
                // 4.5.4 multi-attribute case: a "dataset" map holding one
                // key-value pair for each datasetId, "@none" for the default
                // instance. The pairing below is positional, so `kept` and
                // `instances` must both still be whole here.
                let mut ds = Map::new();
                for (orig, simple) in kept.iter().zip(instances.iter()) {
                    let key = orig
                        .get("datasetId")
                        .and_then(Value::as_str)
                        .unwrap_or("@none");
                    ds.insert(key.to_owned(), simple.clone());
                }
                out.insert(
                    k.clone(),
                    serde_json::json!({ "dataset": Value::Object(ds) }),
                );
            }
        } else {
            out.insert(k.clone(), Value::Array(instances));
        }
    }
    Value::Object(out)
}

fn transform_instance(inst: &Value, r: &Repr) -> Value {
    let Some(obj) = inst.as_object() else {
        return inst.clone();
    };
    // lang filter first: LanguageProperty → Property under a selected language
    let mut obj = obj.clone();
    if let Some(lang) = &r.lang {
        apply_lang(&mut obj, lang);
    }

    if r.key_values {
        return simplified_value(&obj);
    }

    let mut out = Map::new();
    for (k, v) in &obj {
        match k.as_str() {
            // 6.3.11: expiresAt shares the sysAttrs gate on attribute
            // instances (current-state and temporal alike).
            "createdAt" | "modifiedAt" | "expiresAt" if !r.sys_attrs => continue,
            "type" if r.concise => continue,
            _ => {}
        }
        // sub-attributes recurse
        if let Some(arr) = v.as_array().filter(|_| !is_reserved_member(k)) {
            let subs: Vec<Value> = arr.iter().map(|i| transform_instance(i, r)).collect();
            out.insert(k.clone(), Value::Array(subs));
        } else {
            out.insert(k.clone(), v.clone());
        }
    }
    if r.concise {
        // bare-value collapse: a Property with only `value` collapses
        if out.len() == 1 {
            if let Some(v) = out.get("value") {
                return v.clone();
            }
        }
    }
    Value::Object(out)
}

/// 4.15 Language Filter on one attribute instance (5.7.2.5, and the `lang`
/// rows of Tables 6.18.3.2-1 / 6.19.3.1 for the temporal forms): a
/// LanguageProperty "shall be converted into a Property" holding the chosen
/// languageMap entry, with the non-reified `lang` member naming it.
pub(crate) fn apply_lang(obj: &mut Map<String, Value>, lang: &str) {
    if obj.get("type").and_then(Value::as_str) != Some("LanguageProperty") {
        return;
    }
    let Some(lm) = obj.get("languageMap").and_then(Value::as_object) else {
        return;
    };
    if let Some((chosen_lang, value)) = select_lang(lm, lang) {
        obj.remove("languageMap");
        obj.insert("type".into(), Value::String("Property".into()));
        obj.insert("value".into(), value);
        obj.insert("lang".into(), Value::String(chosen_lang));
    }
}

/// 4.15 Language Filter: pick one languageMap entry for a lang priority
/// list. Ranges are ordered by their q weights (RFC 3282, default 1, list
/// position breaking ties); tags compare case-insensitively (RFC 5646); a
/// range matches an exact tag, then a longer tag by prefix (fr → fr-CH),
/// then a shorter tag by truncation (fr-CH → fr). "*" — or no match at
/// all — "shall default to any supported language" (@none preferred).
fn select_lang(lm: &Map<String, Value>, lang: &str) -> Option<(String, Value)> {
    let mut ranges: Vec<(f64, usize, &str)> = lang
        .split(',')
        .enumerate()
        .filter_map(|(i, part)| {
            let mut it = part.trim().split(';');
            let tag = it.next()?.trim();
            if tag.is_empty() {
                return None;
            }
            let q = it
                .find_map(|p| {
                    p.trim()
                        .strip_prefix("q=")
                        .and_then(|v| v.parse::<f64>().ok())
                })
                .unwrap_or(1.0);
            Some((q, i, tag))
        })
        .collect();
    ranges.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.1.cmp(&b.1))
    });
    let ci = |k: &str, want: &str| k.eq_ignore_ascii_case(want);
    for (q, _, want) in &ranges {
        // q=0 = "not acceptable" (RFC 3282); "*" = any → the fallback below
        if *q <= 0.0 || *want == "*" {
            continue;
        }
        if let Some((k, v)) = lm.iter().find(|(k, _)| ci(k, want)) {
            return Some((k.clone(), v.clone()));
        }
        if let Some((k, v)) = lm.iter().find(|(k, _)| {
            k.len() > want.len()
                && k.as_bytes().get(want.len()) == Some(&b'-')
                && ci(&k[..want.len()], want)
        }) {
            return Some((k.clone(), v.clone()));
        }
        let mut w = *want;
        while let Some(cut) = w.rfind('-') {
            w = &w[..cut];
            if let Some((k, v)) = lm.iter().find(|(k, _)| ci(k, w)) {
                return Some((k.clone(), v.clone()));
            }
        }
    }
    // any: prefer @none, then first
    if let Some(v) = lm.get("@none") {
        return Some(("@none".to_owned(), v.clone()));
    }
    lm.iter().next().map(|(k, v)| (k.clone(), v.clone()))
}

fn is_reserved_member(k: &str) -> bool {
    matches!(
        k,
        "type"
            | "value"
            | "object"
            | "objectType"
            | "datasetId"
            | "observedAt"
            | "unitCode"
            | "lang"
            | "languageMap"
            | "vocab"
            | "json"
            | "valueList"
            | "objectList"
            | "createdAt"
            | "modifiedAt"
            | "deletedAt"
            | "instanceId"
            | "previousValue"
            | "previousObject"
            | "previousLanguageMap"
            | "previousJson"
            | "previousVocab"
    )
}

/// 4.5.4: the simplified (keyValues) value of one instance — bare value for
/// Property/GeoProperty, bare URI(s) for a Relationship, bare ordered arrays
/// for ListProperty/ListRelationship, but the single-key wrapper objects
/// {"languageMap": …} / {"json": …} / {"vocab": …} for the Language, Json
/// and Vocab subtypes (Examples 4–6).
fn simplified_value(obj: &Map<String, Value>) -> Value {
    for k in ["value", "object", "valueList", "objectList"] {
        if let Some(v) = obj.get(k) {
            return v.clone();
        }
    }
    for k in ["languageMap", "json", "vocab"] {
        if let Some(v) = obj.get(k) {
            return serde_json::json!({ k: v.clone() });
        }
    }
    Value::Object(obj.clone())
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
pub(crate) const MAX_JOIN_LOOKUPS: usize = 1_000;

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
#[allow(clippy::too_many_arguments)] // one param per piece of the traversal's state
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

#[allow(clippy::too_many_arguments)] // one param per piece of the traversal's state
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
#[allow(clippy::too_many_arguments)] // one param per piece of the traversal's state
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

#[allow(clippy::too_many_arguments)] // one param per piece of the traversal's state
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 4.5.4 Examples 1–16: the simplified value of one instance per
    /// attribute type — bare for Property/GeoProperty/Relationship/List*,
    /// wrapped single-key objects for Language/Json/Vocab subtypes.
    #[test]
    fn simplified_values_per_attribute_type() {
        let v = |j: Value| simplified_value(j.as_object().unwrap());
        assert_eq!(v(json!({"type": "Property", "value": 5})), json!(5));
        assert_eq!(
            v(json!({"type": "Relationship", "object": "urn:a"})),
            json!("urn:a")
        );
        assert_eq!(
            v(json!({"type": "ListProperty", "valueList": [1, 2]})),
            json!([1, 2])
        );
        assert_eq!(
            v(json!({"type": "ListRelationship", "objectList": ["urn:a"]})),
            json!(["urn:a"])
        );
        assert_eq!(
            v(json!({"type": "LanguageProperty", "languageMap": {"en": "hi"}})),
            json!({"languageMap": {"en": "hi"}})
        );
        assert_eq!(
            v(json!({"type": "JsonProperty", "json": {"k": 1}})),
            json!({"json": {"k": 1}})
        );
        assert_eq!(
            v(json!({"type": "VocabProperty", "vocab": "V"})),
            json!({"vocab": "V"})
        );
    }
}

#[cfg(test)]
mod clause_4_15 {
    use super::*;
    use serde_json::json;

    fn lm(pairs: &[(&str, &str)]) -> Map<String, Value> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), json!(*v)))
            .collect()
    }

    /// 4.15 EXAMPLE 4: quality value ranking — entries are ordered by their
    /// q weight (default 1), not by list position.
    #[test]
    fn q_values_rank_the_priority_list() {
        let m = lm(&[("en", "red"), ("fr", "rouge")]);
        let (l, v) = select_lang(&m, "en;q=0.2,fr;q=0.9").expect("pick");
        assert_eq!((l.as_str(), &v), ("fr", &json!("rouge")), "fr outranks en");
        // default q=1: plain fr-CH beats fr;q=0.9
        let m = lm(&[("fr-CH", "rouge suisse"), ("fr", "rouge")]);
        let (l, _) = select_lang(&m, "fr-CH,fr;q=0.9").expect("pick");
        assert_eq!(l, "fr-CH");
        // wildcard with low q still yields a fallback when nothing else fits
        let m = lm(&[("de", "rot")]);
        let (l, _) = select_lang(&m, "fr;q=0.9,*;q=0.5").expect("pick");
        assert_eq!(l, "de");
    }

    /// RFC 5646 (via 4.15): language tags compare case-insensitively.
    #[test]
    fn langtags_compare_case_insensitively() {
        let m = lm(&[("en-US", "color")]);
        let (l, v) = select_lang(&m, "en-us").expect("pick");
        assert_eq!((l.as_str(), &v), ("en-US", &json!("color")));
        let m = lm(&[("fr", "rouge"), ("de", "rot")]);
        let (l, _) = select_lang(&m, "FR").expect("pick");
        assert_eq!(l, "fr");
    }

    /// RFC 5646 lookup (via 4.15): a shorter range matches a longer tag
    /// (lang=fr picks fr-CH) and a longer range truncates onto a shorter tag
    /// (lang=fr-CH picks fr) — the `lang` subproperty reports the ACTUAL tag.
    #[test]
    fn prefix_and_truncation_fallbacks() {
        // decoy `de` sorts first — a naive any-fallback would pick it
        let m = lm(&[("de", "rot"), ("fr-CH", "rouge suisse")]);
        let (l, _) = select_lang(&m, "fr").expect("pick");
        assert_eq!(l, "fr-CH", "range fr matches tag fr-CH, not the decoy");
        let m = lm(&[("de", "rot"), ("fr", "rouge")]);
        let (l, _) = select_lang(&m, "fr-CH").expect("pick");
        assert_eq!(l, "fr", "range fr-CH truncates onto tag fr, not the decoy");
        // an exact match still beats a prefix match at the same rank
        let m = lm(&[("fr-CH", "suisse"), ("fr", "rouge")]);
        let (l, _) = select_lang(&m, "fr").expect("pick");
        assert_eq!(l, "fr");
    }

    /// 4.15: "If the Context Broker cannot serve any matching language, it
    /// shall default to any supported language" — and the augmented `lang`
    /// subproperty carries the actually returned one.
    #[test]
    fn no_match_falls_back_to_any_supported_language() {
        let m = lm(&[("de", "rot")]);
        let (l, v) = select_lang(&m, "pt").expect("fallback");
        assert_eq!((l.as_str(), &v), ("de", &json!("rot")));
        // the transform augments with the actual language and converts the
        // LanguageProperty to a Property — languageMap must NOT survive
        let inst = json!({"type": "LanguageProperty",
            "languageMap": {"en": "red", "fr": "rouge"}});
        let r = Repr {
            lang: Some("fr".into()),
            ..Repr::default()
        };
        let out = transform_instance(&inst, &r);
        assert_eq!(out["type"], "Property");
        assert_eq!(out["value"], "rouge");
        assert_eq!(out["lang"], "fr");
        assert!(
            out.get("languageMap").is_none(),
            "languageMap must not remain after conversion"
        );
    }
}

#[cfg(test)]
mod clause_4_21 {
    use super::*;
    use antares_jsonld::Loader;

    /// 4.21: "either a comma or a pipe character can be used as alternative
    /// representations of the or operator" — including inside a nested
    /// LinkedEntityTerm (EXAMPLE 3).
    #[test]
    fn pipe_and_comma_are_both_or_operators() {
        let ctx = Loader::new().core();
        let comma = parse_projection("temperature,humidity", &ctx).expect("comma");
        let pipe = parse_projection("temperature|humidity", &ctx).expect("pipe");
        assert_eq!(comma.len(), 2);
        assert_eq!(pipe.len(), 2);
        assert_eq!(comma[0].raw, pipe[0].raw);
        assert_eq!(comma[1].raw, pipe[1].raw);
        let nested = parse_projection("observation{temperature|humidity}", &ctx).expect("nested");
        assert_eq!(nested.len(), 1);
        let kids = nested[0].children.as_ref().expect("children");
        assert_eq!(kids.len(), 2, "pipe splits inside the braces too");
    }

    /// 4.21 grammar: an empty member or unbalanced braces are violations.
    #[test]
    fn grammar_rejections_hold_for_both_spellings() {
        let ctx = Loader::new().core();
        for bad in ["a||b", "a|,b", "|a", "a|", "a{b|}"] {
            assert!(
                parse_projection(bad, &ctx).is_err(),
                "{bad:?} must be rejected"
            );
        }
    }

    /// Each `{…}` level of a projection is one Linked Entity hop (5.7.1.4),
    /// so a selection deeper than the joinLevel ceiling can never be
    /// satisfied. The depth is bounded BEFORE the recursive descent parses
    /// it: pick= arrives as a plain string inside a query body, where the
    /// JSON nesting wall never sees its braces.
    #[test]
    fn projection_nesting_is_bounded_before_the_parser_recurses() {
        let ctx = Loader::new().core();
        let cap = crate::bounds::MAX_JOIN_LEVEL;
        let nested = |n: usize| "a{".repeat(n) + "b" + &"}".repeat(n);
        // the body path can carry a projection far past any stack budget
        assert!(
            parse_projection(&nested(200_000), &ctx).is_err(),
            "a body-sized projection must be rejected, not recursed into"
        );
        assert!(parse_projection(&nested(cap), &ctx).is_ok(), "at the cap");
        assert!(
            parse_projection(&nested(cap + 1), &ctx).is_err(),
            "one level over the cap must be rejected"
        );
    }

    /// 5.7.1.4: proj_depth reports the hops a projection implies, so the
    /// joinLevel comparison upstream is made against the deepest branch.
    #[test]
    fn projection_depth_counts_the_deepest_branch() {
        let ctx = Loader::new().core();
        let d = |s: &str| proj_depth(&parse_projection(s, &ctx).expect("parse"));
        assert_eq!(d("a,b"), 0, "a flat selection implies no hop");
        assert_eq!(d("a{b}"), 1);
        assert_eq!(d("a,b{c{d}}"), 2, "the deepest branch wins");
    }
}

#[cfg(test)]
mod clause_6_3_7 {
    use super::*;
    use antares_jsonld::Loader;
    use serde_json::json;

    fn params(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    /// 6.3.7: an unknown options or format value is not silently ignored.
    #[test]
    fn unknown_options_and_format_values_are_rejected() {
        let ctx = Loader::new().core();
        for p in [
            params(&[("options", "sysattrs")]),
            params(&[("options", "keyValues,bogus")]),
            params(&[("options", "")]),
            params(&[("format", "verbose")]),
            params(&[("format", "KeyValues")]),
        ] {
            let e = parse_repr(&p, &ctx).expect_err("must be rejected");
            assert!(
                matches!(e, NgsiError::InvalidRequest(_)),
                "unsupported representation value is InvalidRequest, got {e:?}"
            );
        }
    }

    /// 6.3.7: format wins over options when the two disagree, and keyValues
    /// is the older spelling of simplified.
    #[test]
    fn format_wins_over_options_on_conflict() {
        let ctx = Loader::new().core();
        let r = parse_repr(
            &params(&[("options", "concise"), ("format", "simplified")]),
            &ctx,
        )
        .expect("parse");
        assert!(r.key_values, "format=simplified wins");
        assert!(!r.concise, "options=concise must NOT survive the conflict");
        let r = parse_repr(
            &params(&[("options", "keyValues"), ("format", "normalized")]),
            &ctx,
        )
        .expect("parse");
        assert!(!r.key_values && !r.concise, "normalized is neither");
        let r = parse_repr(&params(&[("options", "sysAttrs,keyValues")]), &ctx).expect("parse");
        assert!(r.sys_attrs && r.key_values);
    }

    /// 4.21: pick, omit and attrs are mutually exclusive — any pair is a 400,
    /// each one alone is fine.
    #[test]
    fn pick_omit_and_attrs_cannot_be_combined() {
        let ctx = Loader::new().core();
        for p in [
            params(&[("pick", "a"), ("omit", "b")]),
            params(&[("pick", "a"), ("attrs", "b")]),
            params(&[("omit", "a"), ("attrs", "b")]),
            params(&[("pick", "a"), ("omit", "b"), ("attrs", "c")]),
        ] {
            let e = parse_repr(&p, &ctx).expect_err("must be rejected");
            assert!(matches!(e, NgsiError::BadRequestData(_)), "got {e:?}");
        }
        assert!(parse_repr(&params(&[("pick", "a")]), &ctx).is_ok());
        assert!(parse_repr(&params(&[("attrs", "a")]), &ctx).is_ok());
    }

    /// attrs= selects ATTRIBUTES: an entity meta member or @context is not an
    /// attribute name, and an empty member is a grammar violation.
    #[test]
    fn attrs_rejects_entity_members_and_empty_names() {
        let ctx = Loader::new().core();
        for bad in ["id", "type", "scope", "createdAt", "@context", "a,,b", ""] {
            assert!(
                parse_repr(&params(&[("attrs", bad)]), &ctx).is_err(),
                "attrs={bad:?} must be rejected"
            );
        }
        let r = parse_repr(&params(&[("attrs", "temperature, humidity")]), &ctx).expect("parse");
        let list = r.attrs.expect("attrs");
        assert_eq!(list.len(), 2);
        assert!(list.iter().all(|a| a.contains("://")), "names are expanded");
    }

    fn entity() -> Value {
        json!({
            "id": "urn:ngsi-ld:E:1",
            "type": "T",
            "createdAt": "2026-01-01T00:00:00Z",
            "modifiedAt": "2026-01-02T00:00:00Z",
            "https://example.org/temperature": [
                {"type": "Property", "value": 21,
                 "createdAt": "2026-01-01T00:00:00Z",
                 "https://example.org/accuracy": [{"type": "Property", "value": 0.5}]},
                {"type": "Property", "value": 9, "datasetId": "urn:ds:2"}
            ]
        })
    }

    /// 6.3.11 Table 6.3.11-1: createdAt/modifiedAt are system attributes —
    /// they must be absent from the default representation, on the entity AND
    /// on every attribute instance, and present with options=sysAttrs.
    #[test]
    fn system_attributes_stay_hidden_without_sysattrs() {
        let plain = apply(&entity(), &Repr::default());
        assert!(plain.get("createdAt").is_none(), "entity createdAt leaked");
        assert!(
            plain.get("modifiedAt").is_none(),
            "entity modifiedAt leaked"
        );
        assert!(
            plain["https://example.org/temperature"][0]
                .get("createdAt")
                .is_none(),
            "instance createdAt leaked"
        );
        assert_eq!(plain["id"], "urn:ngsi-ld:E:1");
        let sys = apply(
            &entity(),
            &Repr {
                sys_attrs: true,
                ..Repr::default()
            },
        );
        assert_eq!(sys["createdAt"], "2026-01-01T00:00:00Z");
        assert_eq!(
            sys["https://example.org/temperature"][0]["createdAt"],
            "2026-01-01T00:00:00Z"
        );
    }

    /// 4.5.4 concise: the type member is dropped and an instance left with a
    /// lone value collapses to that bare value — sub-attributes included.
    #[test]
    fn concise_drops_type_and_collapses_bare_values() {
        let out = apply(
            &entity(),
            &Repr {
                concise: true,
                ..Repr::default()
            },
        );
        let inst = &out["https://example.org/temperature"][0];
        assert!(inst.get("type").is_none(), "concise keeps no type member");
        assert_eq!(
            inst["https://example.org/accuracy"],
            json!([0.5]),
            "a value-only sub-attribute collapses to the bare value"
        );
        // an instance carrying more than value keeps the object form
        let d = apply(
            &json!({"https://example.org/a": [{"type": "Property", "value": 1, "unitCode": "CEL"}]}),
            &Repr {
                concise: true,
                ..Repr::default()
            },
        );
        assert_eq!(d["https://example.org/a"][0]["unitCode"], "CEL");
        assert_eq!(d["https://example.org/a"][0]["value"], 1);
    }

    /// 4.5.4: with several instances the simplified form is a dataset map
    /// keyed by datasetId, "@none" standing for the default instance — a bare
    /// array of values would lose which instance is which. The map carries one
    /// pair for each datasetId, so a dropped instance is a dropped datasetId.
    #[test]
    fn simplified_multi_instance_uses_the_dataset_map() {
        let out = apply(
            &entity(),
            &Repr {
                key_values: true,
                ..Repr::default()
            },
        );
        let t = &out["https://example.org/temperature"];
        assert!(t.get("dataset").is_some(), "multi-instance needs the map");
        assert_eq!(t["dataset"]["@none"], 21);
        assert_eq!(t["dataset"]["urn:ds:2"], 9);
        assert!(t.as_array().is_none(), "not a bare array");
        // one instance stays a bare value
        let one = apply(
            &json!({"https://example.org/a": [{"type": "Property", "value": 1}]}),
            &Repr {
                key_values: true,
                ..Repr::default()
            },
        );
        assert_eq!(one["https://example.org/a"], json!(1));
        // 4.5.4 EXAMPLE 2 is three instances; the map is built by pairing
        // instances with the rows they came from, so an instance lost on the
        // way leaves a map one pair short rather than an error
        let three = apply(
            &json!({"https://example.org/name": [
                {"type": "Property", "value": "David Robert Jones"},
                {"type": "Property", "value": "David Bowie",
                 "datasetId": "urn:ngsi-ld:datasetId:001"},
                {"type": "Property", "value": "Ziggy Stardust",
                 "datasetId": "urn:ngsi-ld:datasetId:002"}
            ]}),
            &Repr {
                key_values: true,
                ..Repr::default()
            },
        );
        let ds = three["https://example.org/name"]["dataset"]
            .as_object()
            .expect("dataset map");
        assert_eq!(ds.len(), 3, "one pair for each datasetId");
        assert_eq!(ds["@none"], "David Robert Jones");
        assert_eq!(ds["urn:ngsi-ld:datasetId:001"], "David Bowie");
        assert_eq!(ds["urn:ngsi-ld:datasetId:002"], "Ziggy Stardust");
    }

    /// datasetId= selects instances; "@none" selects the default one. An
    /// attribute left with no surviving instance is absent, not empty.
    #[test]
    fn dataset_id_filter_selects_instances_and_drops_empty_attributes() {
        let sel = |ids: &[&str]| {
            apply(
                &entity(),
                &Repr {
                    dataset_id: Some(ids.iter().map(|s| (*s).to_owned()).collect()),
                    ..Repr::default()
                },
            )
        };
        let out = sel(&["urn:ds:2"]);
        let insts = out["https://example.org/temperature"]
            .as_array()
            .expect("array");
        assert_eq!(insts.len(), 1);
        assert_eq!(insts[0]["value"], 9, "the default instance must be gone");
        let out = sel(&["@none"]);
        let insts = out["https://example.org/temperature"]
            .as_array()
            .expect("array");
        assert_eq!(insts.len(), 1);
        assert_eq!(insts[0]["value"], 21);
        let out = sel(&["urn:ds:absent"]);
        assert!(
            out.get("https://example.org/temperature").is_none(),
            "an attribute with no matching instance is omitted entirely"
        );
        assert_eq!(out["id"], "urn:ngsi-ld:E:1", "entity members survive");
    }

    /// The reserved members of an attribute instance carry values, not
    /// sub-attributes: an array-valued `value` (or objectList, previousValue…)
    /// must be passed through untouched, never walked as a list of instances.
    #[test]
    fn array_valued_reserved_members_are_not_sub_attributes() {
        let doc = json!({"https://example.org/a": [{
            "type": "ListProperty",
            "valueList": [1, 2, 3],
            "value": [{"type": "Property", "value": 7}],
            "previousValue": [9],
            "https://example.org/note": [{"type": "Property", "value": "sub"}]
        }]});
        let out = apply(&doc, &Repr::default());
        let inst = &out["https://example.org/a"][0];
        assert_eq!(inst["valueList"], json!([1, 2, 3]));
        assert_eq!(
            inst["value"],
            json!([{"type": "Property", "value": 7}]),
            "an array value is data, not an instance list to transform"
        );
        assert_eq!(inst["previousValue"], json!([9]));
        // a genuine sub-attribute IS walked
        assert_eq!(inst["https://example.org/note"][0]["value"], "sub");
        // …and the walk applies the representation to it
        let sys = apply(
            &json!({"https://example.org/a": [{"type": "Property", "value": 1,
                "https://example.org/note": [{"type": "Property", "value": "s",
                    "modifiedAt": "2026-01-01T00:00:00Z"}]}]}),
            &Repr::default(),
        );
        assert!(
            sys["https://example.org/a"][0]["https://example.org/note"][0]
                .get("modifiedAt")
                .is_none(),
            "the sysAttrs gate reaches sub-attributes"
        );
    }

    /// 4.21: pick constrains core members too — an entity member not picked
    /// does not survive; omit only drops the heads it names outright.
    #[test]
    fn pick_is_strict_over_core_members_and_omit_is_not() {
        let ctx = Loader::new().core();
        let picked = apply(
            &entity(),
            &Repr {
                pick: Some(parse_projection("id", &ctx).expect("pick")),
                ..Repr::default()
            },
        );
        assert_eq!(picked["id"], "urn:ngsi-ld:E:1");
        assert!(picked.get("type").is_none(), "type was not picked");
        assert!(
            picked.get("https://example.org/temperature").is_none(),
            "an unpicked attribute must not survive"
        );
        let omitted = apply(
            &entity(),
            &Repr {
                omit: Some(parse_projection("scope", &ctx).expect("omit")),
                ..Repr::default()
            },
        );
        assert_eq!(omitted["type"], "T", "omit leaves the rest of the entity");
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
