//! NGSI-LD expansion + structural validation (the NGSIObject-equivalent
//! pass): compacted/concise input → internal expanded form.
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

/// 4.5.1: "Terms defined in the Core Context as non-reified Properties (such
/// as datasetId, instanceId, etc.) shall not be used as Attribute names."
/// These are the core terms whose IRI local name equals the term (the reified
/// value containers like value→hasValue alias away and cannot collide).
const NON_REIFIED_TERMS: &[&str] = &[
    "datasetId",
    "instanceId",
    "observedAt",
    "unitCode",
    "lang",
    "objectType",
    "previousValue",
    "previousObject",
    "previousLanguageMap",
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
    "entity",
    "entityList",
    "entityIdSealed",
    "entityTypeSealed",
    "valueType",
    "createdAt",
    "modifiedAt",
    "deletedAt",
    "expiresAt",
    "instanceId",
    "previousValue",
    "previousObject",
    "previousLanguageMap",
    "previousJson",
    "previousVocab",
    "previousValueList",
    "previousObjectList",
];

const GEO_TYPES: &[&str] = &[
    "Point",
    "MultiPoint",
    "LineString",
    "MultiLineString",
    "Polygon",
    "MultiPolygon",
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
    /// Merge fragment (5.5.12): the one input where 5.5.4 permits
    /// "urn:ngsi-ld:null" as the value of a key inside a JSON object that is
    /// a Property's value. Implies `allow_null`.
    pub merge: bool,
    /// Temporal representation: repeated instances of the same datasetId are
    /// legal (4.5.6), so the multi-instance uniqueness check is skipped.
    pub temporal: bool,
    /// Keep instance-level createdAt/modifiedAt: federation import needs them
    /// for 4.5.5.3 recency resolution. Provisioning paths re-stamp, so the
    /// flag stays off everywhere else.
    pub sys: bool,
}

/// 5.5.4 General NGSI-LD validation: "urn:ngsi-ld:null" as a first-level
/// member value is BadRequestData — legal only in NGSI-LD Fragments used in
/// partial update and merge operations (5.5.8, 5.5.12).
pub fn reject_first_level_nulls(doc: &Map<String, Value>) -> Result<(), NgsiError> {
    for (k, v) in doc {
        if v.as_str() == Some("urn:ngsi-ld:null") {
            return Err(NgsiError::BadRequestData(format!(
                "member {k}: \"urn:ngsi-ld:null\" is only allowed in partial \
                 update or merge fragments (5.5.4)"
            )));
        }
    }
    Ok(())
}

/// Does any key-value pair anywhere inside `v` carry the NGSI-LD Null as its
/// value? (5.5.4: banned inside a JSON object that is a Property's value.)
fn has_object_member_null(v: &Value) -> bool {
    match v {
        Value::Object(m) => m
            .values()
            .any(|x| x.as_str() == Some("urn:ngsi-ld:null") || has_object_member_null(x)),
        Value::Array(a) => a.iter().any(has_object_member_null),
        _ => false,
    }
}

