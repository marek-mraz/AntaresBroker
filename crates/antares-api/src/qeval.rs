//! In-memory `q=` evaluation against internal expanded entities (the same
//! evaluator the subscription matcher uses — §evaluate.rs in the design).

use antares_jsonld::Context;
use antares_ql::{CmpOp, QNode, QValue};
use serde_json::Value;

pub fn eval_q(node: &QNode, entity: &Value, ctx: &Context) -> bool {
    match node {
        QNode::And(items) => items.iter().all(|n| eval_q(n, entity, ctx)),
        QNode::Or(items) => items.iter().any(|n| eval_q(n, entity, ctx)),
        QNode::Exists { path, negated } => {
            let found = !resolve_targets(entity, path, ctx).is_empty();
            found != *negated
        }
        QNode::Cmp { path, op, value } => resolve_targets(entity, path, ctx)
            .iter()
            .any(|v| compare(v, *op, value)),
    }
}

/// Resolve a dotted q path to candidate JSON values (across instances).
fn resolve_targets(entity: &Value, path: &[String], ctx: &Context) -> Vec<Value> {
    let Some(first) = path.first() else {
        return vec![];
    };
    let iri = ctx.expand_key(first);
    let Some(instances) = entity.get(&iri).and_then(Value::as_array) else {
        return vec![];
    };
    let mut out = Vec::new();
    for inst in instances {
        collect(inst, &path[1..], ctx, &mut out);
    }
    out
}

fn collect(inst: &Value, rest: &[String], ctx: &Context, out: &mut Vec<Value>) {
    if rest.is_empty() {
        // terminal: the comparable value of this instance
        if let Some(v) = comparable_value(inst) {
            out.push(v.clone());
        }
        return;
    }
    let seg = &rest[0];
    // 1. sub-attribute step (expanded key)
    let iri = ctx.expand_key(seg);
    if let Some(subs) = inst.get(&iri).and_then(Value::as_array) {
        for s in subs {
            collect(s, &rest[1..], ctx, out);
        }
        return;
    }
    // 2. value-path step: navigate into the value object
    if let Some(v) = comparable_value(inst) {
        if let Some(nested) = navigate(v, rest) {
            out.push(nested.clone());
        }
    }
}

fn navigate<'a>(v: &'a Value, path: &[String]) -> Option<&'a Value> {
    let mut cur = v;
    for seg in path {
        cur = cur.get(seg)?;
    }
    Some(cur)
}

fn comparable_value(inst: &Value) -> Option<&Value> {
    let obj = inst.as_object()?;
    for k in [
        "value",
        "object",
        "languageMap",
        "vocab",
        "json",
        "valueList",
        "objectList",
    ] {
        if let Some(v) = obj.get(k) {
            return Some(v);
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

fn compare(target: &Value, op: CmpOp, want: &QValue) -> bool {
    // 4.9 ValueList — Equal p.90: "identical or equivalent to ANY of the list
    // values"; Unequal p.91: "neither identical nor equivalent to any of the
    // list values" / "does not include ANY of the list values" — i.e. every
    // per-element != must hold. Unfold before the array unwrap so the array
    // rules apply per list element.
    if let QValue::List(vals) = want {
        return match op {
            CmpOp::Eq => vals.iter().any(|v| compare(target, op, v)),
            CmpOp::Ne => vals.iter().all(|v| compare(target, op, v)),
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
            CmpOp::Ne | CmpOp::NotPattern => items.iter().all(|i| compare(i, op, want)),
            _ => items.iter().any(|i| compare(i, op, want)),
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
                CmpOp::Pattern => regex::Regex::new(s).is_ok_and(|re| re.is_match(t)),
                // p.92: target "shall not be in the L(R)" — an invalid regex
                // has no L(R), treat as not matching (same posture as ~=)
                CmpOp::NotPattern => regex::Regex::new(s).is_ok_and(|re| !re.is_match(t)),
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
mod tests {
    use super::*;
    use antares_jsonld::Loader;
    use antares_ql::parse_q;
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
            assert_eq!(eval_q(&ast, &e, &ctx), want, "q={q}");
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
            assert_eq!(eval_q(&ast, &e, &ctx), want, "q={q}");
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
            eval_q(&ast, &mk(json!(["blue", "black", "green"])), &ctx),
            "no element equals red ⇒ matches"
        );
        assert!(
            !eval_q(&ast, &mk(json!(["blue", "red", "green"])), &ctx),
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
                eval_q(&ast, &mk(target.clone()), &ctx),
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
                eval_q(&ast, &mk(target.clone()), &ctx),
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
        assert!(eval_q(&ast, &e, &ctx));
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
                eval_q(&ast, &mk(target.clone()), &ctx),
                want,
                "q={q} target={target}"
            );
        }
    }
}
