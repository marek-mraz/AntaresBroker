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

fn compare(target: &Value, op: CmpOp, want: &QValue) -> bool {
    // arrays: any element satisfying the comparison satisfies it
    if let Value::Array(items) = target {
        return items.iter().any(|i| compare(i, op, want));
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
            }
        }
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
        CmpOp::Pattern => false,
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
}