pub fn expand_entity(
    doc: &Map<String, Value>,
    ctx: &Context,
    opts: ExpandOpts,
) -> Result<Value, NgsiError> {
    let bad = |m: &str| NgsiError::BadRequestData(m.to_owned());
    let mut out = Map::new();

    // 5.5.4: first-level member nulls are only legal in null-allowing
    // (partial update / merge) fragments.
    if !opts.allow_null {
        reject_first_level_nulls(doc)?;
    }

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
        // 4.18: "urn:ngsi-ld:null" shall ONLY appear for deleted scopes —
        // creatable solely on null-allowing (merge/patch) inputs.
        let valid_scope = |s: &str| -> bool {
            if s == "urn:ngsi-ld:null" {
                return opts.allow_null;
            }
            valid_scope_value(s)
        };
        let scopes: Vec<Value> = match v {
            Value::String(s) => vec![Value::String(s.clone())],
            Value::Array(a) => {
                let mut items = Vec::new();
                for s in a {
                    match s {
                        Value::String(s) => items.push(Value::String(s.clone())),
                        // 4.5.6: on temporal input the scope is the temporal
                        // representation of a Property — instance objects
                        // whose value is a scope string or array thereof.
                        Value::Object(o) if opts.temporal => {
                            let vals: Vec<&str> = match o.get("value") {
                                Some(Value::String(s)) => vec![s.as_str()],
                                Some(Value::Array(vs)) if vs.iter().all(Value::is_string) => {
                                    vs.iter().filter_map(Value::as_str).collect()
                                }
                                _ => return Err(bad("scope instance needs a string value")),
                            };
                            for sv in vals {
                                if !valid_scope(sv) {
                                    return Err(bad(&format!(
                                        "invalid scope {sv:?} (4.18 grammar)"
                                    )));
                                }
                            }
                            items.push(s.clone());
                        }
                        _ => return Err(bad("scope entries must be strings")),
                    }
                }
                items
            }
            _ => return Err(bad("scope must be a string or array of strings")),
        };
        for s in scopes.iter().filter_map(Value::as_str) {
            if !valid_scope(s) {
                return Err(bad(&format!("invalid scope {s:?} (4.18 grammar)")));
            }
        }
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

    // 4.8: with sys expansion the ENTITY-level system timestamps survive —
    // a federated import (5.7.2.4 forwards request options=sysAttrs) must
    // keep the remote system's createdAt/modifiedAt/deletedAt rather than
    // dropping them (they are re-stamped only on local writes).
    if opts.sys {
        for k in ["createdAt", "modifiedAt", "deletedAt"] {
            if let Some(Value::String(ts)) = doc.get(k) {
                if parse_datetime(ts) {
                    out.insert(k.to_owned(), Value::String(ts.clone()));
                }
            }
        }
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
        let iri = expand_attr_name(key, ctx)?;
        // 4.5.1: core non-reified terms shall not be used as Attribute names.
        if iri
            .strip_prefix(crate::context::NGSI_LD_BASE)
            .is_some_and(|t| NON_REIFIED_TERMS.contains(&t))
        {
            return Err(bad(&format!(
                "{key} is a core non-reified term and cannot be used as an Attribute name (4.5.1)"
            )));
        }
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
        // 4.6.2: Entity Type names obey the name grammar (BadRequestData).
        if !valid_name(t) {
            return Err(bad(&format!(
                "entity type {t:?} violates the 4.6.2 name grammar"
            )));
        }
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

/// 4.5.1/5.5.4: an Attribute or sub-Attribute name shall expand to an
/// absolute IRI. A user @context is merged before the Core one (4.4), so a
/// term defined as `{"@id": "id"}` stays RELATIVE and would otherwise land
/// on a reserved member (`id`, `value`, `datasetId`, `observedAt`, …) and
/// overwrite it — skipping that member's own validation. Same rule
/// expand_types applies to Entity Type names.
fn expand_attr_name(name: &str, ctx: &Context) -> Result<String, NgsiError> {
    let iri = ctx.expand_key(name);
    if crate::context::is_absolute_iri(&iri) {
        Ok(iri)
    } else {
        Err(NgsiError::BadRequestData(format!(
            "attribute name {name:?} does not expand to an absolute IRI"
        )))
    }
}

/// 4.5.5.1: a datasetId is a URI string; "datasetId": "@none" designates the
/// default Attribute instance, which never carries one — normalized to
/// absent (`Ok(None)`) so storage, matching and responses treat it as such.
/// 5.5.8/5.5.12: "A datasetId cannot be deleted by setting it to the value
/// urn:ngsi-ld:null" — rejected on every input.
fn dataset_id_member(d: &Value) -> Result<Option<Value>, NgsiError> {
    let bad = |m: &str| NgsiError::BadRequestData(m.to_owned());
    let s = d.as_str().ok_or_else(|| bad("datasetId must be a URI"))?;
    if s == "urn:ngsi-ld:null" {
        return Err(bad(
            "a datasetId cannot be set or deleted via \"urn:ngsi-ld:null\" (5.5.8)",
        ));
    }
    if s == "@none" {
        return Ok(None);
    }
    antares_model::EntityId::new(s).map_err(|_| bad("datasetId must be a URI"))?;
    Ok(Some(d.clone()))
}

/// 5.2.1: "In all other cases, implementations shall raise an error of type
/// BadRequestData if an NGSI-LD Null value is encountered"; 5.5.4 bans it as
/// the value of a key-value pair inside a Property's compound value except
/// in merge fragments. The concise forms hand the client's JSON back as the
/// Property value unchanged, so they carry the same two checks as the
/// normalized path.
fn check_value_nulls(name: &str, val: &Value, opts: ExpandOpts) -> Result<(), NgsiError> {
    let bad = |m: String| NgsiError::BadRequestData(m);
    let nullish = match val {
        Value::String(s) => s == "urn:ngsi-ld:null",
        Value::Array(a) => a.iter().any(is_ngsi_null),
        _ => false,
    };
    if !opts.allow_null && nullish {
        return Err(bad(format!(
            "attribute {name}: the NGSI-LD Null is only allowed in \
             partial update or merge inputs (5.2.1)"
        )));
    }
    if !opts.merge && has_object_member_null(val) {
        return Err(bad(format!(
            "attribute {name}: \"urn:ngsi-ld:null\" inside a compound value \
             is only allowed in merge fragments (5.5.4)"
        )));
    }
    Ok(())
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

/// 4.5.21.2/4.5.22.2: a List deletion is "an array consisting of a single
/// NGSI-LD Null" as the valueList/objectList (bare null tolerated too).
pub fn is_ngsi_null_list(v: &Value) -> bool {
    is_ngsi_null(v)
        || v.as_array()
            .is_some_and(|a| a.len() == 1 && is_ngsi_null(&a[0]))
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
                || o.get("valueList").is_some_and(is_ngsi_null_list)
                || o.get("objectList").is_some_and(is_ngsi_null_list)
        })
}

/// Expand one attribute's value into a normalized instance list.
/// 4.6.2 Supported names: `name = unicodeLetter *(unicodeLetter /
/// unicodeNumber / "_")`. A key containing ':' is a compact or absolute IRI
/// (the spec's prefix:name production) and is outside the term grammar.
// Known ceiling: colon-keys are exempt wholesale — a malformed "pre fix:x" slips
// through as an IRI; tighten to per-part validation if it ever matters.
/// 4.18 Scope grammar: [/] ScopeLevel *(/ScopeLevel), ScopeLevel =
/// unicodeLetter *(letter/digit/_) — shared by entity scopes and the 5.2.9
/// registration scope member.
pub fn valid_scope_value(s: &str) -> bool {
    let body = s.strip_prefix('/').unwrap_or(s);
    !body.is_empty()
        && body.split('/').all(|level| {
            let mut ch = level.chars();
            ch.next().is_some_and(char::is_alphabetic)
                && ch.all(|c| c.is_alphabetic() || c.is_numeric() || c == '_')
        })
}

pub(crate) fn valid_name(s: &str) -> bool {
    if s.contains(':') {
        return true;
    }
    let mut ch = s.chars();
    ch.next().is_some_and(char::is_alphabetic)
        && ch.all(|c| c.is_alphabetic() || c.is_numeric() || c == '_')
}

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
    // 4.6.2: Property/Relationship names with characters outside the name
    // grammar raise BadRequestData.
    if !valid_name(name) {
        return Err(bad(format!(
            "attribute name {name:?} violates the 4.6.2 name grammar"
        )));
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
            check_value_nulls(name, prim, opts)?;
            return Ok(json!({"type": "Property", "value": prim.clone()}));
        }
    };

    let declared = obj.get("type").and_then(Value::as_str);
    let attr_type: &str = match declared {
        Some(t) if ATTR_TYPES.contains(&t) => t,
        Some(t) if GEO_TYPES.contains(&t) && obj.contains_key("coordinates") => {
            // concise GeoProperty: bare GeoJSON object as the value
            check_value_nulls(name, v, opts)?;
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
                // 4.5.2.3: type may be omitted — "Property can be inferred by
                // the presence of the value attribute. An exception to this
                // inference rule occurs for geospatial Property Values, where
                // the GeoProperty sub-type shall be inferred instead, if the
                // Property Value resolves to a supported GeoJSON geometry."
                let v = &obj["value"];
                if v.get("type")
                    .and_then(Value::as_str)
                    .is_some_and(|t| GEO_TYPES.contains(&t))
                    && v.get("coordinates").is_some()
                {
                    "GeoProperty"
                } else {
                    "Property"
                }
            } else {
                // whole object is a Property value (4.5.2.3)
                check_value_nulls(name, v, opts)?;
                return Ok(json!({"type": "Property", "value": v.clone()}));
            }
        }
    };

    let mut out = Map::new();
    out.insert("type".into(), Value::String(attr_type.to_owned()));

    // 4.5.2.2 Prohibited (mirrored by 4.5.3.2 and the 4.5.18–4.5.24
    // subclasses): an instance "shall never include" the value-defining
    // member of a DIFFERENT attribute type, nor the output-only members
    // produced by inline Linked Entity retrieval and showChanges
    // notifications; entityIdSealed/entityTypeSealed only under ngsildproof.
    {
        const VALUE_OWNERS: &[(&str, &[&str])] = &[
            ("value", &["Property", "GeoProperty"]),
            ("object", &["Relationship"]),
            ("languageMap", &["LanguageProperty"]),
            ("json", &["JsonProperty"]),
            ("vocab", &["VocabProperty"]),
            ("valueList", &["ListProperty"]),
            ("objectList", &["ListRelationship"]),
        ];
        const OUTPUT_ONLY: &[&str] = &[
            "entity",
            "entityList",
            "previousValue",
            "previousObject",
            "previousLanguageMap",
            "previousJson",
            "previousVocab",
            "previousValueList",
            "previousObjectList",
        ];
        for (m, owners) in VALUE_OWNERS {
            if obj.contains_key(*m) && !owners.contains(&attr_type) {
                return Err(bad(format!(
                    "attribute {name}: {m} is not allowed on a {attr_type} (4.5.2.2)"
                )));
            }
        }
        if let Some(m) = OUTPUT_ONLY.iter().find(|m| obj.contains_key(**m)) {
            return Err(bad(format!(
                "attribute {name}: {m} is output-only and not allowed in input (4.5.2.2)"
            )));
        }
        if name != "ngsildproof"
            && (obj.contains_key("entityIdSealed") || obj.contains_key("entityTypeSealed"))
        {
            return Err(bad(format!(
                "attribute {name}: entityIdSealed/entityTypeSealed are only allowed \
                 under ngsildproof (4.5.2.2)"
            )));
        }
        // 4.5.2.2 / C.11 / annex B: ngsildproof's NON-REIFIED sealed
        // subproperties — entityIdSealed is a plain string term,
        // entityTypeSealed is "@type": "@vocab" (it seals the entity type,
        // so its value expands like a type name). They are reserved
        // members, so without this explicit copy they silently vanish.
        if name == "ngsildproof" {
            if let Some(v) = obj.get("entityIdSealed") {
                let s = v.as_str().ok_or_else(|| {
                    bad(format!(
                        "attribute {name}: entityIdSealed must be a string (4.5.2.2)"
                    ))
                })?;
                out.insert("entityIdSealed".into(), Value::String(s.to_owned()));
            }
            if let Some(v) = obj.get("entityTypeSealed") {
                let s = v.as_str().ok_or_else(|| {
                    bad(format!(
                        "attribute {name}: entityTypeSealed must be a string (4.5.2.2)"
                    ))
                })?;
                out.insert("entityTypeSealed".into(), Value::String(ctx.expand_key(s)));
            }
            // "The value of its \"value\" element shall be an object
            // containing the W3C Data integrity \"proof\" structure"
            if let Some(v) = obj.get("value") {
                if !v.is_object() && !(opts.allow_null && is_ngsi_null(v)) {
                    return Err(bad(format!(
                        "attribute {name}: ngsildproof value shall be an object \
                         containing the W3C proof structure (4.5.2.2)"
                    )));
                }
            }
        }
        // 4.5.3.2: "unitCode shall never be present, as Relationships are
        // unitless." 4.5.18.2/3 and 4.5.20.2/3 extend the prohibition to
        // LanguageProperty and VocabProperty ("always strings and hence
        // unitless").
        // (4.5.24.2/3 add JsonProperty — "raw JSON objects are unitless".)
        if obj.contains_key("unitCode")
            && matches!(
                attr_type,
                "Relationship"
                    | "ListRelationship"
                    | "LanguageProperty"
                    | "VocabProperty"
                    | "JsonProperty"
            )
        {
            return Err(bad(format!(
                "attribute {name}: unitCode is not allowed on a {attr_type}"
            )));
        }
    }

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
            // 4.7.2: a whole geometry may arrive as an encoded JSON string,
            // accepted "if and only if" it parses into a valid geometry —
            // normalized here to the object form so storage, geo-queries and
            // responses all see one representation.
            let val = match val {
                Value::String(s) if !(opts.allow_null && is_ngsi_null(val)) => {
                    serde_json::from_str::<Value>(s).map_err(|_| {
                        bad(format!(
                            "attribute {name}: string-encoded geometry is not valid JSON"
                        ))
                    })?
                }
                _ => val.clone(),
            };
            if !(opts.allow_null && is_ngsi_null(&val)) {
                validate_geojson(name, &val)?;
            }
            out.insert("value".into(), val);
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
            // 4.5.18.2: "a JSON object consisting of a set of non-empty
            // language tags (RFC 5646) or the language tag "@none"", each
            // mapping to a single string or array of strings.
            let lm = obj
                .get("languageMap")
                .ok_or_else(|| bad(format!("attribute {name}: needs languageMap")))?;
            // 4.6.5: {"@none": "urn:ngsi-ld:null"} is exclusively the
            // partial/merge-patch deletion encoding — outside allow_null it
            // is an NGSI-LD Null in a create and thus BadRequestData.
            let ok = if is_ngsi_null_langmap(lm) {
                opts.allow_null
            } else {
                lm.as_object().is_some_and(|m| {
                    m.keys().all(|k| !k.is_empty())
                        && m.values().all(|v| {
                            v.is_string()
                                || v.as_array().is_some_and(|a| a.iter().all(Value::is_string))
                        })
                })
            };
            if !ok {
                return Err(bad(format!("attribute {name}: invalid languageMap")));
            }
            out.insert("languageMap".into(), lm.clone());
        }
        "JsonProperty" => {
            // 4.5.24.2: json is "a raw JSON object (or array of objects)" —
            // never expanded or compacted; kept verbatim. The bare NGSI-LD
            // Null deletion form passes through under allow_null.
            let j = obj
                .get("json")
                .ok_or_else(|| bad(format!("attribute {name}: needs json")))?;
            let ok = j.is_object()
                || j.as_array().is_some_and(|a| a.iter().all(Value::is_object))
                || (opts.allow_null && is_ngsi_null(j));
            if !ok {
                return Err(bad(format!(
                    "attribute {name}: json must be an object or array of objects"
                )));
            }
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
            // 4.5.22.2/4.5.22.3: objectList is an ordered array of
            // Relationship objects — {"object": <URI>} objects (normalized)
            // or bare URI strings (concise). Internal form is bare URIs; the
            // normalized output shape is restored at compaction. The [null]
            // deletion form (single NGSI-LD Null) passes through under
            // allow_null.
            let l = obj
                .get("objectList")
                .ok_or_else(|| bad(format!("attribute {name}: needs objectList")))?;
            let normalized = match l {
                _ if opts.allow_null && is_ngsi_null_list(l) => l.clone(),
                Value::Array(items) => {
                    let mut uris = Vec::with_capacity(items.len());
                    for it in items {
                        let uri = match it {
                            Value::String(s) => s.as_str(),
                            Value::Object(o)
                                if o.len() == 1
                                    && o.get("object").is_some_and(Value::is_string) =>
                            {
                                o["object"].as_str().unwrap_or_default()
                            }
                            _ => {
                                return Err(bad(format!(
                                    "attribute {name}: objectList entries must be URIs \
                                     or {{\"object\": <URI>}} objects"
                                )))
                            }
                        };
                        antares_model::EntityId::new(uri).map_err(|_| {
                            bad(format!("attribute {name}: objectList entry is not a URI"))
                        })?;
                        uris.push(Value::String(uri.to_owned()));
                    }
                    Value::Array(uris)
                }
                _ => {
                    return Err(bad(format!(
                        "attribute {name}: objectList must be an array"
                    )))
                }
            };
            out.insert("objectList".into(), normalized);
        }
        _ => unreachable!(),
    }

    // optional standard members
    if let Some(d) = obj.get("datasetId") {
        if let Some(d) = dataset_id_member(d).map_err(|e| bad(format!("attribute {name}: {e}")))? {
            out.insert("datasetId".into(), d);
        }
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
        // 4.8/4.5.7: deletedAt marks a deletion instance in a Temporal
        // Evolution — dropping it here would strip remote tombstones of the
        // timestamp their deletedAt-window matching needs (5.7.3.4 merge).
        for k in ["createdAt", "modifiedAt", "deletedAt"] {
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
    if let Some(vt) = obj.get("valueType") {
        // 4.5.2.2: "valueType": a string value which shall be type coerced
        // into a datatype URI — the non-reified alternative to a native
        // JSON-LD @type on the Property value.
        let s = vt
            .as_str()
            .ok_or_else(|| bad(format!("attribute {name}: valueType must be a string")))?;
        // Table 5.2.32-1: on a LanguageProperty valueType "shall be equal
        // to langString" (the rdf:langString datatype) — kept literal.
        if attr_type == "LanguageProperty" {
            if s != "langString" {
                return Err(bad(format!(
                    "attribute {name}: valueType shall be \"langString\" on a LanguageProperty"
                )));
            }
            out.insert("valueType".into(), vt.clone());
        } else {
            out.insert("valueType".into(), Value::String(ctx.expand_key(s)));
        }
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
        if k == "@context" {
            // 4.5.1/5.5.7: "Attributes shall not contain any embedded
            // @context" — a nested user context could override core terms,
            // so it "should result in an error of type BadRequestData".
            return Err(bad(format!(
                "attribute {name}: embedded @context is not allowed (4.5.1/5.5.7)"
            )));
        }
        if RESERVED_MEMBERS.contains(&k.as_str()) {
            continue;
        }
        if k.is_empty() {
            return Err(bad(format!("attribute {name}: empty sub-attribute name")));
        }
        let iri = expand_attr_name(k, ctx)?;
        let instances = expand_attribute(k, sub, ctx, opts, depth + 1)?;
        out.insert(iri, Value::Array(instances));
    }

    // 5.2.1: "In all other cases, implementations shall raise an error of
    // type BadRequestData if an NGSI-LD Null value is encountered" — the
    // deletion marker is only meaningful on partial-update/merge inputs
    // (allow_null). `json` is exempt: raw JSON is never interpreted.
    if !opts.allow_null {
        let nullish = |v: &Value| match v {
            Value::String(s) => s == "urn:ngsi-ld:null",
            Value::Array(a) => a.iter().any(|x| x.as_str() == Some("urn:ngsi-ld:null")),
            _ => false,
        };
        for k in ["value", "object", "vocab", "valueList", "objectList"] {
            if out.get(k).is_some_and(&nullish) {
                return Err(bad(format!(
                    "attribute {name}: the NGSI-LD Null is only allowed in \
                     partial update or merge inputs (5.2.1)"
                )));
            }
        }
        if out
            .get("languageMap")
            .and_then(Value::as_object)
            .is_some_and(|m| m.values().any(nullish))
        {
            return Err(bad(format!(
                "attribute {name}: the NGSI-LD Null is only allowed in \
                 partial update or merge inputs (5.2.1)"
            )));
        }
    }

    // 5.5.4: "urn:ngsi-ld:null" as the value of a key-value pair within a
    // JSON object that is the Property's value is BadRequestData — excepted
    // solely for merge fragments (5.5.12). `json` stays exempt (raw JSON).
    if !opts.merge && out.get("value").is_some_and(has_object_member_null) {
        return Err(bad(format!(
            "attribute {name}: \"urn:ngsi-ld:null\" inside a compound value \
             is only allowed in merge fragments (5.5.4)"
        )));
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
                // 5.5.4: a null inside a compound value is legal in merge
                // fragments only — a partial update (5.5.8) is not one.
                if has_object_member_null(v) {
                    return Err(bad("\"urn:ngsi-ld:null\" inside a compound value is only \
                         allowed in merge fragments (5.5.4)"
                        .into()));
                }
                out.insert("value".into(), v.clone());
            }
            // 4.5.5.1/5.5.8: the fragment's datasetId selects the instance to
            // patch and is copied onto it — it obeys the same URI-string rule
            // as a full instance, or the patched instance stops answering to
            // the datasetId lookups that keep one default instance per name.
            "datasetId" => {
                if let Some(d) = dataset_id_member(v)? {
                    out.insert("datasetId".into(), d);
                }
            }
            _ if RESERVED_MEMBERS.contains(&k.as_str()) => {
                out.insert(k.clone(), v.clone());
            }
            _ => {
                let iri = expand_attr_name(k, ctx)?;
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

/// 4.5.5.1: "There can only be one default Attribute instance for an
/// Attribute with a given Attribute name in any request or response";
/// datasetIds must be distinct per attribute (explicit "@none" is normalized
/// to absent before this check, so absent + "@none" counts as two defaults).
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

/// 4.6.3: supported Value geometries are "All the GeoJSON Geometries \[8\]
/// with the exception of GeometryCollection" — GEO_TYPES holds exactly that
/// set, and every geometry carries coordinates.
pub fn validate_geojson(name: &str, v: &Value) -> Result<(), NgsiError> {
    let ok = v.as_object().is_some_and(|o| {
        o.get("type")
            .and_then(Value::as_str)
            .is_some_and(|t| GEO_TYPES.contains(&t))
            && o.contains_key("coordinates")
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
///   up to a maximum of six \[digits\]. … In requests, also a comma instead of a
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
    use serde_json::json;

    /// 4.5.5.1: explicit "datasetId": "@none" designates the default
    /// instance — normalized to absent, so it never appears in responses and
    /// absent + "@none" in one request is two default instances (rejected).
    #[test]
    fn dataset_id_none_is_the_default_instance() {
        let doc = json!({"id": "urn:x", "type": "T",
            "speed": {"type": "Property", "value": 1, "datasetId": "@none"}});
        let out = expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default())
            .expect("@none accepted");
        let inst = &out["https://uri.etsi.org/ngsi-ld/default-context/speed"][0];
        assert!(inst.get("datasetId").is_none(), "@none must be dropped");
        // absent + "@none" = two defaults → BadRequestData
        let doc = json!({"id": "urn:x", "type": "T",
            "speed": [{"type": "Property", "value": 1},
                      {"type": "Property", "value": 2, "datasetId": "@none"}]});
        assert!(expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default()).is_err());
    }

    /// 4.5.2.2 / C.11: "ngsildproof": a Property with the non-reified
    /// subproperties "entityIdSealed" and "entityTypeSealed" as specified
    /// in [35]; annex B maps entityIdSealed as a plain term and
    /// entityTypeSealed with "@type": "@vocab" (the value expands like a
    /// type name). Both must survive the expand→compact round trip — they
    /// were RESERVED_MEMBERS with no explicit copy and silently vanished.
    #[test]
    fn ngsildproof_sealed_members_round_trip() {
        let doc = json!({"id": "urn:ngsi-ld:Store:002", "type": "Store",
            "ngsildproof": {"type": "Property",
                "entityIdSealed": "urn:ngsi-ld:Store:002",
                "entityTypeSealed": "Store",
                "value": {"type": "DataIntegrityProof",
                          "cryptosuite": "eddsa-rdfc-2022",
                          "created": "2025-01-27T21:02:24Z",
                          "proofPurpose": "assertionMethod",
                          "proofValue": "zQeVbY4oey5q2M3XKaxup3tmzN4DRFTLVqpLMweBrSxMY"}}});
        let out = expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default())
            .expect("C.11-shaped ngsildproof is valid");
        let inst = &out["https://uri.etsi.org/ngsi-ld/default-context/ngsildproof"][0];
        assert_eq!(inst["entityIdSealed"], "urn:ngsi-ld:Store:002");
        assert_eq!(
            inst["entityTypeSealed"], "https://uri.etsi.org/ngsi-ld/default-context/Store",
            "entityTypeSealed is @vocab-coerced (annex B)"
        );

        let back = crate::compact::compact_entity(&out, &core());
        let np = &back["ngsildproof"];
        assert_eq!(np["entityIdSealed"], "urn:ngsi-ld:Store:002");
        assert_eq!(np["entityTypeSealed"], "Store", "compacts back to the term");
        assert_eq!(
            np["value"]["proofValue"], "zQeVbY4oey5q2M3XKaxup3tmzN4DRFTLVqpLMweBrSxMY",
            "the W3C proof structure is untouched"
        );
        // negative: non-reified means BARE strings — never Property objects,
        // and never dropped
        assert!(np["entityIdSealed"].is_string());
        assert!(np["entityTypeSealed"].is_string());
    }

    /// 4.5.2.2: the sealed members are strings ([35] seals the entity id and
    /// type); "The value of its \"value\" element shall be an object
    /// containing the W3C Data integrity \"proof\" structure" — a
    /// non-object proof value is BadRequestData.
    #[test]
    fn ngsildproof_shapes_are_validated() {
        // non-string sealed members
        for bad_seal in [json!(42), json!({"type": "Property", "value": true})] {
            let doc = json!({"id": "urn:x", "type": "Store",
                "ngsildproof": {"type": "Property", "value": {"type": "DataIntegrityProof"},
                    "entityIdSealed": bad_seal}});
            assert!(
                expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default()).is_err(),
                "entityIdSealed must be a string"
            );
        }
        let doc = json!({"id": "urn:x", "type": "Store",
            "ngsildproof": {"type": "Property", "value": {"type": "DataIntegrityProof"},
                "entityTypeSealed": ["Store"]}});
        assert!(
            expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default()).is_err(),
            "entityTypeSealed must be a string"
        );
        // the proof value shall be an object
        let doc = json!({"id": "urn:x", "type": "Store",
            "ngsildproof": {"type": "Property", "value": "not-a-proof"}});
        assert!(
            expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default()).is_err(),
            "ngsildproof value shall be an object (4.5.2.2)"
        );
        // sealed members on an ordinary attribute stay rejected (the
        // existing 4.5.2.2 guard — pinned here as the negative pair)
        let doc = json!({"id": "urn:x", "type": "Store",
            "speed": {"type": "Property", "value": 1, "entityIdSealed": "urn:x"}});
        assert!(
            expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default()).is_err(),
            "sealed members only under ngsildproof"
        );
    }

    /// 4.5.3.3: "type: If missing, Relationship can be inferred by the
    /// presence of the object attribute" — and the shared prohibitions apply
    /// to the inferred instance too.
    #[test]
    fn concise_relationship_inference() {
        let doc = json!({"id": "urn:x", "type": "T",
            "isParked": {"object": "urn:ngsi-ld:P:1",
                         "observedAt": "2026-01-01T00:00:00Z"}});
        let out = expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default())
            .expect("concise relationship");
        let rel = &out["https://uri.etsi.org/ngsi-ld/default-context/isParked"][0];
        assert_eq!(rel["type"], "Relationship");
        assert_eq!(rel["object"], "urn:ngsi-ld:P:1");
        // inferred Relationship still rejects a Property value member
        let doc = json!({"id": "urn:x", "type": "T",
            "isParked": {"object": "urn:ngsi-ld:P:1", "value": 1}});
        assert!(expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default()).is_err());
    }

    /// 4.5.3.2: a normalized Relationship "shall never include" unitCode
    /// ("Relationships are unitless") or the value-defining members of the
    /// Property family — while objectType stays a legal optional member.
    #[test]
    fn relationship_prohibited_members_rejected() {
        let mk = |extra: (&str, Value)| {
            json!({
                "id": "urn:x", "type": "T",
                "isParked": {"type": "Relationship", "object": "urn:ngsi-ld:P:1",
                             extra.0: extra.1}
            })
        };
        for (m, v) in [
            ("unitCode", json!("MTR")),
            ("value", json!(1)),
            ("languageMap", json!({"en": "x"})),
            ("valueList", json!([1])),
            ("previousObject", json!("urn:a")),
        ] {
            let doc = mk((m, v));
            assert!(
                expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default()).is_err(),
                "{m} must be prohibited on a Relationship"
            );
        }
        let ok = mk(("objectType", json!("Parking")));
        assert!(expand_entity(ok.as_object().unwrap(), &core(), ExpandOpts::default()).is_ok());
    }

    /// 4.5.2.3: concise Property forms — a geometry-shaped value infers
    /// GeoProperty (both as the whole object and as the value member); an
    /// object carrying a "type" member is treated as normalized; a concise
    /// object mixing value with another type's defining member rejects.
    #[test]
    fn concise_property_inference_rules() {
        let geo = json!({"type": "Point", "coordinates": [1.0, 2.0]});
        // whole object IS the geometry
        let doc = json!({"id": "urn:x", "type": "T", "area": geo});
        let out = expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default())
            .expect("bare geometry");
        assert_eq!(
            out["https://uri.etsi.org/ngsi-ld/default-context/area"][0]["type"],
            "GeoProperty"
        );
        // geometry as the value member of a type-less object
        let doc = json!({"id": "urn:x", "type": "T",
            "area": {"value": {"type": "Point", "coordinates": [1.0, 2.0]},
                     "observedAt": "2026-01-01T00:00:00Z"}});
        let out = expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default())
            .expect("geometry value");
        assert_eq!(
            out["https://uri.etsi.org/ngsi-ld/default-context/area"][0]["type"],
            "GeoProperty"
        );
        // an object with a "type" member is normalized — unknown type rejects
        let doc = json!({"id": "urn:x", "type": "T", "a": {"type": "Custom", "x": 1}});
        assert!(expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default()).is_err());
        // concise mix of value and a foreign defining member rejects
        let doc = json!({"id": "urn:x", "type": "T",
            "a": {"value": 1, "languageMap": {"en": "x"}}});
        assert!(expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default()).is_err());
    }

    /// 4.5.2.2: a normalized Property "shall never include" the value-defining
    /// members of other attribute types, output-only members, or the sealed
    /// members outside ngsildproof — and valueType coerces to a datatype URI.
    #[test]
    fn property_prohibited_members_rejected() {
        let mk = |extra: (&str, Value)| {
            json!({
                "id": "urn:x", "type": "T",
                "speed": {"type": "Property", "value": 1, extra.0: extra.1}
            })
        };
        for (m, v) in [
            ("object", json!("urn:ngsi-ld:other:1")),
            ("languageMap", json!({"en": "hi"})),
            ("json", json!({"k": 1})),
            ("vocab", json!("term")),
            ("valueList", json!([1, 2])),
            ("objectList", json!(["urn:a"])),
            ("entity", json!({"id": "urn:a", "type": "T"})),
            ("entityList", json!([])),
            ("previousValue", json!(0)),
            ("previousObject", json!("urn:a")),
            ("entityIdSealed", json!(true)),
        ] {
            let doc = mk((m, v));
            assert!(
                expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default()).is_err(),
                "{m} must be prohibited on a Property"
            );
        }
        // valueType is a legal optional member and coerces to a datatype URI
        let doc = json!({
            "id": "urn:x", "type": "T",
            "speed": {"type": "Property", "value": 1.5, "valueType": "xsd:double"}
        });
        let out = expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default())
            .expect("valueType is optional");
        let attr = &out["https://uri.etsi.org/ngsi-ld/default-context/speed"][0];
        assert!(attr["valueType"]
            .as_str()
            .is_some_and(|s| s.contains("double")));
    }

    /// 4.5.1: "Terms defined in the Core Context as non-reified Properties
    /// (such as datasetId, instanceId, etc.) shall not be used as Attribute
    /// names."
    #[test]
    fn core_non_reified_terms_rejected_as_attribute_names() {
        for name in ["datasetId", "instanceId", "observedAt", "unitCode"] {
            let doc = json!({
                "id": "urn:x", "type": "T",
                name: {"type": "Property", "value": 1}
            });
            assert!(
                expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default()).is_err(),
                "{name} must be rejected as an Attribute name"
            );
        }
        let ok = json!({
            "id": "urn:x", "type": "T",
            "speed": {"type": "Property", "value": 1}
        });
        assert!(expand_entity(ok.as_object().unwrap(), &core(), ExpandOpts::default()).is_ok());
    }

    /// 4.5.1: "Attributes shall not contain any embedded @context" — 5.5.7:
    /// such content "should result in an error of type BadRequestData".
    #[test]
    fn embedded_context_in_attribute_rejected() {
        let doc = json!({
            "id": "urn:x", "type": "T",
            "speed": {"type": "Property", "value": 1,
                      "@context": {"speed": "https://evil.example/speed"}}
        });
        assert!(expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default()).is_err());
        // nested inside a sub-attribute as well
        let doc = json!({
            "id": "urn:x", "type": "T",
            "speed": {"type": "Property", "value": 1,
                      "source": {"type": "Property", "value": "s",
                                 "@context": {"x": "https://e/x"}}}
        });
        assert!(expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default()).is_err());
    }

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

    /// 4.7.1/4.7.2/4.7.3 Geospatial Properties: location & co. must be
    /// GeoProperties (4.7.1); a whole geometry MAY arrive as an encoded JSON
    /// string, accepted "if and only if" it parses into a valid geometry of
    /// the stated type (4.7.2, normalized to the object form); the concise
    /// forms infer GeoProperty from a resolving geometry value (4.7.3).
    #[test]
    fn geo_property_rules() {
        let ent = |attr: Value| {
            let doc = json!({"id": "urn:x", "type": "T", "g": attr});
            expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default())
        };
        // 4.7.2: string-encoded geometry accepted and normalized to the object
        let out = ent(json!({"type": "GeoProperty",
            "value": "{\"type\": \"Point\", \"coordinates\": [17.1, 48.7]}"}))
        .expect("string-encoded geometry");
        let val = &out["https://uri.etsi.org/ngsi-ld/default-context/g"][0]["value"];
        assert!(
            !val.is_string(),
            "value must be normalized, not stay a string"
        );
        assert_eq!(val["type"], "Point");
        assert_eq!(val["coordinates"][0], 17.1);
        // iff: unparseable, non-geometry and GeometryCollection strings → 400
        for bad_s in [
            "not json",
            "{\"a\": 1}",
            "{\"type\": \"GeometryCollection\", \"geometries\": []}",
        ] {
            let err = ent(json!({"type": "GeoProperty", "value": bad_s})).expect_err(bad_s);
            assert!(matches!(err, NgsiError::BadRequestData(_)), "{bad_s}");
        }
        // 4.7.1: location shall be a GeoProperty
        let doc = json!({"id": "urn:x", "type": "T",
            "location": {"type": "Property", "value": 1}});
        assert!(
            expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default()).is_err(),
            "location as plain Property"
        );
        // 4.7.3: concise inference — bare geometry and resolving value
        let out = ent(json!({"type": "Point", "coordinates": [1.0, 2.0]}))
            .expect("bare geometry concise form");
        assert_eq!(
            out["https://uri.etsi.org/ngsi-ld/default-context/g"][0]["type"],
            "GeoProperty"
        );
        let out = ent(json!({"value": {"type": "Point", "coordinates": [1.0, 2.0]}}))
            .expect("resolving value");
        assert_eq!(
            out["https://uri.etsi.org/ngsi-ld/default-context/g"][0]["type"],
            "GeoProperty"
        );
        // an ordinary string value must NOT be inferred as GeoProperty
        let out = ent(json!({"value": "Point"})).expect("plain string value");
        assert_eq!(
            out["https://uri.etsi.org/ngsi-ld/default-context/g"][0]["type"],
            "Property"
        );
    }

    /// 5.5.4 General NGSI-LD validation: "urn:ngsi-ld:null" as a first-level
    /// member value is BadRequestData outside partial-update/merge fragments;
    /// as the value of a key inside a JSON object that is a Property's value
    /// it is BadRequestData everywhere EXCEPT merge fragments (5.5.12).
    #[test]
    fn clause_5_5_4_null_placement() {
        let e =
            |doc: Value, opts: ExpandOpts| expand_entity(doc.as_object().unwrap(), &core(), opts);
        // first-level member value — id and type are first-level members too
        assert!(
            e(
                json!({"id": "urn:ngsi-ld:null", "type": "T"}),
                ExpandOpts::default()
            )
            .is_err(),
            "null URN as id must 400 on create"
        );
        assert!(
            e(
                json!({"id": "urn:x", "type": "urn:ngsi-ld:null"}),
                ExpandOpts::default()
            )
            .is_err(),
            "null URN as type must 400 on create"
        );
        // null inside a JSON object that is a Property value
        let nested = json!({"id": "urn:x", "type": "T",
            "p": {"type": "Property", "value": {"a": "urn:ngsi-ld:null"}}});
        assert!(
            e(nested.clone(), ExpandOpts::default()).is_err(),
            "nested null in value object must 400 on create"
        );
        // partial update allows top-level nulls (allow_null) but the
        // object-nested form is excepted for merge ONLY
        assert!(
            e(
                nested.clone(),
                ExpandOpts {
                    fragment: true,
                    allow_null: true,
                    ..Default::default()
                }
            )
            .is_err(),
            "nested null in value object must 400 on partial update"
        );
        // merge fragment: accepted and preserved for merge_into
        let ok = e(
            nested,
            ExpandOpts {
                fragment: true,
                allow_null: true,
                merge: true,
                ..Default::default()
            },
        )
        .expect("merge fragment keeps the nested null");
        assert_eq!(
            ok["https://uri.etsi.org/ngsi-ld/default-context/p"][0]["value"]["a"],
            "urn:ngsi-ld:null"
        );
        // deep nesting (object in object, object in array) is caught too
        let deep = json!({"id": "urn:x", "type": "T",
            "p": {"type": "Property", "value": {"a": {"b": "urn:ngsi-ld:null"}}}});
        assert!(
            e(deep, ExpandOpts::default()).is_err(),
            "deep nested null must 400"
        );
        let in_array = json!({"id": "urn:x", "type": "T",
            "p": {"type": "Property", "value": [{"a": "urn:ngsi-ld:null"}]}});
        assert!(
            e(in_array, ExpandOpts::default()).is_err(),
            "null in object in array must 400"
        );
        // negative: a benign object value passes and carries no null
        let fine = e(
            json!({"id": "urn:x", "type": "T",
                "p": {"type": "Property", "value": {"a": 1}}}),
            ExpandOpts::default(),
        )
        .expect("plain object value stays legal");
        assert!(
            !fine.to_string().contains("urn:ngsi-ld:null"),
            "no null leakage"
        );
        // top-level attribute null stays the fragment deletion form:
        // rejected on create, accepted under allow_null (5.5.8/5.5.12)
        let top = json!({"id": "urn:x", "type": "T", "p": "urn:ngsi-ld:null"});
        assert!(e(top.clone(), ExpandOpts::default()).is_err());
        e(
            top,
            ExpandOpts {
                fragment: true,
                allow_null: true,
                ..Default::default()
            },
        )
        .expect("first-level null is the deletion form in fragments");
    }

    /// 5.5.8: "A datasetId cannot be deleted by setting it to the value
    /// urn:ngsi-ld:null" — such a fragment is rejected on every input,
    /// including null-allowing (update/merge) ones.
    #[test]
    fn clause_5_5_8_dataset_id_null_rejected() {
        let doc = json!({"id": "urn:x", "type": "T",
            "speed": {"type": "Property", "value": 1,
                      "datasetId": "urn:ngsi-ld:null"}});
        for opts in [
            ExpandOpts::default(),
            ExpandOpts {
                fragment: true,
                allow_null: true,
                ..Default::default()
            },
            ExpandOpts {
                fragment: true,
                allow_null: true,
                merge: true,
                ..Default::default()
            },
        ] {
            assert!(
                expand_entity(doc.as_object().unwrap(), &core(), opts).is_err(),
                "datasetId null must be rejected (opts {opts:?})"
            );
        }
        // the attribute-level fragment path (5.6.4) rejects it too
        let frag = json!({"type": "Property", "value": 1,
                          "datasetId": "urn:ngsi-ld:null"});
        assert!(expand_attr_fragment(frag.as_object().unwrap(), &core()).is_err());
        // a REAL datasetId still passes and is preserved
        let ok = expand_entity(
            json!({"id": "urn:x", "type": "T",
                "speed": {"type": "Property", "value": 1,
                          "datasetId": "urn:ngsi-ld:Dataset:a"}})
            .as_object()
            .unwrap(),
            &core(),
            ExpandOpts::default(),
        )
        .expect("real datasetId");
        assert_eq!(
            ok["https://uri.etsi.org/ngsi-ld/default-context/speed"][0]["datasetId"],
            "urn:ngsi-ld:Dataset:a"
        );
    }

    /// 4.6.5 Supported data types for LanguageMaps: keys are RFC 5646 tags
    /// or "@none", values are strings or arrays of strings; the
    /// {"@none": "urn:ngsi-ld:null"} form is ONLY the partial/merge-patch
    /// deletion encoding — invalid in a create/append (no allow_null).
    #[test]
    fn language_map_data_types() {
        let lp = |lm: Value, opts: ExpandOpts| {
            let doc = json!({"id": "urn:x", "type": "T",
                "brandName": {"type": "LanguageProperty", "languageMap": lm}});
            expand_entity(doc.as_object().unwrap(), &core(), opts)
        };
        let out = lp(
            json!({"sk": "škola", "en": ["school", "academy"], "@none": "default"}),
            ExpandOpts::default(),
        )
        .expect("strings and arrays of strings");
        let m = &out["https://uri.etsi.org/ngsi-ld/default-context/brandName"][0]["languageMap"];
        assert_eq!(m["en"][1], "academy");
        assert!(m.get("urn:ngsi-ld:null").is_none(), "no null leakage");
        // non-string value rejected
        assert!(lp(json!({"en": 5}), ExpandOpts::default()).is_err());
        assert!(lp(json!({"en": [5]}), ExpandOpts::default()).is_err());
        // the null encoding is a deletion marker: rejected on create,
        // accepted under allow_null (patch/merge)
        assert!(
            lp(json!({"@none": "urn:ngsi-ld:null"}), ExpandOpts::default()).is_err(),
            "langmap null form invalid outside patch/merge"
        );
        lp(
            json!({"@none": "urn:ngsi-ld:null"}),
            ExpandOpts {
                allow_null: true,
                ..ExpandOpts::default()
            },
        )
        .expect("deletion form under allow_null");
    }

    /// 4.6.3 Supported data types for Values: "All the GeoJSON Geometries
    /// [8] with the exception of GeometryCollection" — a GeoProperty value
    /// of type GeometryCollection is BadRequestData; plain JSON values and
    /// a bare JSON null follow 4.5.2 (null rejected outside merge-patch).
    #[test]
    fn value_data_types_rules() {
        let geo = |val: Value| {
            let doc = json!({"id": "urn:x", "type": "T",
                "location": {"type": "GeoProperty", "value": val}});
            expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default())
        };
        let out = geo(json!({"type": "Point", "coordinates": [17.1, 48.7]}))
            .expect("Point is a supported geometry");
        let loc = &out["https://uri.etsi.org/ngsi-ld/location"][0];
        assert_eq!(loc["value"]["type"], "Point");
        let err = geo(json!({"type": "GeometryCollection", "geometries": [
            {"type": "Point", "coordinates": [1.0, 2.0]}]}))
        .expect_err("GeometryCollection is excluded by 4.6.3");
        assert!(matches!(err, NgsiError::BadRequestData(_)), "{err:?}");
        // a geometry type must still carry coordinates
        assert!(geo(json!({"type": "Point"})).is_err(), "no coordinates");
        // bare JSON null is not a legal Value outside merge-patch (4.5.2)
        let doc = json!({"id": "urn:x", "type": "T",
            "speed": {"type": "Property", "value": null}});
        assert!(
            expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default()).is_err(),
            "bare null value"
        );
    }

    /// 4.6.2 Supported names: name = unicodeLetter *(letter|number|_) —
    /// Entity Type / Property / Relationship names with other characters are
    /// BadRequestData; keys containing ':' (compact or absolute IRIs) are
    /// out of the term grammar's scope.
    #[test]
    fn name_grammar_rules() {
        let attr = |name: &str| {
            let doc = json!({"id": "urn:x", "type": "T",
                name: {"type": "Property", "value": 1}});
            expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default())
        };
        for bad_name in ["my attr", "1temp", "temp-erature", "temp!", "_hidden"] {
            let err = attr(bad_name).expect_err(bad_name);
            // 4.6.2 names the error type: BadRequestData, nothing else
            assert!(
                matches!(err, NgsiError::BadRequestData(_)),
                "{bad_name}: {err:?}"
            );
        }
        for good in [
            "teplota_1",
            "Ωmega",
            "výška",
            "ns:temp",
            "https://example.com/a b",
        ] {
            attr(good).expect(good);
        }
        // sub-attribute names obey the same grammar
        let doc = json!({"id": "urn:x", "type": "T",
            "speed": {"type": "Property", "value": 1,
                "bad sub": {"type": "Property", "value": 2}}});
        assert!(
            expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default()).is_err(),
            "sub-attribute with space must be rejected"
        );
        // entity type names too; multi-type checks each entry
        let ty = |t: Value| {
            let doc = json!({"id": "urn:x", "type": t, "speed": {"type": "Property", "value": 1}});
            expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default())
        };
        assert!(ty(json!("My Type")).is_err(), "type with space");
        assert!(
            ty(json!(["T", "9T"])).is_err(),
            "type starting with a digit"
        );
        ty(json!("Škola")).expect("unicode-letter type");
    }

    /// 4.5.24.2/4.5.24.3: JsonProperty — json is a raw object or array of
    /// objects (kept verbatim), unitCode and value prohibited, concise
    /// inference from json.
    #[test]
    fn json_property_rules() {
        let mk = |attr: Value| json!({"id": "urn:x", "type": "T", "tickets": attr});
        // valid: object and array-of-objects, kept verbatim (no expansion)
        let doc = mk(json!({"type": "JsonProperty", "json": {"id": "x", "value": 1}}));
        let out = expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default())
            .expect("valid json object");
        let inst = &out["https://uri.etsi.org/ngsi-ld/default-context/tickets"][0];
        assert_eq!(
            inst["json"],
            json!({"id": "x", "value": 1}),
            "raw JSON kept verbatim"
        );
        let doc = mk(json!({"type": "JsonProperty", "json": [{"a": 1}, {"b": 2}]}));
        expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default())
            .expect("array of objects");
        // concise inference
        let doc = mk(json!({"json": {"a": 1}}));
        let out = expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default())
            .expect("concise");
        assert_eq!(
            out["https://uri.etsi.org/ngsi-ld/default-context/tickets"][0]["type"],
            "JsonProperty"
        );
        // scalar json rejected
        let doc = mk(json!({"type": "JsonProperty", "json": 5}));
        assert!(expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default()).is_err());
        // unitCode prohibited
        let doc = mk(json!({"type": "JsonProperty", "json": {"a": 1}, "unitCode": "MTR"}));
        assert!(expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default()).is_err());
        // value prohibited
        let doc = mk(json!({"type": "JsonProperty", "json": {"a": 1}, "value": 1}));
        assert!(expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default()).is_err());
    }

    /// 4.5.21/4.5.22: ListProperty and ListRelationship — objectList accepts
    /// bare URIs or {"object": URI} objects (normalized to bare URIs
    /// internally), non-URIs rejected, [null] deletion form, value/object
    /// prohibited, concise inference.
    #[test]
    fn list_property_and_relationship_rules() {
        let mk = |name: &str, attr: Value| json!({"id": "urn:x", "type": "T", name: attr});
        // ListProperty: ordered array of Property Values, value prohibited
        let doc = mk(
            "steps",
            json!({"type": "ListProperty", "valueList": [1, "two", true]}),
        );
        expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default()).expect("valid");
        let doc = mk(
            "steps",
            json!({"type": "ListProperty", "valueList": [1], "value": 2}),
        );
        assert!(expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default()).is_err());
        // concise inference
        let doc = mk("steps", json!({"valueList": [1]}));
        let out = expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default())
            .expect("concise");
        assert_eq!(
            out["https://uri.etsi.org/ngsi-ld/default-context/steps"][0]["type"],
            "ListProperty"
        );
        // ListRelationship: both entry forms normalize to bare URIs
        let doc = mk(
            "route",
            json!({"type": "ListRelationship",
            "objectList": ["urn:ngsi-ld:R:1", {"object": "urn:ngsi-ld:R:2"}]}),
        );
        let out = expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default())
            .expect("both forms");
        assert_eq!(
            out["https://uri.etsi.org/ngsi-ld/default-context/route"][0]["objectList"],
            json!(["urn:ngsi-ld:R:1", "urn:ngsi-ld:R:2"])
        );
        // non-URI entry rejected
        let doc = mk(
            "route",
            json!({"type": "ListRelationship", "objectList": ["not a uri"]}),
        );
        assert!(expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default()).is_err());
        // object prohibited on ListRelationship
        let doc = mk(
            "route",
            json!({"type": "ListRelationship",
            "objectList": ["urn:ngsi-ld:R:1"], "object": "urn:ngsi-ld:R:2"}),
        );
        assert!(expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default()).is_err());
        // [null] deletion form accepted under allow_null
        let doc = mk(
            "route",
            json!({"type": "ListRelationship",
            "objectList": ["urn:ngsi-ld:null"]}),
        );
        let opts = ExpandOpts {
            allow_null: true,
            fragment: true,
            ..Default::default()
        };
        expand_entity(doc.as_object().unwrap(), &core(), opts).expect("deletion form");
        assert!(is_ngsi_null_list(&json!(["urn:ngsi-ld:null"])));
        assert!(!is_ngsi_null_list(&json!(["urn:ngsi-ld:null", "urn:x"])));
    }

    /// 4.5.20.2/4.5.20.3: VocabProperty — vocab is a string or array of
    /// strings coerced to IRIs; unitCode and value prohibited; concise
    /// inference from vocab.
    #[test]
    fn vocab_property_rules() {
        let mk = |attr: Value| json!({"id": "urn:x", "type": "T", "category": attr});
        // normalized: term expands to an IRI under the context
        let doc = mk(json!({"type": "VocabProperty", "vocab": "non-commercial"}));
        let out = expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default())
            .expect("valid vocab");
        let inst = &out["https://uri.etsi.org/ngsi-ld/default-context/category"][0];
        assert_eq!(
            inst["vocab"],
            "https://uri.etsi.org/ngsi-ld/default-context/non-commercial"
        );
        // concise inference from vocab
        let doc = mk(json!({"vocab": ["a", "b"]}));
        let out = expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default())
            .expect("concise");
        assert_eq!(
            out["https://uri.etsi.org/ngsi-ld/default-context/category"][0]["type"],
            "VocabProperty"
        );
        // non-string vocab rejected
        let doc = mk(json!({"type": "VocabProperty", "vocab": 5}));
        assert!(expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default()).is_err());
        // unitCode prohibited
        let doc = mk(json!({"type": "VocabProperty", "vocab": "x", "unitCode": "MTR"}));
        assert!(expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default()).is_err());
        // value prohibited
        let doc = mk(json!({"type": "VocabProperty", "vocab": "x", "value": 1}));
        assert!(expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default()).is_err());
    }

    /// 4.5.18.2/4.5.18.3: LanguageProperty — non-empty language tags, unitCode
    /// prohibited, value prohibited; concise inference from languageMap.
    #[test]
    fn language_property_rules() {
        let mk = |attr: Value| json!({"id": "urn:x", "type": "T", "says": attr});
        // valid normalized + "@none" tag
        let doc = mk(json!({"type": "LanguageProperty",
                            "languageMap": {"en": "hi", "@none": "hey"}}));
        expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default()).expect("valid");
        // concise inference from languageMap
        let doc = mk(json!({"languageMap": {"en": "hi"}}));
        let out = expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default())
            .expect("concise");
        assert_eq!(
            out["https://uri.etsi.org/ngsi-ld/default-context/says"][0]["type"],
            "LanguageProperty"
        );
        // empty language tag rejected
        let doc = mk(json!({"type": "LanguageProperty", "languageMap": {"": "hi"}}));
        assert!(expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default()).is_err());
        // unitCode prohibited
        let doc = mk(json!({"type": "LanguageProperty",
                            "languageMap": {"en": "hi"}, "unitCode": "MTR"}));
        assert!(expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default()).is_err());
        // value prohibited
        let doc = mk(json!({"type": "LanguageProperty",
                            "languageMap": {"en": "hi"}, "value": 1}));
        assert!(expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default()).is_err());
        // non-string languageMap values rejected
        let doc = mk(json!({"type": "LanguageProperty", "languageMap": {"en": 5}}));
        assert!(expand_entity(doc.as_object().unwrap(), &core(), ExpandOpts::default()).is_err());
    }
}

