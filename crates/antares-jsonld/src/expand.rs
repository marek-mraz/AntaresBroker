//! NGSI-LD expansion + structural validation (the NGSIObject-equivalent pass,
//! §4 point 2): compacted/concise input → internal expanded form.
//!
//! Internal (expanded) form:
//! - `id` — string URI
//! - `type` — ALWAYS an array of absolute IRIs
//! - `scope` — array of scope strings (when present)
//! - every attribute key is an absolute IRI, its value ALWAYS an array of
//!   normalized instance objects; instance members keep their short NGSI-LD
//!   names (`type`, `value`, `object`, `datasetId`, `observedAt`, …) and
//!   sub-attributes are IRI-keyed arrays recursively.

use crate::context::Context;
use antares_model::NgsiError;
use serde_json::{json, Map, Value};

pub const ATTR_TYPES: &[&str] = &[
    "Property",
    "Relationship",
    "GeoProperty",
    "LanguageProperty",
    "JsonProperty",
    "VocabProperty",
    "ListProperty",
    "ListRelationship",
];

/// Instance members that are NOT sub-attributes.
const RESERVED_MEMBERS: &[&str] = &[
    "type",
    "value",
    "object",
    "objectType",
    "datasetId",
    "observedAt",
    "unitCode",
    "lang",
    "languageMap",
    "vocab",
    "json",
    "valueList",
    "objectList",
    "entityList",
    "createdAt",
    "modifiedAt",
    "deletedAt",
    "instanceId",
    "previousValue",
    "previousObject",
    "previousLanguageMap",
];

const GEO_TYPES: &[&str] = &[
    "Point",
    "MultiPoint",
    "LineString",
    "MultiLineString",
    "Polygon",
    "MultiPolygon",
    "GeometryCollection",
];

/// Entity members that must be GeoProperties (core IRIs).
const GEO_ENTITY_MEMBERS: &[&str] = &[
    "https://uri.etsi.org/ngsi-ld/location",
    "https://uri.etsi.org/ngsi-ld/observationSpace",
    "https://uri.etsi.org/ngsi-ld/operationSpace",
];

#[derive(Debug, Clone, Copy, Default)]
pub struct ExpandOpts {
    /// Fragment mode: id/type not required (append/update/partial inputs).
    pub fragment: bool,
    /// Allow NGSI-LD null (`"urn:ngsi-ld:null"`) as a deletion marker
    /// (merge-patch inputs, 5.5.12).
    pub allow_null: bool,
    /// Temporal representation: repeated instances of the same datasetId are
    /// legal (4.5.6), so the multi-instance uniqueness check is skipped.
    pub temporal: bool,
    /// Keep instance-level createdAt/modifiedAt: federation import needs them
    /// for 4.5.5.3 recency resolution. Provisioning paths re-stamp, so the
    /// flag stays off everywhere else.
    pub sys: bool,
}

