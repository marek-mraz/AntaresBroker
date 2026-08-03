//! NGSI-LD Query Language (CIM 009 clause 4.9) — parser seed.
//!
//! v0 grammar subset: comparisons over dotted attribute paths, `;` (AND) and
//! `|` (OR), parentheses. This is the risk-#2 crate: it grows test-first
//! against the CI/Cons TPs (docs/deep-analysis.md §12).

use antares_model::NgsiError;

#[derive(Debug, Clone, PartialEq)]
pub enum QNode {
    And(Vec<QNode>),
    Or(Vec<QNode>),
    Cmp {
        path: Vec<String>,
        op: CmpOp,
        value: QValue,
    },
    /// Bare attribute path = existence check (`q=temperature`).
    Exists {
        path: Vec<String>,
        negated: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    Eq,      // ==
    Ne,      // !=
    Gt,      // >
    Ge,      // >=
    Lt,      // <
    Le,      // <=
    Pattern, // ~=
}

#[derive(Debug, Clone, PartialEq)]
pub enum QValue {
    Str(String),
    Num(f64),
    Bool(bool),
}

/// Parse an NGSI-LD `q=` expression.
pub fn parse_q(input: &str) -> Result<QNode, NgsiError> {
    let mut p = Parser { rest: input.trim() };
    let node = p.or_expr()?;
    if p.rest.is_empty() {
        Ok(node)
    } else {
        Err(bad(input, "trailing input"))
    }
}

fn bad(input: &str, why: &str) -> NgsiError {
    NgsiError::BadRequestData(format!("invalid q expression {input:?}: {why}"))
}

struct Parser<'a> {
    rest: &'a str,
}

impl<'a> Parser<'a> {
    fn or_expr(&mut self) -> Result<QNode, NgsiError> {
        let mut items = vec![self.and_expr()?];
        while self.eat('|') {
            items.push(self.and_expr()?);
        }
        Ok(if items.len() == 1 {
            items.pop().expect("non-empty")
        } else {
            QNode::Or(items)
        })
    }

    fn and_expr(&mut self) -> Result<QNode, NgsiError> {
        let mut items = vec![self.term()?];
        while self.eat(';') {
            items.push(self.term()?);
        }
        Ok(if items.len() == 1 {
            items.pop().expect("non-empty")
        } else {
            QNode::And(items)
        })
    }

    fn term(&mut self) -> Result<QNode, NgsiError> {
        if self.eat('(') {
            let node = self.or_expr()?;
            if !self.eat(')') {
                return Err(bad(self.rest, "expected ')'"));
            }
            return Ok(node);
        }
        let negated = self.eat('!');
        let path = self.path()?;
        if let Some(op) = self.cmp_op() {
            if negated {
                return Err(bad(self.rest, "'!' only prefixes an existence check"));
            }
            let value = self.value()?;
            Ok(QNode::Cmp { path, op, value })
        } else {
            Ok(QNode::Exists { path, negated })
        }
    }

    fn path(&mut self) -> Result<Vec<String>, NgsiError> {
        let end = self
            .rest
            .find(|c: char| "=!<>~;|()".contains(c))
            .unwrap_or(self.rest.len());
        let (raw, rest) = self.rest.split_at(end);
        let raw = raw.trim();
        if raw.is_empty() {
            return Err(bad(rest, "expected attribute path"));
        }
        self.rest = rest;
        Ok(raw.split('.').map(str::to_owned).collect())
    }

    fn cmp_op(&mut self) -> Option<CmpOp> {
        for (tok, op) in [
            ("==", CmpOp::Eq),
            ("!=", CmpOp::Ne),
            ("~=", CmpOp::Pattern),
            (">=", CmpOp::Ge),
            ("<=", CmpOp::Le),
            (">", CmpOp::Gt),
            ("<", CmpOp::Lt),
        ] {
            if let Some(rest) = self.rest.strip_prefix(tok) {
                self.rest = rest;
                return Some(op);
            }
        }
        None
    }

    fn value(&mut self) -> Result<QValue, NgsiError> {
        if let Some(rest) = self.rest.strip_prefix('"') {
            let end = rest
                .find('"')
                .ok_or_else(|| bad(rest, "unterminated string"))?;
            let (s, rest) = rest.split_at(end);
            self.rest = &rest[1..];
            return Ok(QValue::Str(s.to_owned()));
        }
        let end = self
            .rest
            .find(|c: char| ";|()".contains(c))
            .unwrap_or(self.rest.len());
        let (raw, rest) = self.rest.split_at(end);
        let raw = raw.trim();
        self.rest = rest;
        match raw {
            "true" => Ok(QValue::Bool(true)),
            "false" => Ok(QValue::Bool(false)),
            _ => raw
                .parse::<f64>()
                .map(QValue::Num)
                // unquoted non-numeric literal (spec allows e.g. dates)
                .or_else(|_| {
                    if raw.is_empty() {
                        Err(bad(raw, "expected value"))
                    } else {
                        Ok(QValue::Str(raw.to_owned()))
                    }
                }),
        }
    }

    fn eat(&mut self, c: char) -> bool {
        if let Some(rest) = self.rest.strip_prefix(c) {
            self.rest = rest;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_comparison() {
        let q = parse_q(r#"brandName=="Mercedes""#).expect("parse");
        assert_eq!(
            q,
            QNode::Cmp {
                path: vec!["brandName".into()],
                op: CmpOp::Eq,
                value: QValue::Str("Mercedes".into())
            }
        );
    }

    #[test]
    fn and_or_precedence() {
        // `a==1;b==2|c==3` == (a AND b) OR c per grammar: OR binds looser
        let q = parse_q("a==1;b==2|c==3").expect("parse");
        match q {
            QNode::Or(items) => {
                assert_eq!(items.len(), 2);
                assert!(matches!(items[0], QNode::And(_)));
            }
            other => panic!("expected Or, got {other:?}"),
        }
    }

    #[test]
    fn dotted_path_and_numbers() {
        let q = parse_q("speed.value>=80.5").expect("parse");
        assert_eq!(
            q,
            QNode::Cmp {
                path: vec!["speed".into(), "value".into()],
                op: CmpOp::Ge,
                value: QValue::Num(80.5)
            }
        );
    }

    #[test]
    fn existence_and_negation() {
        assert_eq!(
            parse_q("!temperature").expect("parse"),
            QNode::Exists {
                path: vec!["temperature".into()],
                negated: true
            }
        );
    }

    #[test]
    fn parens_group() {
        let q = parse_q("(a==1|b==2);c==3").expect("parse");
        assert!(matches!(q, QNode::And(_)));
    }

    #[test]
    fn rejects_garbage() {
        for bad in ["", "==5", "a==\"unterminated", "a==1)"] {
            assert!(parse_q(bad).is_err(), "should reject {bad:?}");
        }
    }
}