#[cfg(test)]
mod bench {
    use super::*;

    /// The go/no-go threshold for a hand-rolled JSON-LD processor was
    /// ≥5k expansions/s/core. Antares hand-rolled its processor from day one
    /// rather than forking a `json-ld` crate — this measures it. Run with
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

#[cfg(test)]
mod clause_4_18 {
    use super::*;
    use crate::loader::Loader;
    use serde_json::json;

    fn expand(doc: serde_json::Value) -> Result<Value, NgsiError> {
        expand_entity(
            doc.as_object().expect("obj"),
            &Loader::new().core(),
            ExpandOpts::default(),
        )
    }

    fn with_scope(s: serde_json::Value) -> serde_json::Value {
        json!({"id": "urn:x", "type": "T", "scope": s})
    }

    /// 4.18 Scope grammar: [/] ScopeLevel *(/ScopeLevel), ScopeLevel =
    /// unicodeLetter *(letter/number/_). EXAMPLES 1-4 must pass.
    #[test]
    fn scope_grammar_accepts_the_examples() {
        for s in [
            "/Madrid",
            "Madrid",
            "/Madrid/Gardens/ParqueNorte",
            "/CompanyA/OrganizationB/UnitC",
        ] {
            assert!(expand(with_scope(json!(s))).is_ok(), "{s} must be valid");
        }
        let out = expand(with_scope(json!(["/A", "B/C_2"]))).expect("multi scope");
        assert_eq!(out["scope"], json!(["/A", "B/C_2"]));
    }

