//! Geoquery evaluation (CIM 009 4.10) — in-memory, over GeoJSON values,
//! via `geo`'s DE-9IM relate (tasks.md H7: polygon holes, edge-crossing
//! intersects, line/line, MultiPolygon, topological equals — the planar
//! approximations this file used to carry are retired).
//!
//! The query geometry is parsed ONCE at construction; targets are parsed per
//! evaluation. `near` is the metric minimum distance between the two
//! geometries (`min_distance_m`: local equirectangular projection, so
//! closest-point selection is metric and extended reference/target
//! geometries measure from their true closest points); the small
//! equirectangular residual vs PostGIS `ST_DWithin` on geography is the
//! remaining documented ceiling (the SQL path C11b is the metric authority).
//! ponytail: per-call relate without a prepared edge index — a
//! PreparedGeometry cache is the §6.5 matcher lever when 10k subscriptions
//! demand it.

use antares_jsonld::Context;
use antares_model::NgsiError;
use geo::Relate;
use serde_json::Value;
use std::collections::HashMap;

const LOCATION_IRI: &str = "https://uri.etsi.org/ngsi-ld/location";

#[derive(Debug, Clone)]
pub enum Georel {
    Near { max: Option<f64>, min: Option<f64> },
    Within,
    Contains,
    Intersects,
    Equals,
    Disjoint,
    Overlaps,
}

#[derive(Debug, Clone)]
pub struct GeoQuery {
    pub rel: Georel,
    pub geometry: String,
    pub coordinates: Value,
    pub geoproperty: String,
    /// Parsed once at construction (H7).
    query_geom: geo_types::Geometry<f64>,
}

