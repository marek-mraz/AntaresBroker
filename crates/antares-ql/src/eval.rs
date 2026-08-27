// SPDX-License-Identifier: EUPL-1.2
//! In-memory `q=` evaluation against internal expanded entities — the
//! evaluator the query path and the subscription matcher share, so the two
//! cannot disagree; a gateway evaluating the same `q` gets the same answer.

use crate::{CmpOp, QNode, QPath, QValue};
use antares_jsonld::Context;
use serde_json::Value;
use std::cell::Cell;

/// Entity lookups one `q=` expression may buy while resolving 4.9
/// linked-entity terms (`attr{…}`, EXAMPLE 13/14). The hop count is capped
/// by the query language itself, but each hop fans out over every object of
/// a Relationship, so the walk costs fan-out^hops store reads. Exhausting
/// the budget yields no further target — the same outcome an unresolvable
/// linked entity already has — instead of a store scan per candidate entity.
pub const MAX_Q_LINK_LOOKUPS: usize = 512;

/// Entity resolver for 4.9 linked-entity subqueries (`attr{path}`,
/// EXAMPLE 13/14). Returns the expanded entity for a URI, or None when the
/// entity is unknown or the evaluation context has no store access — a
/// linked term then simply does not match.
pub type EntityLookup<'a> = &'a dyn Fn(&str) -> Option<Value>;

/// One q expression buys `MAX_Q_LINK_LOOKUPS` entity lookups for its
/// 4.9 linked-entity terms, shared by every term and every recursion branch.
pub fn eval_q(node: &QNode, entity: &Value, ctx: &Context, lookup: EntityLookup) -> bool {
    eval_node(node, entity, ctx, lookup, &Cell::new(MAX_Q_LINK_LOOKUPS))
}

fn eval_node(
    node: &QNode,
    entity: &Value,
    ctx: &Context,
    lookup: EntityLookup,
    budget: &Cell<usize>,
) -> bool {
    match node {
        QNode::And(items) => items
            .iter()
            .all(|n| eval_node(n, entity, ctx, lookup, budget)),
        QNode::Or(items) => items
            .iter()
            .any(|n| eval_node(n, entity, ctx, lookup, budget)),
        QNode::Exists { path, negated } => {
            let found = !resolve_qpath(entity, path, ctx, lookup, budget).is_empty();
            found != *negated
        }
        // 4.9: "If the target element corresponds to a Relationship or
        // ListRelationship, the combination of such target element with any
        // operator different than equal or unequal shall result in not
        // matching."
        QNode::Cmp { path, op, value } => {
            // The pattern of `~=` / `!~=` belongs to the Query Term, not to
            // the target: it is compiled once per term instead of once per
            // candidate value, and the compiled program is shared
            // process-wide, so re-evaluating the same term over the next
            // candidate entity or the next event costs no compile at all.
            // A pattern that does not compile has no L(R), so neither
            // operator matches (4.9 p.92) — that is what the `None` below
            // means downstream.
            let re = match (op, value) {
                (CmpOp::Pattern | CmpOp::NotPattern, QValue::Str(s)) => {
                    crate::regex::compile(s).ok()
                }
                _ => None,
            };
            resolve_qpath(entity, path, ctx, lookup, budget)
                .iter()
                .any(|(kind, v)| kind_allows(*kind, *op) && compare(v, *op, value, re.as_deref()))
        }
    }
}

/// 4.9 expandValues: rewrite the string values of query terms whose
/// top-level attribute is named in the comma-separated `expandValues` list —
/// each is expanded against the @context (JSON-LD type coercion), so e.g.
/// `gender==Male&expandValues=gender` compares against the Male URI
/// (EXAMPLE 12).
pub fn apply_expand_values(node: QNode, expand_values: Option<&str>, ctx: &Context) -> QNode {
    let Some(list) = expand_values else {
        return node;
    };
    let names: Vec<&str> = list.split(',').map(str::trim).collect();
    fn expand_val(v: QValue, ctx: &Context) -> QValue {
        match v {
            QValue::Str(s) => QValue::Str(ctx.expand_key(&s)),
            QValue::List(items) => {
                QValue::List(items.into_iter().map(|i| expand_val(i, ctx)).collect())
            }
            other => other,
        }
    }
    fn walk(node: QNode, names: &[&str], ctx: &Context) -> QNode {
        match node {
            QNode::And(items) => {
                QNode::And(items.into_iter().map(|n| walk(n, names, ctx)).collect())
            }
            QNode::Or(items) => QNode::Or(items.into_iter().map(|n| walk(n, names, ctx)).collect()),
            QNode::Cmp { path, op, value } if path.top().is_some_and(|t| names.contains(&t)) => {
                QNode::Cmp {
                    path,
                    op,
                    value: expand_val(value, ctx),
                }
            }
            other => other,
        }
    }
    walk(node, &names, ctx)
}

/// Which value-defining member the target element carried.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TargetKind {
    Value,
    Object,
    LanguageMap,
    Vocab,
    Json,
    ValueList,
    ObjectList,
}

fn kind_allows(kind: TargetKind, op: CmpOp) -> bool {
    match kind {
        TargetKind::Object | TargetKind::ObjectList => matches!(op, CmpOp::Eq | CmpOp::Ne),
        _ => true,
    }
}

