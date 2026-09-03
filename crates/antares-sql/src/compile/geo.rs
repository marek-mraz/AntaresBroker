// SPDX-License-Identifier: EUPL-1.2
//! Geoquery Language (CIM 009 clause 4.10) compiled to PostGIS
//! over the extracted `entities.location` column (GIST-indexed).
//!
//! Same one-directional contract as the other compilers: this may only
//! NARROW, and `antares_ql::geo::GeoQuery::matches` stays the arbiter. Three
//! places where the two engines could disagree are handled by deliberately
//! widening rather than by hoping they agree:
//!
//! 1. **Rows without an extracted geometry.** `location` holds the DEFAULT
//!    GeoProperty and only when the entity carries exactly one instance of it
//!    (clause 4.5.5 multi-instance sets have no single-geometry spelling, and a
//!    GEOMETRYCOLLECTION would make `within` mean "all of them", which is
//!    stricter). Rows that CARRY the geoproperty but defeated extraction are
//!    flagged `location_ambiguous` at write time; every predicate ORs that
//!    flag, so those rows reach the evaluator, while rows with no geoproperty
//!    at all (which can never match) are excluded in SQL. Both OR arms are
//!    index-shaped (GIST + partial index → BitmapOr).
//! 2. **`near` metric.** The evaluator measures haversine on a sphere;
//!    PostGIS `geography` measures on the WGS84 spheroid. They differ by up
//!    to ~0.5 %, which at the boundary of a radius is the difference between
//!    keeping and losing a matching row — so the compiled radius is inflated
//!    and a compiled `minDistance` floor is deflated.
//! 3. **`near` against an extended query geometry.** The evaluator measures
//!    from the query geometry's FIRST vertex; PostGIS measures from its
//!    nearest point. For `maxDistance` that only widens, but for
//!    `minDistance` it narrows — so `near` compiles only for a Point query
//!    geometry, where the two are the same point by definition.
//!
//! The query geometry travels as bound GeoJSON text (`ST_GeomFromGeoJSON
//! ($n)`), never as SQL text, and so does every distance.

use serde_json::Value;

/// A compiled geoquery: a SQL boolean expression plus its binds, numbered
/// from the offset passed to [`compile_geo`].
pub struct CompiledGeo {
    pub sql: String,
    /// GeoJSON documents, in placeholder order.
    pub geo_binds: Vec<String>,
    /// Distances in metres, in placeholder order (after the geo binds).
    pub num_binds: Vec<f64>,
}

pub use antares_store::filter::{GeoSpec, Rel, LOCATION_IRI};

/// Compile a geoquery over `col` (a `geometry(Geometry,4326)`).
///
/// A `geoproperty` other than the default `location` has no extracted column,
/// so it returns `None` and the evaluator does the work (the documented
/// fallback). `first_bind` numbers the geo binds; numeric binds follow them.
pub fn compile_geo(spec: &GeoSpec<'_>, col: &str, first_bind: usize) -> Option<CompiledGeo> {
    if !spec.geoproperty_iri.is_empty() && spec.geoproperty_iri != LOCATION_IRI {
        return None;
    }
    // The un-extractable row always survives (module docs, point 1) — via the
    // `location_ambiguous` column, not `location IS NULL`: rows WITHOUT any
    // default GeoProperty can never match and are excluded in SQL, and the OR
    // over two indexable conditions BitmapOrs (GIST + partial index) instead
    // of forcing a sequential scan.
    let mut c = predicate(spec, col, first_bind)?;
    c.sql = format!("(({}) OR location_ambiguous)", c.sql);
    Some(c)
}

/// The same predicate over a PER-INSTANCE geometry column
/// (`attr_instances.geo_value`, 5.7.4.4 S3): each row is one instance, so
/// there is no ambiguity flag — a NULL `geo_value` (a value the extractor
/// could not take) is the "reaches the evaluator" arm instead. No geoproperty
/// restriction: the caller binds the attr IRI itself.
pub fn compile_geo_instance(
    spec: &GeoSpec<'_>,
    col: &str,
    first_bind: usize,
) -> Option<CompiledGeo> {
    let mut c = predicate(spec, col, first_bind)?;
    c.sql = format!("(({}) OR {col} IS NULL)", c.sql);
    Some(c)
}

