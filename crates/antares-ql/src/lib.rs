// SPDX-License-Identifier: EUPL-1.2
//! NGSI-LD Query Language (CIM 009 clause 4.9): one AST, two backends.
//!
//! [`parse_q`] turns a `q=` expression into a [`QNode`]; [`eval`] evaluates
//! it against an in-memory expanded entity (the broker's query path and its
//! subscription matcher share this evaluator), [`sql`] lowers it to a
//! bind-parameter jsonpath predicate for Postgres. The AST is `Serialize`
//! and `Clone`, and renders back to `q=` syntax through `Display`, so a
//! gateway can inspect or rewrite a query (strip an attribute, AND in an
//! authorization predicate) and forward it with the broker's own semantics.
#![cfg_attr(not(test), warn(clippy::expect_used))]
#![deny(missing_docs)]
#![cfg_attr(test, allow(clippy::unwrap_used))]

pub mod eval;
pub mod geo;
pub mod regex;
mod render;
pub mod scope;
pub mod sql;

use antares_model::NgsiError;

/// Entity Type Selection Language (4.17) match against expanded type IRIs:
/// `,`/`|` = OR of alternatives, `(a;b)` = AND within one alternative.
pub fn type_selection_matches(sel: &str, types: &[&str], ctx: &antares_jsonld::Context) -> bool {
    sel.split([',', '|']).any(|alt| {
        alt.trim()
            .trim_start_matches('(')
            .trim_end_matches(')')
            .split(';')
            .all(|t| types.contains(&ctx.expand_key(t.trim()).as_str()))
    })
}