    /// 4.18: levels start with a letter, carry only letters/digits/_, no
    /// empty levels; "urn:ngsi-ld:null" "shall be only used and only appear
    /// in case of deleted scopes" — never creatable.
    #[test]
    fn scope_grammar_rejects_malformed_values() {
        for s in [
            "9bad",  // level starts with a digit
            "/a//b", // empty level
            "a-b",   // '-' not a ScopeLevelChar
            "/",     // no level at all
            "",      // empty
            "/a/b/", // trailing empty level
            "a b",   // space
        ] {
            assert!(
                expand(with_scope(json!(s))).is_err(),
                "{s:?} must be rejected"
            );
        }
        assert!(
            expand(with_scope(json!("urn:ngsi-ld:null"))).is_err(),
            "the NGSI-LD Null scope is only for deletions, not creation"
        );
        assert!(
            expand(with_scope(json!(["/ok", "9bad"]))).is_err(),
            "one bad entry poisons the array"
        );
    }
}

#[cfg(test)]
mod clause_5_2_1 {
    use super::*;
    use crate::loader::Loader;
    use serde_json::json;

    fn expand_create(doc: serde_json::Value) -> Result<Value, NgsiError> {
        expand_entity(
            doc.as_object().expect("obj"),
            &Loader::new().core(),
            ExpandOpts::default(), // create: allow_null = false
        )
    }

