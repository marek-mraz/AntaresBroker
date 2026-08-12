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
        path: QPath,
        op: CmpOp,
        value: QValue,
    },
    /// Bare attribute path = existence check (`q=temperature`).
    Exists {
        path: QPath,
        negated: bool,
    },
}

/// 4.9 `Attribute = LinkedEntityRelation` — zero or more `attr{[T[,T]:]…}`
/// hops (EXAMPLE 13/14), then `ValuePath = DottedPath *1([DottedPath])`:
/// a dotted path plus an optional single trailing bracket that is either a
/// compound-value member path (EXAMPLE 9/10/11) or a language filter
/// (`[en]` / `[*]`, Equal/Unequal languageMap semantics).
#[derive(Debug, Clone, PartialEq)]
pub struct QPath {
    pub links: Vec<Link>,
    pub path: Vec<String>,
    pub bracket: Option<Vec<String>>,
}

/// One `attr{…}` linked-entity hop with its optional EntityType hints.
#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub attr: String,
    pub types: Vec<String>,
}

impl QPath {
    /// Plain dotted path (the pre-4.9-extension shape).
    pub fn dotted(path: Vec<String>) -> Self {
        Self {
            links: Vec::new(),
            path,
            bracket: None,
        }
    }

    /// The top-level Attribute name this path filters on.
    pub fn top(&self) -> Option<&str> {
        self.links
            .first()
            .map(|l| l.attr.as_str())
            .or_else(|| self.path.first().map(String::as_str))
    }
}

impl QNode {
    /// Every top-level attribute name this expression references, in source
    /// order.
    ///
    /// Purge (5.6.21.4 b/c) qualifies an `attrs` list or a `q` only when it
    /// includes "at least one non-system Attribute", so the caller needs the
    /// referenced names rather than just "is there a q".
    pub fn attribute_paths(&self) -> Vec<&str> {
        let mut out = Vec::new();
        self.collect_paths(&mut out);
        out
    }

    /// True when any referenced Attribute path uses a `attr{…}` linked-entity
    /// hop (4.9 LinkedEntityRelation). Purge (5.6.21.4) must reject filter
    /// conditions that include Linked Entity attributes.
    pub fn has_linked_paths(&self) -> bool {
        match self {
            QNode::And(ns) | QNode::Or(ns) => ns.iter().any(Self::has_linked_paths),
            QNode::Cmp { path, .. } | QNode::Exists { path, .. } => !path.links.is_empty(),
        }
    }

    fn collect_paths<'a>(&'a self, out: &mut Vec<&'a str>) {
        match self {
            QNode::And(ns) | QNode::Or(ns) => {
                for n in ns {
                    n.collect_paths(out);
                }
            }
            QNode::Cmp { path, .. } | QNode::Exists { path, .. } => out.extend(path.top()),
        }
    }
}

/// System-generated members that never count as a "non-system Attribute"
/// (5.6.21.4). `id`/`type`/`scope` are Entity members, the timestamps are the
/// system temporal attributes of 6.3.11.
pub const SYSTEM_ATTRS: &[&str] = &[
    "id",
    "type",
    "scope",
    "createdAt",
    "modifiedAt",
    "expiresAt",
    "deletedAt",
    "instanceId",
];

/// True when `name` is an ordinary (non-system) Attribute name.
pub fn is_non_system_attr(name: &str) -> bool {
    !SYSTEM_ATTRS.contains(&name)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    Eq,         // ==
    Ne,         // !=
    Gt,         // >
    Ge,         // >=
    Lt,         // <
    Le,         // <=
    Pattern,    // ~=
    NotPattern, // !~= (4.9 notPatternOp)
}