fn predicate(spec: &GeoSpec<'_>, col: &str, first_bind: usize) -> Option<CompiledGeo> {
    let GeoSpec {
        rel,
        geometry,
        coordinates,
        ..
    } = spec;
    let geojson = serde_json::to_string(&serde_json::json!({
        "type": geometry, "coordinates": coordinates
    }))
    .ok()?;

    let geo_binds = vec![geojson];
    let mut num_binds: Vec<f64> = Vec::new();
    // placeholder for the single geometry bind
    let g = format!("ST_SetSRID(ST_GeomFromGeoJSON(${first_bind}), 4326)");

    let pred = match rel {
        Rel::Near { max, min } => {
            if *geometry != "Point" {
                return None; // see module docs, point 3
            }
            // 4.10 PositiveNumber is an RFC 8259 Number, but `inf` and `NaN`
            // both parse as `f64`: inflating one gives a bound that EXCLUDES
            // every row rather than widening. Refuse instead of narrowing.
            if [max, min].into_iter().flatten().any(|d| !d.is_finite()) {
                return None;
            }
            let mut parts = Vec::new();
            // Numeric binds are numbered after ALL geo binds; there is exactly
            // one geo bind here, hence the +1 base.
            if let Some(m) = max {
                let n = first_bind + 1 + num_binds.len();
                // widen: spheroid-vs-sphere slack, plus a metre of absolute
                // slack so a 0 m radius still behaves
                num_binds.push(m * 1.005 + 1.0);
                parts.push(format!(
                    "ST_DWithin({col}::geography, {g}::geography, ${n})"
                ));
            }
            if let Some(m) = min {
                let n = first_bind + 1 + num_binds.len();
                num_binds.push((m * 0.995 - 1.0).max(0.0));
                parts.push(format!(
                    "ST_Distance({col}::geography, {g}::geography) >= ${n}"
                ));
            }
            if parts.is_empty() {
                return None; // `near` with neither bound is not a filter
            }
            parts.join(" AND ")
        }
        Rel::Within => format!("ST_Within({col}, {g})"),
        Rel::Contains => format!("ST_Contains({col}, {g})"),
        Rel::Intersects => format!("ST_Intersects({col}, {g})"),
        Rel::Disjoint => format!("ST_Disjoint({col}, {g})"),
        Rel::Overlaps => format!("ST_Overlaps({col}, {g})"),
        Rel::Equals => format!("ST_Equals({col}, {g})"),
    };
    Some(CompiledGeo {
        sql: pred,
        geo_binds,
        num_binds,
    })
}

