// SPDX-License-Identifier: EUPL-1.2
//! Scope Query Language (CIM 009 clause 4.19), evaluated in memory over the
//! entity's `scope` member.

use serde_json::Value;

/// Scope Query evaluation (4.19) — `|`/`,` = OR, `(a;b)` = AND (parenthesis
/// grouping), `+` one level, trailing `#` the subtree incl. the node, `/#`
/// any non-empty scope.
pub fn scope_matches(scope_q: &str, doc: &Value) -> bool {
    // scope is an array in the entity internal form, but a bare string is
    // legal on documents stored verbatim (e.g. registrations, 5.2.9)
    let scopes: Vec<&str> = match doc.get("scope") {
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).collect(),
        Some(Value::String(s)) => vec![s.as_str()],
        _ => Vec::new(),
    };
    scope_q.split([',', '|']).any(|and_group| {
        and_group
            .trim()
            .trim_start_matches('(')
            .trim_end_matches(')')
            .split(';')
            .all(|pat| scopes.iter().any(|s| scope_pattern_matches(pat.trim(), s)))
    })
}

/// The Scope Query that selects what BOTH arguments select (4.19).
///
/// A Scope Query is a disjunction of conjunctions, and the conjunction is
/// over predicates that are independent of each other — each pattern asks
/// whether SOME Scope of the Entity matches it — so `and` distributes over
/// `or` and the intersection is a disjunction of the pairwise unions:
/// `(a1,a2)` and `(b1,b2)` select what `(a1;b1),(a1;b2),(a2;b1),(a2;b2)`
/// selects. Every term of the result is a `;`-conjunction of plain
/// `ScopeQ`s, which is what the grammar's parenthesized `OrScopeQ` derives,
/// so the answer is a Scope Query and not a broker-private structure.
///
/// `None` when either side contributes no group, or when the product would
/// exceed [`MAX_SCOPE_Q_BYTES`]: the caller then has an intersection it
/// cannot express and must refuse rather than serve the wider of the two.
pub fn intersect_scope_q(a: &str, b: &str) -> Option<String> {
    let groups = |s: &str| -> Vec<String> {
        s.split([',', '|'])
            .map(|g| {
                g.trim()
                    .trim_start_matches('(')
                    .trim_end_matches(')')
                    .trim()
            })
            .filter(|g| !g.is_empty())
            .map(str::to_owned)
            .collect()
    };
    let (ga, gb) = (groups(a), groups(b));
    if ga.is_empty() || gb.is_empty() {
        return None;
    }
    let mut out = String::new();
    for x in &ga {
        for y in &gb {
            if !out.is_empty() {
                out.push(',');
            }
            // `/#` selects every Entity that carries any Scope at all, so it
            // adds nothing to a conjunction that already names one — and it
            // is the one ScopesQ alternative the grammar does not derive
            // inside a parenthesized group.
            match (x.as_str(), y.as_str()) {
                (ANY_SCOPE, ANY_SCOPE) => out.push_str(ANY_SCOPE),
                (ANY_SCOPE, term) | (term, ANY_SCOPE) => out.push_str(term),
                (l, r) => {
                    out.push('(');
                    out.push_str(l);
                    out.push(';');
                    out.push_str(r);
                    out.push(')');
                }
            }
            if out.len() > MAX_SCOPE_Q_BYTES {
                return None;
            }
        }
    }
    Some(out)
}

/// The ScopesQ that selects every Entity carrying a non-empty Scope (4.19).
const ANY_SCOPE: &str = "/#";

/// Ceiling on a Scope Query this crate will build. The store's own compiler
/// declines a longer one and leaves it to the in-memory arbiter; an
/// intersection past it is refused instead, so no caller is handed a query
/// that is quietly wider than the one it asked for.
pub const MAX_SCOPE_Q_BYTES: usize = 4096;

/// One 4.19 ScopeQ against one Entity Scope: `/`-separated levels compared
/// in order, `+` standing for any single level and a trailing `#` for the
/// rest of the hierarchy including the node itself.
fn scope_pattern_matches(pat: &str, scope: &str) -> bool {
    if pat == ANY_SCOPE {
        return true;
    }
    let pseg: Vec<&str> = pat.split('/').filter(|s| !s.is_empty()).collect();
    let sseg: Vec<&str> = scope.split('/').filter(|s| !s.is_empty()).collect();
    let mut i = 0;
    for (pi, p) in pseg.iter().enumerate() {
        if *p == "#" {
            // multi-level wildcard: matches the rest (including nothing)
            return pi == pseg.len() - 1;
        }
        let Some(sv) = sseg.get(i) else { return false };
        if *p != "+" && p != sv {
            return false;
        }
        i += 1;
    }
    i == sseg.len()
}

#[cfg(test)]
mod clause_4_19 {
    use super::{intersect_scope_q, scope_matches};
    use serde_json::json;

    fn doc(scopes: &[&str]) -> serde_json::Value {
        json!({"id": "urn:x", "type": ["T"], "scope": scopes})
    }

