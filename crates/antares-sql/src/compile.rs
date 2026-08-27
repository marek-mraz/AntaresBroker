// SPDX-License-Identifier: EUPL-1.2
//! AST → (SQL fragment, binds). The structure of every statement comes from
//! this module; every value a client supplied travels as a bind.

pub mod geo;
pub mod q;
pub mod qprefilter;
pub mod scope;
pub mod temporal;

#[cfg(test)]
mod tests {
    use super::{geo, q, scope};
    use serde_json::json;

    /// One statement carries several fragments, each numbered from the count
    /// of binds already collected. Two fragments sharing a `$n` would silently
    /// compare a jsonpath against a scope regex, so assert the whole statement
    /// references every placeholder exactly once, densely, from 1.
    #[test]
    fn fragments_combined_at_an_offset_never_share_a_placeholder() {
        // $1 is the tenant, as the entity query lays it out
        let mut binds = 1usize;
        let mut sql = vec!["tenant_id = $1".to_owned()];

        let node = antares_ql::parse_q("a==1|b>2").expect("parse");
        let c = q::compile_q(&node, "entity", binds + 1, &|t| t.to_owned()).expect("q compiles");
        binds += c.binds.len();
        sql.push(c.sql);

        let c = scope::compile_scope_q("/A;/B,/C", "scopes", binds + 1).expect("scope compiles");
        binds += c.binds.len();
        sql.push(c.sql);

        let coords = json!([2.29, 48.85]);
        let c = geo::compile_geo(
            &geo::GeoSpec {
                rel: geo::Rel::Near {
                    max: Some(2000.0),
                    min: Some(500.0),
                },
                geometry: "Point",
                coordinates: &coords,
                geoproperty_iri: "",
            },
            "location",
            binds + 1,
        )
        .expect("geo compiles");
        binds += c.geo_binds.len() + c.num_binds.len();
        sql.push(c.sql);

        let statement = sql.join(" AND ");
        let mut seen: Vec<usize> = statement
            .split('$')
            .skip(1)
            .filter_map(|t| {
                t.chars()
                    .take_while(char::is_ascii_digit)
                    .collect::<String>()
                    .parse()
                    .ok()
            })
            .collect();
        seen.sort_unstable();
        // a fragment may reference its OWN bind twice (`near` measures from
        // the same query geometry twice); what must never happen is a second
        // fragment claiming an index the first already owns, which shows up as
        // a gap at the top of the range
        seen.dedup();
        assert_eq!(
            seen,
            (1..=binds).collect::<Vec<_>>(),
            "placeholders must be dense and unique: {statement}"
        );
    }
}