    /// 5.2.1: outside partial-update/merge inputs, "implementations shall
    /// raise an error of type BadRequestData if an NGSI-LD Null value is
    /// encountered" — Property value, Relationship object, sub-attribute.
    #[test]
    fn ngsi_ld_null_is_rejected_outside_merge_inputs() {
        for doc in [
            json!({"id": "urn:x", "type": "T",
                "p": {"type": "Property", "value": "urn:ngsi-ld:null"}}),
            json!({"id": "urn:x", "type": "T",
                "r": {"type": "Relationship", "object": "urn:ngsi-ld:null"}}),
            json!({"id": "urn:x", "type": "T",
                "p": {"type": "Property", "value": 1,
                    "sub": {"type": "Property", "value": "urn:ngsi-ld:null"}}}),
        ] {
            let e = expand_create(doc).expect_err("NGSI-LD Null on create must be rejected");
            assert!(
                matches!(e, NgsiError::BadRequestData(_)),
                "must be BadRequestData, got {e:?}"
            );
        }
    }

    /// 5.2.1: the deletion marker stays legal on null-allowing inputs
    /// (merge/partial-update, 5.5.8/5.5.12).
    #[test]
    fn ngsi_ld_null_survives_on_merge_inputs() {
        let doc = json!({"p": {"type": "Property", "value": "urn:ngsi-ld:null"}});
        let out = expand_entity(
            doc.as_object().expect("obj"),
            &Loader::new().core(),
            ExpandOpts {
                fragment: true,
                allow_null: true,
                ..ExpandOpts::default()
            },
        )
        .expect("merge fragment expands");
        let inst = &out["https://uri.etsi.org/ngsi-ld/default-context/p"][0];
        assert!(
            is_deletion_instance(inst),
            "the marker must remain recognizable: {inst}"
        );
    }
}

#[cfg(test)]
mod clause_5_2_4 {
    use super::*;
    use crate::loader::Loader;
    use serde_json::json;