    /// 4.19 EXAMPLES 1-3: direct scope, `#` subtree (including the node
    /// itself), `+` single-level wildcard, `/#` any non-empty scope.
    #[test]
    fn wildcards_and_direct_scopes() {
        assert!(scope_matches("/Madrid", &doc(&["/Madrid"])));
        assert!(!scope_matches("/Madrid", &doc(&["/Madrid/Gardens"])));
        for s in [
            "/Madrid/Gardens",
            "/Madrid/Gardens/ParqueNorte",
            "/Madrid/Gardens/ParqueNorte/Parterre1",
        ] {
            assert!(scope_matches("/Madrid/Gardens/#", &doc(&[s])), "{s}");
        }
        assert!(!scope_matches(
            "/Madrid/Gardens/#",
            &doc(&["/Madrid/Sights"])
        ));
        assert!(scope_matches(
            "/Madrid/+/ParqueNorte",
            &doc(&["/Madrid/Sights/ParqueNorte"])
        ));
        assert!(!scope_matches(
            "/Madrid/+/ParqueNorte",
            &doc(&["/Madrid/ParqueNorte"])
        ));
        assert!(scope_matches("/#", &doc(&["/Anything"])));
        assert!(
            !scope_matches("/#", &doc(&[])),
            "no scope = no match for /#"
        );
    }

    /// 4.19 EXAMPLES 4/5: conjunction needs parentheses; disjunction is `|`
    /// OR the compatibility comma.
    #[test]
    fn conjunction_and_both_or_spellings() {
        let both = doc(&["/Madrid/Districts", "/CompanyA"]);
        let only_b = doc(&["/CompanyB"]);
        let only_madrid = doc(&["/Madrid/Districts"]);
        assert!(scope_matches("(/Madrid/Districts;/CompanyA)", &both));
        assert!(
            !scope_matches("(/Madrid/Districts;/CompanyA)", &only_madrid),
            "conjunction requires ALL scopes"
        );
        for sel in [
            "(/Madrid/Districts;/CompanyA)|/CompanyB",
            "(/Madrid/Districts;/CompanyA),/CompanyB",
        ] {
            assert!(scope_matches(sel, &both), "{sel}");
            assert!(scope_matches(sel, &only_b), "{sel}");
            assert!(!scope_matches(sel, &only_madrid), "{sel}");
        }
    }

    /// The ABNF puts `andOp` inside the parenthesized `OrScopeQ` alone, so
    /// `(a;b),(c;d)` is the only spelling it derives — yet the official
    /// suite's `019_01_06 QueryWithAndScope` sends `a;b` bare and expects
    /// 200 (`testsuite-doubts.md`). Both are served, and they have to MEAN
    /// the same thing: `;` binds tighter than `,`/`|`, which is the reading
    /// the parentheses would have forced. A gateway narrowing a request by
    /// rewriting `scopeQ` may emit either form.
    #[test]
    fn a_conjunction_means_the_same_parenthesized_or_bare() {
        let docs = [
            doc(&[]),
            doc(&["/A"]),
            doc(&["/B"]),
            doc(&["/C"]),
            doc(&["/A", "/B"]),
            doc(&["/C", "/D"]),
            doc(&["/A", "/D"]),
            doc(&["/A", "/B", "/C", "/D"]),
        ];
        for d in &docs {
            assert_eq!(
                scope_matches("(/A;/B),(/C;/D)", d),
                scope_matches("/A;/B,/C;/D", d),
                "the two spellings disagree on {d}"
            );
            assert_eq!(
                scope_matches("(/A;/B)|(/C;/D)", d),
                scope_matches("/A;/B|/C;/D", d),
                "the two spellings disagree on {d}"
            );
        }
        // and the grouping is the one the parentheses state, not the other
        // one: `/A;/B,/C` is `(/A AND /B) OR /C`, never `/A AND (/B OR /C)`.
        assert!(scope_matches("/A;/B,/C", &doc(&["/C"])));
        assert!(!scope_matches("/A;/B,/C", &doc(&["/A"])));
        assert!(scope_matches("/A;/B,/C", &doc(&["/A", "/B"])));
    }

    /// The intersection has to select what BOTH select, for every Entity —
    /// a policy engine narrowing a request that brought its own Scope Query
    /// is the caller, and an intersection that is wider than either side is
    /// a disclosure.
    #[test]
    fn an_intersection_selects_exactly_what_both_select() {
        let docs = [
            doc(&[]),
            doc(&["/A"]),
            doc(&["/B"]),
            doc(&["/BB"]),
            doc(&["/BB/Traffic"]),
            doc(&["/A", "/B"]),
            doc(&["/A", "/BB/Traffic"]),
            doc(&["/BB", "/BB/Traffic"]),
            doc(&["/A", "/B", "/BB", "/BB/Traffic"]),
        ];
        for (a, b) in [
            ("/A", "/B"),
            ("/A,/B", "/B,/C"),
            ("/BB", "/BB/Traffic"),
            ("/BB/#", "/BB/Traffic"),
            ("(/A;/B)", "/BB/#"),
            ("(/A;/B),/BB", "(/BB;/BB/Traffic),/A"),
            ("/#", "/A,/B"),
            ("/#", "/#"),
            ("/A", "/A"),
        ] {
            let both = intersect_scope_q(a, b).expect("expressible");
            for d in &docs {
                assert_eq!(
                    scope_matches(&both, d),
                    scope_matches(a, d) && scope_matches(b, d),
                    "{a} INTERSECT {b} = {both} disagrees on {d}"
                );
            }
        }
    }

    /// Nothing to intersect, and an intersection too large to write down:
    /// both leave the caller to refuse rather than serve the wider side.
    #[test]
    fn an_inexpressible_intersection_is_none() {
        assert_eq!(intersect_scope_q("", "/A"), None);
        assert_eq!(intersect_scope_q("/A", "  "), None);
        let wide = (0..200)
            .map(|n| format!("/S{n}"))
            .collect::<Vec<_>>()
            .join(",");
        assert_eq!(intersect_scope_q(&wide, &wide), None, "200x200 groups");
    }
}