/// 4.7.1: Polygon/MultiPolygon rings need ≥4 positions and closure — the
/// malformed shapes the suite probes with (testsuite-doubts class) are 400s.
fn validate_rings(gtype: &str, coords: &Value) -> Result<(), String> {
    let check_ring = |ring: &Value| -> Result<(), String> {
        let pts = ring.as_array().ok_or("ring must be an array")?;
        if pts.len() < 4 {
            return Err(format!("ring has {} positions (minimum 4)", pts.len()));
        }
        if pts.first() != pts.last() {
            return Err("ring is not closed (first != last position)".into());
        }
        Ok(())
    };
    match gtype {
        "Polygon" => {
            for ring in coords.as_array().into_iter().flatten() {
                check_ring(ring)?;
            }
        }
        "MultiPolygon" => {
            for poly in coords.as_array().into_iter().flatten() {
                for ring in poly.as_array().into_iter().flatten() {
                    check_ring(ring)?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// 4.23: reference geometry for distance ordering (orderFrom/orderGeometry).
pub(crate) fn parse_ref_geometry(
    gtype: &str,
    coords: &Value,
) -> Result<geo_types::Geometry<f64>, String> {
    parse_geometry(gtype, coords)
}

/// 4.23: metres from the reference geometry to a target GeoJSON value —
/// the same metric minimum distance as `near` (4.10). None when the target
/// is not a valid geometry.
pub(crate) fn order_distance_m(refg: &geo_types::Geometry<f64>, target: &Value) -> Option<f64> {
    let (t, c) = (
        target.get("type").and_then(Value::as_str)?,
        target.get("coordinates")?,
    );
    let target = parse_geometry(t, c).ok()?;
    Some(min_distance_m(refg, &target))
}

/// metres per degree of latitude (WGS-84 mean)
const DEG_M: f64 = 111_319.490_793;

/// 4.10 near / 4.23 ordering: minimum distance in metres between two
/// geometries. Both are projected into a local equirectangular plane
/// (x = lon·cos(lat₀)), so closest-point selection is metric and EXTENDED
/// reference and target geometries both measure from their true closest
/// points; intersecting/containing pairs are distance 0.
/// ponytail: equirectangular residual (<~0.5 % over sub-1000 km spans, no
/// antimeridian wrap) — the PostGIS geography path stays the metric
/// authority; geodesic segment distance if a geo TP ever demands exactness.
fn min_distance_m(a: &geo_types::Geometry<f64>, b: &geo_types::Geometry<f64>) -> f64 {
    use geo::algorithm::line_measures::{Distance, Euclidean};
    use geo::algorithm::{BoundingRect, MapCoords};
    let mid_lat =
        |g: &geo_types::Geometry<f64>| g.bounding_rect().map(|r| (r.min().y + r.max().y) / 2.0);
    let lat0 = match (mid_lat(a), mid_lat(b)) {
        (Some(x), Some(y)) => (x + y) / 2.0,
        _ => 0.0,
    };
    let k = lat0.to_radians().cos().max(1e-9);
    let proj =
        |g: &geo_types::Geometry<f64>| g.map_coords(|c| geo_types::Coord { x: c.x * k, y: c.y });
    Euclidean.distance(&proj(a), &proj(b)) * DEG_M
}

/// GeoJSON `{type, coordinates}` → geo_types, with ring validation.
fn parse_geometry(gtype: &str, coords: &Value) -> Result<geo_types::Geometry<f64>, String> {
    validate_rings(gtype, coords)?;
    let gj = serde_json::json!({"type": gtype, "coordinates": coords});
    let geom: geojson::Geometry =
        serde_json::from_value(gj).map_err(|e| format!("invalid GeoJSON geometry: {e}"))?;
    geo_types::Geometry::<f64>::try_from(geom).map_err(|e| format!("invalid geometry: {e}"))
}

impl GeoQuery {
    /// Build from query params; `None` when no georel present. Validates per
    /// 4.10 (georel present ⇒ geometry+coordinates required and well-formed).
    pub fn from_params(params: &HashMap<String, String>) -> Result<Option<Self>, NgsiError> {
        let Some(georel) = params.get("georel") else {
            if params.contains_key("geometry") || params.contains_key("coordinates") {
                return Err(NgsiError::BadRequestData(
                    "geometry/coordinates given without georel".into(),
                ));
            }
            return Ok(None);
        };
        let bad = |m: String| NgsiError::BadRequestData(m);
        let mut parts = georel.split(';');
        let base = parts.next().unwrap_or("").trim();
        let mut max = None;
        let mut min = None;
        // 4.10 PositiveNumber: RFC 8259 Number "excluding the 'minus' symbol
        // and excluding the number 0".
        let positive = |v: &str, name: &str| -> Result<f64, NgsiError> {
            let n = v
                .parse::<f64>()
                .map_err(|_| bad(format!("invalid {name} {v:?}")))?;
            // NaN parses as a valid f64 — Greater-only comparison rejects it too
            if n.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
                return Err(bad(format!("{name} must be a positive non-zero number")));
            }
            Ok(n)
        };
        for p in parts {
            let p = p.trim();
            // 4.10 nearRel = nearOp andOp distance equal PositiveNumber —
            // exactly one distance modifier is in the grammar.
            if max.is_some() || min.is_some() {
                return Err(bad("near takes a single distance modifier".into()));
            }
            if let Some(v) = p.strip_prefix("maxDistance==") {
                max = Some(positive(v, "maxDistance")?);
            } else if let Some(v) = p.strip_prefix("minDistance==") {
                min = Some(positive(v, "minDistance")?);
            } else {
                return Err(bad(format!("invalid georel modifier {p:?}")));
            }
        }
        let rel = match base {
            "near" => {
                if max.is_none() && min.is_none() {
                    return Err(bad("near requires maxDistance or minDistance".into()));
                }
                Georel::Near { max, min }
            }
            "within" => Georel::Within,
            "contains" => Georel::Contains,
            "intersects" => Georel::Intersects,
            "equals" => Georel::Equals,
            "disjoint" => Georel::Disjoint,
            "overlaps" => Georel::Overlaps,
            other => return Err(bad(format!("invalid georel {other:?}"))),
        };
        let geometry = params
            .get("geometry")
            .ok_or_else(|| bad("georel requires geometry".into()))?
            .clone();
        const GEOMETRIES: &[&str] = &[
            "Point",
            "MultiPoint",
            "LineString",
            "MultiLineString",
            "Polygon",
            "MultiPolygon",
        ];
        if !GEOMETRIES.contains(&geometry.as_str()) {
            return Err(bad(format!("invalid geometry {geometry:?}")));
        }
        let coords_raw = params
            .get("coordinates")
            .ok_or_else(|| bad("georel requires coordinates".into()))?;
        let coordinates: Value = serde_json::from_str(coords_raw)
            .map_err(|_| bad(format!("invalid coordinates {coords_raw:?}")))?;
        if !coordinates.is_array() {
            return Err(bad("coordinates must be a JSON array".into()));
        }
        let query_geom = parse_geometry(&geometry, &coordinates).map_err(bad)?;
        Ok(Some(Self {
            rel,
            geometry,
            coordinates,
            geoproperty: params.get("geoproperty").cloned().unwrap_or_default(),
            query_geom,
        }))
    }

    pub fn matches(&self, doc: &Value, ctx: &Context) -> bool {
        let iri = if self.geoproperty.is_empty() {
            LOCATION_IRI.to_owned()
        } else {
            ctx.expand_key(&self.geoproperty)
        };
        let Some(instances) = doc.get(&iri).and_then(Value::as_array) else {
            return false;
        };
        instances.iter().any(|inst| {
            inst.get("value")
                .is_some_and(|geo| self.matches_geometry(geo))
        })
    }

    /// C11b: the same query, in the shape `antares-sql` compiles to PostGIS.
    /// `geoproperty` is expanded here because the compiler only pushes down
    /// the DEFAULT GeoProperty — the one with an extracted column.
    pub fn to_sql_spec<'a>(
        &'a self,
        ctx: &antares_jsonld::Context,
    ) -> Option<antares_sql::compile::geo::GeoSpec<'a>> {
        let (spec, iri) = self.to_instance_spec(ctx);
        (iri == antares_sql::compile::geo::LOCATION_IRI).then_some(spec)
    }

    /// 5.7.4.4 S3: the prefilter shape for the temporal store — like
    /// `to_sql_spec` but for the per-instance `attr_instances.geo_value`
    /// rows, where EVERY geoproperty has extracted geometries; returns the
    /// spec plus the expanded IRI the windowed EXISTS binds as `attr_id`.
    pub fn to_instance_spec<'a>(
        &'a self,
        ctx: &antares_jsonld::Context,
    ) -> (antares_sql::compile::geo::GeoSpec<'a>, String) {
        use antares_sql::compile::geo::Rel;
        let rel = match self.rel {
            Georel::Near { max, min } => Rel::Near { max, min },
            Georel::Within => Rel::Within,
            Georel::Contains => Rel::Contains,
            Georel::Intersects => Rel::Intersects,
            Georel::Equals => Rel::Equals,
            Georel::Disjoint => Rel::Disjoint,
            Georel::Overlaps => Rel::Overlaps,
        };
        let iri = if self.geoproperty.is_empty() {
            antares_sql::compile::geo::LOCATION_IRI.to_owned()
        } else {
            ctx.expand_key(&self.geoproperty)
        };
        (
            antares_sql::compile::geo::GeoSpec {
                rel,
                geometry: &self.geometry,
                coordinates: &self.coordinates,
                geoproperty_iri: "",
            },
            iri,
        )
    }

    /// One target GeoJSON value against the query. A malformed TARGET is a
    /// non-match (queries 400 at parse; stored data must never 500 a read).
    pub fn matches_geometry(&self, geo: &Value) -> bool {
        let (Some(t), Some(c)) = (
            geo.get("type").and_then(Value::as_str),
            geo.get("coordinates"),
        ) else {
            return false;
        };
        let Ok(target) = parse_geometry(t, c) else {
            return false;
        };
        let q = &self.query_geom;
        match &self.rel {
            Georel::Near { max, min } => {
                // 4.10: metric minimum distance between the geometries;
                // maxDistance = within the (closed) buffer => d <= max;
                // minDistance = DISJOINT with the buffer — boundary contact
                // is not disjoint => strictly d > min.
                let d = min_distance_m(q, &target);
                max.is_none_or(|m| d <= m) && min.is_none_or(|m| d > m)
            }
            Georel::Equals => {
                // literal-identical geometry is equal even when the ring is
                // technically invalid (self-intersecting fixtures exist in
                // the wild — DE-9IM is undefined there); topo-equal covers
                // reordered-but-equivalent rings.
                (t == self.geometry && *c == self.coordinates) || target.relate(q).is_equal_topo()
            }
            Georel::Within => target.relate(q).is_within(),
            Georel::Contains => target.relate(q).is_contains(),
            Georel::Intersects => target.relate(q).is_intersects(),
            Georel::Disjoint => !target.relate(q).is_intersects(),
            Georel::Overlaps => target.relate(q).is_overlaps(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use antares_jsonld::Loader;
    use serde_json::json;

    fn q(rel: &str, gtype: &str, coords: &str) -> GeoQuery {
        let mut params = HashMap::new();
        params.insert("georel".to_owned(), rel.to_owned());
        params.insert("geometry".to_owned(), gtype.to_owned());
        params.insert("coordinates".to_owned(), coords.to_owned());
        GeoQuery::from_params(&params).unwrap().unwrap()
    }

    fn geoval(gtype: &str, coords: Value) -> Value {
        json!({"type": gtype, "coordinates": coords})
    }

    /// 4.10 near with an EXTENDED reference geometry: "near;maxDistance==x
    /// (in meters)" is the distance to the reference GEOMETRY — measured
    /// from its closest point, not from its first coordinate.
    #[test]
    fn clause_4_10_near_extended_reference() {
        // LineString [0,60]→[10,60]; the target sits ~1 km north of the
        // EAST end (~557 km from the first coordinate)
        let g = q("near;maxDistance==2000", "LineString", "[[0,60],[10,60]]");
        assert!(
            g.matches_geometry(&geoval("Point", json!([10.0, 60.009]))),
            "distance must be measured from the closest point of the line"
        );
        // ~111 km north of the line must NOT match
        assert!(!g.matches_geometry(&geoval("Point", json!([10.0, 61.0]))));
    }

    /// 4.10 near: closest-point selection on an extended TARGET is METRIC —
    /// at lat 85 a 1°-longitude offset (~9.7 km) is closer than a
    /// 0.6°-latitude offset (~67 km), though planar lon/lat says otherwise.
    #[test]
    fn clause_4_10_near_metric_closest_point_high_latitude() {
        let target = geoval("LineString", json!([[0.0, 85.6], [1.0, 85.0]]));
        let g = q("near;maxDistance==20000", "Point", "[0,85]");
        assert!(
            g.matches_geometry(&target),
            "metric closest point is the lon-offset end (~9.7 km)"
        );
        // ...but it is farther than 5 km — must NOT match
        let g5 = q("near;maxDistance==5000", "Point", "[0,85]");
        assert!(!g5.matches_geometry(&target));
    }

    #[test]
    fn near_point() {
        let g = q("near;maxDistance==2000", "Point", "[2.29,48.85]");
        let ctx = Loader::new().core();
        let doc = json!({
            "https://uri.etsi.org/ngsi-ld/location": [
                {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [2.30, 48.86]}}
            ]
        });
        assert!(g.matches(&doc, &ctx));
        let far = json!({
            "https://uri.etsi.org/ngsi-ld/location": [
                {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [10.0, 50.0]}}
            ]
        });
        assert!(!g.matches(&far, &ctx));
    }

    #[test]
    fn within_polygon_and_holes() {
        // H7: a polygon HOLE excludes points — the planar approximation
        // this replaces got this wrong by only reading outer rings.
        let g = q(
            "within",
            "Polygon",
            "[[[0,0],[10,0],[10,10],[0,10],[0,0]],[[4,4],[6,4],[6,6],[4,6],[4,4]]]",
        );
        assert!(g.matches_geometry(&geoval("Point", json!([2, 2]))));
        assert!(
            !g.matches_geometry(&geoval("Point", json!([5, 5]))),
            "point in the hole is NOT within"
        );
    }

    #[test]
    fn edge_crossing_intersects_and_line_line() {
        // two lines crossing mid-edge share no vertex — the old
        // shared-point approximation missed this
        let g = q("intersects", "LineString", "[[0,0],[10,10]]");
        assert!(g.matches_geometry(&geoval("LineString", json!([[0, 10], [10, 0]]))));
        assert!(!g.matches_geometry(&geoval("LineString", json!([[20, 20], [30, 30]]))));
        // polygon edge crossing without contained vertices
        let g = q("intersects", "Polygon", "[[[0,0],[4,0],[4,4],[0,4],[0,0]]]");
        assert!(g.matches_geometry(&geoval(
            "Polygon",
            json!([[[-1, 1], [5, 1], [5, 3], [-1, 3], [-1, 1]]])
        )));
    }

    #[test]
    fn multipolygon_and_topological_equals() {
        let g = q(
            "within",
            "MultiPolygon",
            "[[[[0,0],[4,0],[4,4],[0,4],[0,0]]],[[[10,10],[14,10],[14,14],[10,14],[10,10]]]]",
        );
        assert!(g.matches_geometry(&geoval("Point", json!([12, 12]))));
        assert!(!g.matches_geometry(&geoval("Point", json!([7, 7]))));
        // equals is topological, not literal-coordinate order
        let g = q("equals", "Polygon", "[[[0,0],[4,0],[4,4],[0,4],[0,0]]]");
        assert!(g.matches_geometry(&geoval(
            "Polygon",
            json!([[[4, 0], [4, 4], [0, 4], [0, 0], [4, 0]]])
        )));
    }

    #[test]
    fn malformed_rings_are_400_on_query_and_nonmatch_on_target() {
        let mut params = HashMap::new();
        params.insert("georel".to_owned(), "within".to_owned());
        params.insert("geometry".to_owned(), "Polygon".to_owned());
        params.insert("coordinates".to_owned(), "[[[0,0],[4,0],[4,4]]]".to_owned());
        assert!(
            GeoQuery::from_params(&params).is_err(),
            "3-position ring must 400"
        );
        params.insert(
            "coordinates".to_owned(),
            "[[[0,0],[4,0],[4,4],[0,4]]]".to_owned(),
        );
        assert!(
            GeoQuery::from_params(&params).is_err(),
            "unclosed ring must 400"
        );
        // stored (target) data malformed: non-match, never an error
        let g = q("within", "Polygon", "[[[0,0],[4,0],[4,4],[0,4],[0,0]]]");
        assert!(!g.matches_geometry(&geoval("Polygon", json!([[[0, 0], [4, 0]]]))));
    }

    #[test]
    fn rejects_bad_params() {
        let mut params = HashMap::new();
        params.insert("georel".to_owned(), "nearish".to_owned());
        assert!(GeoQuery::from_params(&params).is_err());
    }
}

#[cfg(test)]
mod clause_4_10_grammar {
    use super::*;
    use antares_jsonld::Loader;
    use serde_json::json;

    fn params(rel: &str) -> HashMap<String, String> {
        let mut p = HashMap::new();
        p.insert("georel".to_owned(), rel.to_owned());
        p.insert("geometry".to_owned(), "Point".to_owned());
        p.insert("coordinates".to_owned(), "[8,40]".to_owned());
        p
    }

    /// 4.10 PositiveNumber: "excluding the 'minus' symbol and excluding the
    /// number 0" — a zero or negative distance is a grammar violation, 400.
    #[test]
    fn distance_must_be_a_positive_nonzero_number() {
        for rel in [
            "near;maxDistance==0",
            "near;maxDistance==-100",
            "near;minDistance==0",
            "near;minDistance==-0.5",
        ] {
            assert!(
                GeoQuery::from_params(&params(rel)).is_err(),
                "{rel} must be rejected"
            );
        }
        // a positive number stays valid
        assert!(GeoQuery::from_params(&params("near;maxDistance==0.5")).is_ok());
    }

    /// 4.10 nearRel = nearOp andOp distance equal PositiveNumber — exactly
    /// ONE distance modifier; a second (or duplicate) one is not in the
    /// grammar.
    #[test]
    fn near_takes_exactly_one_distance_modifier() {
        assert!(GeoQuery::from_params(&params("near;maxDistance==5;minDistance==1")).is_err());
        assert!(GeoQuery::from_params(&params("near;maxDistance==5;maxDistance==7")).is_err());
    }

    /// 4.10: "Entities which do not convey the target GeoProperty of the
    /// query shall be considered as non-matching."
    #[test]
    fn missing_target_geoproperty_is_a_nonmatch() {
        let ctx = Loader::new().core();
        let g = GeoQuery::from_params(&params("near;maxDistance==2000"))
            .unwrap()
            .unwrap();
        let doc = json!({
            "https://uri.etsi.org/ngsi-ld/default-context/temperature": [
                {"type": "Property", "value": 20}
            ]
        });
        assert!(!g.matches(&doc, &ctx), "no location => non-matching");
    }
}
