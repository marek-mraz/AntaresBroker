//! C10 — NGSI-LD `q=` (CIM 009 clause 4.9) compiled to SQL jsonpath.
//!
//! Strategy is Scorpio's, proven against the ETSI suite (§8.1): the predicate
//! becomes `entity @? $n::jsonpath` over the stored expanded document (the
//! operator spelling of `jsonb_path_exists`, because only the operator form
//! matches the GIN `jsonb_path_ops` index). §16.2 is absolute here — **the
//! jsonpath travels as a bind, never
//! as SQL text**. Nothing a client typed is ever concatenated into a
//! statement; the compiler emits `$n` placeholders and hands the paths back
//! as a separate list.
//!
//! The compiler is deliberately partial. It returns `None` for any shape it
//! cannot reproduce EXACTLY as `qeval::eval_q` would evaluate it, and the
//! caller then falls back to fetching the rows the other predicates select
//! and filtering them in memory. A wrong row is a compliance bug; a slow
//! query is a benchmark item.
//!
//! Stored document shape (what the paths address): attribute keys are
//! expanded IRIs, each holding an ARRAY of instances, each instance carrying
//! its comparable value under one of `value`/`object`/`languageMap`/`vocab`/
//! `json`/`valueList`/`objectList`.
//!
//! The AST holds TERMS, not IRIs — `qeval` expands them against the request
//! `@context` at evaluation time. The compiler therefore takes the same
//! expander as a closure rather than depending on `antares-jsonld`: one
//! function, no crate edge, and the two paths cannot disagree about what a
//! term means because they are handed the same one.

use antares_ql::{CmpOp, QNode, QValue};

/// The comparable-value members, in `qeval::comparable_value` order. That
/// function returns the FIRST present member; we OR over all of them, which
/// is identical for valid NGSI-LD (an attribute instance carries exactly one)
/// and only diverges for a document that is already invalid.
const VALUE_KEYS: &[&str] = &[
    "value",
    "object",
    "languageMap",
    "vocab",
    "json",
    "valueList",
    "objectList",
];

/// A compiled `q=`: a SQL boolean expression plus the jsonpath binds it
/// references. `sql` contains `$n` placeholders numbered from `first_bind`.
pub struct CompiledQ {
    pub sql: String,
    pub binds: Vec<String>,
}

/// Compile `node` into a SQL predicate over column `col` (a `jsonb`).
/// `first_bind` is the 1-based number of the next free placeholder.
/// `None` = this expression is outside the exact subset; filter in memory.
pub fn compile_q(
    node: &QNode,
    col: &str,
    first_bind: usize,
    expand: &dyn Fn(&str) -> String,
) -> Option<CompiledQ> {
    let mut binds = Vec::new();
    let sql = emit(node, col, first_bind, expand, &mut binds)?;
    Some(CompiledQ { sql, binds })
}

fn emit(
    node: &QNode,
    col: &str,
    first: usize,
    expand: &dyn Fn(&str) -> String,
    binds: &mut Vec<String>,
) -> Option<String> {
    match node {
        QNode::And(items) => join(items, " AND ", col, first, expand, binds),
        QNode::Or(items) => join(items, " OR ", col, first, expand, binds),
        QNode::Exists { path, negated } => {
            let p = path_expr(path, expand)?;
            let sql = value_or(&p, None, col, first, binds)?;
            Some(if *negated {
                format!("NOT ({sql})")
            } else {
                sql
            })
        }
        QNode::Cmp { path, op, value } => {
            let p = path_expr(path, expand)?;
            value_or(&p, Some((*op, value)), col, first, binds)
        }
    }
}

/// `first` is the ORIGINAL placeholder offset throughout; the running count
/// is `binds.len()` alone. Adding the offset again per level is how you get
/// two predicates pointing at the same `$n`.
fn join(
    items: &[QNode],
    sep: &str,
    col: &str,
    first: usize,
    expand: &dyn Fn(&str) -> String,
    binds: &mut Vec<String>,
) -> Option<String> {
    let mut parts = Vec::with_capacity(items.len());
    for it in items {
        parts.push(emit(it, col, first, expand, binds)?);
    }
    Some(format!("({})", parts.join(sep)))
}

/// One `jsonb_path_exists` per comparable-value member, OR'd — the SQL
/// spelling of "whichever member this instance carries".
fn value_or(
    prefix: &str,
    cmp: Option<(CmpOp, &QValue)>,
    col: &str,
    first: usize,
    binds: &mut Vec<String>,
) -> Option<String> {
    let filter = match cmp {
        Some((op, v)) => Some(cmp_filter(op, v)?),
        None => None,
    };
    let mut parts = Vec::with_capacity(VALUE_KEYS.len());
    for key in VALUE_KEYS {
        // lax mode (the default) auto-unwraps arrays at every step, which is
        // exactly `qeval::compare`'s "any element of an array value matches".
        let jp = match &filter {
            Some(f) => format!("{prefix}.\"{key}\"{f}"),
            None => format!("{prefix}.\"{key}\""),
        };
        // the OPERATOR form of jsonb_path_exists: identical lax semantics,
        // but the planner can match `@?` against the GIN jsonb_path_ops
        // index — the function form never uses it (audit 2026-08-08)
        parts.push(format!("{col} @? ${}::jsonpath", first + binds.len()));
        binds.push(jp);
    }
    Some(format!("({})", parts.join(" OR ")))
}

