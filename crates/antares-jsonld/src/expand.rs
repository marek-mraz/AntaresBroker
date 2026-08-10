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
    "expiresAt",
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

    // expiresAt (4.22 transient storage): the one client-settable temporal
    // meta member. Keep it as a bare top-level DateTime string — the shape the
    // read-boundary filter (filter::expired_at), the GC sweep, the postgres
    // `expires_at` column extraction and temporal `meta_of` all expect. Missing
    // this made 4.22 dead code on every backend.
    if let Some(v) = doc.get("expiresAt") {
        // In a merge/partial fragment an NGSI-LD Null asks for the expiry's
        // removal (5.5.12) — pass it through for merge_into to act on.
        let is_null_removal = opts.allow_null && v.as_str() == Some("urn:ngsi-ld:null");
        let s = v
            .as_str()
            .filter(|s| is_null_removal || parse_datetime(s))
            .ok_or_else(|| bad("expiresAt must be an ISO 8601 DateTime"))?;
        out.insert("expiresAt".into(), Value::String(s.to_owned()));
    }

    for (key, v) in doc {
        match key.as_str() {
            "id" | "@id" | "type" | "@type" | "@context" | "scope" | "expiresAt" | "createdAt"
            | "modifiedAt" | "deletedAt" => continue,
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
///
/// This is where the 4.2.2 Meta Model's own SHALLs are enforced: "An NGSI-LD
/// Property shall have a value, stated through hasValue" and "An NGSI-LD
/// Relationship shall have an object stated through hasObject" — a Property
/// without `value` (and each specialized property type without its own
/// value member, 5.2.5/5.2.32/5.2.35–5.2.38) or a Relationship without a
/// URI `object` is rejected as BadRequestData. "An NGSI-LD Value shall be
/// either a rdfs:Literal or a node object" — any JSON literal, array or
/// object is accepted as `value`, a bare JSON `null` is not (4.5.2, the
/// null sentinel is the string form only).
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
    if let Some(e) = obj.get("expiresAt") {
        // 4.22 transient attribute instances carry their own expiresAt.
        let s = e
            .as_str()
            .filter(|s| parse_datetime(s))
            .ok_or_else(|| bad(format!("attribute {name}: invalid expiresAt")))?;
        out.insert("expiresAt".into(), Value::String(s.to_owned()));
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

/// ISO 8601 DateTime check — 4.6.3, `YYYY-MM-DDThh:mm:ss[.ffffff]Z`.
///
/// The clause is strict in three ways this used to get wrong:
/// - "The trailing timestamp component … shall always be equal to the
///   character `Z`. Therefore, all timestamps shall be expressed in UTC" —
///   so `+HH:MM`/`-HH:MM` offsets are INVALID, not an alternative form.
/// - "All the referred components shall appear in the string; reduced
///   representations are not permitted" — a bare 19-char form has no zone.
/// - "The Seconds component may optionally contain a decimal fraction …
///   up to a maximum of six [digits]. … In requests, also a comma instead of a
///   decimal point may be used as separator for compatibility reasons."
///
/// Digit-shape alone is not enough: `2026-13-45T00:00:00Z` is all digits in
/// the right places, and letting it through let one write make every later
/// temporal query in that tenant fail on the `::timestamptz` cast.
pub fn parse_datetime(s: &str) -> bool {
    let b = s.as_bytes();
    // shortest legal form is 19 chars + the mandatory Z
    if b.len() < 20 || *b.last().expect("non-empty") != b'Z' {
        return false;
    }
    let digits = |r: std::ops::Range<usize>| b[r].iter().all(u8::is_ascii_digit);
    let shape = digits(0..4)
        && b[4] == b'-'
        && digits(5..7)
        && b[7] == b'-'
        && digits(8..10)
        && b[10] == b'T'
        && digits(11..13)
        && b[13] == b':'
        && digits(14..16)
        && b[16] == b':'
        && digits(17..19);
    if !shape {
        return false;
    }
    // between second 19 and the trailing Z: nothing, or a fraction of 1..=6
    let frac = &s[19..s.len() - 1];
    if !frac.is_empty() {
        let Some(rest) = frac.strip_prefix('.').or_else(|| frac.strip_prefix(',')) else {
            return false;
        };
        if rest.is_empty() || rest.len() > 6 || !rest.bytes().all(|c| c.is_ascii_digit()) {
            return false;
        }
    }
    // real calendar date/time, not just digits in the right slots
    let normalized = format!("{}Z", &s[..19]);
    chrono::DateTime::parse_from_rfc3339(&normalized).is_ok()
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
    fn expires_at_kept_as_meta_not_property() {
        // 4.22: a top-level expiresAt must survive as a bare DateTime string
        // (the shape the read-boundary filter / GC / expires_at column read),
        // never as a Property under its IRI.
        let doc = serde_json::json!({
            "id": "urn:ngsi-ld:T:1",
            "type": "T",
            "expiresAt": "2020-01-01T00:00:00Z",
            "foo": {"type": "Property", "value": 1}
        });
        let out = expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default())
            .expect("expand");
        assert_eq!(out["expiresAt"], "2020-01-01T00:00:00Z");
        assert!(out.get("https://uri.etsi.org/ngsi-ld/expiresAt").is_none());
        // a non-DateTime expiresAt is rejected
        let bad = serde_json::json!({"id": "urn:ngsi-ld:T:2", "type": "T", "expiresAt": "soon"});
        assert!(expand_entity(bad.as_object().unwrap(), &core(), ExpandOpts::default()).is_err());
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

    /// 4.8: "Temporal Properties in NGSI-LD shall be represented based on
    /// the DateTime data type as mandated by clause 4.6.3" and "a
    /// TemporalProperty does not allow reification" — only a valid UTC
    /// DateTime STRING is a legal observedAt.
    #[test]
    fn rejects_bad_observed_at() {
        for bad in [
            serde_json::json!("not-a-date"),
            // 4.6.3: trailing component shall be Z — offsets are invalid
            serde_json::json!("2026-08-10T12:00:00+02:00"),
            // non-reified: a Property-shaped observedAt is not a DateTime
            serde_json::json!({"type": "Property", "value": "2026-08-10T12:00:00Z"}),
        ] {
            let doc = serde_json::json!({
                "id": "urn:ngsi-ld:Building:1",
                "type": "Building",
                "a": {"type": "Property", "value": 1, "observedAt": bad}
            });
            assert!(
                expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default()).is_err(),
                "observedAt {bad} must be rejected (4.8/4.6.3)"
            );
        }
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

    /// 4.2.2 Meta Model: "An NGSI-LD Property shall have a value, stated
    /// through hasValue. An NGSI-LD Relationship shall have an object stated
    /// through hasObject." The member is REQUIRED — a typed attribute
    /// without it is rejected, per specialized type as well (5.2.32/5.2.38).
    #[test]
    fn meta_model_required_member_per_attribute_type() {
        for (ty, wrong_member) in [
            ("Property", "object"),
            ("Relationship", "value"),
            ("LanguageProperty", "value"),
            ("JsonProperty", "value"),
            ("VocabProperty", "value"),
            ("ListProperty", "value"),
            ("ListRelationship", "valueList"),
        ] {
            let doc = serde_json::json!({
                "id": "urn:ngsi-ld:A:1",
                "type": "T",
                "attr": {"type": ty, wrong_member: "x"}
            });
            assert!(
                expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default()).is_err(),
                "{ty} without its required member must be rejected (4.2.2)"
            );
        }
    }

    /// 4.2.2: "An NGSI-LD Value shall be either a rdfs:Literal or a node
    /// object" — every JSON literal, array and object is a legal value;
    /// a bare JSON null is NOT (the null sentinel is the string form,
    /// 4.5.2 / 057_03_02).
    #[test]
    fn meta_model_value_space() {
        for v in [
            serde_json::json!(17),
            serde_json::json!(1.5),
            serde_json::json!(true),
            serde_json::json!("text"),
            serde_json::json!([1, 2, 3]),
            serde_json::json!({"nested": {"deep": [1]}}),
        ] {
            let doc = serde_json::json!({
                "id": "urn:ngsi-ld:A:1", "type": "T",
                "attr": {"type": "Property", "value": v}
            });
            assert!(
                expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default()).is_ok(),
                "literal/node-object value must be accepted (4.2.2): {v}"
            );
        }
        let doc = serde_json::json!({
            "id": "urn:ngsi-ld:A:1", "type": "T",
            "attr": {"type": "Property", "value": null}
        });
        assert!(
            expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default()).is_err(),
            "bare JSON null is not an NGSI-LD Value (4.2.2/4.5.2)"
        );
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
        // 4.6.3, p.80-81. Accepted forms:
        assert!(parse_datetime("2020-09-09T16:40:00Z"));
        assert!(parse_datetime("2020-09-09T16:40:00.000Z"));
        assert!(
            parse_datetime("2020-09-09T16:40:00.123456Z"),
            "6 fraction digits"
        );
        // "In requests, also a comma instead of a decimal point may be used as
        // separator for compatibility reasons."
        assert!(
            parse_datetime("2020-09-09T16:40:00,123Z"),
            "comma separator"
        );
        assert!(
            parse_datetime("2020-02-29T00:00:00Z"),
            "2020 is a leap year"
        );

        // Rejected. NOTE: the offset case previously asserted the OPPOSITE —
        // 4.6.3 is explicit that "the trailing timestamp component … shall
        // always be equal to the character Z. Therefore, all timestamps shall
        // be expressed in UTC", so an offset is invalid, not an alternative.
        assert!(
            !parse_datetime("2020-09-09T16:40:00+02:00"),
            "offset forbidden"
        );
        assert!(
            !parse_datetime("2020-09-09T16:40:00-05:00"),
            "offset forbidden"
        );
        // "All the referred components shall appear in the string; reduced
        // representations are not permitted."
        assert!(!parse_datetime("2020-09-09T16:40:00"), "no zone");
        assert!(!parse_datetime("2020-09-09"));
        assert!(!parse_datetime("nope"));
        // fraction bounds: 1..=6 digits, and a separator is required
        assert!(!parse_datetime("2020-09-09T16:40:00.Z"), "empty fraction");
        assert!(!parse_datetime("2020-09-09T16:40:00.1234567Z"), "7 digits");
        assert!(!parse_datetime("2020-09-09T16:40:00123Z"), "no separator");
        // calendar reality — digit-shape alone let this through, and one such
        // write made every later temporal query in the tenant 500 on the
        // ::timestamptz cast
        assert!(!parse_datetime("2026-13-45T00:00:00Z"), "month 13, day 45");
        assert!(
            !parse_datetime("2021-02-29T00:00:00Z"),
            "2021 is not a leap year"
        );
        assert!(!parse_datetime("2020-09-09T25:00:00Z"), "hour 25");
    }
}