    fn expand(doc: serde_json::Value) -> Result<Value, NgsiError> {
        expand_entity(
            doc.as_object().expect("obj"),
            &Loader::new().core(),
            ExpandOpts::default(),
        )
    }

    /// Table 5.2.4-1: id must be a valid URI; type accepts a short name, a
    /// URI, or an array of either; expiresAt must be a 4.6.3 DateTime.
    #[test]
    fn entity_member_table_restrictions() {
        assert!(expand(json!({"id": "not a uri", "type": "T"})).is_err());
        assert!(expand(json!({"id": "urn:x", "type": "T"})).is_ok());
        assert!(expand(json!({"id": "urn:x", "type": ["T", "https://ex.org/U"]})).is_ok());
        assert!(expand(json!({"id": "urn:x", "type": 5})).is_err());
        assert!(
            expand(json!({"id": "urn:x", "type": "T", "expiresAt": "2020-01-01"})).is_err(),
            "expiresAt must be a DateTime, not a Date"
        );
        assert!(
            expand(json!({"id": "urn:x", "type": "T", "expiresAt": "2030-01-01T00:00:00Z"}))
                .is_ok()
        );
    }

    /// Table 5.2.4-1: location/observationSpace/operationSpace are
    /// GeoProperties (5.2.7) — a plain Property under those names is a
    /// violation (4.7.1).
    #[test]
    fn default_geo_names_must_be_geoproperties() {
        for name in ["location", "observationSpace", "operationSpace"] {
            let doc = json!({"id": "urn:x", "type": "T",
                name: {"type": "Property", "value": 3}});
            assert!(expand(doc).is_err(), "{name} as a plain Property must 400");
            let ok = json!({"id": "urn:x", "type": "T",
                name: {"type": "GeoProperty",
                       "value": {"type": "Point", "coordinates": [8, 40]}}});
            assert!(expand(ok).is_ok(), "{name} as a GeoProperty is fine");
        }
    }
}

#[cfg(test)]
mod clause_5_2_5 {
    use super::*;
    use crate::loader::Loader;
    use serde_json::json;

    fn expand(doc: serde_json::Value) -> Result<Value, NgsiError> {
        expand_entity(
            doc.as_object().expect("obj"),
            &Loader::new().core(),
            ExpandOpts::default(),
        )
    }

    fn with_p(p: serde_json::Value) -> serde_json::Value {
        json!({"id": "urn:x", "type": "T", "p": p})
    }

    /// Table 5.2.5-1: value mandatory (any JSON value), datasetId a URI,
    /// observedAt/expiresAt DateTimes, unitCode a string, sub-attributes
    /// nest per their own tables.
    #[test]
    fn property_member_table_restrictions() {
        assert!(
            expand(with_p(json!({"type": "Property"}))).is_err(),
            "value mandatory"
        );
        assert!(
            expand(with_p(json!({"type": "Property", "value": {"k": [1, "x"]},
            "datasetId": "urn:ds:1", "observedAt": "2020-09-09T16:40:00Z",
            "unitCode": "CEL",
            "sub": {"type": "Relationship", "object": "urn:o:1"}})))
            .is_ok()
        );
        assert!(expand(with_p(json!({"type": "Property", "value": 1,
            "datasetId": "not a uri"})))
        .is_err());
        assert!(expand(with_p(json!({"type": "Property", "value": 1,
            "observedAt": "2020-09-09"})))
        .is_err());
        assert!(expand(with_p(json!({"type": "Property", "value": 1,
            "unitCode": 7})))
        .is_err());
    }

    /// 5.2.5: in the concise representation type="Property" is inferred from
    /// `value` — but a GeoJSON-object value "would be interpreted as a
    /// GeoProperty" and so infers GeoProperty, not Property.
    #[test]
    fn concise_inference_and_the_geojson_value_carveout() {
        let out = expand(with_p(json!({"value": 42}))).expect("concise Property");
        let inst = &out["https://uri.etsi.org/ngsi-ld/default-context/p"][0];
        assert_eq!(inst["type"], "Property");
        let out = expand(with_p(json!({"value":
            {"type": "Point", "coordinates": [8, 40]}})))
        .expect("concise geo value");
        let inst = &out["https://uri.etsi.org/ngsi-ld/default-context/p"][0];
        assert_eq!(
            inst["type"], "GeoProperty",
            "a GeoJSON object value infers GeoProperty (5.2.5/5.2.7)"
        );
    }
}

#[cfg(test)]
mod clause_5_2_6 {
    use super::*;
    use crate::loader::Loader;
    use serde_json::json;

    fn expand(doc: serde_json::Value) -> Result<Value, NgsiError> {
        expand_entity(
            doc.as_object().expect("obj"),
            &Loader::new().core(),
            ExpandOpts::default(),
        )
    }

