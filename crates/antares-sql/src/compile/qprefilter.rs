//! 5.7.4.4 S2 — the values filter compiled to a SUPERSET SQL prefilter.
//!
//! The temporal store keeps one row per Attribute instance, so the entity
//! doc `qeval::eval_q` sees does not exist SQL-side until the expensive
//! per-entity reconstruction has already happened. This module narrows the
//! ENTITY set before reconstruction instead: each q leaf the exact compiler
//! (`compile::q`) can reproduce becomes one windowed EXISTS over
//! `attr_instances`; every shape outside that subset becomes TRUE. The
//! structure rules keep the superset invariant total over the 4.9 grammar:
//!
//! * `And` — AND of the compiled members; an uncompilable member is TRUE and
//!   is simply dropped (dropping a conjunct only widens).
//! * `Or`  — OR of the compiled members, but ANY uncompilable branch makes
//!   the whole disjunction TRUE (a TRUE branch absorbs the OR).
//! * leaf  — `EXISTS(instance of that attr, inside the widened column
//!   window, satisfying the exact per-instance jsonpath)`. Per 5.7.4.4 the
//!   values filter is checked against "the Attribute instances resulting
//!   from the initial filtering performed by the temporal query", so the
//!   window belongs INSIDE the existence test.
//! * extension leaves (superset-only, see `instance_predicate`): `!=` as
//!   NOT-of-Eq, `[lang]`/`[*]` via the languageMap wildcard, string
//!   ordering via `COLLATE "C"` with array pass-through.
//! * negated existence, patterns, dotted/linked paths — TRUE (entity-level
//!   negation over per-instance rows is not superset-safe to push; regex
//!   dialects differ).
//!
//! `eval_q` remains the arbiter — the API always re-evaluates q on the rows
//! that come back, so a defect here can only fail to narrow, never narrow
//! wrongly. `None` from the top level means "no narrowing at all".

use antares_ql::{CmpOp, QNode, QPath, QValue};

use super::q::{compile_instance_leaf, CompiledQ};
use super::temporal::{column_range_bound, InstanceRange};

/// Compile `node` into a SQL predicate over the entity row aliased `entity`
/// (`temporal_entities m`). Placeholders are numbered from `first_bind`;
/// every bind is text (`$n::timestamptz` / `$n::jsonpath` casts in the SQL).
pub fn compile_prefilter(
    node: &QNode,
    range: Option<&InstanceRange<'_>>,
    entity: &str,
    first_bind: usize,
    expand: &dyn Fn(&str) -> String,
) -> Option<CompiledQ> {
    let (sql, binds, _) = emit(node, range, entity, first_bind, expand)?;
    Some(CompiledQ { sql, binds })
}

/// Did the whole filter compile EXACTLY — no member dropped, no branch
/// refused, every leaf a `Cmp` whose window carries the byte-exact text
/// predicate? An exact prefilter's entity verdict equals the evaluator's,
/// which is what makes SQL entity-paging with `q=` safe (the caller's gate).
/// Existence leaves are deliberately NOT exact: the evaluator's treatment of
/// deletion instances has no SQL twin yet.
pub fn prefilter_exact(
    node: &QNode,
    range: Option<&InstanceRange<'_>>,
    expand: &dyn Fn(&str) -> String,
) -> bool {
    emit(node, range, "m", 1, expand).is_some_and(|(_, _, exact)| exact)
}

/// `first` is the ABSOLUTE number the member's first bind will get; refused
/// subtrees return `None` without having committed any binds, so numbering
/// stays dense.
fn emit(
    node: &QNode,
    range: Option<&InstanceRange<'_>>,
    entity: &str,
    first: usize,
    expand: &dyn Fn(&str) -> String,
) -> Option<(String, Vec<String>, bool)> {
    match node {
        QNode::And(items) => {
            let mut sqls = Vec::new();
            let mut binds = Vec::new();
            let mut exact = true;
            for it in items {
                if let Some((s, b, e)) = emit(it, range, entity, first + binds.len(), expand) {
                    sqls.push(s);
                    binds.extend(b);
                    exact &= e;
                } else {
                    // a dropped conjunct only widens — but the result is no
                    // longer the evaluator's verdict
                    exact = false;
                }
            }
            (!sqls.is_empty()).then(|| (format!("({})", sqls.join(" AND ")), binds, exact))
        }
        QNode::Or(items) => {
            let mut sqls = Vec::new();
            let mut binds = Vec::new();
            let mut exact = true;
            for it in items {
                let (s, b, e) = emit(it, range, entity, first + binds.len(), expand)?;
                sqls.push(s);
                binds.extend(b);
                exact &= e;
            }
            (!sqls.is_empty()).then(|| (format!("({})", sqls.join(" OR ")), binds, exact))
        }
        QNode::Exists { path, negated } => {
            if *negated {
                return None;
            }
            leaf(path, None, range, entity, first, expand)
        }
        QNode::Cmp { path, op, value } => {
            leaf(path, Some((*op, value)), range, entity, first, expand)
        }
    }
}

