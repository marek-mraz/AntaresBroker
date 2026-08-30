// SPDX-License-Identifier: EUPL-1.2
//! Representation transforms (6.3.7, 4.5.4, concise, sysAttrs, attrs
//! projection, lang filter) — applied on the INTERNAL expanded form before
//! compaction.

use antares_jsonld::Context;
use antares_model::NgsiError;
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
            if t.is_empty() || ENTITY_META.contains(&t) || t == "@context" {
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

pub(crate) const ENTITY_META: &[&str] = &[
    "id",
    "type",
    "scope",
    "createdAt",
    "modifiedAt",
    "deletedAt",
    "expiresAt",
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
        let instances: Vec<Value> = kept
            .iter()
            .map(|inst| transform_instance(inst, r))
            .collect();
        if instances.is_empty() {
            continue;
        }
        if r.key_values {
            if instances.len() == 1 {
                out.insert(k.clone(), instances.into_iter().next().expect("one"));
            } else {
                // 4.5.4 multi-instance simplified form: a "dataset" map keyed
                // by datasetId ("@none" for the default instance)
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
        if obj.get("type").and_then(Value::as_str) == Some("LanguageProperty") {
            if let Some(lm) = obj.get("languageMap").and_then(Value::as_object).cloned() {
                let pick = select_lang(&lm, lang);
                if let Some((chosen_lang, value)) = pick {
                    obj.remove("languageMap");
                    obj.insert("type".into(), Value::String("Property".into()));
                    obj.insert("value".into(), value);
                    obj.insert("lang".into(), Value::String(chosen_lang));
                }
            }
        }
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
        if v.is_array() && !is_reserved_member(k) {
            let subs: Vec<Value> = v
                .as_array()
                .expect("array")
                .iter()
                .map(|i| transform_instance(i, r))
                .collect();
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
    /// array of values would lose which instance is which.
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
}