pub fn expand_entity(
    doc: &Map<String, Value>,
    ctx: &Context,
    opts: ExpandOpts,
) -> Result<Value, NgsiError> {
    let bad = |m: &str| NgsiError::BadRequestData(m.to_owned());
    let mut out = Map::new();

    // id
    match doc.get("id").or_else(|| doc.get("@id")) {
        Some(Value::String(id)) => {
            antares_model::EntityId::new(id)?;
            out.insert("id".into(), Value::String(id.clone()));
        }
        Some(_) => return Err(bad("entity id must be a string URI")),
        None if opts.fragment => {}
        None => return Err(bad("entity id is required")),
    }

    // type
    match doc.get("type").or_else(|| doc.get("@type")) {
        Some(v) => {
            let types = expand_types(v, ctx)?;
            out.insert("type".into(), Value::Array(types));
        }
        None if opts.fragment => {}
        None => return Err(bad("entity type is required")),
    }

    // scope
    if let Some(v) = doc.get("scope") {
        let scopes: Vec<Value> = match v {
            Value::String(s) => vec![Value::String(s.clone())],
            Value::Array(a) => {
                let mut items = Vec::new();
                for s in a {
                    match s {
                        Value::String(s) => items.push(Value::String(s.clone())),
                        _ => return Err(bad("scope entries must be strings")),
                    }
                }
                items
            }
            _ => return Err(bad("scope must be a string or array of strings")),
        };
        out.insert("scope".into(), Value::Array(scopes));
    }

    for (key, v) in doc {
        match key.as_str() {
            "id" | "@id" | "type" | "@type" | "@context" | "scope" | "createdAt" | "modifiedAt"
            | "deletedAt" => continue,
            _ => {}
        }
        if key.is_empty() {
            return Err(bad("empty attribute name"));
        }
        let iri = ctx.expand_key(key);
        let instances = expand_attribute(key, v, ctx, opts, 0)?;
        if GEO_ENTITY_MEMBERS.contains(&iri.as_str()) {
            for inst in &instances {
                let t = inst.get("type").and_then(Value::as_str);
                let is_deletion = opts.allow_null && inst.get("value").is_some_and(is_ngsi_null);
                if t != Some("GeoProperty") && !is_deletion {
                    return Err(bad(&format!("{key} must be a GeoProperty")));
                }
            }
        }
        if !opts.temporal {
            validate_dataset_ids(key, &instances)?;
        }
        out.insert(iri, Value::Array(instances.into_iter().collect()));
    }
    Ok(Value::Object(out))
}

pub fn expand_types(v: &Value, ctx: &Context) -> Result<Vec<Value>, NgsiError> {
    let bad = |m: &str| NgsiError::BadRequestData(m.to_owned());
    // 5.5.4/4.6.2: an Entity Type must expand to an absolute IRI; a name that
    // is a JSON-LD-keyword alias in the @context (e.g. "type" → "@type") is
    // invalid (001_02_04).
    let one = |t: &str| -> Result<Value, NgsiError> {
        let iri = ctx.expand_key(t);
        if crate::context::is_absolute_iri(&iri) {
            Ok(Value::String(iri))
        } else {
            Err(bad(&format!("entity type {t:?} does not expand to an IRI")))
        }
    };
    match v {
        Value::String(t) if !t.is_empty() => Ok(vec![one(t)?]),
        Value::Array(a) if !a.is_empty() => {
            let mut out = Vec::new();
            for t in a {
                match t {
                    Value::String(t) if !t.is_empty() => out.push(one(t)?),
                    _ => return Err(bad("entity type entries must be non-empty strings")),
                }
            }
            Ok(out)
        }
        _ => Err(bad("entity type must be a non-empty string or array")),
    }
}

/// The NGSI-LD null sentinel — ONLY the string form (a plain JSON null is
/// invalid data, 057_03_02).
pub fn is_ngsi_null(v: &Value) -> bool {
    matches!(v, Value::String(s) if s == "urn:ngsi-ld:null")
}

/// A LanguageProperty deletion carries `{"@none": "urn:ngsi-ld:null"}`.
pub fn is_ngsi_null_langmap(v: &Value) -> bool {
    is_ngsi_null(v)
        || v.as_object()
            .is_some_and(|m| m.len() == 1 && m.get("@none").is_some_and(is_ngsi_null))
}

/// Whole-instance deletion marker (merge patch, 5.5.12).
pub fn is_deletion_instance(inst: &Value) -> bool {
    is_ngsi_null(inst)
        || inst.as_object().is_some_and(|o| {
            o.get("value").is_some_and(is_ngsi_null)
                || o.get("object").is_some_and(is_ngsi_null)
                || o.get("languageMap").is_some_and(is_ngsi_null_langmap)
                || o.get("json").is_some_and(is_ngsi_null)
                || o.get("vocab").is_some_and(is_ngsi_null)
                || o.get("valueList").is_some_and(is_ngsi_null)
                || o.get("objectList").is_some_and(is_ngsi_null)
        })
}