fn leaf(
    path: &QPath,
    cmp: Option<(CmpOp, &QValue)>,
    range: Option<&InstanceRange<'_>>,
    entity: &str,
    first: usize,
    expand: &dyn Fn(&str) -> String,
) -> Option<(String, Vec<String>, bool)> {
    // links and dotted paths stay in memory (compile::q's ambiguity rules);
    // brackets are handled by the languageMap extension leaf below
    if !path.links.is_empty() || path.path.len() != 1 {
        return None;
    }
    // binds: [attr IRI, timeproperty + window time(s)…, jsonpath(s)…]
    let mut binds = vec![expand(path.path.first()?)];
    let mut window = String::new();
    let mut win_exact = range.is_none();
    if let Some(r) = range {
        match crate::compile::temporal::compile_instance_range(r, "qi.data", first + binds.len()) {
            // byte-exact text predicate (the arbiter's own window semantics),
            // plus the widened column bound REUSING its time binds so the
            // (tenant, entity, attr, observed_at) btree still serves the range
            Some(c) => {
                let time_bind = first + binds.len() + 1;
                window = format!(" AND {}", c.sql);
                binds.extend(c.binds);
                if let Some(cb) = column_range_bound(r, "qi", time_bind) {
                    window.push_str(&format!(" AND {cb}"));
                }
                win_exact = true;
            }
            // refused text shape: superset-only widened column bound
            None => {
                if let Some(cb) = column_range_bound(r, "qi", first + binds.len()) {
                    binds.push(r.time_at.to_owned());
                    if r.timerel == "between" {
                        binds.push(r.end_time_at?.to_owned());
                    }
                    window = format!(" AND {cb}");
                }
            }
        }
    }
    let (inner, arm_exact) = instance_predicate(path, cmp, first, &mut binds)?;
    let sql = format!(
        "EXISTS (SELECT 1 FROM attr_instances qi \
         WHERE qi.tenant_id = {entity}.tenant_id AND qi.entity_id = {entity}.id \
         AND qi.attr_id = ${first}{window} AND {inner})"
    );
    Some((sql, binds, win_exact && arm_exact))
}