#[cfg(test)]
mod bench {
    use super::*;

    /// J1 (risk #1): the phase-0 go/no-go was ≥5k expansions/s/core.
    /// Antares hand-rolled its processor from day one (the fork-or-hand-roll
    /// decision the box anticipated) — this measures it. Run with
    /// `cargo test -p antares-jsonld --release -- --ignored bench_expansion`.
    #[test]
    #[ignore = "benchmark — run explicitly in release"]
    fn bench_expansion_rate() {
        let loader = crate::Loader::new();
        let ctx = loader.core();
        let entity: serde_json::Map<String, serde_json::Value> = serde_json::from_str(
            r#"{
            "id": "urn:ngsi-ld:Vehicle:bench-1", "type": "Vehicle",
            "speed": {"type": "Property", "value": 55.1,
                      "observedAt": "2026-08-04T12:00:00Z", "unitCode": "KMH",
                      "source": {"type": "Property", "value": "GPS"}},
            "heading": {"type": "Property", "value": 180},
            "isParked": {"type": "Relationship",
                          "object": "urn:ngsi-ld:OffStreetParking:p1",
                          "providedBy": {"type": "Relationship",
                                          "object": "urn:ngsi-ld:Person:bob"}},
            "location": {"type": "GeoProperty",
                          "value": {"type": "Point", "coordinates": [13.35, 52.51]}},
            "name": {"type": "LanguageProperty",
                      "languageMap": {"en": "car", "de": "Auto"}}
        }"#,
        )
        .expect("entity");
        let n = 20_000u32;
        let start = std::time::Instant::now();
        for _ in 0..n {
            let out = expand_entity(&entity, &ctx, ExpandOpts::default()).expect("expand");
            std::hint::black_box(out);
        }
        let secs = start.elapsed().as_secs_f64();
        let rate = f64::from(n) / secs;
        eprintln!("expansion rate: {rate:.0}/s/core ({n} iterations in {secs:.2}s)");
        assert!(
            rate >= 5_000.0,
            "expansion rate {rate:.0}/s is below the 5k/s/core phase-0 gate"
        );
    }
}