/// Resolve a 4.9 attribute path — linked-entity hops first (EXAMPLE 13/14),
/// then the dotted path with its optional trailing bracket.
///
/// The hop count is capped by the query language, but each hop fans out over
/// every object of the Relationship, so the walk is bounded by WORK: the
/// shared `budget` counts entity lookups across every branch, and an
/// exhausted budget resolves to no target — the outcome an unresolvable
/// linked entity already has.
fn resolve_qpath(
    entity: &Value,
    qp: &QPath,
    ctx: &Context,
    lookup: EntityLookup,
    budget: &Cell<usize>,
) -> Vec<(TargetKind, Value)> {
    let Some(link) = qp.links.first() else {
        return resolve_targets(entity, qp, ctx);
    };
    let iri = ctx.expand_key(&link.attr);
    let Some(instances) = entity.get(&iri).and_then(Value::as_array) else {
        return vec![];
    };
    let mut uris: Vec<&str> = Vec::new();
    for inst in instances {
        match inst.get("object") {
            Some(Value::String(s)) => uris.push(s),
            Some(Value::Array(a)) => uris.extend(a.iter().filter_map(Value::as_str)),
            _ => {}
        }
        if let Some(Value::Array(a)) = inst.get("objectList") {
            uris.extend(a.iter().filter_map(Value::as_str));
        }
    }
    let rest = QPath {
        links: qp.links[1..].to_vec(),
        path: qp.path.clone(),
        bracket: qp.bracket.clone(),
    };
    let mut out = Vec::new();
    for uri in uris {
        let Some(left) = budget.get().checked_sub(1) else {
            break;
        };
        budget.set(left);
        let Some(linked) = lookup(uri) else { continue };
        // EXAMPLE 14 type hint: only consider target entities of these types
        if !link.types.is_empty() {
            let matched = linked["type"].as_array().is_some_and(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .any(|t| link.types.iter().any(|hint| ctx.expand_key(hint) == t))
            });
            if !matched {
                continue;
            }
        }
        out.extend(resolve_qpath(&linked, &rest, ctx, lookup, budget));
    }
    out
}

/// Resolve a dotted q path to candidate (kind, value) targets across
/// instances.
fn resolve_targets(entity: &Value, qp: &QPath, ctx: &Context) -> Vec<(TargetKind, Value)> {
    let Some(first) = qp.path.first() else {
        return vec![];
    };
    let iri = ctx.expand_key(first);
    let Some(instances) = entity.get(&iri).and_then(Value::as_array) else {
        return vec![];
    };
    let mut out = Vec::new();
    for inst in instances {
        collect(inst, &qp.path[1..], qp.bracket.as_deref(), ctx, &mut out);
    }
    out
}

fn collect(
    inst: &Value,
    rest: &[String],
    bracket: Option<&[String]>,
    ctx: &Context,
    out: &mut Vec<(TargetKind, Value)>,
) {
    if rest.is_empty() {
        terminal(inst, bracket, ctx, out);
        return;
    }
    let seg = &rest[0];
    // 1. sub-attribute step (expanded key)
    let iri = ctx.expand_key(seg);
    if let Some(subs) = inst.get(&iri).and_then(Value::as_array) {
        for s in subs {
            collect(s, &rest[1..], bracket, ctx, out);
        }
        return;
    }
    // 2. legacy value-path step: navigate into the value object (pre-bracket
    // dotted access, kept as a superset of the 4.9 bracket form)
    if let Some((kind, v)) = comparable_value(inst) {
        if let Some(nested) = navigate(v, rest) {
            let nested = nested.clone();
            match bracket {
                None => out.push((kind, nested)),
                Some(b) => {
                    if let Some(deeper) = navigate(&nested, b) {
                        out.push((kind, deeper.clone()));
                    }
                }
            }
        }
    }
}

/// Terminal instance: extract the target value, applying the trailing
/// bracket — a language filter on a LanguageProperty (4.9 Equal/Unequal
/// languageMap semantics), a MemberExpression into a compound value
/// (EXAMPLE 9/10/11) otherwise.
fn terminal(
    inst: &Value,
    bracket: Option<&[String]>,
    ctx: &Context,
    out: &mut Vec<(TargetKind, Value)>,
) {
    let Some((kind, v)) = comparable_value(inst) else {
        return;
    };
    match (bracket, kind) {
        (None, TargetKind::Vocab) => {
            // 4.9: "If the target element is a VocabProperty, the target
            // value shall be expanded according to the @context."
            out.push((kind, expand_vocab(v, ctx)));
        }
        (None, _) => out.push((kind, v.clone())),
        (Some(b), TargetKind::LanguageMap) => {
            let Some(map) = v.as_object() else { return };
            if b.len() != 1 {
                return;
            }
            if b[0] == "*" {
                // any language: ONE array target so that != requires no
                // matching value in ANY language (4.9 Unequal, color[*])
                let mut all = Vec::new();
                for val in map.values() {
                    match val {
                        Value::Array(a) => all.extend(a.iter().cloned()),
                        other => all.push(other.clone()),
                    }
                }
                out.push((kind, Value::Array(all)));
            } else if let Some(val) = map.get(&b[0]) {
                out.push((kind, val.clone()));
            }
        }
        (Some(b), _) => {
            // MemberExpression into the compound value; undefined result =
            // "the target element shall be considered as non-existent"
            if let Some(nested) = navigate(v, b) {
                out.push((kind, nested.clone()));
            }
        }
    }
}

/// Expand a vocab value (string or array of strings) against the @context.
fn expand_vocab(v: &Value, ctx: &Context) -> Value {
    match v {
        Value::String(s) => Value::String(ctx.expand_key(s)),
        Value::Array(a) => Value::Array(a.iter().map(|x| expand_vocab(x, ctx)).collect()),
        other => other.clone(),
    }
}