/// Expand one attribute's value into a normalized instance list.
fn expand_attribute(
    name: &str,
    v: &Value,
    ctx: &Context,
    opts: ExpandOpts,
    depth: usize,
) -> Result<Vec<Value>, NgsiError> {
    let bad = |m: String| NgsiError::BadRequestData(m);
    if depth > 8 {
        return Err(bad(format!("attribute {name}: nesting too deep")));
    }
    match v {
        Value::Array(items) if items.iter().all(looks_like_instance) && !items.is_empty() => {
            let mut out = Vec::new();
            for item in items {
                out.push(expand_instance(name, item, ctx, opts, depth)?);
            }
            Ok(out)
        }
        _ => Ok(vec![expand_instance(name, v, ctx, opts, depth)?]),
    }
}

fn looks_like_instance(v: &Value) -> bool {
    v.as_object().is_some_and(|o| {
        o.get("type")
            .and_then(Value::as_str)
            .is_some_and(|t| ATTR_TYPES.contains(&t))
            || [
                "value",
                "object",
                "languageMap",
                "vocab",
                "json",
                "valueList",
                "objectList",
            ]
            .iter()
            .any(|k| o.contains_key(*k))
    })
}

/// Expand a single instance (normalized or concise) to normalized form.
fn expand_instance(
    name: &str,
    v: &Value,
    ctx: &Context,
    opts: ExpandOpts,
    depth: usize,
) -> Result<Value, NgsiError> {
    let bad = |m: String| NgsiError::BadRequestData(m);

    // NGSI-LD null: attribute deletion marker (merge-patch only).
    if is_ngsi_null(v) {
        if opts.allow_null {
            return Ok(json!({"type": "Property", "value": "urn:ngsi-ld:null"}));
        }
        return Err(bad(format!("attribute {name}: null is not allowed here")));
    }

    let obj = match v {
        Value::Object(o) => o,
        // concise: primitive / array value ⇒ Property
        prim => {
            return Ok(json!({"type": "Property", "value": prim.clone()}));
        }
    };

    let declared = obj.get("type").and_then(Value::as_str);
    let attr_type: &str = match declared {
        Some(t) if ATTR_TYPES.contains(&t) => t,
        Some(t) if GEO_TYPES.contains(&t) && obj.contains_key("coordinates") => {
            // concise GeoProperty: bare GeoJSON object as the value
            return Ok(json!({"type": "GeoProperty", "value": v.clone()}));
        }
        Some(t) => {
            return Err(bad(format!(
                "attribute {name}: invalid attribute type {t:?}"
            )))
        }
        None => {
            // concise object form — infer from members
            if obj.contains_key("object") {
                "Relationship"
            } else if obj.contains_key("languageMap") {
                "LanguageProperty"
            } else if obj.contains_key("vocab") {
                "VocabProperty"
            } else if obj.contains_key("json") {
                "JsonProperty"
            } else if obj.contains_key("valueList") {
                "ListProperty"
            } else if obj.contains_key("objectList") {
                "ListRelationship"
            } else if obj.contains_key("value") {
                "Property"
            } else {
                // whole object is a Property value (4.5.2.3)
                return Ok(json!({"type": "Property", "value": v.clone()}));
            }
        }
    };

    let mut out = Map::new();
    out.insert("type".into(), Value::String(attr_type.to_owned()));

    // required member per type
    match attr_type {
        "Property" => {
            let val = obj
                .get("value")
                .ok_or_else(|| bad(format!("attribute {name}: Property needs value")))?;
            if val.is_null() {
                return Err(bad(format!(
                    "attribute {name}: JSON null is not a valid value (use \"urn:ngsi-ld:null\")"
                )));
            }
            out.insert("value".into(), val.clone());
        }
        "GeoProperty" => {
            let val = obj
                .get("value")
                .ok_or_else(|| bad(format!("attribute {name}: GeoProperty needs value")))?;
            if !(opts.allow_null && is_ngsi_null(val)) {
                validate_geojson(name, val)?;
            }
            out.insert("value".into(), val.clone());
        }
        "Relationship" => {
            let objv = obj
                .get("object")
                .ok_or_else(|| bad(format!("attribute {name}: Relationship needs object")))?;
            match objv {
                Value::String(s) => {
                    if !(opts.allow_null && s == "urn:ngsi-ld:null") {
                        antares_model::EntityId::new(s)
                            .map_err(|_| bad(format!("attribute {name}: object must be a URI")))?;
                    }
                }
                Value::Array(items) if !items.is_empty() => {
                    for s in items {
                        let s = s.as_str().ok_or_else(|| {
                            bad(format!("attribute {name}: object entries must be URIs"))
                        })?;
                        antares_model::EntityId::new(s)
                            .map_err(|_| bad(format!("attribute {name}: object must be a URI")))?;
                    }
                }
                _ => return Err(bad(format!("attribute {name}: invalid object"))),
            }
            out.insert("object".into(), objv.clone());
        }
        "LanguageProperty" => {
            let lm = obj
                .get("languageMap")
                .ok_or_else(|| bad(format!("attribute {name}: needs languageMap")))?;
            let ok = lm.as_object().is_some_and(|m| {
                m.values().all(|v| {
                    v.is_string() || v.as_array().is_some_and(|a| a.iter().all(Value::is_string))
                })
            }) || (opts.allow_null && is_ngsi_null_langmap(lm))
                || is_ngsi_null_langmap(lm);
            if !ok {
                return Err(bad(format!("attribute {name}: invalid languageMap")));
            }
            out.insert("languageMap".into(), lm.clone());
        }
        "JsonProperty" => {
            let j = obj
                .get("json")
                .ok_or_else(|| bad(format!("attribute {name}: needs json")))?;
            out.insert("json".into(), j.clone());
        }
        "VocabProperty" => {
            let vv = obj
                .get("vocab")
                .ok_or_else(|| bad(format!("attribute {name}: needs vocab")))?;
            let expanded = match vv {
                Value::String(s) => Value::String(ctx.expand_key(s)),
                Value::Array(a) => Value::Array(
                    a.iter()
                        .map(|s| match s {
                            Value::String(s) => Ok(Value::String(ctx.expand_key(s))),
                            _ => Err(bad(format!("attribute {name}: invalid vocab"))),
                        })
                        .collect::<Result<_, _>>()?,
                ),
                _ => return Err(bad(format!("attribute {name}: invalid vocab"))),
            };
            out.insert("vocab".into(), expanded);
        }
        "ListProperty" => {
            let l = obj
                .get("valueList")
                .ok_or_else(|| bad(format!("attribute {name}: needs valueList")))?;
            if !l.is_array() && !(opts.allow_null && is_ngsi_null(l)) {
                return Err(bad(format!("attribute {name}: valueList must be an array")));
            }
            out.insert("valueList".into(), l.clone());
        }
        "ListRelationship" => {
            let l = obj
                .get("objectList")
                .ok_or_else(|| bad(format!("attribute {name}: needs objectList")))?;
            if !l.is_array() && !(opts.allow_null && is_ngsi_null(l)) {
                return Err(bad(format!(
                    "attribute {name}: objectList must be an array"
                )));
            }
            out.insert("objectList".into(), l.clone());
        }
        _ => unreachable!(),
    }

    // optional standard members
    if let Some(d) = obj.get("datasetId") {
        let s = d
            .as_str()
            .ok_or_else(|| bad(format!("attribute {name}: datasetId must be a URI")))?;
        if s != "@none" {
            antares_model::EntityId::new(s)
                .map_err(|_| bad(format!("attribute {name}: datasetId must be a URI")))?;
        }
        out.insert("datasetId".into(), d.clone());
    }
    if let Some(o) = obj.get("observedAt") {
        let s = o
            .as_str()
            .filter(|s| parse_datetime(s))
            .ok_or_else(|| bad(format!("attribute {name}: invalid observedAt")))?;
        out.insert("observedAt".into(), Value::String(s.to_owned()));
    }
    if opts.sys {
        for k in ["createdAt", "modifiedAt"] {
            if let Some(Value::String(s)) = obj.get(k) {
                if parse_datetime(s) {
                    out.insert(k.into(), Value::String(s.clone()));
                }
            }
        }
    }
    if let Some(u) = obj.get("unitCode") {
        if !u.is_string() {
            return Err(bad(format!("attribute {name}: unitCode must be a string")));
        }
        out.insert("unitCode".into(), u.clone());
    }
    if let Some(l) = obj.get("lang") {
        out.insert("lang".into(), l.clone());
    }
    if let Some(ot) = obj.get("objectType") {
        let expanded = match ot {
            Value::String(s) => Value::String(ctx.expand_key(s)),
            other => other.clone(),
        };
        out.insert("objectType".into(), expanded);
    }

    // sub-attributes
    for (k, sub) in obj {
        if RESERVED_MEMBERS.contains(&k.as_str()) || k == "@context" {
            continue;
        }
        if k.is_empty() {
            return Err(bad(format!("attribute {name}: empty sub-attribute name")));
        }
        let iri = ctx.expand_key(k);
        let instances = expand_attribute(k, sub, ctx, opts, depth + 1)?;
        out.insert(iri, Value::Array(instances));
    }

    Ok(Value::Object(out))
}

