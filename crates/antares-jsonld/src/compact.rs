//! Compaction: internal expanded form → response document under the request
//! @context. Never mutates its input (enforced by the &input signature).

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
        } else if k == "entityTypeSealed" {
            // 4.5.2.2 / annex B: @vocab-coerced — compacts back to a term
            // exactly like a type name; entityIdSealed needs no arm (a
            // plain string passes through the default member handling)
            out.insert("entityTypeSealed".into(), compact_types(v, ctx));
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

    fn ctx_of(v: Value) -> crate::context::Context {
        let mut c = crate::context::Context::default();
        c.merge_object(v.as_object().unwrap()).unwrap();
        c.freeze();
        c
    }

    // ---- compact_entity -----------------------------------------------

    /// Nothing is silently dropped: an attribute IRI the context has no term
    /// for keeps its full IRI, and the member count is preserved.
    #[test]
    fn unmapped_attributes_survive_compaction() {
        let ctx = ctx_of(json!({"name": "https://example.org/name"}));
        let internal = json!({
            "id": "urn:ngsi-ld:B:1",
            "type": ["https://example.org/Building"],
            "https://example.org/name": [{"type": "Property", "value": "x"}],
            "https://elsewhere.example/unmapped": [{"type": "Property", "value": 1}]
        });
        let out = compact_entity(&internal, &ctx);
        let o = out.as_object().unwrap();
        assert_eq!(o.len(), 4, "no member may vanish: {out}");
        assert!(o.contains_key("name"));
        assert!(o.contains_key("https://elsewhere.example/unmapped"));
        assert!(!o.contains_key("https://example.org/name"));
        // single-element type array is unwrapped to a scalar
        assert_eq!(out["type"], json!("https://example.org/Building"));
    }

    /// Reserved entity members must never be overwritten by an attribute that
    /// would compact to the same name: the round-trip guard in compaction
    /// falls back to prefix compaction, so the attribute keeps a key of its
    /// own and the system members keep their values.
    #[test]
    fn reserved_members_are_not_clobbered_by_attributes() {
        let ctx = Loader::new().core();
        let vocab = "https://uri.etsi.org/ngsi-ld/default-context/";
        let mut internal = Map::new();
        internal.insert("id".into(), json!("urn:ngsi-ld:B:1"));
        internal.insert("type".into(), json!(format!("{vocab}Building")));
        for shadow in ["type", "id", "value"] {
            internal.insert(
                format!("{vocab}{shadow}"),
                json!([{"type": "Property", "value": "shadow"}]),
            );
        }
        let out = compact_entity(&Value::Object(internal), &ctx);
        assert_eq!(out["id"], "urn:ngsi-ld:B:1");
        assert_eq!(out["type"], "Building");
        assert_eq!(out.as_object().unwrap().len(), 5, "no member lost: {out}");
        // negative: not one of the three shadows may be rendered under its
        // bare reserved name — each keeps a key that expands back to itself.
        for shadow in ["type", "id", "value"] {
            let key = ctx.compact_iri(&format!("{vocab}{shadow}"));
            assert_ne!(key, shadow, "{shadow} was clobbered: {out}");
            assert_eq!(ctx.expand_key(&key), format!("{vocab}{shadow}"));
            assert_eq!(
                out[&key]["value"], "shadow",
                "{shadow} lost its value: {out}"
            );
        }
    }

    /// Non-object input is returned untouched; timestamps pass through
    /// verbatim; a single-element scope array is unwrapped.
    #[test]
    fn entity_edge_shapes() {
        let ctx = Loader::new().core();
        assert_eq!(
            compact_entity(&json!("not an object"), &ctx),
            json!("not an object")
        );
        assert_eq!(compact_entity(&json!([]), &ctx), json!([]));
        assert_eq!(compact_entity(&json!({}), &ctx), json!({}));
        let out = compact_entity(
            &json!({"id": "urn:x", "scope": ["/a"], "createdAt": "2026-01-01T00:00:00Z",
                    "modifiedAt": "2026-01-02T00:00:00Z", "deletedAt": "2026-01-03T00:00:00Z",
                    "expiresAt": "2026-01-04T00:00:00Z"}),
            &ctx,
        );
        assert_eq!(out["scope"], json!("/a"));
        assert_eq!(out["createdAt"], "2026-01-01T00:00:00Z");
        assert_eq!(out["expiresAt"], "2026-01-04T00:00:00Z");
        let out = compact_entity(&json!({"scope": ["/a", "/b"]}), &ctx);
        assert_eq!(out["scope"], json!(["/a", "/b"]));
    }

    // ---- compact_types ------------------------------------------------

    #[test]
    fn compact_types_shapes() {
        let ctx = ctx_of(json!({"B": "https://example.org/B"}));
        assert_eq!(
            compact_types(&json!("https://example.org/B"), &ctx),
            json!("B")
        );
        assert_eq!(
            compact_types(&json!(["https://example.org/B"]), &ctx),
            json!("B")
        );
        assert_eq!(
            compact_types(
                &json!(["https://example.org/B", "https://example.org/C"]),
                &ctx
            ),
            json!(["B", "https://example.org/C"])
        );
        // an empty list stays a list, non-strings pass through unchanged
        assert_eq!(compact_types(&json!([]), &ctx), json!([]));
        assert_eq!(compact_types(&json!([42]), &ctx), json!(42));
        assert_eq!(compact_types(&json!(null), &ctx), json!(null));
    }

    // ---- compact_instance ---------------------------------------------

    /// Property values are opaque JSON: their inner keys must NOT be compacted
    /// even when they look like IRIs the context knows.
    #[test]
    fn property_values_stay_verbatim() {
        let ctx = ctx_of(json!({"name": "https://example.org/name"}));
        let inst = json!({
            "type": "Property",
            "value": {"https://example.org/name": "not an attribute", "nested": [1, 2]},
            "unitCode": "CEL",
            "observedAt": "2026-01-01T00:00:00Z",
            "https://example.org/name": [{"type": "Property", "value": "sub"}]
        });
        let out = compact_instance(&inst, &ctx);
        assert_eq!(out["value"], inst["value"], "value must not be rewritten");
        assert_eq!(out["unitCode"], "CEL");
        // the sub-attribute IRI, in contrast, does compact to its term
        assert_eq!(out["name"], json!({"type": "Property", "value": "sub"}));
        assert!(out
            .as_object()
            .unwrap()
            .get("https://example.org/name")
            .is_none());
    }

    /// vocab / previousVocab / objectType / entityTypeSealed all carry IRIs
    /// that compact back to terms; a non-object instance is returned as-is.
    #[test]
    fn vocab_and_type_members_compact() {
        let ctx = ctx_of(json!({"cat": "https://example.org/cat",
                                "T": "https://example.org/T"}));
        let out = compact_instance(
            &json!({"type": "VocabProperty",
                    "vocab": ["https://example.org/cat", "https://elsewhere.example/x"],
                    "previousVocab": "https://example.org/cat",
                    "objectType": ["https://example.org/T"],
                    "entityTypeSealed": ["https://example.org/T"]}),
            &ctx,
        );
        assert_eq!(out["vocab"], json!(["cat", "https://elsewhere.example/x"]));
        assert_eq!(out["previousVocab"], json!("cat"));
        assert_eq!(out["objectType"], json!("T"));
        assert_eq!(out["entityTypeSealed"], json!("T"));
        assert_eq!(compact_instance(&json!(7), &ctx), json!(7));
        assert_eq!(compact_instance(&json!("s"), &ctx), json!("s"));
    }

    /// 4.5.22.2: bare URIs are wrapped, entries already in object form are not
    /// wrapped twice, and a non-array objectList passes through.
    #[test]
    fn object_list_wrapping_edges() {
        let ctx = Loader::new().core();
        let out = compact_instance(
            &json!({"type": "ListRelationship",
                    "objectList": ["urn:a", {"object": "urn:b"}],
                    "previousObjectList": ["urn:c"]}),
            &ctx,
        );
        assert_eq!(
            out["objectList"],
            json!([{"object": "urn:a"}, {"object": "urn:b"}])
        );
        assert_eq!(out["previousObjectList"], json!([{"object": "urn:c"}]));
        let out = compact_instance(&json!({"objectList": "urn:a"}), &ctx);
        assert_eq!(out["objectList"], json!("urn:a"));
    }

    /// Multi-instance attributes keep their array; a single instance is
    /// unwrapped; an empty instance array stays empty.
    #[test]
    fn attribute_instance_arrays() {
        let ctx = ctx_of(json!({"a": "https://example.org/a"}));
        let out = compact_entity(
            &json!({"https://example.org/a": [
                {"type": "Property", "value": 1, "datasetId": "urn:d:1"},
                {"type": "Property", "value": 2}]}),
            &ctx,
        );
        assert_eq!(out["a"].as_array().unwrap().len(), 2);
        let out = compact_entity(&json!({"https://example.org/a": []}), &ctx);
        assert_eq!(out["a"], json!([]));
    }

    // ---- compact_entity_shallow / compact_simplified_value ------------

    /// Simplified values are plain JSON and stay verbatim — a single-ring
    /// polygon must not be reshaped, and a two-member object is not mistaken
    /// for a VocabProperty.
    #[test]
    fn simplified_values_stay_verbatim() {
        let ctx = ctx_of(json!({"loc": "https://example.org/loc",
                                "v": "https://example.org/v"}));
        let out = compact_entity_shallow(
            &json!({
                "id": "urn:x", "type": "https://example.org/T",
                "https://example.org/loc": {"type": "Polygon", "coordinates": [[[0,0],[1,0],[0,1],[0,0]]]},
                "https://example.org/v": {"vocab": "https://example.org/loc", "extra": 1}
            }),
            &ctx,
        );
        assert_eq!(
            out["loc"],
            json!({"type": "Polygon", "coordinates": [[[0,0],[1,0],[0,1],[0,0]]]})
        );
        assert_eq!(
            out["v"],
            json!({"vocab": "https://example.org/loc", "extra": 1})
        );
        assert_eq!(compact_entity_shallow(&json!("x"), &ctx), json!("x"));
    }

    /// A "dataset" member whose value is not an object is left alone.
    #[test]
    fn simplified_dataset_edges() {
        let ctx = Loader::new().core();
        let out = compact_entity_shallow(
            &json!({"https://uri.etsi.org/ngsi-ld/default-context/a": {"dataset": 5}}),
            &ctx,
        );
        assert_eq!(out["a"], json!({"dataset": 5}));
    }

    /// The mutual recursion compact_attr_value ↔ compact_instance (and the
    /// nested "dataset" recursion) is bounded by the JSON parser: serde_json
    /// refuses documents nested deeper than its own recursion limit, so an
    /// attacker cannot drive compaction to a stack overflow.
    #[test]
    fn nesting_depth_is_bounded_by_the_parser() {
        fn nest(levels: usize) -> String {
            let mut s = String::from("{\"type\":\"Property\",\"value\":1}");
            for _ in 0..levels {
                s = format!("{{\"https://example.org/s\":[{{\"type\":\"Property\",\"value\":1,\"https://example.org/s\":[{s}]}}]}}");
            }
            s
        }
        assert!(
            serde_json::from_str::<Value>(&nest(200)).is_err(),
            "the parser must refuse deeply nested documents"
        );
        let ctx = ctx_of(json!({"s": "https://example.org/s"}));
        let doc: Value = serde_json::from_str(&nest(20)).expect("within the parser limit");
        let out = compact_entity(&doc, &ctx);
        assert!(out.get("s").is_some());
    }
}
