// SPDX-License-Identifier: EUPL-1.2
//! Scope Query Language (CIM 009 clause 4.19) compiled to SQL over the
//! extracted `scopes text[]` column (GIN-indexed).
//!
//! Same one-directional contract as `q` (see `compile::q`): this may only
//! NARROW. `antares_api::scope_matches` stays the arbiter, so a predicate
//! that is slightly looser than the matcher is fine and one that is stricter
//! is a compliance bug. Every construct below is therefore built to be
//! loose-or-equal on purpose:
//!
//! * separators match `/+`, not `/`, because the matcher drops empty segments
//!   — `/A//B` is two segments to it and must not be excluded here;
//! * leading and trailing slashes are optional on both sides, for the same
//!   reason;
//! * `+` is one segment, `#` is "the rest, including nothing", and `#` is only
//!   a wildcard in final position (`scope_pattern_matches` returns false for a
//!   non-terminal `#`) — a pattern that puts it elsewhere refuses to compile.
//!
//! The generated regex is OURS; the only client-supplied text inside it is a
//! literal segment, which is regex-escaped and then travels as a bind.

/// A compiled `scopeQ`: a SQL boolean expression plus the regex binds it
/// references, numbered from the offset passed to [`compile_scope_q`].
pub struct CompiledScope {
    pub sql: String,
    pub binds: Vec<String>,
}

/// Longest `scopeQ` compiled. One pattern is one bind and the whole thing is
/// unbounded upstream (a POST query body carries it too), so past this ceiling
/// the statement could ask for more placeholders than the wire protocol has —
/// and a refusal only means the matcher does the work.
const MAX_SCOPE_Q_BYTES: usize = 4096;

/// Compile `scope_q` into a predicate over `col` (a `text[]`).
/// `None` = outside the exact subset; the caller filters in memory.
pub fn compile_scope_q(scope_q: &str, col: &str, first_bind: usize) -> Option<CompiledScope> {
    if scope_q.len() > MAX_SCOPE_Q_BYTES {
        return None;
    }
    let mut binds = Vec::new();
    let mut or_parts = Vec::new();
    // 4.19: orOp = `|` / `,`; a conjunction is parenthesized — the parens
    // only group and must not reach the per-segment regexes.
    for and_group in scope_q.split([',', '|']) {
        let and_group = and_group
            .trim()
            .trim_start_matches('(')
            .trim_end_matches(')');
        let mut and_parts = Vec::new();
        for pat in and_group.split(';') {
            let re = pattern_regex(pat.trim())?;
            // "some scope of this entity matches the pattern" — the SQL
            // spelling of the matcher's `scopes.iter().any(...)`. An entity
            // with no scopes matches nothing, exactly as `any()` over an
            // empty list is false.
            and_parts.push(format!(
                "EXISTS (SELECT 1 FROM unnest({col}) AS s WHERE s ~ ${})",
                first_bind + binds.len()
            ));
            binds.push(re);
        }
        if and_parts.is_empty() {
            return None;
        }
        or_parts.push(format!("({})", and_parts.join(" AND ")));
    }
    if or_parts.is_empty() {
        return None;
    }
    Some(CompiledScope {
        sql: format!("({})", or_parts.join(" OR ")),
        binds,
    })
}

/// One scope pattern → an anchored POSIX regex over a stored scope string.
fn pattern_regex(pat: &str) -> Option<String> {
    let segs: Vec<&str> = pat.split('/').filter(|s| !s.is_empty()).collect();
    // `/#` (or a bare `#`) matches any scope at all — the matcher short-
    // circuits to true before it even looks at the segments.
    if segs.is_empty() || segs == ["#"] {
        return Some("^.*$".to_owned());
    }
    let mut out = String::from("^/*");
    for (i, seg) in segs.iter().enumerate() {
        let last = i == segs.len() - 1;
        if *seg == "#" {
            if !last {
                return None; // non-terminal `#` never matches; refuse to guess
            }
            // "the rest, including nothing": the separator is part of the
            // optional group so `/A/#` still matches the scope `/A`.
            out.push_str("(/+.*)?");
            out.push_str("/*$");
            return Some(out);
        }
        if i > 0 {
            out.push_str("/+");
        }
        if *seg == "+" {
            out.push_str("[^/]+");
        } else {
            out.push_str(&escape(seg));
        }
    }
    out.push_str("/*$");
    Some(out)
}

