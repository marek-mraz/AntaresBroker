//! AST → SQL compilation + schema (docs/deep-analysis.md §8).
//!
//! Phase-0 seed: the q= compiler emits parameterized SQL — structure from the
//! compiler, values as binds ONLY (§16.2). sqlx store implementations land in
//! phase 1; migrations live in `migrations/`.

pub mod store;

use antares_ql::{CmpOp, QNode, QValue};

/// A compiled predicate: SQL text with `$n` placeholders + the bind values.
#[derive(Debug, PartialEq)]
pub struct CompiledQ {
    pub sql: String,
    pub binds: Vec<Bind>,
}

#[derive(Debug, PartialEq)]
pub enum Bind {
    Text(String),
    Num(f64),
    Bool(bool),
}

/// Compile a parsed q= AST against the expanded-entity JSONB column.
///
/// v0 supports top-level-attribute value comparison and existence via
/// `jsonb_path_exists`; dotted-path and datasetId semantics grow test-first
/// against the CI/Cons TPs.
pub fn compile_q(node: &QNode, dollar: &mut usize) -> CompiledQ {
    match node {
        QNode::And(items) => join(items, " AND ", dollar),
        QNode::Or(items) => join(items, " OR ", dollar),
        QNode::Exists { path, negated } => {
            *dollar += 1;
            let n = *dollar;
            let sql = if *negated {
                format!("NOT jsonb_path_exists(entity, format('$.%s', ${n}::text)::jsonpath)")
            } else {
                format!("jsonb_path_exists(entity, format('$.%s', ${n}::text)::jsonpath)")
            };
            CompiledQ {
                sql,
                binds: vec![Bind::Text(path.join("."))],
            }
        }
        QNode::Cmp { path, op, value } => {
            *dollar += 2;
            let p = *dollar - 1;
            let v = *dollar;
            let sql_op = match op {
                CmpOp::Eq => "==",
                CmpOp::Ne => "!=",
                CmpOp::Gt => ">",
                CmpOp::Ge => ">=",
                CmpOp::Lt => "<",
                CmpOp::Le => "<=",
                CmpOp::Pattern => "like_regex",
            };
            // Path and value both travel as binds; jsonpath text is assembled
            // in SQL from bound parameters, never spliced from user input.
            let sql = format!(
                "jsonb_path_exists(entity, format('$.%s ? (@ {sql_op} %s)', ${p}::text, ${v}::text)::jsonpath)"
            );
            let bind_v = match value {
                QValue::Str(s) => Bind::Text(format!("\"{s}\"")),
                QValue::Num(n) => Bind::Num(*n),
                QValue::Bool(b) => Bind::Bool(*b),
            };
            CompiledQ {
                sql,
                binds: vec![Bind::Text(path.join(".")), bind_v],
            }
        }
    }
}

fn join(items: &[QNode], sep: &str, dollar: &mut usize) -> CompiledQ {
    let mut parts = Vec::with_capacity(items.len());
    let mut binds = Vec::new();
    for item in items {
        let c = compile_q(item, dollar);
        parts.push(format!("({})", c.sql));
        binds.extend(c.binds);
    }
    CompiledQ {
        sql: parts.join(sep),
        binds,
    }
}

/// The transaction preamble that makes RLS effective (§3): always SET LOCAL,
/// never session-level SET.
pub const SET_TENANT_SQL: &str = "SELECT set_config('antares.tenant', $1, true)";

#[cfg(test)]
mod tests {
    use super::*;
    use antares_ql::parse_q;

    #[test]
    fn compiles_comparison_with_binds_only() {
        let ast = parse_q("speed>=80").expect("parse");
        let mut dollar = 0;
        let c = compile_q(&ast, &mut dollar);
        assert!(
            c.sql.contains("$1") && c.sql.contains("$2"),
            "sql: {}",
            c.sql
        );
        assert_eq!(c.binds[0], Bind::Text("speed".into()));
        assert_eq!(c.binds[1], Bind::Num(80.0));
        // No user value ever appears in the SQL text itself:
        assert!(
            !c.sql.contains("80"),
            "value must be a bind, sql: {}",
            c.sql
        );
    }

    #[test]
    fn injection_attempt_stays_in_binds() {
        let evil = r#"a=="'; DROP TABLE entities; --""#;
        let ast = parse_q(evil).expect("parse");
        let mut dollar = 0;
        let c = compile_q(&ast, &mut dollar);
        assert!(
            !c.sql.to_lowercase().contains("drop table"),
            "sql: {}",
            c.sql
        );
        assert!(matches!(&c.binds[1], Bind::Text(s) if s.contains("DROP TABLE")));
    }

    #[test]
    fn and_or_compose() {
        let ast = parse_q("a==1;b==2|c==3").expect("parse");
        let mut dollar = 0;
        let c = compile_q(&ast, &mut dollar);
        assert!(c.sql.contains(" OR "), "sql: {}", c.sql);
        assert_eq!(c.binds.len(), 6);
    }
}