/// Expand a PARTIAL-UPDATE attribute fragment (5.6.4): reserved members are
/// kept, others become sub-attributes — and crucially NO attribute-type
/// inference happens (a fragment `{providedBy: …}` patches the sub-attribute,
/// it is not a concise Property value).
pub fn expand_attr_fragment(obj: &Map<String, Value>, ctx: &Context) -> Result<Value, NgsiError> {
    let bad = |m: String| NgsiError::BadRequestData(m);
    let mut out = Map::new();
    for (k, v) in obj {
        match k.as_str() {
            "@context" | "createdAt" | "modifiedAt" | "instanceId" => continue,
            "type" => {
                let t = v
                    .as_str()
                    .filter(|t| ATTR_TYPES.contains(t))
                    .ok_or_else(|| bad("invalid attribute type in fragment".into()))?;
                out.insert("type".into(), Value::String(t.to_owned()));
            }
            "observedAt" => {
                let sdt = v
                    .as_str()
                    .filter(|s| parse_datetime(s))
                    .ok_or_else(|| bad("invalid observedAt in fragment".into()))?;
                out.insert("observedAt".into(), Value::String(sdt.to_owned()));
            }
            "value" => {
                if v.is_null() {
                    return Err(bad("JSON null is not a valid value".into()));
                }
                out.insert("value".into(), v.clone());
            }
            _ if RESERVED_MEMBERS.contains(&k.as_str()) => {
                out.insert(k.clone(), v.clone());
            }
            _ => {
                let iri = ctx.expand_key(k);
                let instances = expand_attribute(
                    k,
                    v,
                    ctx,
                    ExpandOpts {
                        fragment: true,
                        allow_null: true,
                        ..Default::default()
                    },
                    1,
                )?;
                out.insert(iri, Value::Array(instances));
            }
        }
    }
    Ok(Value::Object(out))
}

