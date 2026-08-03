//! Compaction: internal expanded form → response document under the request
//! @context. Never mutates its input (§14.4 — enforced by &input signature).

use crate::context::Context;
use serde_json::{Map, Value};

/// Entity-instance members whose values stay verbatim during compaction.
const VERBATIM: &[&str] = &[
    "type", "value", "object", "datasetId", "observedAt", "unitCode", "lang", "languageMap",
    "json", "valueList", "objectList", "createdAt", "modifiedAt", "deletedAt", "instanceId",
    "previousValue", "previousObject", "previousLanguageMap",
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
        if k == "vocab" {
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
            out.insert("vocab".into(), compacted);
        } else if k == "objectType" {
            out.insert("objectType".into(), compact_types(v, ctx));
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
                out.insert(ctx.compact_iri(k), v.clone());
            }
        }
    }
    Value::Object(out)
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
}
