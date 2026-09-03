// SPDX-License-Identifier: EUPL-1.2
//! The in-memory `q=` evaluator lives in `antares_ql::eval` (shared with
//! gateways); this module keeps the crate-internal paths in place.
pub use antares_ql::eval::*;

/// 4.9 names two lists of Attributes: `expandValues`, whose values "should
/// be expanded against the supplied @context using JSON-LD type coercion
/// prior to executing the query", and `jsonKeys`, whose values "are to be
/// considered uninterpretable as JSON-LD and should not be expanded" the
/// same way. The clause states no precedence for a name in both, so the
/// broker settles it: `jsonKeys` says what the value IS, `expandValues` only
/// asks for a comparison, and coercing a value the client has declared
/// unreadable builds a term the stored value can never carry. A name in
/// both lists is therefore left out of the expansion.
///
/// Returns the list [`apply_expand_values`] should read, or `None` when
/// nothing is left to expand.
pub fn expansion_list(expand_values: Option<&str>, json_keys: Option<&str>) -> Option<String> {
    let names = expand_values?;
    let Some(raw) = json_keys else {
        return Some(names.to_owned());
    };
    let raw: Vec<&str> = raw.split(',').map(str::trim).collect();
    let kept: Vec<&str> = names
        .split(',')
        .map(str::trim)
        .filter(|n| !raw.contains(n))
        .collect();
    (!kept.is_empty()).then(|| kept.join(","))
}