/// Dotted q path → jsonpath prefix addressing the instance objects.
/// Exact only while every segment is an attribute step (`attr.sub.subsub`);
/// `qeval::collect` falls back to navigating INTO a value object when a
/// segment is not a sub-attribute, and that ambiguity is not reproducible in
/// one jsonpath — so those queries stay in-memory.
fn path_expr(path: &[String], expand: &dyn Fn(&str) -> String) -> Option<String> {
    // Only single-segment paths are unambiguous. For a longer one `qeval::
    // collect` picks between a sub-attribute step and navigation INTO the
    // value object based on what the DOCUMENT happens to hold — a per-row
    // decision no single jsonpath reproduces. Refuse rather than guess.
    if path.len() != 1 {
        return None;
    }
    Some(format!("$.{}[*]", quoted(&expand(path.first()?))))
}

fn cmp_filter(op: CmpOp, want: &QValue) -> Option<String> {
    // 4.9 ValueList / Range (CompEqualityValue). Only `==` compiles:
    // - Eq+List p.90 is "identical to ANY of the list values" — an OR of
    //   equality filters, existential like jsonpath's lax arrays. Exact.
    // - Eq+Range p.90 is a closed interval — exact for numbers; string
    //   endpoints would order through the database collation (see the
    //   ordering note below), so those stay in memory.
    // - Ne+List / Ne+Range inherit every `!=` caveat (type-mismatch matches,
    //   universal quantification over arrays) — declined with it.
    match want {
        QValue::List(vals) => {
            if op != CmpOp::Eq {
                return None;
            }
            let mut parts = Vec::with_capacity(vals.len());
            for v in vals {
                parts.push(format!("@ == {}", literal(v)?));
            }
            return Some(format!(" ? ({})", parts.join(" || ")));
        }
        QValue::Range(lo, hi) => {
            if op != CmpOp::Eq {
                return None;
            }
            let (QValue::Num(a), QValue::Num(b)) = (lo.as_ref(), hi.as_ref()) else {
                return None;
            };
            let (a, b) = (
                literal(&QValue::Num(*a))?,
                literal(&QValue::Num(*b))?,
            );
            return Some(format!(" ? (@ >= {a} && @ <= {b})"));
        }
        _ => {}
    }
    // Ordering against a STRING is left to the evaluator: `qeval::compare`
    // orders with Rust's byte-wise `str` comparison, while jsonpath orders
    // through the database collation. They agree on ASCII and can disagree
    // elsewhere, and disagreeing HERE drops a matching row (a compliance
    // bug), not merely a fast row (a benchmark item).
    if matches!(want, QValue::Str(_)) && matches!(op, CmpOp::Gt | CmpOp::Ge | CmpOp::Lt | CmpOp::Le)
    {
        return None;
    }
    let lit = literal(want)?;
    Some(match op {
        CmpOp::Eq => format!(" ? (@ == {lit})"),
        // 4.9 p.92: "If the data type of the target value and the data type of
        // the Query Term value are different, then they shall be considered
        // unequal" — a type mismatch MATCHES `!=`. PostgreSQL jsonpath compares
        // across types as `unknown`, so `@ != lit` silently DROPS exactly those
        // rows, and 4.9 p.91 additionally requires every element of an array to
        // differ (jsonpath quantifies existentially). Neither is reproducible
        // here, so per this module's contract — decline rather than narrow
        // wrongly — `!=` is left to the in-memory evaluator.
        CmpOp::Ne => return None,
        CmpOp::Gt => format!(" ? (@ > {lit})"),
        CmpOp::Ge => format!(" ? (@ >= {lit})"),
        CmpOp::Lt => format!(" ? (@ < {lit})"),
        CmpOp::Le => format!(" ? (@ <= {lit})"),
        // ~= / !~= are regexes over strings; jsonpath's like_regex is
        // POSIX-ish and does not match Rust's `regex` crate on every pattern,
        // so both are left to the in-memory evaluator.
        CmpOp::Pattern | CmpOp::NotPattern => return None,
    })
}

