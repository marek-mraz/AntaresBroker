//! Geoquery evaluation (CIM 009 4.10) — in-memory, over GeoJSON values.
//!
//! ponytail: planar approximations except `near` (haversine); upgrade to a
//! real geometry lib if a geo TP demands exactness.

use antares_jsonld::Context;
use antares_model::NgsiError;
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
        for p in parts {
            let p = p.trim();
            if let Some(v) = p.strip_prefix("maxDistance==") {
                max = Some(v.parse::<f64>().map_err(|_| bad(format!("invalid maxDistance {v:?}")))?);
            } else if let Some(v) = p.strip_prefix("minDistance==") {
                min = Some(v.parse::<f64>().map_err(|_| bad(format!("invalid minDistance {v:?}")))?);
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
            "Point", "MultiPoint", "LineString", "MultiLineString", "Polygon", "MultiPolygon",
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
        Ok(Some(Self {
            rel,
            geometry,
            coordinates,
            geoproperty: params.get("geoproperty").cloned().unwrap_or_default(),
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

    pub fn matches_geometry(&self, geo: &Value) -> bool {
        let target = Geometry::parse(geo);
        let query = Geometry {
            gtype: self.geometry.clone(),
            coords: self.coordinates.clone(),
        };
        let (Some(t), q) = (target, query) else {
            return false;
        };
        match &self.rel {
            Georel::Near { max, min } => {
                let d = t.distance_m(&q);
                max.is_none_or(|m| d <= m) && min.is_none_or(|m| d >= m)
            }
            Georel::Equals => t.coords == q.coords && t.gtype == q.gtype,
            Georel::Within => t.within(&q),
            Georel::Contains => q.within(&t),
            Georel::Intersects | Georel::Overlaps => t.intersects(&q),
            Georel::Disjoint => !t.intersects(&q),
        }
    }
}

struct Geometry {
    gtype: String,
    coords: Value,
}

impl Geometry {
    fn parse(geo: &Value) -> Option<Self> {
        Some(Self {
            gtype: geo.get("type")?.as_str()?.to_owned(),
            coords: geo.get("coordinates")?.clone(),
        })
    }

    fn points(&self) -> Vec<(f64, f64)> {
        let mut out = Vec::new();
        collect_points(&self.coords, &mut out);
        out
    }

    /// Representative point (first).
    fn point(&self) -> Option<(f64, f64)> {
        self.points().into_iter().next()
    }

    fn distance_m(&self, other: &Geometry) -> f64 {
        match (self.point(), other.point()) {
            (Some(a), Some(b)) => haversine_m(a, b),
            _ => f64::INFINITY,
        }
    }

    fn within(&self, container: &Geometry) -> bool {
        // degenerate: Point containers only "contain" identical points
        if container.gtype == "Point" || container.gtype == "MultiPoint" {
            let cpts = container.points();
            return !self.points().is_empty()
                && self.points().iter().all(|p| cpts.contains(p));
        }
        if container.gtype != "Polygon" && container.gtype != "MultiPolygon" {
            return false;
        }
        let rings = polygon_rings(container);
        self.points()
            .iter()
            .all(|p| rings.iter().any(|ring| point_in_ring(*p, ring)))
    }

    fn intersects(&self, other: &Geometry) -> bool {
        // approximation: any point of one inside the other, or equal points
        if other.gtype == "Polygon" || other.gtype == "MultiPolygon" {
            let rings = polygon_rings(other);
            if self
                .points()
                .iter()
                .any(|p| rings.iter().any(|ring| point_in_ring(*p, ring)))
            {
                return true;
            }
        }
        if self.gtype == "Polygon" || self.gtype == "MultiPolygon" {
            let rings = polygon_rings(self);
            if other
                .points()
                .iter()
                .any(|p| rings.iter().any(|ring| point_in_ring(*p, ring)))
            {
                return true;
            }
        }
        let a = self.points();
        other.points().iter().any(|p| a.contains(p))
    }
}

fn polygon_rings(g: &Geometry) -> Vec<Vec<(f64, f64)>> {
    let mut rings = Vec::new();
    match g.gtype.as_str() {
        "Polygon" => {
            if let Some(outer) = g.coords.as_array().and_then(|a| a.first()) {
                rings.push(ring_points(outer));
            }
        }
        "MultiPolygon" => {
            for poly in g.coords.as_array().into_iter().flatten() {
                if let Some(outer) = poly.as_array().and_then(|a| a.first()) {
                    rings.push(ring_points(outer));
                }
            }
        }
        _ => {}
    }
    rings
}

fn ring_points(ring: &Value) -> Vec<(f64, f64)> {
    ring.as_array()
        .map(|pts| {
            pts.iter()
                .filter_map(|p| {
                    let a = p.as_array()?;
                    Some((a.first()?.as_f64()?, a.get(1)?.as_f64()?))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn collect_points(v: &Value, out: &mut Vec<(f64, f64)>) {
    if let Some(arr) = v.as_array() {
        if arr.len() >= 2 && arr[0].is_number() && arr[1].is_number() {
            if let (Some(x), Some(y)) = (arr[0].as_f64(), arr[1].as_f64()) {
                out.push((x, y));
            }
            return;
        }
        for item in arr {
            collect_points(item, out);
        }
    }
}

fn point_in_ring((x, y): (f64, f64), ring: &[(f64, f64)]) -> bool {
    let mut inside = false;
    let n = ring.len();
    if n < 3 {
        return false;
    }
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = ring[i];
        let (xj, yj) = ring[j];
        if ((yi > y) != (yj > y)) && (x < (xj - xi) * (y - yi) / (yj - yi) + xi) {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn haversine_m((lon1, lat1): (f64, f64), (lon2, lat2): (f64, f64)) -> f64 {
    let r = 6_371_000.0f64;
    let (p1, p2) = (lat1.to_radians(), lat2.to_radians());
    let dp = (lat2 - lat1).to_radians();
    let dl = (lon2 - lon1).to_radians();
    let a = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    2.0 * r * a.sqrt().asin()
}

#[cfg(test)]
mod tests {
    use super::*;
    use antares_jsonld::Loader;
    use serde_json::json;

    #[test]
    fn near_point() {
        let mut params = HashMap::new();
        params.insert("georel".to_owned(), "near;maxDistance==2000".to_owned());
        params.insert("geometry".to_owned(), "Point".to_owned());
        params.insert("coordinates".to_owned(), "[2.29,48.85]".to_owned());
        let g = GeoQuery::from_params(&params).unwrap().unwrap();
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
    fn within_polygon() {
        let mut params = HashMap::new();
        params.insert("georel".to_owned(), "within".to_owned());
        params.insert("geometry".to_owned(), "Polygon".to_owned());
        params.insert(
            "coordinates".to_owned(),
            "[[[0,0],[4,0],[4,4],[0,4],[0,0]]]".to_owned(),
        );
        let g = GeoQuery::from_params(&params).unwrap().unwrap();
        let ctx = Loader::new().core();
        let inside = json!({
            "https://uri.etsi.org/ngsi-ld/location": [
                {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [2, 2]}}
            ]
        });
        assert!(g.matches(&inside, &ctx));
    }

    #[test]
    fn rejects_bad_params() {
        let mut params = HashMap::new();
        params.insert("georel".to_owned(), "nearish".to_owned());
        assert!(GeoQuery::from_params(&params).is_err());
    }
}