fn validate_dataset_ids(name: &str, instances: &[Value]) -> Result<(), NgsiError> {
    let mut seen: Vec<&str> = Vec::new();
    let mut default_count = 0usize;
    for inst in instances {
        match inst.get("datasetId").and_then(Value::as_str) {
            Some(d) => {
                if seen.contains(&d) {
                    return Err(NgsiError::BadRequestData(format!(
                        "attribute {name}: duplicate datasetId {d}"
                    )));
                }
                seen.push(d);
            }
            None => default_count += 1,
        }
    }
    if default_count > 1 {
        return Err(NgsiError::BadRequestData(format!(
            "attribute {name}: more than one instance without datasetId"
        )));
    }
    Ok(())
}

pub fn validate_geojson(name: &str, v: &Value) -> Result<(), NgsiError> {
    let ok = v.as_object().is_some_and(|o| {
        o.get("type")
            .and_then(Value::as_str)
            .is_some_and(|t| GEO_TYPES.contains(&t))
            && (o.contains_key("coordinates") || o.contains_key("geometries"))
    });
    if ok {
        Ok(())
    } else {
        Err(NgsiError::BadRequestData(format!(
            "attribute {name}: value is not a valid GeoJSON geometry"
        )))
    }
}

/// ISO 8601 DateTime check (4.6.3) — YYYY-MM-DDTHH:MM:SS(.f)?(Z|±HH:MM).
pub fn parse_datetime(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() < 19 {
        return false;
    }
    let digits = |r: std::ops::Range<usize>| b[r].iter().all(u8::is_ascii_digit);
    digits(0..4)
        && b[4] == b'-'
        && digits(5..7)
        && b[7] == b'-'
        && digits(8..10)
        && b[10] == b'T'
        && digits(11..13)
        && b[13] == b':'
        && digits(14..16)
        && b[16] == b':'
        && digits(17..19)
        && (b.len() == 19
            || s[19..].starts_with('Z')
            || s[19..].starts_with('.')
            || s[19..].starts_with('+')
            || s[19..].starts_with('-'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::Loader;

    fn core() -> std::sync::Arc<Context> {
        Loader::new().core()
    }

    #[test]
    fn expands_simple_entity() {
        let doc = serde_json::json!({
            "id": "urn:ngsi-ld:Building:1",
            "type": "Building",
            "name": {"type": "Property", "value": "Eiffel Tower"}
        });
        let out = expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default())
            .expect("expand");
        assert_eq!(out["id"], "urn:ngsi-ld:Building:1");
        assert_eq!(
            out["type"][0],
            "https://uri.etsi.org/ngsi-ld/default-context/Building"
        );
        let name = &out["https://uri.etsi.org/ngsi-ld/default-context/name"];
        assert_eq!(name[0]["value"], "Eiffel Tower");
    }

    #[test]
    fn concise_and_multi_instance() {
        let doc = serde_json::json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": 55,
            "brand": [
                {"type": "Property", "value": "Volvo", "datasetId": "urn:ngsi-ld:d:1"},
                {"type": "Property", "value": "Ford"}
            ]
        });
        let out = expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default())
            .expect("expand");
        let speed = &out["https://uri.etsi.org/ngsi-ld/default-context/speed"];
        assert_eq!(speed[0]["type"], "Property");
        assert_eq!(speed[0]["value"], 55);
        let brand = &out["https://uri.etsi.org/ngsi-ld/default-context/brand"];
        assert_eq!(brand.as_array().unwrap().len(), 2);
    }

    #[test]
    fn rejects_missing_type() {
        let doc = serde_json::json!({"id": "urn:ngsi-ld:Building:1"});
        assert!(expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default()).is_err());
    }

    #[test]
    fn rejects_bad_observed_at() {
        let doc = serde_json::json!({
            "id": "urn:ngsi-ld:Building:1",
            "type": "Building",
            "a": {"type": "Property", "value": 1, "observedAt": "not-a-date"}
        });
        assert!(expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default()).is_err());
    }

    #[test]
    fn relationship_needs_uri_object() {
        let doc = serde_json::json!({
            "id": "urn:ngsi-ld:A:1",
            "type": "T",
            "rel": {"type": "Relationship", "object": "not a uri"}
        });
        assert!(expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default()).is_err());
    }

    #[test]
    fn location_must_be_geo() {
        let doc = serde_json::json!({
            "id": "urn:ngsi-ld:A:1",
            "type": "T",
            "location": {"type": "Property", "value": 3}
        });
        assert!(expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default()).is_err());
        let ok = serde_json::json!({
            "id": "urn:ngsi-ld:A:1",
            "type": "T",
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]}}
        });
        assert!(expand_entity(ok.as_object().unwrap(), &core(), ExpandOpts::default()).is_ok());
    }

    #[test]
    fn datetime_validation() {
        assert!(parse_datetime("2020-09-09T16:40:00.000Z"));
        assert!(parse_datetime("2020-09-09T16:40:00Z"));
        assert!(parse_datetime("2020-09-09T16:40:00+02:00"));
        assert!(!parse_datetime("nope"));
        assert!(!parse_datetime("2020-09-09"));
    }
}