fn navigate<'a>(v: &'a Value, path: &[String]) -> Option<&'a Value> {
    let mut cur = v;
    for seg in path {
        cur = cur.get(seg)?;
    }
    Some(cur)
}

fn comparable_value(inst: &Value) -> Option<(TargetKind, &Value)> {
    let obj = inst.as_object()?;
    for (k, kind) in [
        ("value", TargetKind::Value),
        ("object", TargetKind::Object),
        ("languageMap", TargetKind::LanguageMap),
        ("vocab", TargetKind::Vocab),
        ("json", TargetKind::Json),
        ("valueList", TargetKind::ValueList),
        ("objectList", TargetKind::ObjectList),
    ] {
        if let Some(v) = obj.get(k) {
            return Some((kind, v));
        }
    }
    None
}

/// Do target and Query Term value share a datatype? 4.9 hangs two opposite
/// rules on this: Equal (and the ordering operators) treat a mismatch as "not
/// matching", Unequal treats it as unequal — i.e. a MATCH.
fn same_datatype(target: &Value, want: &QValue) -> bool {
    match want {
        QValue::Num(_) => target.is_number(),
        QValue::Str(_) => target.is_string(),
        QValue::Bool(_) => target.is_boolean(),
        // a Range's value space is its endpoints' (the parser pins both to
        // one variant); Lists never reach this guard — they are unfolded
        // into per-element compares first, each applying its own rule
        QValue::Range(lo, _) => same_datatype(target, lo),
        QValue::List(_) => true,
    }
}

/// `re` is the pre-compiled pattern of the enclosing Query Term, `None`
/// when the operator is not a pattern operator or the pattern is invalid.
fn compare(target: &Value, op: CmpOp, want: &QValue, re: Option<&regex::Regex>) -> bool {
    // 4.9 ValueList — Equal p.90: "identical or equivalent to ANY of the list
    // values"; Unequal p.91: "neither identical nor equivalent to any of the
    // list values" / "does not include ANY of the list values" — i.e. every
    // per-element != must hold. Unfold before the array unwrap so the array
    // rules apply per list element.
    if let QValue::List(vals) = want {
        return match op {
            CmpOp::Eq => vals.iter().any(|v| compare(target, op, v, re)),
            CmpOp::Ne => vals.iter().all(|v| compare(target, op, v, re)),
            _ => false, // grammar-unreachable (parser rejects), stay safe
        };
    }
    if let Value::Array(items) = target {
        // 4.9 Unequal, p.91: "The target value does not include any of the list
        // values, if the target value is an array (e.g. matches
        // ["blue","black","green"], but not ["blue","red","green"])" — so for
        // `!=` EVERY element must differ. Same reading for `!~=` ("shall not
        // be in L(R)" — one matching element would be in it). `.any()` is
        // right for the rest.
        return match op {
            CmpOp::Ne | CmpOp::NotPattern => items.iter().all(|i| compare(i, op, want, re)),
            _ => items.iter().any(|i| compare(i, op, want, re)),
        };
    }
    // 4.9 Unequal, p.92: "If the data type of the target value and the data
    // type of the Query Term value are different, then they shall be
    // considered unequal." Equal carries the mirror-image rule, so this guard
    // is deliberately asymmetric and must run before the casts below — which
    // all return false on a failed cast. (`!~=` is NOT symmetric with `!=`
    // here: p.92 "If the target value data type is different than String then
    // it shall be considered as not matching" — so no early true for it.)
    if op == CmpOp::Ne && !same_datatype(target, want) {
        return true;
    }
    // 4.9 Range — Equal p.90: "in the interval between the minimum and
    // maximum of the range (both included)"; Unequal p.91: "not in the
    // interval".
    if let QValue::Range(lo, hi) = want {
        return match op {
            CmpOp::Eq => in_range(target, lo, hi),
            CmpOp::Ne => !in_range(target, lo, hi),
            _ => false, // grammar-unreachable (parser rejects), stay safe
        };
    }
    match want {
        QValue::Num(n) => {
            let Some(t) = target.as_f64() else {
                return false;
            };
            num_cmp(t, op, *n)
        }
        QValue::Bool(b) => match op {
            CmpOp::Eq => target.as_bool() == Some(*b),
            CmpOp::Ne => target.as_bool().is_some_and(|t| t != *b),
            _ => false,
        },
        QValue::Str(s) => {
            let Some(t) = target.as_str() else {
                return false;
            };
            match op {
                CmpOp::Eq => t == s,
                CmpOp::Ne => t != s,
                CmpOp::Gt => t > s.as_str(),
                CmpOp::Ge => t >= s.as_str(),
                CmpOp::Lt => t < s.as_str(),
                CmpOp::Le => t <= s.as_str(),
                CmpOp::Pattern => re.is_some_and(|re| re.is_match(t)),
                // p.92: target "shall not be in the L(R)" — an invalid regex
                // has no L(R), treat as not matching (same posture as ~=)
                CmpOp::NotPattern => re.is_some_and(|re| !re.is_match(t)),
            }
        }
        QValue::List(_) | QValue::Range(..) => unreachable!("handled above"),
    }
}