/// The per-instance predicate inside the EXISTS: the exact `compile::q`
/// leaf when possible, else one of the SUPERSET-ONLY extension leaves —
/// each may only widen relative to `eval_q`, never narrow:
///
/// * `[lang]`/`[*]` — the value under ANY language (`languageMap.*`): a
///   superset of the specific-tag semantics (case-insensitive BCP 47 tag
///   matching stays in memory) and exactly `[*]`'s own meaning.
/// * `!=` — NOT of the existential-equality member-OR: 4.9 p.91's
///   universal quantification over arrays and p.92's datatype-mismatch-
///   matches both fall out of the negation; a deletion instance passes to
///   the evaluator (which is why the arm is never exact).
/// * string ordering — `COLLATE "C"` byte comparison, the SQL spelling of
///   the p.89 RFC 8259 code-unit SHALL; scalar strings compare exactly,
///   array values pass through, non-string scalars are a datatype
///   mismatch on both sides.
fn instance_predicate(
    path: &QPath,
    cmp: Option<(CmpOp, &QValue)>,
    first: usize,
    binds: &mut Vec<String>,
) -> Option<(String, bool)> {
    use super::q;
    if path.bracket.is_some() {
        let filter = match cmp {
            Some((op, v)) => Some(q::lang_filter(op, v)?),
            None => None,
        };
        let jp = match filter {
            Some(f) => format!("$.\"languageMap\".*{f}"),
            None => "$.\"languageMap\".*".to_owned(),
        };
        let n = first + binds.len();
        binds.push(jp);
        return Some((format!("qi.data @? ${n}::jsonpath"), false));
    }
    if let Some((CmpOp::Ne, v)) = cmp {
        let f = q::eq_filter(v)?;
        // value_or_filter numbers as `first + binds.len()` itself — hand it
        // the leaf's base offset, not an already-advanced one
        let sql = q::value_or_filter("$", Some(&f), "qi.data", first, binds);
        return Some((format!("NOT {sql}"), false));
    }
    if let Some(l) = compile_instance_leaf(cmp, "qi.data", first + binds.len()) {
        binds.extend(l.binds);
        // existence leaves stay inexact: deletion-instance semantics differ
        return Some((l.sql, cmp.is_some()));
    }
    if let Some((op @ (CmpOp::Gt | CmpOp::Ge | CmpOp::Lt | CmpOp::Le), QValue::Str(sv))) = cmp {
        let o = match op {
            CmpOp::Gt => ">",
            CmpOp::Ge => ">=",
            CmpOp::Lt => "<",
            _ => "<=",
        };
        let mut parts = Vec::new();
        for key in [
            "value",
            "object",
            "vocab",
            "json",
            "valueList",
            "objectList",
        ] {
            let n = first + binds.len();
            binds.push(sv.clone());
            parts.push(format!(
                "(jsonb_typeof(qi.data->'{key}') = 'string' \
                 AND (qi.data->>'{key}') COLLATE \"C\" {o} ${n}::text) \
                 OR jsonb_typeof(qi.data->'{key}') = 'array'"
            ));
        }
        return Some((format!("({})", parts.join(" OR ")), false));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use antares_ql::parse_q;

    fn ex(t: &str) -> String {
        format!("https://uri.etsi.org/ngsi-ld/default-context/{t}")
    }

    fn between() -> InstanceRange<'static> {
        InstanceRange {
            timerel: "between",
            time_at: "2026-03-01T00:00:00Z",
            end_time_at: Some("2026-03-02T00:00:00Z"),
            timeproperty: "observedAt",
        }
    }

    fn pf(q: &str) -> Option<CompiledQ> {
        let r = between();
        compile_prefilter(&parse_q(q).expect("parse"), Some(&r), "m", 3, &ex)
    }

    /// max `$n` referenced must equal first-1+binds.len(), all dense
    fn assert_dense(c: &CompiledQ, first: usize) {
        for n in first..first + c.binds.len() {
            assert!(c.sql.contains(&format!("${n}")), "missing ${n}: {}", c.sql);
        }
        assert!(
            !c.sql.contains(&format!("${}", first + c.binds.len())),
            "overshoot: {}",
            c.sql
        );
    }

    #[test]
    fn leaf_is_a_windowed_exists_with_the_iri_as_a_bind() {
        let c = pf("speed>25").expect("compiles");
        assert!(!c.sql.contains("speed"), "IRI must travel as a bind");
        assert_eq!(c.binds[0], ex("speed"));
        assert!(c.sql.contains("EXISTS (SELECT 1 FROM attr_instances qi"));
        assert!(c.sql.contains("qi.attr_id = $3"), "{}", c.sql);
        // byte-exact text window predicate INSIDE the existence test
        // (5.7.4.4 S2 — this is what makes a Cmp leaf EXACT), its binds
        // [timeproperty, timeAt, endTimeAt] at $4..$6…
        assert_eq!(c.binds[1], "observedAt");
        assert_eq!(c.binds[2], "2026-03-01T00:00:00Z");
        assert_eq!(c.binds[3], "2026-03-02T00:00:00Z");
        // …with the widened column bound REUSING the time binds
        assert!(
            c.sql
                .contains("qi.observed_at >= $5::timestamptz - interval '48 hours'"),
            "{}",
            c.sql
        );
        assert!(
            c.sql
                .contains("qi.observed_at < $6::timestamptz + interval '48 hours'"),
            "{}",
            c.sql
        );
        // the jsonpath leaf is rooted at the instance, not the entity doc
        assert!(c.binds[4].starts_with("$.\"value\""), "{}", c.binds[4]);
        assert_dense(&c, 3);
    }

    #[test]
    fn exactness_flags_the_pageable_subset() {
        let r = between();
        let e = |q: &str| prefilter_exact(&parse_q(q).expect("parse"), Some(&r), &ex);
        // every leaf a compiled Cmp with the text window → exact
        assert!(e("speed>25"));
        assert!(e("speed>=5;heading<90"));
        assert!(e("speed>25|heading>100"));
        assert!(e("speed==10..40"));
        assert!(e(r#"route=="550","551""#));
        // dropped conjunct / refused branch / existence / negation → inexact
        assert!(!e(r#"speed>25;name~="^x""#), "And drop widens");
        assert!(!e(r#"speed>25|name~="^x""#), "Or refusal is trivial");
        assert!(!e("speed"), "existence: deletion semantics differ");
        assert!(!e("!speed"));
        assert!(!e("speed!=10"));
        // no range at all still exact (nothing to window)
        assert!(prefilter_exact(
            &parse_q("speed>25").expect("parse"),
            None,
            &ex
        ));
    }

    #[test]
    fn and_drops_an_uncompilable_member_or_keeps_both() {
        // pattern leaf is outside the exact subset → dropped, one EXISTS left
        let c = pf(r#"speed>25;name~="^x""#).expect("compiles");
        assert_eq!(c.sql.matches("EXISTS").count(), 1, "{}", c.sql);
        // two compilable members: both EXISTS, AND'd, dense numbering
        let c = pf("speed>25;heading<90").expect("compiles");
        assert_eq!(c.sql.matches("EXISTS").count(), 2);
        assert!(c.sql.contains(" AND "), "{}", c.sql);
        assert_dense(&c, 3);
    }

    #[test]
    fn or_with_an_uncompilable_branch_is_trivial() {
        // a TRUE branch absorbs the OR — no prefilter at all
        assert!(pf(r#"speed>25|name~="^x""#).is_none());
        // both branches compile → OR of EXISTS
        let c = pf("speed>25|heading>100").expect("compiles");
        assert!(c.sql.contains(" OR "), "{}", c.sql);
        assert_eq!(c.sql.matches("EXISTS").count(), 2);
    }

    #[test]
    fn shapes_outside_the_exact_subset_are_trivial_not_wrong() {
        for q in [
            "!speed",        // negated existence: not superset-safe per-row
            r#"name~="^x""#, // regex dialect mismatch
            "a.b==1",        // dotted path ambiguity
        ] {
            assert!(pf(q).is_none(), "{q} must be trivial");
        }
    }

    #[test]
    fn extension_leaves_compile_superset_only() {
        // != — NOT of the existential-equality member-OR
        let c = pf("speed!=10").expect("compiles");
        assert!(c.sql.contains("AND NOT ("), "{}", c.sql);
        assert!(pf(r#"speed!="10",30"#).is_some(), "Ne+List");
        assert!(pf(r#"name!~="^x""#).is_none(), "!~= stays a regex refusal");
        // [lang]/[*] — the languageMap wildcard
        let c = pf(r#"label[en]=="hi""#).expect("compiles");
        assert!(
            c.binds.iter().any(|b| b.starts_with("$.\"languageMap\".*")),
            "{:?}",
            c.binds
        );
        assert!(pf(r#"label[*]=="hi""#).is_some());
        // string ordering — COLLATE "C" scalar compare + array pass-through
        let c = pf(r#"name>"m""#).expect("compiles");
        assert!(c.sql.contains("COLLATE \"C\" >"), "{}", c.sql);
        assert!(c.sql.contains("= 'array'"), "{}", c.sql);
        // all three are superset-only: never page-exact
        let r = between();
        for q in [r#"name>"m""#, "speed!=10", r#"label[en]=="hi""#] {
            assert!(
                !prefilter_exact(&parse_q(q).expect("parse"), Some(&r), &ex),
                "{q} must stay inexact"
            );
        }
    }

    #[test]
    fn existence_list_and_range_leaves_compile() {
        let c = pf("speed").expect("existence compiles");
        assert!(c.sql.contains("EXISTS"), "{}", c.sql);
        assert!(pf(r#"route=="550","551""#).is_some(), "Eq+List");
        assert!(pf("speed==10..40").is_some(), "Eq+Range");
    }

    #[test]
    fn window_is_omitted_when_no_range_or_no_column() {
        let ast = parse_q("speed>25").expect("parse");
        let c = compile_prefilter(&ast, None, "m", 1, &ex).expect("compiles");
        assert!(!c.sql.contains("observed_at"), "{}", c.sql);
        assert_dense(&c, 1);
        // deletedAt now HAS a column bound — NULL-tolerant, because the
        // column was unfilled before migration 0009's era
        let r = InstanceRange {
            timeproperty: "deletedAt",
            ..between()
        };
        let c = compile_prefilter(&ast, Some(&r), "m", 1, &ex).expect("compiles");
        assert!(
            c.sql.contains("qi.deleted_at IS NULL OR"),
            "old rows must pass to the text predicate: {}",
            c.sql
        );
    }
}
