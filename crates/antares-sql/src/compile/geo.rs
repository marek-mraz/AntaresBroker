//! C11/C11b — Geoquery Language (CIM 009 clause 4.10) compiled to PostGIS
//! over the extracted `entities.location` column (GIST-indexed, §8.1).
//!
//! Same one-directional contract as the other compilers: this may only
//! NARROW, and `antares_api::geo::GeoQuery::matches` stays the arbiter. Three
//! places where the two engines could disagree are handled by deliberately
//! widening rather than by hoping they agree:
//!
//! 1. **Rows without an extracted geometry.** `location` holds the DEFAULT
//!    GeoProperty and only when the entity carries exactly one instance of it
//!    (§4.5.5 multi-instance sets have no single-geometry spelling, and a
//!    GEOMETRYCOLLECTION would make `within` mean "all of them", which is
//!    stricter). Every predicate is therefore guarded by `location IS NULL OR
//!    …`, so an entity we could not extract is handed to the evaluator
//!    instead of being dropped.
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
//! §16.2: the query geometry travels as bound GeoJSON text (`ST_GeomFromGeoJSON
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

/// The default GeoProperty — the only one with an extracted column.
pub const LOCATION_IRI: &str = "https://uri.etsi.org/ngsi-ld/location";

/// `georel` as the API already parsed it.
pub enum Rel {
    /// metres; either bound may be absent
    Near {
        max: Option<f64>,
        min: Option<f64>,
    },
    Within,
    Contains,
    Intersects,
    Disjoint,
    Overlaps,
    Equals,
}

/// A geoquery as the API already validated it (4.10 params).
pub struct GeoSpec<'a> {
    pub rel: Rel,
    pub geometry: &'a str,
    pub coordinates: &'a Value,
    /// EXPANDED `geoproperty`; empty means the default (`location`).
    pub geoproperty_iri: &'a str,
}

/// Compile a geoquery over `col` (a `geometry(Geometry,4326)`).
///
/// A `geoproperty` other than the default `location` has no extracted column,
/// so it returns `None` and the evaluator does the work (the documented C11b
/// fallback). `first_bind` numbers the geo binds; numeric binds follow them.
pub fn compile_geo(spec: &GeoSpec<'_>, col: &str, first_bind: usize) -> Option<CompiledGeo> {
    let GeoSpec {
        rel,
        geometry,
        coordinates,
        geoproperty_iri,
    } = spec;
    if !geoproperty_iri.is_empty() && *geoproperty_iri != LOCATION_IRI {
        return None;
    }
    let geojson = serde_json::to_string(&serde_json::json!({
        "type": geometry, "coordinates": coordinates
    }))
    .ok()?;

    let mut geo_binds = vec![geojson];
    let mut num_binds: Vec<f64> = Vec::new();
    // placeholder for the single geometry bind
    let g = format!("ST_SetSRID(ST_GeomFromGeoJSON(${first_bind}), 4326)");

    let pred = match rel {
        Rel::Near { max, min } => {
            if *geometry != "Point" {
                return None; // see module docs, point 3
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
    // The un-extractable row always survives (module docs, point 1).
    geo_binds.shrink_to_fit();
    Some(CompiledGeo {
        sql: format!("({col} IS NULL OR ({pred}))"),
        geo_binds,
        num_binds,
    })
}

/// Extract the geometry to store in `entities.location` at write time (C11b).
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
    // must not silently become a NULL that looks like "no location"
    let t = value.get("type")?.as_str()?;
    if !matches!(
        t,
        "Point"
            | "MultiPoint"
            | "LineString"
            | "MultiLineString"
            | "Polygon"
            | "MultiPolygon"
            | "GeometryCollection"
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
                c.sql.starts_with("(location IS NULL OR "),
                "a row with no extracted geometry must reach the evaluator: {}",
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
    }
}