/// `t ∈ [lo, hi]`, both included (4.9 p.90). The parser guarantees both
/// endpoints share one variant and are never booleans.
fn in_range(target: &Value, lo: &QValue, hi: &QValue) -> bool {
    match (lo, hi) {
        (QValue::Num(a), QValue::Num(b)) => target.as_f64().is_some_and(|t| t >= *a && t <= *b),
        (QValue::Str(a), QValue::Str(b)) => target
            .as_str()
            .is_some_and(|t| t >= a.as_str() && t <= b.as_str()),
        _ => false,
    }
}

fn num_cmp(t: f64, op: CmpOp, n: f64) -> bool {
    match op {
        CmpOp::Eq => t == n,
        CmpOp::Ne => t != n,
        CmpOp::Gt => t > n,
        CmpOp::Ge => t >= n,
        CmpOp::Lt => t < n,
        CmpOp::Le => t <= n,
        CmpOp::Pattern | CmpOp::NotPattern => false,
    }
}

#[cfg(test)]
mod clause_4_9_extensions {
    use super::*;
    use crate::parse_q;
    use antares_jsonld::{Context, Loader};
    use serde_json::json;

    fn ctx() -> std::sync::Arc<Context> {
        Loader::new().core()
    }

    fn expand(doc: serde_json::Value) -> Value {
        antares_jsonld::expand_entity(
            doc.as_object().expect("obj"),
            &ctx(),
            antares_jsonld::ExpandOpts::default(),
        )
        .expect("expand")
    }

    fn q(doc: &Value, q: &str) -> bool {
        let ast = parse_q(q).expect(q);
        eval_q(&ast, doc, &ctx(), &|_| None)
    }

