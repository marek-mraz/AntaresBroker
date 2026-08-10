//! Compaction: internal expanded form → response document under the request
//! @context. Never mutates its input (§14.4 — enforced by &input signature).

use crate::context::Context;
use serde_json::{Map, Value};

/// Entity-instance members whose values stay verbatim during compaction.
const VERBATIM: &[&str] = &[
    "type",
    "value",
    "object",
    "datasetId",
    "observedAt",
    "unitCode",
    "lang",
    "languageMap",
    "json",
    "valueList",
    "objectList",
    "createdAt",
    "modifiedAt",
    "deletedAt",
    "instanceId",
    "previousValue",
    "previousObject",
    "previousLanguageMap",
];

/// Compact an internal expanded entity for output.
pub fn compact_entity(internal: &Value, ctx: &Context) -> Value {
    let Some(obj) = internal.as_object() else {
        return internal.clone();
    };
    let mut out = Map::new();
    for (k, v) in obj {
        match k.as_str() {
            "id" => {
                out.insert("id".into(), v.clone());
            }
            "type" => {
                out.insert("type".into(), compact_types(v, ctx));
            }
            "scope" => {
                out.insert("scope".into(), unwrap_single(v.clone()));
            }
            "createdAt" | "modifiedAt" | "deletedAt" | "expiresAt" => {
                out.insert(k.clone(), v.clone());
            }
            _ => {
                let term = ctx.compact_iri(k);
                out.insert(term, compact_attr_value(v, ctx));
            }
        }
    }
    Value::Object(out)
}

pub fn compact_types(v: &Value, ctx: &Context) -> Value {
    match v {
        Value::Array(items) => unwrap_single(Value::Array(
            items
                .iter()
                .map(|t| match t {
                    Value::String(iri) => Value::String(ctx.compact_iri(iri)),
                    other => other.clone(),
                })
                .collect(),
        )),
        Value::String(iri) => Value::String(ctx.compact_iri(iri)),
        other => other.clone(),
    }
}

fn compact_attr_value(v: &Value, ctx: &Context) -> Value {
    match v {
        Value::Array(instances) => unwrap_single(Value::Array(
            instances.iter().map(|i| compact_instance(i, ctx)).collect(),
        )),
        other => compact_instance(other, ctx),
    }
}

/// Compact one attribute instance (public for temporal presentation, which
/// keeps instance arrays un-unwrapped).
pub fn compact_instance(inst: &Value, ctx: &Context) -> Value {
    let Some(obj) = inst.as_object() else {
        return inst.clone();
    };
    let mut out = Map::new();
    for (k, v) in obj {
        if k == "vocab" || k == "previousVocab" {
            // vocab values compact back to terms
            let compacted = match v {
                Value::String(iri) => Value::String(ctx.compact_iri(iri)),
                Value::Array(a) => Value::Array(
                    a.iter()
                        .map(|s| match s {
                            Value::String(iri) => Value::String(ctx.compact_iri(iri)),
                            o => o.clone(),
                        })
                        .collect(),
                ),
                o => o.clone(),
            };
            out.insert(k.clone(), compacted);
        } else if k == "objectType" {
            out.insert("objectType".into(), compact_types(v, ctx));
        } else if k == "objectList" || k == "previousObjectList" {
            // 4.5.22.2: the normalized objectList is an ordered array of
            // JSON objects each "containing a single Attribute with a key
            // called "object"" — the internal form stores bare URIs.
            let wrapped = match v {
                Value::Array(a) => Value::Array(
                    a.iter()
                        .map(|it| match it {
                            Value::String(uri) => serde_json::json!({ "object": uri }),
                            other => other.clone(),
                        })
                        .collect(),
                ),
                other => other.clone(),
            };
            out.insert(k.clone(), wrapped);
        } else if VERBATIM.contains(&k.as_str()) {
            out.insert(k.clone(), v.clone());
        } else {
            // sub-attribute
            out.insert(ctx.compact_iri(k), compact_attr_value(v, ctx));
        }
    }
    Value::Object(out)
}

/// Shallow compaction for simplified (keyValues) docs: rename top-level keys
/// and compact type values, leave attribute VALUES verbatim (they are plain
/// JSON — recursing would mangle e.g. single-ring polygons).
pub fn compact_entity_shallow(internal: &Value, ctx: &Context) -> Value {
    let Some(obj) = internal.as_object() else {
        return internal.clone();
    };
    let mut out = Map::new();
    for (k, v) in obj {
        match k.as_str() {
            "id" | "scope" | "createdAt" | "modifiedAt" | "deletedAt" | "expiresAt" => {
                out.insert(k.clone(), v.clone());
            }
            "type" => {
                out.insert("type".into(), compact_types(v, ctx));
            }
            _ => {
                out.insert(ctx.compact_iri(k), compact_simplified_value(v, ctx));
            }
        }
    }
    Value::Object(out)
}

