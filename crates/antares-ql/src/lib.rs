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

/// Longest `q=` accepted, before parsing. The URI cap is 8 KiB but a POST
/// query body carries `q` too, where the only other ceiling is the 4 MiB body
/// limit — so the string needs its own bound at the one entry point.
const MAX_Q_BYTES: usize = 4096;

/// Nesting depth ceiling. `(` costs three stack frames (`or_expr` → `and_expr`
/// → `term`), and a Rust stack overflow is a guard-page abort, NOT a catchable
/// panic — no tower layer can contain it, so the parser must refuse before it
/// recurses rather than be rescued afterwards. 64 is far past any real query
/// and far below the ~2 MiB tokio worker stack.
const MAX_Q_DEPTH: usize = 64;

/// AST size cap (§16.3) — checked after parsing, which is safe once depth and
/// length are bounded first.
const MAX_Q_NODES: usize = 512;

/// Parse an NGSI-LD `q=` expression.
pub fn parse_q(input: &str) -> Result<QNode, NgsiError> {
    if input.len() > MAX_Q_BYTES {
        return Err(NgsiError::TooComplexQuery(format!(
            "q expression exceeds {MAX_Q_BYTES} bytes"
        )));
    }
    let mut p = Parser {
        rest: input.trim(),
        depth: 0,
    };
    let node = p.or_expr()?;
    if !p.rest.is_empty() {
        return Err(bad(input, "trailing input"));
    }
    if q_nodes(&node) > MAX_Q_NODES {
        return Err(NgsiError::TooComplexQuery(format!(
            "q expression exceeds {MAX_Q_NODES} nodes"
        )));
    }
    Ok(node)
}

fn q_nodes(n: &QNode) -> usize {
    match n {
        QNode::And(xs) | QNode::Or(xs) => 1 + xs.iter().map(q_nodes).sum::<usize>(),
        _ => 1,
    }
}

fn bad(input: &str, why: &str) -> NgsiError {
    NgsiError::BadRequestData(format!("invalid q expression {input:?}: {why}"))
}

struct Parser<'a> {
    rest: &'a str,
    /// open parentheses currently on the stack (see `MAX_Q_DEPTH`)
    depth: usize,
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
            // refuse BEFORE recursing — an overflow here aborts the process
            self.depth += 1;
            if self.depth > MAX_Q_DEPTH {
                return Err(NgsiError::TooComplexQuery(format!(
                    "q expression nests deeper than {MAX_Q_DEPTH}"
                )));
            }
            let node = self.or_expr()?;
            self.depth -= 1;
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

#[cfg(test)]
mod complexity_tests {
    use super::*;

    #[test]
    fn q_complexity_cap_is_403_class() {
        // I2/§16.3: >512 nodes → TooComplexQuery, small trees untouched.
        let ok = "a==1;b==2|c==3";
        assert!(parse_q(ok).is_ok());
        let huge = (0..600)
            .map(|i| format!("a{i}==1"))
            .collect::<Vec<_>>()
            .join(";");
        match parse_q(&huge) {
            Err(NgsiError::TooComplexQuery(_)) => {}
            other => panic!("expected TooComplexQuery, got {other:?}"),
        }
    }

    /// Security audit C1 (2026-08-04): the parser recursed once per `(` with
    /// no depth counter. A Rust stack overflow is a guard-page ABORT, not a
    /// catchable panic — no tower layer can contain it — so ~4000 parens in a
    /// query string killed the whole broker process, and a percent-encoded
    /// copy stored in a subscription made that a restart-surviving crash loop.
    #[test]
    fn deep_nesting_is_refused_before_it_can_overflow_the_stack() {
        let deep = format!("{}a==1{}", "(".repeat(50_000), ")".repeat(50_000));
        assert!(
            matches!(parse_q(&deep), Err(NgsiError::TooComplexQuery(_))),
            "deep nesting must be a 403, never an abort"
        );
        // the length cap fires first on that one; check depth alone too
        let deep = format!("{}a==1{}", "(".repeat(300), ")".repeat(300));
        assert!(deep.len() < MAX_Q_BYTES);
        assert!(matches!(parse_q(&deep), Err(NgsiError::TooComplexQuery(_))));
        // and ordinary grouping still parses
        assert!(parse_q("((a==1|b==2);c==3)").is_ok());
    }

    #[test]
    fn overlong_q_is_refused_at_the_entry_point() {
        // a POST query body carries `q` too, where the URI cap does not apply
        let long = format!("a=={}", "x".repeat(MAX_Q_BYTES));
        assert!(matches!(parse_q(&long), Err(NgsiError::TooComplexQuery(_))));
    }
}