    /// 4.9 EXAMPLE 9/10/11: trailing [path] navigates the compound value
    /// (MemberExpression); undefined member = target non-existent.
    #[test]
    fn compound_value_trailing_path() {
        let e = expand(json!({"id": "urn:x", "type": "T",
            "address": {"type": "Property",
                "value": {"city": "Berlin", "street": "Ulrich Strasse"}},
            "sensor": {"type": "Property", "value": 40,
                "rawdata": {"type": "Property",
                    "value": {"airquality": {"particulate": 40, "PM20": 85}}}},
            "parkingTickets": {"type": "JsonProperty",
                "json": {"id": "85a6cc52", "value": "Overstay 60 minutes"}}}));
        assert!(q(&e, r#"address[city]=="Berlin""#), "EXAMPLE 9");
        assert!(!q(&e, r#"address[city]=="Paris""#));
        assert!(!q(&e, r#"address[postcode]=="Berlin""#), "undefined member");
        assert!(
            q(&e, "sensor.rawdata[airquality.particulate]==40"),
            "EXAMPLE 10"
        );
        assert!(!q(&e, "sensor.rawdata[airquality.missing]==40"));
        assert!(
            q(&e, r#"parkingTickets[value]=="Overstay 60 minutes""#),
            "EXAMPLE 11 (JsonProperty raw json navigation)"
        );
        // existence through the bracket: defined member exists, missing not
        assert!(q(&e, "address[city]"));
        assert!(!q(&e, "address[postcode]"));
    }

    /// 4.9 Equal/Unequal languageMap semantics: [lang] targets one language,
    /// [*] any; != over [*] requires NO value to match.
    #[test]
    fn language_property_filters() {
        let e = expand(json!({"id": "urn:x", "type": "T",
            "color": {"type": "LanguageProperty",
                "languageMap": {"fr": "rouge", "en": "red", "de": "rot"}},
            "names": {"type": "LanguageProperty",
                "languageMap": {"fr": ["chat", "rouge"], "en": ["red", "cat"]}}}));
        assert!(q(&e, r#"color[en]=="red""#));
        assert!(
            !q(&e, r#"color[en]=="rouge""#),
            "wrong language must not match"
        );
        assert!(q(&e, r#"color[*]=="rouge""#), "any-language match");
        assert!(!q(&e, r#"color[*]=="blau""#));
        assert!(
            q(&e, r#"names[en]=="cat""#),
            "array element in one language"
        );
        // Unequal: no matching value in ANY of the values
        assert!(q(&e, r#"color[en]!="rouge""#));
        assert!(!q(&e, r#"color[en]!="red""#));
        assert!(!q(&e, r#"color[*]!="red""#), "some language holds red");
        assert!(q(&e, r#"color[*]!="blau""#));
    }

    /// 4.9: "If the target element is a VocabProperty, the target value shall
    /// be expanded according to the @context" — the default-context expansion
    /// makes a URI out of the vocab term, so only URI comparisons match.
    #[test]
    fn vocab_property_target_expansion() {
        let e = expand(json!({"id": "urn:x", "type": "T",
            "category": {"type": "VocabProperty", "vocab": "commercial"}}));
        assert!(
            q(
                &e,
                r#"category=="https://uri.etsi.org/ngsi-ld/default-context/commercial""#
            ),
            "expanded URI equality"
        );
        assert!(
            !q(&e, r#"category=="somethingelse""#),
            "non-matching literal"
        );
    }

    /// 4.9: "If the target element corresponds to a Relationship or
    /// ListRelationship, the combination of such target element with any
    /// operator different than equal or unequal shall result in not matching."
    #[test]
    fn relationship_ordering_operators_never_match() {
        let e = expand(json!({"id": "urn:x", "type": "T",
            "isParked": {"type": "Relationship", "object": "urn:ngsi-ld:P:5"}}));
        assert!(q(&e, r#"isParked=="urn:ngsi-ld:P:5""#));
        assert!(
            !q(&e, r#"isParked>"urn:ngsi-ld:P:4""#),
            "ordering op on Relationship"
        );
        assert!(!q(&e, r#"isParked<"urn:ngsi-ld:P:6""#));
        assert!(!q(&e, r#"isParked~="urn.*""#), "pattern op on Relationship");
    }

    /// 4.9 EXAMPLE 12: expandValues coerces the query term value through the
    /// @context, so a VocabProperty short term matches its expanded URI.
    #[test]
    fn expand_values_coercion() {
        let e = expand(json!({"id": "urn:x", "type": "T",
            "category": {"type": "VocabProperty", "vocab": "commercial"}}));
        let ast = parse_q("category==commercial").expect("parse");
        assert!(
            !eval_q(&ast, &e, &ctx(), &|_| None),
            "without expandValues the literal does not match the expanded vocab"
        );
        let ast = apply_expand_values(ast, Some("category"), &ctx());
        assert!(eval_q(&ast, &e, &ctx(), &|_| None), "EXAMPLE 12");
        // other attributes' values stay untouched
        let ast = apply_expand_values(
            parse_q(r#"other=="commercial""#).expect("parse"),
            Some("category"),
            &ctx(),
        );
        match ast {
            QNode::Cmp { value, .. } => assert_eq!(value, QValue::Str("commercial".into())),
            other => panic!("unexpected {other:?}"),
        }
    }

    /// 4.9 EXAMPLE 13/14: linked entity subquery attr{[Type:]path} follows
    /// the Relationship object through the resolver; a missing resolver or
    /// non-matching type hint yields no match.
    #[test]
    fn linked_entity_subquery() {
        let station = expand(json!({"id": "urn:ngsi-ld:WS:123", "type": "WeatherStation",
            "sensor": {"type": "Relationship", "object": "urn:ngsi-ld:Device:345"}}));
        let device = expand(json!({"id": "urn:ngsi-ld:Device:345", "type": "Device",
            "humidity": {"type": "Property", "value": 40}}));
        let lookup = |id: &str| (id == "urn:ngsi-ld:Device:345").then(|| device.clone());
        let ast = parse_q("sensor{humidity}==40").expect("parse");
        assert!(eval_q(&ast, &station, &ctx(), &lookup), "EXAMPLE 13");
        let ast = parse_q("sensor{humidity}==50").expect("parse");
        assert!(!eval_q(&ast, &station, &ctx(), &lookup));
        // EXAMPLE 14: type hint — matching and non-matching
        let ast = parse_q("sensor{Device:humidity}==40").expect("parse");
        assert!(eval_q(&ast, &station, &ctx(), &lookup), "EXAMPLE 14");
        let ast = parse_q("sensor{Vehicle:humidity}==40").expect("parse");
        assert!(
            !eval_q(&ast, &station, &ctx(), &lookup),
            "type hint must filter"
        );
        // no resolver → no match, never an error
        let ast = parse_q("sensor{humidity}==40").expect("parse");
        assert!(!eval_q(&ast, &station, &ctx(), &|_| None));
    }
}

#[cfg(test)]
mod bounds_and_patterns {
    use super::*;
    use crate::parse_q;
    use antares_jsonld::Loader;
    use serde_json::json;
    use std::cell::Cell;

    const DC: &str = "https://uri.etsi.org/ngsi-ld/default-context/";

    fn ctx() -> std::sync::Arc<Context> {
        Loader::new().core()
    }

    fn with_value(attr: &str, v: Value) -> Value {
        json!({
            "id": "urn:ngsi-ld:Vehicle:9",
            "type": [format!("{DC}Vehicle")],
            format!("{DC}{attr}"): [{"type": "Property", "value": v}],
        })
    }

    /// 4.9 patternOp/notPatternOp: the pattern is compiled once per Query
    /// Term now, so pin the outcomes it has to keep — including the invalid
    /// pattern, which has no L(R) and therefore matches nothing.
    #[test]
    fn pattern_operators_keep_their_outcomes() {
        let ctx = ctx();
        for (q, target, want) in [
            (r#"brandName~="^Merc""#, json!("Mercedes"), true),
            (r#"brandName~="^Merc""#, json!("Volvo"), false),
            // one matching element is enough for ~=, none may match for !~=
            (r#"brandName~="^Merc""#, json!(["Volvo", "Mercedes"]), true),
            (r#"brandName!~="^Merc""#, json!(["Volvo", "Skoda"]), true),
            // non-string target: 4.9 p.92 "considered as not matching"
            (r#"brandName~="^Merc""#, json!(7), false),
            (r#"brandName!~="^Merc""#, json!(7), false),
            // an invalid pattern compiles to no language at all
            (r#"brandName~="[""#, json!("Mercedes"), false),
            (r#"brandName!~="[""#, json!("Mercedes"), false),
            // …and an empty array has no element inside that (empty) language
            (r#"brandName!~="[""#, json!([]), true),
            (r#"brandName~="[""#, json!([]), false),
        ] {
            let ast = parse_q(q).expect(q);
            assert_eq!(
                eval_q(
                    &ast,
                    &with_value("brandName", target.clone()),
                    &ctx,
                    &|_| None
                ),
                want,
                "q={q} target={target}"
            );
        }
    }

    /// 4.9 patternOp: the Query Term's pattern is compiled through the
    /// process-wide cache, so re-evaluating the same term over the next
    /// candidate entity (or the next event, for a subscription) reuses the
    /// compiled program instead of rebuilding it.
    #[test]
    fn pattern_term_compiles_through_the_shared_cache() {
        let _serial = crate::regex::serial_lock();
        let ctx = ctx();
        let pat = "^Merc[a-z]+-qterm$";
        assert!(
            crate::regex::cached(pat).is_none(),
            "the probe pattern must start uncompiled"
        );
        let q = format!(r#"brandName~="{pat}""#);
        let ast = parse_q(&q).expect(&q);
        let hit = with_value("brandName", json!("Mercedes-qterm"));
        assert!(eval_q(&ast, &hit, &ctx, &|_| None));
        let held = crate::regex::cached(pat).expect("the term's pattern is retained");
        // the next candidate reuses it: a recompile would replace the entry
        let miss = with_value("brandName", json!("Volvo"));
        assert!(!eval_q(&ast, &miss, &ctx, &|_| None));
        assert!(
            crate::regex::cached(pat).is_some_and(|now| std::sync::Arc::ptr_eq(&held, &now)),
            "the second candidate must not rebuild the program"
        );
        // an invalid pattern still has no L(R) (p.92) and is still not held
        let bad = parse_q(r#"brandName~="[qterm""#).expect("parses");
        assert!(!eval_q(
            &bad,
            &with_value("brandName", json!("[qterm")),
            &ctx,
            &|_| None
        ));
        assert!(
            crate::regex::cached("[qterm").is_none(),
            "an uncompilable pattern is never retained"
        );
    }

    /// A hostile `~=` pattern costs compile time, not match time (the regex
    /// engine is linear in the input). The crate's compiled-size limit turns
    /// an exploding pattern into a compile error, which 4.9 p.92 treats as
    /// not matching — so the term is rejected instead of consuming memory.
    #[test]
    fn hostile_pattern_cannot_blow_the_compile_budget() {
        let ctx = ctx();
        let e = with_value("brandName", json!("Mercedes"));
        let bombs = [
            "((((a{1000}){1000}){1000}){1000})".to_owned(),
            format!("(?:{})", "a{255}".repeat(64)),
            format!("{}a", "(".repeat(200)) + &")".repeat(200),
        ];
        let started = std::time::Instant::now();
        for p in bombs {
            let q = format!(r#"brandName~="{p}""#);
            // an unparseable q is an equally acceptable outcome — what must
            // not happen is an accepted term that compiles the bomb
            if let Ok(ast) = parse_q(&q) {
                assert!(!eval_q(&ast, &e, &ctx, &|_| None), "matched: {q}");
            }
        }
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "pattern compilation is not bounded"
        );
    }

    /// 4.9 LinkedEntityRelation: every hop consumes one `attr{…}` level, and
    /// the parser caps those at 8 — so a Relationship cycle is walked a
    /// bounded number of times and the resolver always terminates.
    #[test]
    fn linked_walk_terminates_on_a_cycle_within_the_hop_cap() {
        let ctx = ctx();
        // urn:A points at itself twice: the walk can only end by running out
        // of hops, never by running out of entities
        let a = json!({
            "id": "urn:A",
            "type": [format!("{DC}Node")],
            format!("{DC}r"): [
                {"type": "Relationship", "object": "urn:A"},
                {"type": "Relationship", "object": "urn:A", "datasetId": "urn:d:2"},
            ],
            format!("{DC}v"): [{"type": "Property", "value": 1}],
        });
        let calls = Cell::new(0usize);
        let lookup = |id: &str| {
            calls.set(calls.get() + 1);
            (id == "urn:A").then(|| a.clone())
        };
        let q = format!("{}v{}==1", "r{".repeat(8), "}".repeat(8));
        let ast = parse_q(&q).expect(&q);
        assert_eq!(ast.max_link_depth(), 8);
        assert!(eval_q(&ast, &a, &ctx, &lookup), "the cycle resolves");
        // 2 objects per hop over 8 hops: 2 + 4 + … + 2^8 — exponential in
        // the fan-out, which is why the work budget, not the hop cap, is
        // what bounds this walk.
        assert_eq!(calls.get(), 510, "resolver lookups per query term");
        assert!(calls.get() <= MAX_Q_LINK_LOOKUPS);

        // one hop deeper is refused before any entity is touched
        let deep = format!("{}v{}==1", "r{".repeat(9), "}".repeat(9));
        assert!(
            matches!(
                parse_q(&deep),
                Err(antares_model::NgsiError::TooComplexQuery(_))
            ),
            "the 9th hop must be rejected"
        );
    }

    /// 4.9 linked-entity resolution is bounded by WORK, not only by hops:
    /// a wide Relationship fan-out costs F^hops entity lookups, so one q
    /// term may only buy `MAX_Q_LINK_LOOKUPS` of them.
    #[test]
    fn linked_walk_lookups_are_capped_by_the_work_budget() {
        let ctx = ctx();
        let fan: Vec<Value> = (0..40)
            .map(|i| {
                json!({"type": "Relationship", "object": "urn:A",
                       "datasetId": format!("urn:ngsi-ld:Dataset:{i}")})
            })
            .collect();
        let a = json!({
            "id": "urn:A",
            "type": [format!("{DC}Node")],
            format!("{DC}r"): fan,
            format!("{DC}v"): [{"type": "Property", "value": 1}],
        });
        let calls = Cell::new(0usize);
        let lookup = |id: &str| {
            calls.set(calls.get() + 1);
            (id == "urn:A").then(|| a.clone())
        };
        let ast = parse_q("r{r{r{v}}}==1").expect("q");
        assert!(
            eval_q(&ast, &a, &ctx, &lookup),
            "a target reachable inside the budget still matches"
        );
        assert!(
            calls.get() <= MAX_Q_LINK_LOOKUPS,
            "one q term bought {} entity lookups",
            calls.get()
        );
    }

    /// The evaluator is fed whatever the store and the notification path
    /// hold: shapes that are not entities, and paths that navigate into a
    /// scalar, must return "no match" rather than panic.
    #[test]
    fn non_entity_shapes_never_panic() {
        let ctx = ctx();
        let ast = parse_q("speed>10").expect("q");
        for doc in [json!(null), json!(7), json!("text"), json!([]), json!({})] {
            assert!(!eval_q(&ast, &doc, &ctx, &|_| None), "doc={doc}");
        }
        // a MemberExpression into a scalar value resolves to nothing
        let e = with_value("speed", json!(80));
        for q in ["speed[unit]==80", "speed.unit.deep==80", "speed[a.b]"] {
            let ast = parse_q(q).expect(q);
            assert!(!eval_q(&ast, &e, &ctx, &|_| None), "q={q}");
        }
        // an instance carrying no value-defining member is not a target
        let bare = json!({
            "id": "urn:ngsi-ld:Vehicle:9",
            "type": [format!("{DC}Vehicle")],
            format!("{DC}speed"): [{"type": "Property", "unitCode": "KMH"}],
        });
        assert!(!eval_q(&parse_q("speed").expect("q"), &bare, &ctx, &|_| {
            None
        }));
    }

    /// 4.9 logical operators: `|` is OR, `;` is AND, `!` negates existence.
    #[test]
    fn or_and_negated_existence() {
        let ctx = ctx();
        let e = with_value("speed", json!(80));
        for (q, want) in [
            ("speed==80|speed==90", true),
            ("speed==70|speed==90", false),
            ("speed==80;!color", true),
            ("speed==80;color", false),
            ("(speed==70|speed==80);!color", true),
        ] {
            let ast = parse_q(q).expect(q);
            assert_eq!(eval_q(&ast, &e, &ctx, &|_| None), want, "q={q}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_q;
    use antares_jsonld::Loader;
    use serde_json::json;

    fn entity() -> Value {
        json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": ["https://uri.etsi.org/ngsi-ld/default-context/Vehicle"],
            "https://uri.etsi.org/ngsi-ld/default-context/speed": [
                {"type": "Property", "value": 85,
                 "https://uri.etsi.org/ngsi-ld/default-context/accuracy": [
                    {"type": "Property", "value": 0.9}]}
            ],
            "https://uri.etsi.org/ngsi-ld/default-context/brandName": [
                {"type": "Property", "value": "Mercedes"}
            ]
        })
    }

    #[test]
    fn comparisons_and_paths() {
        let ctx = Loader::new().core();
        let e = entity();
        for (q, want) in [
            ("speed>80", true),
            ("speed<80", false),
            (r#"brandName=="Mercedes""#, true),
            ("speed.accuracy>0.5", true),
            ("speed>80;brandName!=\"BMW\"", true),
            ("speed", true),
            ("!color", true),
            ("color", false),
        ] {
            let ast = parse_q(q).expect(q);
            assert_eq!(eval_q(&ast, &e, &ctx, &|_| None), want, "q={q}");
        }
    }

    #[test]
    fn unequal_matches_on_datatype_mismatch() {
        // 4.9 Unequal, p.92: "If the data type of the target value and the data
        // type of the Query Term value are different, then they shall be
        // considered unequal" — so `!=` MATCHES. Equal carries the mirror rule
        // ("considered as not matching"); the asymmetry is deliberate.
        let ctx = Loader::new().core();
        let e = json!({
            "id": "urn:ngsi-ld:Vehicle:2",
            "type": ["https://uri.etsi.org/ngsi-ld/default-context/Vehicle"],
            // a STRING where the query asks about a number, and vice versa
            "https://uri.etsi.org/ngsi-ld/default-context/speed": [
                {"type": "Property", "value": "fast"}
            ],
            "https://uri.etsi.org/ngsi-ld/default-context/brandName": [
                {"type": "Property", "value": 7}
            ]
        });
        for (q, want) in [
            ("speed!=10", true),                // string vs number ⇒ unequal
            ("speed==10", false),               // …but not equal
            (r#"brandName!="Mercedes""#, true), // number vs string ⇒ unequal
            (r#"brandName=="Mercedes""#, false),
            ("speed>10", false), // ordering on a mismatch does NOT match
        ] {
            let ast = parse_q(q).expect(q);
            assert_eq!(eval_q(&ast, &e, &ctx, &|_| None), want, "q={q}");
        }
    }

    #[test]
    fn unequal_over_an_array_requires_every_element_to_differ() {
        // 4.9 Unequal, p.91: "The target value does not include any of the list
        // values, if the target value is an array (e.g. matches
        // ["blue","black","green"], but not ["blue","red","green"])."
        let ctx = Loader::new().core();
        let mk = |vals: Value| {
            json!({
                "id": "urn:ngsi-ld:Vehicle:3",
                "type": ["https://uri.etsi.org/ngsi-ld/default-context/Vehicle"],
                "https://uri.etsi.org/ngsi-ld/default-context/color": [
                    {"type": "Property", "value": vals}
                ]
            })
        };
        let ast = parse_q(r#"color!="red""#).expect("q");
        assert!(
            eval_q(&ast, &mk(json!(["blue", "black", "green"])), &ctx, &|_| {
                None
            }),
            "no element equals red ⇒ matches"
        );
        assert!(
            !eval_q(&ast, &mk(json!(["blue", "red", "green"])), &ctx, &|_| None),
            "red is included ⇒ must NOT match (was matching on the 'blue' element)"
        );
    }

    /// 4.9 ValueList — Equal p.90 and Unequal p.91, including the spec's own
    /// array examples verbatim.
    #[test]
    fn value_list_semantics() {
        let ctx = Loader::new().core();
        let mk = |v: Value| {
            json!({
                "id": "urn:ngsi-ld:Vehicle:4",
                "type": ["https://uri.etsi.org/ngsi-ld/default-context/Vehicle"],
                "https://uri.etsi.org/ngsi-ld/default-context/color": [
                    {"type": "Property", "value": v}
                ]
            })
        };
        for (q, target, want) in [
            // Eq p.90: identical to ANY list value (e.g. matches "red")
            (r#"color=="black","red""#, json!("red"), true),
            (r#"color=="black","red""#, json!("blue"), false),
            // Eq p.90: array includes ANY of the query values
            (r#"color=="black","red""#, json!(["red", "blue"]), true),
            (r#"color=="black","red""#, json!(["blue", "green"]), false),
            // Ne p.91: identical to NO list value (e.g. matches "blue")
            (r#"color!="black","red""#, json!("blue"), true),
            (r#"color!="black","red""#, json!("red"), false),
            // Ne p.91 verbatim: matches ["blue","yellow","green"],
            // but not ["blue","red","green"]
            (
                r#"color!="black","red""#,
                json!(["blue", "yellow", "green"]),
                true,
            ),
            (
                r#"color!="black","red""#,
                json!(["blue", "red", "green"]),
                false,
            ),
        ] {
            let ast = parse_q(q).expect(q);
            assert_eq!(
                eval_q(&ast, &mk(target.clone()), &ctx, &|_| None),
                want,
                "q={q} target={target}"
            );
        }
    }

    /// 4.9 Range — Equal p.90 ("both included") and Unequal p.91.
    #[test]
    fn range_semantics() {
        let ctx = Loader::new().core();
        let mk = |v: Value| {
            json!({
                "id": "urn:ngsi-ld:Vehicle:5",
                "type": ["https://uri.etsi.org/ngsi-ld/default-context/Vehicle"],
                "https://uri.etsi.org/ngsi-ld/default-context/temperature": [
                    {"type": "Property", "value": v}
                ]
            })
        };
        for (q, target, want) in [
            ("temperature==10..20", json!(15), true),
            ("temperature==10..20", json!(10), true), // min included
            ("temperature==10..20", json!(20), true), // max included
            ("temperature==10..20", json!(9), false),
            ("temperature!=10..20", json!(9), true), // p.91: "matches 9"
            ("temperature!=10..20", json!(15), false),
            // type mismatch: p.92 "considered unequal" ⇒ != matches, == not
            ("temperature==10..20", json!("hot"), false),
            ("temperature!=10..20", json!("hot"), true),
        ] {
            let ast = parse_q(q).expect(q);
            assert_eq!(
                eval_q(&ast, &mk(target.clone()), &ctx, &|_| None),
                want,
                "q={q} target={target}"
            );
        }
        // DateTime range endpoints (Str..Str, temporal == lexicographic in Z
        // form). `eventTime` and not `observedAt`: the latter is a CORE term
        // and would expand to the core IRI, not the default context.
        let ast = parse_q("eventTime==2021-01-01T00:00:00Z..2021-06-01T00:00:00Z").expect("q");
        let e = json!({
            "id": "urn:ngsi-ld:Vehicle:6",
            "type": ["https://uri.etsi.org/ngsi-ld/default-context/Vehicle"],
            "https://uri.etsi.org/ngsi-ld/default-context/eventTime": [
                {"type": "Property", "value": "2021-03-15T12:00:00Z"}
            ]
        });
        assert!(eval_q(&ast, &e, &ctx, &|_| None));
    }

    /// 4.9 notPatternOp p.92: NOT in L(R); non-string targets are "not
    /// matching" — deliberately NOT the `!=` type-mismatch rule.
    #[test]
    fn not_pattern_semantics() {
        let ctx = Loader::new().core();
        let mk = |v: Value| {
            json!({
                "id": "urn:ngsi-ld:Vehicle:7",
                "type": ["https://uri.etsi.org/ngsi-ld/default-context/Vehicle"],
                "https://uri.etsi.org/ngsi-ld/default-context/brandName": [
                    {"type": "Property", "value": v}
                ]
            })
        };
        for (q, target, want) in [
            (r#"brandName!~="^Merc""#, json!("Volvo"), true),
            (r#"brandName!~="^Merc""#, json!("Mercedes"), false),
            // non-string target ⇒ not matching (p.92), unlike !=
            (r#"brandName!~="^Merc""#, json!(7), false),
            // an array is outside L(R) only if NO element is in it
            (r#"brandName!~="^Merc""#, json!(["Volvo", "Skoda"]), true),
            (
                r#"brandName!~="^Merc""#,
                json!(["Volvo", "Mercedes"]),
                false,
            ),
        ] {
            let ast = parse_q(q).expect(q);
            assert_eq!(
                eval_q(&ast, &mk(target.clone()), &ctx, &|_| None),
                want,
                "q={q} target={target}"
            );
        }
    }
}