/// 4.5.4 Simplified Representation: the VocabProperty form is the single-key
/// object {"vocab": …} (Example 6) whose IRI(s) compact back to terms, exactly
/// as on the normalized path; multi-instance attributes are the {"dataset":
/// {<datasetId>|"@none": <simplified>}} map (Example 2), compacted per
/// instance. All other simplified values are plain JSON and stay verbatim.
fn compact_simplified_value(v: &Value, ctx: &Context) -> Value {
    let Some(o) = v.as_object() else {
        return v.clone();
    };
    if o.len() == 1 {
        if let Some(vocab) = o.get("vocab") {
            let compacted = match vocab {
                Value::String(iri) => Value::String(ctx.compact_iri(iri)),
                Value::Array(a) => Value::Array(
                    a.iter()
                        .map(|s| match s {
                            Value::String(iri) => Value::String(ctx.compact_iri(iri)),
                            other => other.clone(),
                        })
                        .collect(),
                ),
                other => other.clone(),
            };
            return serde_json::json!({ "vocab": compacted });
        }
        if let Some(Value::Object(m)) = o.get("dataset") {
            let per_instance: Map<String, Value> = m
                .iter()
                .map(|(k, iv)| (k.clone(), compact_simplified_value(iv, ctx)))
                .collect();
            return serde_json::json!({ "dataset": per_instance });
        }
    }
    v.clone()
}

fn unwrap_single(v: Value) -> Value {
    match v {
        Value::Array(mut items) if items.len() == 1 => items.remove(0),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expand::{expand_entity, ExpandOpts};
    use crate::loader::Loader;
    use serde_json::json;

    #[test]
    fn round_trip_under_core_context() {
        let input = json!({
            "id": "urn:ngsi-ld:Building:1",
            "type": "Building",
            "name": {"type": "Property", "value": "Eiffel Tower"},
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [2.29, 48.85]}}
        });
        let ctx = Loader::new().core();
        let expanded =
            expand_entity(input.as_object().unwrap(), &ctx, ExpandOpts::default()).unwrap();
        let compacted = compact_entity(&expanded, &ctx);
        assert_eq!(compacted, input);
        // input untouched by construction (&input); expanded untouched too
        assert_eq!(expanded["id"], "urn:ngsi-ld:Building:1");
    }

    /// 4.5.4 Example 6: simplified VocabProperty vocab IRIs compact to terms;
    /// dataset-map instances compact per instance (Example 9).
    #[test]
    fn simplified_vocab_compacts_to_term() {
        let ctx = Loader::new().core();
        let doc = json!({
            "id": "urn:ngsi-ld:V:1",
            "type": "https://uri.etsi.org/ngsi-ld/default-context/Vehicle",
            "https://uri.etsi.org/ngsi-ld/default-context/category":
                {"vocab": "https://uri.etsi.org/ngsi-ld/default-context/non-commercial"},
            "https://uri.etsi.org/ngsi-ld/default-context/mixed":
                {"dataset": {"@none": {"vocab": "https://uri.etsi.org/ngsi-ld/default-context/rental"}, "urn:ngsi-ld:Dataset:1": 7}}
        });
        let out = compact_entity_shallow(&doc, &ctx);
        assert_eq!(out["category"], json!({"vocab": "non-commercial"}));
        assert_eq!(
            out["mixed"],
            json!({"dataset": {"@none": {"vocab": "rental"}, "urn:ngsi-ld:Dataset:1": 7}})
        );
    }

    /// 4.5.22.2: normalized objectList round-trips — {"object": URI} entries
    /// in, bare URIs internally, {"object": URI} entries out.
    #[test]
    fn object_list_normalized_round_trip() {
        let input = json!({
            "id": "urn:ngsi-ld:B:1",
            "type": "T",
            "route": {"type": "ListRelationship",
                      "objectList": [{"object": "urn:ngsi-ld:R:1"}, "urn:ngsi-ld:R:2"]}
        });
        let ctx = Loader::new().core();
        let expanded =
            expand_entity(input.as_object().unwrap(), &ctx, ExpandOpts::default()).unwrap();
        let route = &expanded["https://uri.etsi.org/ngsi-ld/default-context/route"][0];
        assert_eq!(
            route["objectList"],
            json!(["urn:ngsi-ld:R:1", "urn:ngsi-ld:R:2"])
        );
        let compacted = compact_entity(&expanded, &ctx);
        assert_eq!(
            compacted["route"]["objectList"],
            json!([{"object": "urn:ngsi-ld:R:1"}, {"object": "urn:ngsi-ld:R:2"}])
        );
    }
}