    fn with_r(r: serde_json::Value) -> serde_json::Value {
        json!({"id": "urn:x", "type": "T", "r": r})
    }

    /// Table 5.2.6-1: object mandatory — a URI or an array of URIs; datasetId
    /// a URI; objectType coerced; concise inference from `object`; unitCode
    /// prohibited (4.5.3.2).
    #[test]
    fn relationship_member_table_restrictions() {
        assert!(
            expand(with_r(json!({"type": "Relationship"}))).is_err(),
            "object mandatory"
        );
        assert!(expand(with_r(
            json!({"type": "Relationship", "object": "not a uri"})
        ))
        .is_err());
        assert!(
            expand(with_r(json!({"type": "Relationship",
            "object": ["urn:a", "urn:b"], "datasetId": "urn:ds:1",
            "objectType": "Device"})))
            .is_ok(),
            "array of URIs is legal"
        );
        assert!(
            expand(with_r(json!({"type": "Relationship",
            "object": ["urn:a", "not a uri"]})))
            .is_err(),
            "every array entry must be a URI"
        );
        assert!(
            expand(with_r(json!({"type": "Relationship", "object": "urn:a",
            "unitCode": "C62"})))
            .is_err(),
            "Relationships are unitless"
        );
        // concise inference from the object member
        let out = expand(with_r(json!({"object": "urn:o:1"}))).expect("concise");
        let inst = &out["https://uri.etsi.org/ngsi-ld/default-context/r"][0];
        assert_eq!(inst["type"], "Relationship");
    }
}

#[cfg(test)]
mod clause_5_2_32 {
    use super::*;
    use crate::loader::Loader;
    use serde_json::json;

    fn expand(doc: serde_json::Value) -> Result<Value, NgsiError> {
        expand_entity(
            doc.as_object().expect("obj"),
            &Loader::new().core(),
            ExpandOpts::default(),
        )
    }

    fn with_lp(lp: serde_json::Value) -> serde_json::Value {
        json!({"id": "urn:x", "type": "T", "greeting": lp})
    }

    /// Table 5.2.32-1: languageMap keys are non-empty language tags mapping
    /// to strings or string arrays; valueType, when present, shall be equal
    /// to "langString"; datasetId is a URI; observedAt a DateTime.
    #[test]
    fn language_property_member_table_restrictions() {
        let ok = expand(with_lp(json!({"type": "LanguageProperty",
            "languageMap": {"en": "hello", "sk": ["ahoj", "servus"]},
            "valueType": "langString"})))
        .expect("conformant LanguageProperty");
        let attr = &ok["https://uri.etsi.org/ngsi-ld/default-context/greeting"][0];
        assert_eq!(attr["valueType"], "langString");
        assert!(attr.get("value").is_none(), "languageMap, not value");
        assert!(
            expand(with_lp(json!({"type": "LanguageProperty",
                "languageMap": {"en": "x"}, "valueType": "xsd:string"})))
            .is_err(),
            "valueType shall be equal to langString"
        );
        assert!(
            expand(with_lp(json!({"type": "LanguageProperty",
                "languageMap": {"en": 5}})))
            .is_err(),
            "languageMap values are strings or string arrays"
        );
        assert!(
            expand(with_lp(json!({"type": "LanguageProperty",
                "languageMap": {"": "x"}})))
            .is_err(),
            "empty language tag"
        );
        assert!(
            expand(with_lp(json!({"type": "LanguageProperty",
                "languageMap": {"en": ["a", 5]}})))
            .is_err(),
            "array entries must all be strings"
        );
        assert!(
            expand(with_lp(json!({"type": "LanguageProperty"}))).is_err(),
            "languageMap is mandatory"
        );
        assert!(
            expand(with_lp(json!({"type": "LanguageProperty",
                "languageMap": {"en": "x"}, "observedAt": "not-a-date"})))
            .is_err(),
            "observedAt must be a DateTime"
        );
    }
}

#[cfg(test)]
mod clause_5_2_35 {
    use super::*;
    use crate::loader::Loader;
    use serde_json::json;

    fn expand(doc: serde_json::Value) -> Result<Value, NgsiError> {
        expand_entity(
            doc.as_object().expect("obj"),
            &Loader::new().core(),
            ExpandOpts::default(),
        )
    }

    fn with_vp(vp: serde_json::Value) -> serde_json::Value {
        json!({"id": "urn:x", "type": "T", "category": vp})
    }

    /// Table 5.2.35-1: vocab is a String or String[] type-coerced to URIs
    /// under the @context; unitCode is prohibited (4.5.20.2); concise form
    /// infers VocabProperty from the vocab member.
    #[test]
    fn vocab_property_member_table_restrictions() {
        let ok = expand(with_vp(json!({"type": "VocabProperty", "vocab": "term"})))
            .expect("conformant VocabProperty");
        let attr = &ok["https://uri.etsi.org/ngsi-ld/default-context/category"][0];
        assert_eq!(
            attr["vocab"], "https://uri.etsi.org/ngsi-ld/default-context/term",
            "vocab is term-expanded"
        );
        assert!(attr.get("value").is_none(), "vocab, not value");
        let ok = expand(with_vp(
            json!({"type": "VocabProperty", "vocab": ["a", "b"]}),
        ))
        .expect("string[] form");
        let attr = &ok["https://uri.etsi.org/ngsi-ld/default-context/category"][0];
        assert_eq!(attr["vocab"].as_array().map(Vec::len), Some(2));
        assert!(
            expand(with_vp(json!({"type": "VocabProperty", "vocab": ["a", 5]}))).is_err(),
            "vocab array entries must be strings"
        );
        assert!(
            expand(with_vp(json!({"type": "VocabProperty", "vocab": 5}))).is_err(),
            "vocab must be a string or string array"
        );
        assert!(
            expand(with_vp(json!({"type": "VocabProperty"}))).is_err(),
            "vocab is mandatory"
        );
        assert!(
            expand(with_vp(
                json!({"type": "VocabProperty", "vocab": "t", "unitCode": "C"})
            ))
            .is_err(),
            "unitCode prohibited (4.5.20.2)"
        );
        // concise: the vocab member alone infers VocabProperty
        let ok = expand(with_vp(json!({"vocab": "term"}))).expect("concise inference");
        let attr = &ok["https://uri.etsi.org/ngsi-ld/default-context/category"][0];
        assert_eq!(attr["type"], "VocabProperty");
    }
}

#[cfg(test)]
mod clause_5_2_36 {
    use super::*;
    use crate::loader::Loader;
    use serde_json::json;

    fn expand(doc: serde_json::Value) -> Result<Value, NgsiError> {
        expand_entity(
            doc.as_object().expect("obj"),
            &Loader::new().core(),
            ExpandOpts::default(),
        )
    }

    fn with_list(lp: serde_json::Value) -> serde_json::Value {
        json!({"id": "urn:x", "type": "T", "readings": lp})
    }

    /// Table 5.2.36-1: valueList is a mandatory ordered array of JSON
    /// values; concise form infers ListProperty from the valueList member.
    #[test]
    fn list_property_member_table_restrictions() {
        let ok = expand(with_list(json!({"type": "ListProperty",
            "valueList": [1, "a", {"o": 2}]})))
        .expect("conformant ListProperty");
        let attr = &ok["https://uri.etsi.org/ngsi-ld/default-context/readings"][0];
        assert_eq!(attr["valueList"], json!([1, "a", {"o": 2}]), "order kept");
        assert!(attr.get("value").is_none(), "valueList, not value");
        assert!(
            expand(with_list(json!({"type": "ListProperty", "valueList": 5}))).is_err(),
            "valueList must be an array"
        );
        assert!(
            expand(with_list(json!({"type": "ListProperty"}))).is_err(),
            "valueList is mandatory"
        );
        let ok = expand(with_list(json!({"valueList": [1, 2]}))).expect("concise inference");
        let attr = &ok["https://uri.etsi.org/ngsi-ld/default-context/readings"][0];
        assert_eq!(attr["type"], "ListProperty");
    }
}

#[cfg(test)]
mod clause_5_2_37 {
    use super::*;
    use crate::loader::Loader;
    use serde_json::json;

    fn expand(doc: serde_json::Value) -> Result<Value, NgsiError> {
        expand_entity(
            doc.as_object().expect("obj"),
            &Loader::new().core(),
            ExpandOpts::default(),
        )
    }

    fn with_lr(lr: serde_json::Value) -> serde_json::Value {
        json!({"id": "urn:x", "type": "T", "route" : lr})
    }

    /// Table 5.2.37-1: objectList is a mandatory array of URIs — accepted
    /// both as bare URI strings and as {"object": URI} entries (4.5.22.2);
    /// invalid URIs rejected; unitCode prohibited; concise form infers
    /// ListRelationship from the objectList member.
    #[test]
    fn list_relationship_member_table_restrictions() {
        for form in [
            json!(["urn:a", "urn:b"]),
            json!([{"object": "urn:a"}, {"object": "urn:b"}]),
        ] {
            let ok = expand(with_lr(
                json!({"type": "ListRelationship", "objectList": form}),
            ))
            .expect("conformant ListRelationship");
            let attr = &ok["https://uri.etsi.org/ngsi-ld/default-context/route"][0];
            let list = attr["objectList"].as_array().expect("objectList");
            assert_eq!(list.len(), 2);
            assert!(attr.get("object").is_none(), "objectList, not object");
        }
        assert!(
            expand(with_lr(
                json!({"type": "ListRelationship", "objectList": ["not a uri"]})
            ))
            .is_err(),
            "objectList entries must be URIs"
        );
        assert!(
            expand(with_lr(
                json!({"type": "ListRelationship", "objectList": "urn:a"})
            ))
            .is_err(),
            "objectList must be an array"
        );
        assert!(
            expand(with_lr(json!({"type": "ListRelationship"}))).is_err(),
            "objectList is mandatory"
        );
        assert!(
            expand(with_lr(
                json!({"type": "ListRelationship", "objectList": ["urn:a"],
                "unitCode": "C"})
            ))
            .is_err(),
            "unitCode prohibited on a ListRelationship"
        );
        let ok = expand(with_lr(json!({"objectList": ["urn:a"]}))).expect("concise inference");
        let attr = &ok["https://uri.etsi.org/ngsi-ld/default-context/route"][0];
        assert_eq!(attr["type"], "ListRelationship");
    }
}

#[cfg(test)]
mod clause_5_2_38 {
    use super::*;
    use crate::loader::Loader;
    use serde_json::json;

    fn expand(doc: serde_json::Value) -> Result<Value, NgsiError> {
        expand_entity(
            doc.as_object().expect("obj"),
            &Loader::new().core(),
            ExpandOpts::default(),
        )
    }

    fn with_jp(jp: serde_json::Value) -> serde_json::Value {
        json!({"id": "urn:x", "type": "T", "payload": jp})
    }