/// RFC 3986 percent-decoding of a query value (`q`, `scopeQ` in a
/// subscription body may arrive encoded, 4.9).
pub fn percent_decode(input: &[u8]) -> String {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        if input[i] == b'%' && i + 2 < input.len() {
            // from_str_radix accepts a leading sign, so "%+1" would decode as
            // 0x01. RFC 3986 clause 2.1 admits two hex digits and nothing else.
            let hex = std::str::from_utf8(&input[i + 1..i + 3])
                .ok()
                .filter(|h| h.bytes().all(|b| b.is_ascii_hexdigit()));
            if let Some(b) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(input[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// One parsed 4.9 query expression.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum QNode {
    /// `a;b` — every operand must hold.
    And(Vec<QNode>),
    /// `a|b` — any operand holds.
    Or(Vec<QNode>),
    /// `path op value` — one comparison term.
    Cmp {
        /// The attribute (path) the term targets.
        path: QPath,
        /// The comparison operator.
        op: CmpOp,
        /// The literal compared against.
        value: QValue,
    },
    /// Bare attribute path = existence check (`q=temperature`).
    Exists {
        /// The attribute (path) whose presence is tested.
        path: QPath,
        /// `!path` — the attribute must be absent.
        negated: bool,
    },
}

/// 4.9 `Attribute = LinkedEntityRelation` — zero or more `attr{[T[,T]:]…}`
/// hops (EXAMPLE 13/14), then `ValuePath = DottedPath *1([DottedPath])`:
/// a dotted path plus an optional single trailing bracket that is either a
/// compound-value member path (EXAMPLE 9/10/11) or a language filter
/// (`[en]` / `[*]`, Equal/Unequal languageMap semantics).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct QPath {
    /// Linked-entity hops (`attr{…}`) preceding the path, outermost first.
    pub links: Vec<Link>,
    /// The dotted attribute path (terms, expanded at evaluation time).
    pub path: Vec<String>,
    /// The optional trailing `[…]`: a compound-value member path, or a
    /// language filter (`[en]`, `[*]`).
    pub bracket: Option<Vec<String>>,
}

/// One `attr{…}` linked-entity hop with its optional EntityType hints.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Link {
    /// The Relationship followed.
    pub attr: String,
    /// EntityType hints (`attr{T1,T2:…}`), empty when none.
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

    /// Deepest chain of `attr{…}` hops any referenced path uses — the number
    /// of Linked Entity levels the query needs (5.7.2.4: must not exceed
    /// joinLevel).
    pub fn max_link_depth(&self) -> usize {
        match self {
            QNode::And(ns) | QNode::Or(ns) => {
                ns.iter().map(Self::max_link_depth).max().unwrap_or(0)
            }
            QNode::Cmp { path, .. } | QNode::Exists { path, .. } => path.links.len(),
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

/// The 4.9 comparison operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum CmpOp {
    /// `==`
    Eq,
    /// `!=`
    Ne,
    /// `>`
    Gt,
    /// `>=`
    Ge,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `~=` (patternOp)
    Pattern,
    /// `!~=` (notPatternOp)
    NotPattern,
}

/// A query term literal.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum QValue {
    /// A string (quoted, or an unquoted non-numeric token such as a date).
    Str(String),
    /// A number.
    Num(f64),
    /// `true` / `false`.
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

/// AST size cap — checked after parsing, which is safe once depth and
/// length are bounded first. Public because `/q/health` publishes it: a
/// second constant carrying the same number is one that can drift from the
/// one actually enforced.
pub const MAX_Q_NODES: usize = 512;

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

/// 4.9 p.85: "`Number` shall be a number as mandated by the JSON
/// Specification, following the ABNF Grammar, production rule named `number`,
/// section 6 of IETF RFC 8259" — `[minus] int [frac] [exp]`, the int without
/// a leading zero, the fraction and the exponent with at least one digit each.
/// A float parse is much wider: `+5`, `01`, `.5`, `5.`, `NaN` and `inf` all
/// come back as numbers from it and none of them is a Number here, so each
/// would compare against a number where the term named text.
fn is_json_number(s: &str) -> bool {
    let s = s.strip_prefix('-').unwrap_or(s);
    let digits = |t: &str| t.len() - t.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    let int = digits(s);
    if int == 0 || (int > 1 && s.starts_with('0')) {
        return false;
    }
    let rest = &s[int..];
    let rest = match rest.strip_prefix('.') {
        None => rest,
        Some(frac) => match digits(frac) {
            0 => return false,
            n => &frac[n..],
        },
    };
    match rest.strip_prefix(['e', 'E']) {
        None => rest.is_empty(),
        Some(exp) => {
            let exp = exp.strip_prefix(['+', '-']).unwrap_or(exp);
            !exp.is_empty() && digits(exp) == exp.len()
        }
    }
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
        let first = self.and_expr()?;
        let mut rest = Vec::new();
        while self.eat('|') {
            rest.push(self.and_expr()?);
        }
        Ok(if rest.is_empty() {
            first
        } else {
            QNode::Or(std::iter::once(first).chain(rest).collect())
        })
    }

    fn and_expr(&mut self) -> Result<QNode, NgsiError> {
        let first = self.term()?;
        let mut rest = Vec::new();
        while self.eat(';') {
            rest.push(self.term()?);
        }
        Ok(if rest.is_empty() {
            first
        } else {
            QNode::And(std::iter::once(first).chain(rest).collect())
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
        // 4.9: `patternOp`/`notPatternOp` take a `RegExp` (IEEE 1003.2), not
        // a `quotedStr`, so the operand is the pattern text as written — a
        // backslash in it belongs to the regular expression and is not an
        // RFC 8259 escape. Every other operand is a `quotedStr`, a Number, a
        // 4.6.3 dateTime/date/time or a URI.
        let regexp = matches!(op, CmpOp::Pattern | CmpOp::NotPattern);
        let first = self.scalar(regexp)?;
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
            let hi = self.scalar(false)?;
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
                items.push(self.scalar(false)?);
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
    ///
    /// `regexp` selects the grammar the quoted form follows: a `RegExp`
    /// operand is taken as written, everything else is a `quotedStr`, i.e.
    /// "a text string as mandated by the JSON Specification, following the
    /// ABNF Grammar, production rule named String, section 7 of IETF
    /// RFC 8259" — escapes included, decoded to the text the term compares
    /// against, which is how the entity member reached the store too.
    fn scalar(&mut self, regexp: bool) -> Result<QValue, NgsiError> {
        // The clause writes its own examples with a space before the value
        // (`color!= "black", "red"`, p.90). Deciding quoted-vs-unquoted on an
        // untrimmed head read that as an unquoted token and kept the quotes
        // INSIDE the value, so the term matched a value literally spelled
        // with them.
        self.rest = self.rest.trim_start();
        if self.rest.starts_with('"') {
            if regexp {
                let rest = &self.rest[1..];
                let end = rest
                    .find('"')
                    .ok_or_else(|| bad(rest, "unterminated string"))?;
                let (s, rest) = rest.split_at(end);
                self.rest = &rest[1..];
                return Ok(QValue::Str(s.to_owned()));
            }
            let end = Self::json_string_end(self.rest)
                .ok_or_else(|| bad(self.rest, "unterminated string"))?;
            let (lit, rest) = self.rest.split_at(end);
            let s: String = serde_json::from_str(lit)
                .map_err(|_| bad(lit, "value is not an RFC 8259 String (4.9 quotedStr)"))?;
            self.rest = rest;
            return Ok(QValue::Str(s));
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
        // The unquoted alternatives are `dateTime`/`date`/`time` (4.6.3), a
        // Number and a `URI` (RFC 3986). None of them admits a `\"`, so one
        // here is an unterminated `quotedStr`, not a value.
        if !regexp && raw.contains('"') {
            return Err(bad(raw, "unquoted value must not contain a quote (4.9)"));
        }
        self.rest = rest;
        match raw {
            "true" => Ok(QValue::Bool(true)),
            "false" => Ok(QValue::Bool(false)),
            _ if is_json_number(raw) => match raw.parse::<f64>() {
                Ok(n) => Ok(QValue::Num(n)),
                Err(_) => Ok(QValue::Str(raw.to_owned())),
            },
            // the other unquoted alternatives: a URI, a dateTime, a date or a
            // time, all of them compared as text
            "" => Err(bad(raw, "expected value")),
            _ => Ok(QValue::Str(raw.to_owned())),
        }
    }

    /// Byte index one past the closing quote of the RFC 8259 string at the head
    /// of `s`, or `None` when it never closes. A quote is the closing one only
    /// when it is not itself escaped, so the scan skips the byte after every
    /// backslash — both are ASCII, so a skip can never land mid-character in a
    /// way that reads as either.
    fn json_string_end(s: &str) -> Option<usize> {
        let b = s.as_bytes();
        let mut i = 1;
        while i < b.len() {
            match b[i] {
                b'\\' => i += 2,
                b'"' => return Some(i + 1),
                _ => i += 1,
            }
        }
        None
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

    /// 4.9 p.85: "`Number` shall be a number as mandated by the JSON
    /// Specification, following the ABNF Grammar, production rule named
    /// `number`, section 6 of IETF RFC 8259" — an optional minus, an int with
    /// no leading zero, a fraction of at least one digit, an exponent of at
    /// least one digit. A plain float parse is far wider than that: it takes
    /// `+5`, `01`, `.5`, `5.`, `NaN` and `inf`, none of which is a Number.
    /// The other unquoted alternatives of `ComparableValue` and
    /// `CompEqualityValue` are `dateTime`/`date`/`time` and `URI`, all of
    /// which compare as text, so a token that is not a Number is a String.
    #[test]
    fn an_unquoted_token_is_a_number_only_when_rfc_8259_says_so() {
        for (q, want) in [
            ("x==5", QValue::Num(5.0)),
            ("x==-5", QValue::Num(-5.0)),
            ("x==0", QValue::Num(0.0)),
            ("x==0.5", QValue::Num(0.5)),
            ("x==1e3", QValue::Num(1000.0)),
            ("x==1E+3", QValue::Num(1000.0)),
            ("x==-2.5e-2", QValue::Num(-0.025)),
            // every one of these a plain float parse accepts and RFC 8259
            // does not
            ("x==+5", QValue::Str("+5".into())),
            ("x==01", QValue::Str("01".into())),
            ("x==.5", QValue::Str(".5".into())),
            ("x==5.", QValue::Str("5.".into())),
            ("x==NaN", QValue::Str("NaN".into())),
            ("x==nan", QValue::Str("nan".into())),
            ("x==inf", QValue::Str("inf".into())),
            ("x==-inf", QValue::Str("-inf".into())),
            ("x==infinity", QValue::Str("infinity".into())),
            ("x==1e", QValue::Str("1e".into())),
            ("x==-", QValue::Str("-".into())),
        ] {
            let QNode::Cmp { value, .. } = parse_q(q).expect(q) else {
                panic!("{q} is a comparison");
            };
            assert_eq!(value, want, "{q}");
        }
    }

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
        // spec's own spacing (`color!= "black", "red"`, p.90) must parse to
        // the same values — the space may not end up inside them, and neither
        // may the quotes that delimit them
        assert_eq!(
            parse_q(r#"color!= "black", "red""#).expect("parse"),
            QNode::Cmp {
                path: QPath::dotted(vec!["color".into()]),
                op: CmpOp::Ne,
                value: QValue::List(vec![QValue::Str("black".into()), QValue::Str("red".into())])
            }
        );
        assert_eq!(
            parse_q(r#"color==  "black""#).expect("parse"),
            QNode::Cmp {
                path: QPath::dotted(vec!["color".into()]),
                op: CmpOp::Eq,
                value: QValue::Str("black".into())
            },
            "a single spaced value keeps neither the space nor the quotes"
        );
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

    /// 4.9: `quotedStr = String`, and `String` "shall be a text string as
    /// mandated by the JSON Specification, following the ABNF Grammar,
    /// production rule named String, section 7 of IETF RFC 8259" — whose
    /// `char` production is `unescaped / escape (…)`. So a Query Term value
    /// may carry an escaped quote, an escaped backslash, the two-character
    /// control escapes and `\uXXXX`, and the value the term compares against
    /// is the DECODED text: the entity member it is matched to was decoded by
    /// the JSON parser on the way in.
    #[test]
    fn a_quoted_string_is_an_rfc_8259_string() {
        for (q, want) in [
            (r#"a=="say \"hi\"""#, "say \"hi\""),
            (r#"a=="back\\slash""#, "back\\slash"),
            (r#"a=="line\nbreak""#, "line\nbreak"),
            (r#"a=="tab\there""#, "tab\there"),
            (r#"a=="caf\u00e9""#, "café"),
            (r#"a=="sl\/ash""#, "sl/ash"),
            (r#"a=="""#, ""),
        ] {
            let node = parse_q(q).unwrap_or_else(|e| panic!("{q}: {e:?}"));
            let QNode::Cmp { value, .. } = node else {
                panic!("{q}: expected a comparison")
            };
            assert_eq!(value, QValue::Str(want.to_owned()), "{q}");
        }
    }

    /// The escape only ends the string when it is not itself escaped: a
    /// trailing `\"` continues the literal, and `\\` before the closing
    /// quote does not.
    #[test]
    fn an_escaped_quote_does_not_end_the_string() {
        assert!(
            parse_q(r#"a=="unterminated\""#).is_err(),
            "an escaped quote leaves the string open"
        );
        let node = parse_q(r#"a=="ends with a backslash\\";b"#).expect("parses");
        let QNode::And(items) = node else {
            panic!("expected an And")
        };
        let QNode::Cmp { value, .. } = &items[0] else {
            panic!("expected a comparison")
        };
        assert_eq!(value, &QValue::Str("ends with a backslash\\".to_owned()));
    }

    /// An escape RFC 8259 does not define is not a String, so the term is
    /// not a Query Term — refused rather than silently read as two
    /// characters.
    #[test]
    fn an_undefined_escape_is_not_a_string() {
        for q in [r#"a=="bad\x""#, r#"a=="short\u12""#, r#"a=="\u12zz""#] {
            assert!(parse_q(q).is_err(), "{q} must not parse");
        }
    }

    /// The unquoted alternatives of the grammar are `dateTime`/`date`/`time`
    /// (4.6.3) and `URI` (RFC 3986), and a raw `"` belongs to none of them.
    /// Accepting one produced a value that could not be written back as a
    /// Query Term at all.
    #[test]
    fn an_unquoted_value_may_not_carry_a_bare_quote() {
        for q in [r#"a==x"y"#, r#"a==urn:x:"y"#, r#"a>2020-01-01T00:00:"0Z"#] {
            assert!(parse_q(q).is_err(), "{q} must not parse");
        }
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
        // >512 nodes → TooComplexQuery, small trees untouched.
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

    /// Regression: the parser once recursed per `(` with
    /// no depth counter. A Rust stack overflow is a guard-page ABORT, not a
    /// catchable panic — no tower layer can contain it — so ~4000 parens in a
    /// query string killed the whole broker process, and a percent-encoded
    /// copy stored in a subscription made that a restart-surviving crash loop.
    #[test]
    #[cfg_attr(miri, ignore)] // 50k parens: nine minutes under the interpreter
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

    /// The depth ceiling is exact and counts open parentheses, not the
    /// parentheses seen: sibling groups close what they open, so a query may
    /// carry any number of them.
    #[test]
    fn depth_cap_is_exact_and_counts_only_open_parens() {
        let at_cap = format!("{}a==1{}", "(".repeat(MAX_Q_DEPTH), ")".repeat(MAX_Q_DEPTH));
        assert!(parse_q(&at_cap).is_ok(), "{MAX_Q_DEPTH} nested must parse");
        let over = format!(
            "{}a==1{}",
            "(".repeat(MAX_Q_DEPTH + 1),
            ")".repeat(MAX_Q_DEPTH + 1)
        );
        assert!(matches!(parse_q(&over), Err(NgsiError::TooComplexQuery(_))));
        let siblings = (0..200)
            .map(|i| format!("(a{i}==1)"))
            .collect::<Vec<_>>()
            .join(";");
        assert!(siblings.len() < MAX_Q_BYTES);
        assert!(parse_q(&siblings).is_ok(), "siblings are not cumulative");
    }

    /// The parser is a fuzz target: on any input it returns, and the error it
    /// returns is safe to hand back — the input is echoed Debug-escaped, so a
    /// rejected `q` cannot carry a raw CR/LF into a response or a log line.
    #[test]
    fn hostile_input_is_total_and_its_error_is_escaped() {
        for hostile in [
            "",
            " ",
            "\"",
            "\"\"",
            "a==\"",
            "..",
            "a==..",
            "a==1..",
            "a==..1",
            "a.",
            "a[",
            "a[]",
            "a{",
            "a{}",
            "a{b",
            "a{,:b}",
            ";",
            "|",
            "()",
            "(((",
            ")))",
            "!",
            "!!a",
            "a==1,",
            "a==,1",
            "~=",
            "a~=",
            "a!~=",
            "a==1)b",
            "\u{202e}==1",
            "ä==1",
            "a==\"ä",
            "温度.値>=1",
            "a\u{2028}==1",
            "a=={}",
            "a==1e999",
            "a==-0",
            "a==NaN..1",
            "a{b:c}{d:e}.f[g].h==1",
            &"{".repeat(64),
            &"[".repeat(64),
            &"a{".repeat(64),
            &".".repeat(64),
            &"!".repeat(64),
            &",".repeat(64),
        ] {
            match parse_q(hostile) {
                Ok(_) => {}
                Err(e) => {
                    let msg = e.to_string();
                    assert!(
                        !msg.chars().any(char::is_control),
                        "error text must stay escaped for {hostile:?}: {msg:?}"
                    );
                }
            }
        }
    }

    /// `!` is the existence-check prefix only (4.9): pairing it with a
    /// comparison is a grammar violation, not a silently ignored negation.
    #[test]
    fn negated_comparison_is_a_grammar_violation() {
        assert!(matches!(
            parse_q("!a==1"),
            Err(NgsiError::BadRequestData(_))
        ));
        assert!(matches!(
            parse_q("!a"),
            Ok(QNode::Exists { negated: true, .. })
        ));
        // `!=` after a path is the operator, not a negation prefix
        assert!(matches!(
            parse_q("a!=1"),
            Ok(QNode::Cmp { op: CmpOp::Ne, .. })
        ));
    }
}