/// POSIX-ERE escaping. Postgres `~` is ERE, so the metacharacter set is
/// fixed and small; anything outside it is passed through unchanged.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if "\\^$.[]|()*+?{}".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The matcher this compiler must never be stricter than — copied here as
    /// a REFERENCE ONLY for the shape of the cases. The authoritative parity
    /// proof runs both paths against a live database
    /// (`antares-api/tests/pg_query_parity.rs`).
    #[test]
    fn literal_and_wildcards_produce_anchored_regexes() {
        let c = compile_scope_q("/Madrid/Gardens", "scopes", 3).expect("compiles");
        assert_eq!(c.binds.len(), 1);
        assert_eq!(c.binds[0], "^/*Madrid/+Gardens/*$");
        assert!(c.sql.contains("unnest(scopes)"));
        assert!(c.sql.contains("$3"));

        assert_eq!(
            compile_scope_q("/Madrid/+/Park", "scopes", 1)
                .expect("c")
                .binds[0],
            "^/*Madrid/+[^/]+/+Park/*$"
        );
        assert_eq!(
            compile_scope_q("/Madrid/#", "scopes", 1).expect("c").binds[0],
            "^/*Madrid(/+.*)?/*$"
        );
        assert_eq!(
            compile_scope_q("/#", "scopes", 1).expect("c").binds[0],
            "^.*$"
        );
    }

    #[test]
    fn and_or_structure_matches_the_language() {
        // `,` = OR of AND-groups, `;` = AND inside one
        let c = compile_scope_q("/A;/B,/C", "scopes", 1).expect("compiles");
        assert_eq!(c.binds.len(), 3);
        assert_eq!(c.sql.matches(" AND ").count(), 1, "sql: {}", c.sql);
        assert_eq!(c.sql.matches(" OR ").count(), 1, "sql: {}", c.sql);
        // the AND-group is bracketed as one OR operand — precedence is not
        // left to the reader
        assert!(c.sql.starts_with("((EXISTS"), "sql: {}", c.sql);
        assert!(c.sql.contains("$3"), "sql: {}", c.sql);
    }

    #[test]
    fn non_terminal_multilevel_wildcard_refuses() {
        // `scope_pattern_matches` only honours `#` in final position; rather
        // than reproduce its "returns false" branch, leave it to the matcher.
        assert!(compile_scope_q("/A/#/B", "scopes", 1).is_none());
    }

    #[test]
    fn regex_metacharacters_in_a_segment_are_escaped_not_syntax() {
        let c = compile_scope_q("/a.b+c", "scopes", 1).expect("compiles");
        assert_eq!(c.binds[0], "^/*a\\.b\\+c/*$");
    }

    /// A scope level is `unicodeLetter *(unicodeNumber / unicodeLetter / "_")`
    /// (4.19 ABNF), but nothing upstream enforces that grammar — so anything a
    /// client sends must land in a bind, escaped, and never in the statement.
    #[test]
    fn client_text_never_reaches_the_statement() {
        let c = compile_scope_q("/a' OR 1=1 --", "scopes", 1).expect("compiles");
        for needle in ["OR 1=1", "--", "'"] {
            assert!(!c.sql.contains(needle), "{needle:?} leaked: {}", c.sql);
        }
        assert_eq!(
            c.sql,
            "((EXISTS (SELECT 1 FROM unnest(scopes) AS s WHERE s ~ $1)))"
        );
        assert_eq!(c.binds, vec!["^/*a' OR 1=1 --/*$"]);
        // a regex-level injection is escaped in the bind, not passed through
        let c = compile_scope_q("/(a).*", "scopes", 1).expect("compiles");
        assert_eq!(c.binds, vec!["^/*\\(a\\)\\.\\*/*$"]);
    }

    /// Every degenerate group must widen. `^.*$` matches any stored scope, so
    /// an empty pattern can only ADD rows for the matcher to reject — the one
    /// direction this compiler is allowed to be wrong in.
    #[test]
    fn degenerate_groups_widen_instead_of_narrowing() {
        for q in ["", "/", "/A,", ";/A"] {
            let c = compile_scope_q(q, "scopes", 1).unwrap_or_else(|| panic!("{q} compiles"));
            assert!(
                c.binds.iter().any(|b| b == "^.*$"),
                "{q} must widen, not narrow: {:?}",
                c.binds
            );
        }
    }

    /// `scopeQ` has no length ceiling upstream, and one pattern is one bind —
    /// a POST-body scopeQ can otherwise ask for more placeholders than the
    /// wire protocol has. Past the ceiling the matcher does the work.
    #[test]
    fn an_oversized_scope_query_is_left_to_the_matcher() {
        let huge = vec!["/A"; 40_000].join(",");
        assert!(compile_scope_q(&huge, "scopes", 1).is_none());
        let ok = vec!["/A"; 100].join(",");
        assert_eq!(
            compile_scope_q(&ok, "scopes", 1)
                .expect("compiles")
                .binds
                .len(),
            100
        );
    }

    /// Doubled slashes: the matcher drops empty segments, so `/A//B` IS a
    /// match for `/A/B`. A `/`-exact regex would drop that row.
    #[test]
    fn separators_tolerate_repeats_so_the_matcher_is_never_undercut() {
        let re = &compile_scope_q("/A/B", "scopes", 1).expect("c").binds[0];
        assert!(re.contains("/+"), "separator must be repeatable: {re}");
        assert!(re.starts_with("^/*"), "leading slash optional: {re}");
        assert!(re.ends_with("/*$"), "trailing slash optional: {re}");
    }
}

#[cfg(test)]
mod clause_4_19 {
    use super::*;

    /// 4.19 EXAMPLE 5: `(a;b)|c` — the pipe is an orOp and the parentheses
    /// only group; neither may leak into the compiled per-scope regexes
    /// (a stricter predicate than the in-memory matcher is a compliance bug).
    #[test]
    fn pipe_or_and_parenthesized_conjunction_compile() {
        let c = compile_scope_q("(/Madrid/Districts;/CompanyA)|/CompanyB", "scopes", 1)
            .expect("compiles");
        assert_eq!(c.binds.len(), 3, "two ANDed + one ORed pattern");
        assert!(c.sql.contains(" OR "), "the pipe is a disjunction");
        assert!(
            c.binds.iter().all(|b| !b.contains('(') && !b.contains('|')),
            "grouping characters must not leak into the regexes: {:?}",
            c.binds
        );
    }
}
