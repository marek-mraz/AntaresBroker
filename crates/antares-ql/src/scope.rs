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

/// One 4.19 ScopeQ against one Entity Scope: `/`-separated levels compared
/// in order, `+` standing for any single level and a trailing `#` for the
/// rest of the hierarchy including the node itself.
fn scope_pattern_matches(pat: &str, scope: &str) -> bool {
    if pat == "/#" {
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
    use super::scope_matches;
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
}