fn literal(v: &QValue) -> Option<String> {
    Some(match v {
        QValue::Str(s) => jsonpath_string(s),
        QValue::Bool(b) => b.to_string(),
        QValue::Num(n) => {
            if n.is_finite() {
                // shortest round-trip form; jsonpath numbers are JSON numbers
                let s = n.to_string();
                if s.contains(['e', 'E']) {
                    return None; // exponent forms differ across parsers
                }
                s
            } else {
                return None;
            }
        }
        // composite values never render as one literal — cmp_filter unfolds
        // them (Eq) or declines (everything else) before reaching here
        QValue::List(_) | QValue::Range(..) => return None,
    })
}

/// A jsonpath member name: always double-quoted, because attribute keys are
/// expanded IRIs full of `:` `/` `#` `.` that would otherwise be syntax.
fn quoted(s: &str) -> String {
    jsonpath_string(s)
}

fn jsonpath_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use antares_ql::parse_q;

    /// the expander the API hands in, stubbed: term → default-context IRI
    fn ex(t: &str) -> String {
        format!("https://uri.etsi.org/ngsi-ld/default-context/{t}")
    }

    fn c(q: &str) -> Option<CompiledQ> {
        compile_q(&parse_q(q).expect("parse"), "entity", 2, &ex)
    }

    #[test]
    fn comparison_binds_the_jsonpath_and_never_splices_it() {
        let got = c("temperature>20").expect("compiles");
        // every placeholder is a bind; no client text in the SQL
        assert!(!got.sql.contains("temperature"), "sql: {}", got.sql);
        assert_eq!(got.binds.len(), VALUE_KEYS.len());
        assert_eq!(
            got.binds[0],
            "$.\"https://uri.etsi.org/ngsi-ld/default-context/temperature\"[*].\"value\" ? (@ > 20)"
        );
        assert!(
            got.sql.starts_with("(entity @? $2::jsonpath"),
            "sql: {}",
            got.sql
        );
    }

    #[test]
    fn placeholders_are_numbered_from_the_offset_and_stay_unique() {
        let got = c("a==1;b==2").expect("compiles");
        let n = VALUE_KEYS.len();
        assert_eq!(got.binds.len(), 2 * n);
        for i in 0..2 * n {
            assert!(
                got.sql.contains(&format!("${}::jsonpath", i + 2)),
                "missing ${}",
                i + 2
            );
        }
        assert!(got.sql.contains(") AND ("));
    }

    #[test]
    fn or_and_negated_existence() {
        assert!(c("a==1|b==2").expect("or").sql.contains(") OR ("));
        let neg = c("!a").expect("negated exists");
        assert!(neg.sql.starts_with("NOT ("));
    }

    #[test]
    fn expanded_iri_keys_are_quoted_so_slashes_and_colons_are_not_syntax() {
        let got = c("t==5").expect("compiles");
        assert!(
            got.binds[0].starts_with("$.\"https://uri.etsi.org/ngsi-ld/default-context/t\"[*]"),
            "{}",
            got.binds[0]
        );
    }

    #[test]
    fn quoting_escapes_what_would_break_the_jsonpath() {
        assert_eq!(jsonpath_string("a\"b\\c"), "\"a\\\"b\\\\c\"");
        assert_eq!(jsonpath_string("l\n"), "\"l\\n\"");
    }

    #[test]
    fn unsupported_shapes_refuse_instead_of_guessing() {
        // dotted path: sub-attribute vs value navigation is ambiguous
        assert!(c("address.city==\"Bonn\"").is_none());
        // ~= is a regex dialect mismatch
        assert!(c("name~=\"^ab\"").is_none());
        // ordering on strings: Rust byte-wise vs database collation
        assert!(c("name>\"m\"").is_none());
        assert!(c("name<=\"m\"").is_none());
        // ... but equality on strings is collation-free, so it compiles
        assert!(c("name==\"m\"").is_some());
        assert!(c("n>=3").is_some(), "numeric ordering is unambiguous");
    }

    #[test]
    fn value_list_and_range_compile_for_eq_and_decline_for_ne() {
        // Eq+List: an OR of equality filters — existential, exact
        let got = c(r#"color=="black","red""#).expect("compiles");
        assert!(
            got.binds[0].ends_with(r#" ? (@ == "black" || @ == "red")"#),
            "{}",
            got.binds[0]
        );
        // Eq+Range on numbers: closed interval
        let got = c("t==10..20").expect("compiles");
        assert!(
            got.binds[0].ends_with(" ? (@ >= 10 && @ <= 20)"),
            "{}",
            got.binds[0]
        );
        // Ne inherits the != caveats (type mismatch, array quantification)
        assert!(c(r#"color!="black","red""#).is_none());
        assert!(c("t!=10..20").is_none());
        // string-endpoint ranges order through the collation — in-memory
        assert!(c(r#"name=="a".."m""#).is_none());
        // !~= is a regex — dialect mismatch, in-memory
        assert!(c(r#"name!~="^ab""#).is_none());
    }
}
