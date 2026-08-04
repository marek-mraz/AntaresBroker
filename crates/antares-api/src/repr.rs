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
            .all(|c| c.is_ascii_alphanumeric() || "_,.:{}#/%-+@".contains(c))
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
                ',' if depth == 0 => {
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

fn select_lang(lm: &Map<String, Value>, lang: &str) -> Option<(String, Value)> {
    if lang != "*" {
        for want in lang.split(',') {
            let want = want.trim().split(';').next().unwrap_or("").trim();
            if let Some(v) = lm.get(want) {
                return Some((want.to_owned(), v.clone()));
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

fn simplified_value(obj: &Map<String, Value>) -> Value {
    for k in [
        "value",
        "object",
        "languageMap",
        "vocab",
        "valueList",
        "objectList",
    ] {
        if let Some(v) = obj.get(k) {
            if k == "json" {
                return v.clone();
            }
            return v.clone();
        }
    }
    if let Some(v) = obj.get("json") {
        return v.clone();
    }
    Value::Object(obj.clone())
}