    /// Table 5.2.38-1: json is a mandatory raw JSON object or array of
    /// objects, never expanded; unitCode prohibited (4.5.24.2); concise form
    /// infers JsonProperty from the json member.
    #[test]
    fn json_property_member_table_restrictions() {
        let ok = expand(with_jp(json!({"type": "JsonProperty",
            "json": {"type": "kept-verbatim", "en": 1}})))
        .expect("conformant JsonProperty");
        let attr = &ok["https://uri.etsi.org/ngsi-ld/default-context/payload"][0];
        assert_eq!(
            attr["json"],
            json!({"type": "kept-verbatim", "en": 1}),
            "raw JSON kept verbatim, no expansion"
        );
        assert!(attr.get("value").is_none(), "json, not value");
        let ok = expand(with_jp(
            json!({"type": "JsonProperty", "json": [{"a": 1}, {"b": 2}]}),
        ))
        .expect("array-of-objects form");
        let attr = &ok["https://uri.etsi.org/ngsi-ld/default-context/payload"][0];
        assert_eq!(attr["json"].as_array().map(Vec::len), Some(2));
        assert!(
            expand(with_jp(json!({"type": "JsonProperty", "json": 5}))).is_err(),
            "json must be an object or array of objects"
        );
        assert!(
            expand(with_jp(json!({"type": "JsonProperty", "json": [1, 2]}))).is_err(),
            "array entries must be objects"
        );
        assert!(
            expand(with_jp(json!({"type": "JsonProperty"}))).is_err(),
            "json is mandatory"
        );
        assert!(
            expand(with_jp(
                json!({"type": "JsonProperty", "json": {"a": 1}, "unitCode": "C"})
            ))
            .is_err(),
            "unitCode prohibited on a JsonProperty"
        );
        let ok = expand(with_jp(json!({"json": {"a": 1}}))).expect("concise inference");
        let attr = &ok["https://uri.etsi.org/ngsi-ld/default-context/payload"][0];
        assert_eq!(attr["type"], "JsonProperty");
    }
}

#[cfg(test)]
mod clause_5_5_4 {
    use super::*;
    use crate::loader::Loader;
    use serde_json::json;

    fn expand(doc: serde_json::Value) -> Result<Value, NgsiError> {
        expand_entity(
            doc.as_object().expect("obj"),
            &Loader::new().core(),
            ExpandOpts::default(),
        )
    }

    /// 5.5.4: outside fragments/notifications, "urn:ngsi-ld:null" is
    /// BadRequestData as a first-level member value, as a Property value /
    /// Relationship object, as the languageMap {"@none": null} form, AND as
    /// a key value inside a JSON object that is a Property value.
    #[test]
    fn ngsi_null_rejected_everywhere_on_create() {
        let with = |attr: serde_json::Value| json!({"id": "urn:x", "type": "T", "a": attr});
        assert!(
            expand(json!({"id": "urn:x", "type": "T", "scope": "urn:ngsi-ld:null"})).is_err(),
            "first-level member value"
        );
        assert!(
            expand(with(
                json!({"type": "Property", "value": "urn:ngsi-ld:null"})
            ))
            .is_err(),
            "Property value"
        );
        assert!(
            expand(with(
                json!({"type": "Relationship", "object": "urn:ngsi-ld:null"})
            ))
            .is_err(),
            "Relationship object"
        );
        assert!(
            expand(with(json!({"type": "LanguageProperty",
                "languageMap": {"@none": "urn:ngsi-ld:null"}})))
            .is_err(),
            "languageMap null form"
        );
        assert!(
            expand(with(json!({"type": "Property",
                "value": {"nested": "urn:ngsi-ld:null"}})))
            .is_err(),
            "null inside a compound Property value"
        );
        // control: an ordinary compound value stays creatable
        assert!(expand(with(json!({"type": "Property", "value": {"nested": 1}}))).is_ok());
    }
}

#[cfg(test)]
mod clause_5_2_7 {
    use super::*;
    use crate::loader::Loader;
    use serde_json::json;

    fn expand(doc: serde_json::Value) -> Result<Value, NgsiError> {
        expand_entity(
            doc.as_object().expect("obj"),
            &Loader::new().core(),
            ExpandOpts::default(),
        )
    }

    fn with_g(g: serde_json::Value) -> serde_json::Value {
        json!({"id": "urn:x", "type": "T", "g": g})
    }

    /// Table 5.2.7-1: value must be a 4.7 GeoJSON geometry object (a plain
    /// number/string is a violation), GeometryCollection excluded (4.6.3);
    /// unitCode prohibited — GeoProperties carry coordinates, not units.
    #[test]
    fn geoproperty_member_table_restrictions() {
        assert!(expand(with_g(json!({"type": "GeoProperty", "value": 5}))).is_err());
        assert!(expand(with_g(json!({"type": "GeoProperty",
            "value": {"type": "Nonsense", "coordinates": [1, 2]}})))
        .is_err());
        assert!(expand(with_g(json!({"type": "GeoProperty",
            "value": {"type": "GeometryCollection", "geometries": []}})))
        .is_err());
        assert!(expand(with_g(json!({"type": "GeoProperty",
            "value": {"type": "Point", "coordinates": [8, 40]},
            "datasetId": "urn:ds:1"})))
        .is_ok());
        assert!(expand(with_g(json!({"type": "GeoProperty",
            "value": {"type": "LineString",
                      "coordinates": [[8, 40], [9, 41]]}})))
        .is_ok());
    }
}

#[cfg(test)]
mod reserved_member_guards {
    use super::*;
    use crate::loader::Loader;
    use serde_json::json;

    /// 4.5.1: an Attribute name is expanded against the @context, and the
    /// user @context is merged BEFORE the core one — so a term whose "@id"
    /// is a bare word stays a RELATIVE IRI. Expanding an attribute onto
    /// "id" must not be allowed to replace the Entity id (5.5.4
    /// BadRequestData), the same rule expand_types applies to type names.
    #[tokio::test]
    async fn attribute_name_must_expand_to_an_absolute_iri() {
        let ctx = Loader::new()
            .resolve_quiet(&json!({"hostile": {"@id": "id"}}))
            .await
            .expect("inline @context");
        assert_eq!(ctx.expand_key("hostile"), "id", "term maps to a bare word");

        let doc = json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "T",
            "hostile": {"type": "Property", "value": 1}});
        let out = expand_entity(doc.as_object().expect("obj"), &ctx, ExpandOpts::default());
        assert!(
            out.is_err(),
            "an attribute name that does not expand to an absolute IRI is BadRequestData, got {out:?}"
        );
        // negative: the Entity id must still be its own URI string — never
        // the attribute's instance array.
        if let Ok(v) = &out {
            assert_eq!(v["id"], "urn:ngsi-ld:Vehicle:1");
        }
    }

    /// 4.5.1/5.5.4: the same at sub-attribute level — a term expanding onto
    /// "observedAt" would replace the validated DateTime with an instance
    /// array, bypassing the 4.6.3 DateTime check.
    #[tokio::test]
    async fn sub_attribute_name_must_not_overwrite_observed_at() {
        let ctx = Loader::new()
            .resolve_quiet(&json!({"hostile": {"@id": "observedAt"}}))
            .await
            .expect("inline @context");
        assert_eq!(ctx.expand_key("hostile"), "observedAt");

        let doc = json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "T",
            "speed": {"type": "Property", "value": 1,
                      "observedAt": "2026-01-01T00:00:00Z",
                      "hostile": {"type": "Property", "value": "x"}}});
        let out = expand_entity(doc.as_object().expect("obj"), &ctx, ExpandOpts::default());
        assert!(out.is_err(), "expected BadRequestData, got {out:?}");

        // negative: with a well-behaved @context the sub-attribute lands on
        // its own IRI and observedAt still holds the DateTime string.
        let ctx = Loader::new()
            .resolve_quiet(&json!({"hostile": {"@id": "https://example.org/hostile"}}))
            .await
            .expect("inline @context");
        let out = expand_entity(doc.as_object().expect("obj"), &ctx, ExpandOpts::default())
            .expect("absolute IRI is fine");
        let inst = &out["https://uri.etsi.org/ngsi-ld/default-context/speed"][0];
        assert_eq!(inst["observedAt"], "2026-01-01T00:00:00Z");
        assert!(inst["https://example.org/hostile"].is_array());
    }

    /// 4.5.5.1/5.5.8: "datasetId" is a URI string in a partial-update
    /// fragment exactly as in a full instance — a non-string one is copied
    /// onto the target instance and hides its default slot from every
    /// datasetId-absent lookup.
    #[test]
    fn fragment_dataset_id_must_be_a_uri_string() {
        for bad in [
            json!(42),
            json!(["urn:ngsi-ld:Dataset:a"]),
            json!({"object": "urn:ngsi-ld:Dataset:a"}),
            json!(true),
            json!("not a uri"),
        ] {
            let frag = json!({"type": "Property", "value": 1, "datasetId": bad});
            let out = expand_attr_fragment(frag.as_object().expect("obj"), &core());
            assert!(out.is_err(), "datasetId {bad} must be rejected, got {out:?}");
            // negative: no non-string datasetId ever reaches the output.
            if let Ok(Value::Object(m)) = &out {
                assert!(m.get("datasetId").is_none_or(Value::is_string));
            }
        }
        let frag = json!({"type": "Property", "value": 1, "datasetId": "urn:ngsi-ld:Dataset:a"});
        let out = expand_attr_fragment(frag.as_object().expect("obj"), &core())
            .expect("a URI datasetId is valid");
        assert_eq!(out["datasetId"], "urn:ngsi-ld:Dataset:a");
    }

    /// 5.2.1: "In all other cases, implementations shall raise an error of
    /// type BadRequestData if an NGSI-LD Null value is encountered" — the
    /// concise forms (a bare value, a bare object value) must not be a way
    /// around it, or a plain append deletes the instance it targets.
    #[test]
    fn concise_values_do_not_smuggle_the_ngsi_null() {
        let create = |attr: serde_json::Value| -> Result<Value, NgsiError> {
            let doc = json!({"id": "urn:ngsi-ld:V:1", "type": "T", "a": attr});
            expand_entity(doc.as_object().expect("obj"), &core(), ExpandOpts::default())
        };
        for attr in [
            json!(["urn:ngsi-ld:null"]),
            json!({"foo": "urn:ngsi-ld:null"}),
            json!({"type": "Property", "value": "urn:ngsi-ld:null"}),
            json!({"value": ["urn:ngsi-ld:null"]}),
        ] {
            let out = create(attr.clone());
            assert!(out.is_err(), "{attr} must be BadRequestData, got {out:?}");
        }
        // negative: the same documents stay legal on a merge fragment
        // (5.5.12), and a create with no sentinel keeps its value intact.
        let doc = json!({"a": {"foo": "urn:ngsi-ld:null"}});
        assert!(expand_entity(
            doc.as_object().expect("obj"),
            &core(),
            ExpandOpts {
                fragment: true,
                allow_null: true,
                merge: true,
                ..Default::default()
            }
        )
        .is_ok());
        let doc = json!({"id": "urn:ngsi-ld:V:1", "type": "T", "a": {"foo": "bar"}});
        let out = expand_entity(doc.as_object().expect("obj"), &core(), ExpandOpts::default())
            .expect("plain compound value");
        assert_eq!(
            out["https://uri.etsi.org/ngsi-ld/default-context/a"][0]["value"]["foo"],
            "bar"
        );
    }

    fn core() -> std::sync::Arc<Context> {
        Loader::new().core()
    }
}