/// Extract the geometry to store in `entities.location` at write time.
/// `Some(geojson)` only for exactly one default-GeoProperty instance carrying
/// a GeoJSON value — see module docs, point 1, for why more than one is
/// deliberately `None`.
pub fn extract_location(doc: &Value) -> Option<String> {
    let instances = doc.get(LOCATION_IRI)?;
    let inst = match instances {
        Value::Array(a) if a.len() == 1 => &a[0],
        Value::Array(_) => return None, // multi-instance: let the evaluator judge
        v => v,
    };
    let value = inst.get("value").or_else(|| inst.get("object"))?;
    // must look like a GeoJSON geometry; anything else is not indexable and
    // must not silently become a NULL that looks like "no location".
    // GeometryCollection is deliberately absent: the PostGIS relate
    // predicates refuse one, so an extracted collection turns every later
    // geoquery into a database error — and it can never match anyway, since
    // `GeoQuery::matches_geometry` reads a geometry's `coordinates` and a
    // collection has none. It is flagged ambiguous instead and judged by the
    // evaluator like any other unextractable geoproperty.
    let t = value.get("type")?.as_str()?;
    if !matches!(
        t,
        "Point" | "MultiPoint" | "LineString" | "MultiLineString" | "Polygon" | "MultiPolygon"
    ) {
        return None;
    }
    serde_json::to_string(value).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn coords() -> Value {
        json!([2.29, 48.85])
    }

    fn spec<'a>(rel: Rel, geometry: &'a str, coordinates: &'a Value, gp: &'a str) -> GeoSpec<'a> {
        GeoSpec {
            rel,
            geometry,
            coordinates,
            geoproperty_iri: gp,
        }
    }

    #[test]
    fn relations_bind_the_geometry_and_never_splice_it() {
        let c = compile_geo(&spec(Rel::Within, "Point", &coords(), ""), "location", 4)
            .expect("compiles");
        assert!(!c.sql.contains("48.85"), "sql: {}", c.sql);
        assert!(c.sql.contains("ST_GeomFromGeoJSON($4)"), "sql: {}", c.sql);
        assert!(c.sql.contains("ST_Within(location,"));
        assert_eq!(c.geo_binds.len(), 1);
        assert!(c.geo_binds[0].contains("48.85"));
    }

    #[test]
    fn unextracted_rows_always_survive() {
        for rel in [
            Rel::Within,
            Rel::Contains,
            Rel::Intersects,
            Rel::Disjoint,
            Rel::Overlaps,
            Rel::Equals,
        ] {
            let c =
                compile_geo(&spec(rel, "Point", &coords(), ""), "location", 1).expect("compiles");
            assert!(
                c.sql.ends_with(" OR location_ambiguous)"),
                "a row carrying an unextractable geoproperty must reach the evaluator: {}",
                c.sql
            );
        }
    }

    #[test]
    fn near_widens_max_and_deflates_min() {
        let c = compile_geo(
            &spec(
                Rel::Near {
                    max: Some(2000.0),
                    min: None,
                },
                "Point",
                &coords(),
                "",
            ),
            "location",
            1,
        )
        .expect("compiles");
        assert!(c.sql.contains("ST_DWithin(location::geography"));
        assert!(
            c.num_binds[0] > 2000.0,
            "radius must widen: {:?}",
            c.num_binds
        );

        let c = compile_geo(
            &spec(
                Rel::Near {
                    max: None,
                    min: Some(2000.0),
                },
                "Point",
                &coords(),
                "",
            ),
            "location",
            1,
        )
        .expect("compiles");
        assert!(c.sql.contains("ST_Distance(location::geography"));
        assert!(
            c.num_binds[0] < 2000.0,
            "floor must deflate: {:?}",
            c.num_binds
        );
        assert_eq!(
            compile_geo(
                &spec(
                    Rel::Near {
                        max: Some(0.0),
                        min: Some(0.0)
                    },
                    "Point",
                    &coords(),
                    ""
                ),
                "location",
                1
            )
            .expect("compiles")
            .num_binds[1],
            0.0,
            "a deflated floor never goes negative"
        );
    }

    /// Both `near` bounds in one predicate: the geometry keeps the offset and
    /// the two distances take the next two placeholders, in the order the
    /// caller appends them (geo binds first, then the numbers).
    #[test]
    fn both_near_bounds_take_distinct_placeholders_after_the_geometry() {
        let c = compile_geo(
            &spec(
                Rel::Near {
                    max: Some(2000.0),
                    min: Some(500.0),
                },
                "Point",
                &coords(),
                "",
            ),
            "location",
            1,
        )
        .expect("compiles");
        assert_eq!(c.geo_binds.len(), 1);
        assert_eq!(c.num_binds.len(), 2);
        assert!(c.sql.contains("ST_GeomFromGeoJSON($1)"), "{}", c.sql);
        assert!(c.sql.contains("$2)"), "maxDistance placeholder: {}", c.sql);
        assert!(
            c.sql.contains(">= $3"),
            "minDistance placeholder: {}",
            c.sql
        );
        assert!(!c.sql.contains("$4"), "overshoot: {}", c.sql);
    }

    /// `maxDistance`/`minDistance` reach this compiler as `f64`, and `inf`
    /// parses as one. A non-finite bound would compile to a comparison that
    /// EXCLUDES every row rather than widening — refuse it instead.
    #[test]
    fn a_non_finite_distance_is_left_to_the_evaluator() {
        for (max, min) in [
            (Some(f64::INFINITY), None),
            (None, Some(f64::INFINITY)),
            (Some(f64::NAN), None),
            (None, Some(f64::NAN)),
            (Some(2000.0), Some(f64::INFINITY)),
        ] {
            assert!(
                compile_geo(
                    &spec(Rel::Near { max, min }, "Point", &coords(), ""),
                    "location",
                    1
                )
                .is_none(),
                "non-finite bound must not compile: {max:?}/{min:?}"
            );
        }
    }

    /// `geometry` is a client string. It is a JSON member of the bound
    /// document, never a fragment of the statement.
    #[test]
    fn the_query_geometry_type_travels_in_the_bound_geojson() {
        let c = compile_geo(
            &spec(
                Rel::Within,
                "Polygon'); DROP TABLE entities; --",
                &coords(),
                "",
            ),
            "location",
            1,
        )
        .expect("compiles");
        for needle in ["DROP", "TABLE", "--", "'"] {
            assert!(!c.sql.contains(needle), "{needle:?} leaked: {}", c.sql);
        }
        assert_eq!(
            c.sql,
            "((ST_Within(location, ST_SetSRID(ST_GeomFromGeoJSON($1), 4326))) OR location_ambiguous)"
        );
        assert!(c.geo_binds[0].contains("DROP"), "{}", c.geo_binds[0]);
    }

    #[test]
    fn refusals_leave_it_to_the_evaluator() {
        // near from an extended geometry: evaluator measures the first vertex,
        // PostGIS the nearest point — narrowing risk on minDistance
        assert!(compile_geo(
            &spec(
                Rel::Near {
                    max: Some(10.0),
                    min: None
                },
                "Polygon",
                &json!([[[0, 0], [1, 0], [1, 1], [0, 0]]]),
                ""
            ),
            "location",
            1
        )
        .is_none());
        // a non-default geoproperty has no extracted column
        assert!(compile_geo(
            &spec(
                Rel::Within,
                "Point",
                &coords(),
                "https://example.org/observationSpace"
            ),
            "location",
            1
        )
        .is_none());
        // `near` with no bound is not a filter
        assert!(compile_geo(
            &spec(
                Rel::Near {
                    max: None,
                    min: None
                },
                "Point",
                &coords(),
                ""
            ),
            "location",
            1
        )
        .is_none());
    }

    #[test]
    fn instance_variant_falls_back_to_null_and_takes_any_geoproperty() {
        let c = compile_geo_instance(
            &spec(Rel::Within, "Point", &coords(), ""),
            "gi.geo_value",
            2,
        )
        .expect("compiles");
        assert!(
            c.sql.ends_with(" OR gi.geo_value IS NULL)"),
            "an instance with an unextracted geometry must reach the evaluator: {}",
            c.sql
        );
        assert!(c.sql.contains("ST_Within(gi.geo_value,"), "{}", c.sql);
        assert!(!c.sql.contains("location_ambiguous"), "{}", c.sql);
        // the predicate core still refuses near-from-extended-geometry
        assert!(compile_geo_instance(
            &spec(
                Rel::Near {
                    max: Some(1.0),
                    min: None
                },
                "Polygon",
                &json!([[[0, 0], [1, 0], [1, 1], [0, 0]]]),
                ""
            ),
            "gi.geo_value",
            1
        )
        .is_none());
    }

    #[test]
    fn location_extraction_takes_the_single_instance_only() {
        let one = json!({
            LOCATION_IRI: [{"type": "GeoProperty",
                            "value": {"type": "Point", "coordinates": [1, 2]}}]
        });
        assert_eq!(
            extract_location(&one).expect("extracted"),
            "{\"coordinates\":[1,2],\"type\":\"Point\"}"
        );

        let multi = json!({
            LOCATION_IRI: [
                {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1, 2]}},
                {"type": "GeoProperty", "datasetId": "urn:d", "value": {"type": "Point", "coordinates": [3, 4]}}
            ]
        });
        assert!(
            extract_location(&multi).is_none(),
            "multi-instance stays NULL so the guard hands the row to the evaluator"
        );

        // a non-geometry value must not be extracted as if it were one
        let bogus = json!({ LOCATION_IRI: [{"type": "Property", "value": "somewhere"}] });
        assert!(extract_location(&bogus).is_none());
        assert!(extract_location(&json!({"id": "urn:x"})).is_none());

        // a collection is a GeoJSON geometry, but the relate predicates
        // refuse one — it stays unextracted so the row is flagged ambiguous
        let collection = json!({
            LOCATION_IRI: [{"type": "GeoProperty", "value": {
                "type": "GeometryCollection",
                "geometries": [{"type": "Point", "coordinates": [1, 2]}]
            }}]
        });
        assert!(
            extract_location(&collection).is_none(),
            "a GeometryCollection must not reach a PostGIS relate predicate"
        );
    }
}
