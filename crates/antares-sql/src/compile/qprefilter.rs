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
//! * negated existence, `!=`, patterns, dotted/bracket/linked paths — TRUE
//!   (`compile::q` refuses them; entity-level negation over per-instance
//!   rows is not superset-safe to push).
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
    let (sql, binds) = emit(node, range, entity, first_bind, expand)?;
    Some(CompiledQ { sql, binds })
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
) -> Option<(String, Vec<String>)> {
    match node {
        QNode::And(items) => {
            let mut sqls = Vec::new();
            let mut binds = Vec::new();
            for it in items {
                if let Some((s, b)) = emit(it, range, entity, first + binds.len(), expand) {
                    sqls.push(s);
                    binds.extend(b);
                }
            }
            (!sqls.is_empty()).then(|| (format!("({})", sqls.join(" AND ")), binds))
        }
        QNode::Or(items) => {
            let mut sqls = Vec::new();
            let mut binds = Vec::new();
            for it in items {
                let (s, b) = emit(it, range, entity, first + binds.len(), expand)?;
                sqls.push(s);
                binds.extend(b);
            }
            (!sqls.is_empty()).then(|| (format!("({})", sqls.join(" OR ")), binds))
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
) -> Option<(String, Vec<String>)> {
    // same exact-subset rule as compile::q — anything fancier stays in memory
    if !path.links.is_empty() || path.bracket.is_some() || path.path.len() != 1 {
        return None;
    }
    // binds: [attr IRI, window time(s)…, jsonpath(s)…]
    let mut binds = vec![expand(path.path.first()?)];
    let mut window = String::new();
    if let Some(r) = range {
        if let Some(cb) = column_range_bound(r, "qi", first + binds.len()) {
            binds.push(r.time_at.to_owned());
            if r.timerel == "between" {
                binds.push(r.end_time_at?.to_owned());
            }
            window = format!(" AND {cb}");
        }
    }
    let l = compile_instance_leaf(cmp, "qi.data", first + binds.len())?;
    let sql = format!(
        "EXISTS (SELECT 1 FROM attr_instances qi \
         WHERE qi.tenant_id = {entity}.tenant_id AND qi.entity_id = {entity}.id \
         AND qi.attr_id = ${first}{window} AND {})",
        l.sql
    );
    binds.extend(l.binds);
    Some((sql, binds))
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
        // widened column window INSIDE the existence test (5.7.4.4 S2)
        assert!(
            c.sql
                .contains("qi.observed_at >= $4::timestamptz - interval '48 hours'"),
            "{}",
            c.sql
        );
        assert!(
            c.sql
                .contains("qi.observed_at < $5::timestamptz + interval '48 hours'"),
            "{}",
            c.sql
        );
        assert_eq!(c.binds[1], "2026-03-01T00:00:00Z");
        assert_eq!(c.binds[2], "2026-03-02T00:00:00Z");
        // the jsonpath leaf is rooted at the instance, not the entity doc
        assert!(c.binds[3].starts_with("$.\"value\""), "{}", c.binds[3]);
        assert_dense(&c, 3);
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
            "!speed",             // negated existence: not superset-safe per-row
            "speed!=10",          // 4.9 != caveats (compile::q declines)
            r#"name~="^x""#,      // regex dialect mismatch
            r#"name>"m""#,        // string ordering vs collation
            "a.b==1",             // dotted path ambiguity
            r#"label[en]=="hi""#, // language-filter bracket
        ] {
            assert!(pf(q).is_none(), "{q} must be trivial");
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
        // a timeproperty without a parsed column: EXISTS still compiles,
        // window stays out (superset either way)
        let r = InstanceRange {
            timeproperty: "deletedAt",
            ..between()
        };
        let c = compile_prefilter(&ast, Some(&r), "m", 1, &ex).expect("compiles");
        assert!(!c.sql.contains("deleted_at"), "{}", c.sql);
    }
}