#[derive(Debug, Clone, PartialEq)]
pub enum QValue {
    Str(String),
    Num(f64),
    Bool(bool),
    /// 4.9 `ValueList = Value 1*(, Value)` — scalars only, `==`/`!=` only.
    List(Vec<QValue>),
    /// 4.9 `Range = ComparableValue dots ComparableValue` — `==`/`!=` only.
    /// Endpoints are the same scalar variant (Num..Num or Str..Str; dates and
    /// times ride in Str and order correctly because 4.6.3 pins them to
    /// fixed-width UTC forms).
    Range(Box<QValue>, Box<QValue>),
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

/// Parse an NGSI-LD `q=` expression. Complexity ceilings raise
/// TooComplexQuery per 5.5.6 ("a query operation … so complex that cannot
/// be resolved").
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
        let path = self.qpath()?;
        if let Some(op) = self.cmp_op() {
            if negated {
                return Err(bad(self.rest, "'!' only prefixes an existence check"));
            }
            let value = self.value(op)?;
            Ok(QNode::Cmp { path, op, value })
        } else {
            Ok(QNode::Exists { path, negated })
        }
    }

    /// 4.9 Attribute: `attr{[T[,T]:]…}` linked-entity hops, then a dotted
    /// path, then at most one trailing `[member.path]` / `[lang]` / `[*]`.
    fn qpath(&mut self) -> Result<QPath, NgsiError> {
        let mut links = Vec::new();
        let mut braces = 0usize;
        let mut name = self.name_token()?;
        // LinkedEntityRelation: AttrName{ [EntityType(,EntityType)*:] … }
        while self.eat('{') {
            braces += 1;
            if braces > 8 {
                return Err(NgsiError::TooComplexQuery(
                    "q linked-entity path nests deeper than 8".into(),
                ));
            }
            let mut types = Vec::new();
            let mut inner = self.name_token()?;
            if self.rest.starts_with(',') || self.rest.starts_with(':') {
                types.push(inner);
                while self.eat(',') {
                    types.push(self.name_token()?);
                }
                if !self.eat(':') {
                    return Err(bad(self.rest, "expected ':' after EntityType hints"));
                }
                inner = self.name_token()?;
            }
            links.push(Link { attr: name, types });
            name = inner;
        }
        let mut path = vec![name];
        while self.eat('.') {
            path.push(self.name_token()?);
        }
        let bracket = if self.eat('[') {
            let b = if self.eat('*') {
                vec!["*".to_owned()]
            } else {
                let mut b = vec![self.name_token()?];
                while self.eat('.') {
                    b.push(self.name_token()?);
                }
                b
            };
            if !self.eat(']') {
                return Err(bad(self.rest, "expected ']'"));
            }
            Some(b)
        } else {
            None
        };
        for _ in 0..braces {
            if !self.eat('}') {
                return Err(bad(self.rest, "expected '}'"));
            }
        }
        Ok(QPath {
            links,
            path,
            bracket,
        })
    }

    /// One path segment: everything up to a structural delimiter.
    fn name_token(&mut self) -> Result<String, NgsiError> {
        self.rest = self.rest.trim_start();
        let end = self
            .rest
            .find(|c: char| "=!<>~;|(),.{}[]: ".contains(c))
            .unwrap_or(self.rest.len());
        let (raw, rest) = self.rest.split_at(end);
        if raw.is_empty() {
            return Err(bad(rest, "expected attribute name"));
        }
        // spacing around the segment is insignificant (`a ==1`, `a ; b`)
        self.rest = rest.trim_start();
        Ok(raw.to_owned())
    }

    fn cmp_op(&mut self) -> Option<CmpOp> {
        for (tok, op) in [
            ("==", CmpOp::Eq),
            // "!~=" before "!=" — the longer token must win the prefix race
            ("!~=", CmpOp::NotPattern),
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

    /// Query Term value for `op` — 4.9 p.84 pairs them precisely:
    /// `Operator ComparableValue` (ordering), `equal/unequal CompEqualityValue`
    /// (adds true/false, ValueList, Range, URI), `patternOp/notPatternOp
    /// RegExp`. Lists and ranges with an ordering or pattern operator are a
    /// grammar violation, not an empty result.
    fn value(&mut self, op: CmpOp) -> Result<QValue, NgsiError> {
        let first = self.scalar()?;
        let equality = matches!(op, CmpOp::Eq | CmpOp::Ne);
        self.rest = self.rest.trim_start();
        if let Some(rest) = self.rest.strip_prefix("..") {
            if !equality {
                return Err(bad(
                    self.rest,
                    "a Range is only valid with == or != (4.9 CompEqualityValue)",
                ));
            }
            self.rest = rest.trim_start();
            let hi = self.scalar()?;
            // Range = ComparableValue..ComparableValue: booleans excluded, and
            // an order relation needs both endpoints in one value space
            if std::mem::discriminant(&first) != std::mem::discriminant(&hi)
                || matches!(first, QValue::Bool(_))
            {
                return Err(bad(
                    self.rest,
                    "Range endpoints must be two comparable values of the same type",
                ));
            }
            return Ok(QValue::Range(Box::new(first), Box::new(hi)));
        }
        if self.rest.starts_with(',') {
            if !equality {
                return Err(bad(
                    self.rest,
                    "a ValueList is only valid with == or != (4.9 CompEqualityValue)",
                ));
            }
            let mut items = vec![first];
            while self.eat(',') {
                self.rest = self.rest.trim_start();
                items.push(self.scalar()?);
                self.rest = self.rest.trim_start();
            }
            return Ok(QValue::List(items));
        }
        if matches!(op, CmpOp::Gt | CmpOp::Ge | CmpOp::Lt | CmpOp::Le)
            && matches!(first, QValue::Bool(_))
        {
            return Err(bad(
                self.rest,
                "true/false are only valid with == or != (4.9 OtherValue)",
            ));
        }
        Ok(first)
    }

    /// One scalar literal. Unquoted tokens stop at a delimiter or at `..`
    /// (the Range separator) — a decimal like `10.5` has no `..`, so
    /// `10.5..20.5` still splits at the right place.
    fn scalar(&mut self) -> Result<QValue, NgsiError> {
        if let Some(rest) = self.rest.strip_prefix('"') {
            let end = rest
                .find('"')
                .ok_or_else(|| bad(rest, "unterminated string"))?;
            let (s, rest) = rest.split_at(end);
            self.rest = &rest[1..];
            return Ok(QValue::Str(s.to_owned()));
        }
        let stop = self
            .rest
            .find(|c: char| ";|(),".contains(c))
            .unwrap_or(self.rest.len());
        let end = match self.rest.find("..") {
            Some(d) if d < stop => d,
            _ => stop,
        };
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
                path: QPath::dotted(vec!["brandName".into()]),
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
                path: QPath::dotted(vec!["speed".into(), "value".into()]),
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
                path: QPath::dotted(vec!["temperature".into()]),
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

    #[test]
    fn value_list_parses_with_equality_ops_only() {
        // 4.9 p.85 ValueList = Value 1*(, Value); p.84 pairs it with ==/!= only
        let q = parse_q(r#"color=="black","red""#).expect("parse");
        assert_eq!(
            q,
            QNode::Cmp {
                path: QPath::dotted(vec!["color".into()]),
                op: CmpOp::Eq,
                value: QValue::List(vec![QValue::Str("black".into()), QValue::Str("red".into())])
            }
        );
        // spec's own spacing (`color!= "black", "red"`) must parse too
        let q = parse_q(r#"color!= "black", "red""#).expect("parse");
        assert!(matches!(
            q,
            QNode::Cmp {
                op: CmpOp::Ne,
                value: QValue::List(_),
                ..
            }
        ));
        // mixed scalar kinds are legal (ValueList is over Value)
        assert!(parse_q("a==1,2,3").is_ok());
        // ordering + list is a grammar violation → 400, not empty result
        assert!(parse_q(r#"a>"x","y""#).is_err());
        assert!(parse_q("a>=1,2").is_err());
    }

    #[test]
    fn range_parses_with_equality_ops_only() {
        // 4.9 p.85 Range = ComparableValue dots ComparableValue
        let q = parse_q("temperature==10..20").expect("parse");
        assert_eq!(
            q,
            QNode::Cmp {
                path: QPath::dotted(vec!["temperature".into()]),
                op: CmpOp::Eq,
                value: QValue::Range(Box::new(QValue::Num(10.0)), Box::new(QValue::Num(20.0)))
            }
        );
        // decimals keep their fraction; `..` is not mistaken for `.`
        let q = parse_q("t!=10.5..20.5").expect("parse");
        assert!(matches!(
            q,
            QNode::Cmp {
                op: CmpOp::Ne,
                value: QValue::Range(_, _),
                ..
            }
        ));
        // DateTime endpoints (unquoted, per EXAMPLE 8 style literals)
        let q = parse_q("observedAt==2021-01-01T00:00:00Z..2021-02-01T00:00:00Z").expect("parse");
        assert!(matches!(
            q,
            QNode::Cmp {
                value: QValue::Range(_, _),
                ..
            }
        ));
        // ordering + range violates the grammar; bools are not ComparableValue
        assert!(parse_q("a>1..5").is_err());
        assert!(parse_q("a==true..false").is_err());
        assert!(parse_q("a==1..\"x\"").is_err(), "mixed-type endpoints");
    }

    #[test]
    fn not_pattern_op() {
        // 4.9 p.85 notPatternOp = !~=
        let q = parse_q(r#"name!~="^Merc""#).expect("parse");
        assert_eq!(
            q,
            QNode::Cmp {
                path: QPath::dotted(vec!["name".into()]),
                op: CmpOp::NotPattern,
                value: QValue::Str("^Merc".into())
            }
        );
    }

    #[test]
    fn bool_with_ordering_op_is_a_grammar_violation() {
        // p.84: Operator (ordering) takes ComparableValue; true/false are
        // OtherValue, reachable only through ==/!=
        assert!(parse_q("a>true").is_err());
        assert!(parse_q("a==true").is_ok());
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
