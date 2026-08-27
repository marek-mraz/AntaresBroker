// SPDX-License-Identifier: EUPL-1.2
//! Render an AST back to 4.9 `q=` syntax, so a rewritten query can travel
//! on as a query string. `parse_q(&node.to_string())` yields `node` again.

use crate::{CmpOp, Link, QNode, QPath, QValue};
use std::fmt;

impl fmt::Display for QNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // `;` binds tighter than `|`: an Or inside an And needs the
            // parentheses back, nothing else does.
            QNode::And(items) => join(f, items, ";", |n| matches!(n, QNode::Or(_))),
            QNode::Or(items) => join(f, items, "|", |n| matches!(n, QNode::Or(_))),
            QNode::Cmp { path, op, value } => write!(f, "{path}{op}{value}"),
            QNode::Exists { path, negated } => {
                if *negated {
                    f.write_str("!")?;
                }
                write!(f, "{path}")
            }
        }
    }
}

fn join(
    f: &mut fmt::Formatter<'_>,
    items: &[QNode],
    sep: &str,
    parens: impl Fn(&QNode) -> bool,
) -> fmt::Result {
    for (i, n) in items.iter().enumerate() {
        if i > 0 {
            f.write_str(sep)?;
        }
        if parens(n) {
            write!(f, "({n})")?;
        } else {
            write!(f, "{n}")?;
        }
    }
    Ok(())
}

impl fmt::Display for QPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for Link { attr, types } in &self.links {
            write!(f, "{attr}{{")?;
            if !types.is_empty() {
                write!(f, "{}:", types.join(","))?;
            }
        }
        f.write_str(&self.path.join("."))?;
        if let Some(b) = &self.bracket {
            write!(f, "[{}]", b.join("."))?;
        }
        for _ in &self.links {
            f.write_str("}")?;
        }
        Ok(())
    }
}

impl fmt::Display for CmpOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            CmpOp::Eq => "==",
            CmpOp::Ne => "!=",
            CmpOp::Gt => ">",
            CmpOp::Ge => ">=",
            CmpOp::Lt => "<",
            CmpOp::Le => "<=",
            CmpOp::Pattern => "~=",
            CmpOp::NotPattern => "!~=",
        })
    }
}

impl fmt::Display for QValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // always quoted: the parser reads an unquoted date into Str too,
            // and the quoted form parses back to the same value
            QValue::Str(s) => write!(f, "\"{s}\""),
            QValue::Num(n) => write!(f, "{n}"),
            QValue::Bool(b) => write!(f, "{b}"),
            QValue::List(items) => {
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        f.write_str(",")?;
                    }
                    write!(f, "{v}")?;
                }
                Ok(())
            }
            QValue::Range(lo, hi) => write!(f, "{lo}..{hi}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{parse_q, QNode, QPath, QValue};

    /// Every grammar shape survives parse → render → parse unchanged.
    #[test]
    fn render_round_trips_through_the_parser() {
        for q in [
            r#"brandName=="Mercedes""#,
            "speed>=5;heading<90",
            r#"speed>25|name~="^m""#,
            r#"(speed>25|heading>100);route=="550","551""#,
            "speed==10..40",
            "!heading;speed>1",
            "ref{Vehicle,Car:speed}>3",
            "a{b{c}}==1",
            r#"label[en]=="x""#,
            r#"address[city]=="Paris""#,
            "label[*]!=\"y\"",
            "x!~=\"^y\"",
            "t==2020-01-01T00:00:00Z",
            "flag==true",
            "a;b|c;d",
            "a|(b;c)|d",
        ] {
            let node = parse_q(q).expect(q);
            let rendered = node.to_string();
            let again = parse_q(&rendered).unwrap_or_else(|e| panic!("{q} → {rendered}: {e}"));
            assert_eq!(again, node, "{q} → {rendered}");
        }
    }

    /// A tree built by hand (an Or nested in an And, which the parser only
    /// produces through parentheses) renders with the parentheses restored.
    #[test]
    fn nested_or_inside_and_gets_its_parentheses_back() {
        let node = QNode::And(vec![
            QNode::Or(vec![
                QNode::Exists {
                    path: QPath::dotted(vec!["a".into()]),
                    negated: false,
                },
                QNode::Exists {
                    path: QPath::dotted(vec!["b".into()]),
                    negated: false,
                },
            ]),
            QNode::Cmp {
                path: QPath::dotted(vec!["owner".into()]),
                op: crate::CmpOp::Eq,
                value: QValue::Str("t1".into()),
            },
        ]);
        assert_eq!(node.to_string(), r#"(a|b);owner=="t1""#);
        assert_eq!(parse_q(&node.to_string()).expect("parses"), node);
    }

    #[test]
    fn the_ast_serializes() {
        let node = parse_q("speed>3").expect("parse");
        let json = serde_json::to_value(&node).expect("serialize");
        assert_eq!(json["Cmp"]["op"], "Gt", "{json}");
        assert_eq!(json["Cmp"]["path"]["path"][0], "speed", "{json}");
    }
}
