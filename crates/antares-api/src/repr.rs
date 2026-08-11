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
    // 4.21: pick, omit and attrs are mutually exclusive
    let excl = ["pick", "omit", "attrs"]
        .iter()
        .filter(|k| params.contains_key(**k))
        .count();
    if excl > 1 {
        return Err(NgsiError::BadRequestData(
            "pick, omit and attrs are mutually exclusive (4.21)".into(),
        ));
    }
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

const ENTITY_META: &[&str] = &[
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
    if s.is_empty()
        || s.matches('{').count() != s.matches('}').count()
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
                "createdAt" | "modifiedAt" if !r.sys_attrs => continue,
                _ => {}
            }
            // pick strictly constrains core members too (4.21)
            if let Some(pick) = &r.pick {
                if !pick.iter().any(|n| n.raw == *k) {
                    continue;
                }
            }
            if let Some(omit) = &r.omit {
                if omit.iter().any(|n| n.raw == *k && n.children.is_none()) {
                    continue;
                }
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
            "createdAt" | "modifiedAt" if !r.sys_attrs => continue,
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
}
