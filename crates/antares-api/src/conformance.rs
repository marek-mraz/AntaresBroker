//! Version negotiation (6.3.6/6.3.21 + 5.8.6 `ngsildConformance`): amend
//! response payloads to conform to an earlier NGSI-LD version per the
//! backwards-compatibility fallbacks of clause 4.3.6.8 (Tables 4.3.6.8-1/2/3).
//!
//! `Prefer: ngsi-ld=<major.minor>` ⇒ apply the fallbacks, answer with
//! `Preference-Applied: ngsi-ld=<conformant-version>`, and 203 Non-Authoritative
//! instead of 200 when the payload was actually altered (the response tables'
//! "altered Entity" rows). A subscription's `ngsildConformance` applies the
//! same amendment to every notification (5.8.6).

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use serde_json::Value;

/// Members of an entity that are NOT attributes.
const ENTITY_META: &[&str] = &[
    "id",
    "type",
    "scope",
    "@context",
    "createdAt",
    "modifiedAt",
    "deletedAt",
    "expiresAt",
];

/// `"major.minor"` (4.3.6.8; a patch part as in `1.9.1` is tolerated).
pub fn parse_version(s: &str) -> Option<(u32, u32)> {
    let mut it = s.trim().split('.');
    let major = it.next()?.parse().ok()?;
    let minor = it.next()?.parse().ok()?;
    Some((major, minor))
}

/// The version this broker natively conforms to.
const NATIVE: (u32, u32) = (1, 9);

/// Amend one compacted entity document in place; true when anything changed.
pub fn amend_entity(doc: &mut Value, ver: (u32, u32)) -> bool {
    if ver >= NATIVE {
        return false;
    }
    let Some(obj) = doc.as_object_mut() else {
        return false;
    };
    let mut changed = false;
    // Table 4.3.6.8-1 entity-level members.
    if ver < (1, 9) && obj.remove("expiresAt").is_some() {
        changed = true;
    }
    if ver < (1, 4) && obj.remove("scope").is_some() {
        changed = true;
    }
    if ver < (1, 3) {
        // Note 1: 1.0 knows a single entity type — keep the first.
        if let Some(Value::Array(types)) = obj.get("type") {
            if let Some(first) = types.first().cloned() {
                obj.insert("type".into(), first);
                changed = true;
            }
        }
    }
    let attr_names: Vec<String> = obj
        .keys()
        .filter(|k| !ENTITY_META.contains(&k.as_str()))
        .cloned()
        .collect();
    for name in attr_names {
        if let Some(v) = obj.get_mut(&name) {
            changed |= amend_attr(v, ver);
        }
    }
    changed
}

/// One attribute node (object or multi-instance array), recursively.
fn amend_attr(v: &mut Value, ver: (u32, u32)) -> bool {
    let mut changed = false;
    if let Value::Array(instances) = v {
        // Notes 2/3: 1.0 has no datasetId multi-instances — keep the default
        // instance (the one without datasetId), else the first. Selection runs
        // BEFORE the <1.3 datasetId removal erases the tiebreaker.
        if ver < (1, 3) {
            let pick = instances
                .iter()
                .position(|i| i.get("datasetId").is_none())
                .unwrap_or(0);
            if let Some(mut one) = instances.get(pick).cloned() {
                amend_attr(&mut one, ver);
                *v = one;
                return true;
            }
        }
        for i in instances.iter_mut() {
            changed |= amend_attr(i, ver);
        }
        return changed;
    }
    let Some(obj) = v.as_object_mut() else {
        return false;
    };
    let ty = obj.get("type").and_then(Value::as_str).unwrap_or("");
    match ty {
        // Table 4.3.6.8-1 attribute-type fallbacks.
        "LanguageProperty" if ver < (1, 4) => {
            obj.insert("type".into(), Value::String("Property".into()));
            if let Some(lm) = obj.remove("languageMap") {
                obj.insert("value".into(), lm);
            }
            changed = true;
        }
        "JsonProperty" if ver < (1, 8) => {
            obj.insert("type".into(), Value::String("Property".into()));
            if let Some(j) = obj.remove("json") {
                obj.insert("value".into(), j);
            }
            changed = true;
        }
        "VocabProperty" if ver < (1, 8) => {
            obj.insert("type".into(), Value::String("Property".into()));
            if let Some(vv) = obj.remove("vocab") {
                obj.insert("value".into(), vv);
            }
            changed = true;
        }
        "ListProperty" if ver < (1, 8) => {
            obj.insert("type".into(), Value::String("Property".into()));
            if let Some(vl) = obj.remove("valueList") {
                obj.insert("value".into(), vl);
            }
            changed = true;
        }
        "ListRelationship" if ver < (1, 8) => {
            obj.insert("type".into(), Value::String("Relationship".into()));
            if let Some(ol) = obj.remove("objectList") {
                obj.insert("object".into(), ol);
            }
            changed = true;
        }
        _ => {}
    }
    // Tables 4.3.6.8-2/3 sub-member removals (shared version boundaries).
    if ver < (1, 3) {
        for k in ["datasetId", "observedAt", "unitCode"] {
            if obj.remove(k).is_some() {
                changed = true;
            }
        }
    }
    if ver < (1, 8) && obj.remove("objectType").is_some() {
        changed = true;
    }
    if ver < (1, 9) {
        for k in ["valueType", "expiresAt"] {
            if obj.remove(k).is_some() {
                changed = true;
            }
        }
    }
    // Sub-attributes (properties-of-properties) get the same treatment.
    let subs: Vec<String> = obj
        .keys()
        .filter(|k| {
            !matches!(
                k.as_str(),
                "type"
                    | "value"
                    | "object"
                    | "languageMap"
                    | "json"
                    | "vocab"
                    | "valueList"
                    | "objectList"
                    | "datasetId"
                    | "observedAt"
                    | "unitCode"
                    | "objectType"
                    | "valueType"
                    | "expiresAt"
                    | "createdAt"
                    | "modifiedAt"
                    | "instanceId"
            )
        })
        .cloned()
        .collect();
    for name in subs {
        if let Some(sub) = obj.get_mut(&name) {
            if sub.is_object() || sub.is_array() {
                changed |= amend_attr(sub, ver);
            }
        }
    }
    changed
}

/// Amend whatever entity-bearing payload shape a response carries.
pub fn amend_payload(doc: &mut Value, ver: (u32, u32)) -> bool {
    match doc {
        Value::Array(items) => {
            let mut changed = false;
            for i in items {
                changed |= amend_payload(i, ver);
            }
            changed
        }
        Value::Object(o) => {
            if o.get("type").and_then(Value::as_str) == Some("Notification") {
                match o.get_mut("data") {
                    Some(d) => amend_payload(d, ver),
                    None => false,
                }
            } else if o.contains_key("id") && o.contains_key("type") {
                amend_entity(doc, ver)
            } else {
                false
            }
        }
        _ => false,
    }
}

/// `Prefer: ngsi-ld=<version>` from a raw Prefer header value (RFC 7240 —
/// preferences are comma-separated `token=value` pairs).
pub fn preferred_version(prefer: &str) -> Option<(u32, u32)> {
    prefer.split(',').find_map(|p| {
        let (k, v) = p.split_once('=')?;
        (k.trim() == "ngsi-ld").then(|| parse_version(v.trim().trim_matches('"')))?
    })
}

/// Router middleware (6.3.6): honour `Prefer: ngsi-ld=` on JSON responses.
pub async fn prefer_version_layer(req: Request<Body>, next: Next) -> Response {
    let requested = req
        .headers()
        .get("prefer")
        .and_then(|h| h.to_str().ok())
        .and_then(preferred_version);
    let Some(ver) = requested else {
        return next.run(req).await;
    };
    let resp = next.run(req).await;
    let is_json = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|h| h.to_str().ok())
        .is_some_and(|ct| {
            ct.starts_with("application/json") || ct.starts_with("application/ld+json")
        });
    if resp.status() != StatusCode::OK || !is_json {
        return resp;
    }
    let (mut parts, body) = resp.into_parts();
    // Honouring the preference is optional (RFC 7240 section 2), so the
    // buffer is bounded by the same cap the request wall advertises: a
    // response bigger than MAX_BODY_BYTES passes through byte-identical,
    // unamended and without Preference-Applied.
    use futures_util::StreamExt;
    let mut stream = body.into_data_stream();
    let mut buf: Vec<u8> = Vec::new();
    loop {
        match stream.next().await {
            None => break,
            Some(Err(_)) => return Response::from_parts(parts, Body::empty()),
            Some(Ok(chunk)) => {
                if buf.len() + chunk.len() > crate::bounds::MAX_BODY_BYTES {
                    // stitch the already-read prefix back in front of the
                    // untouched remainder of the stream
                    let read = futures_util::stream::iter([
                        Ok::<_, axum::Error>(axum::body::Bytes::from(buf)),
                        Ok(chunk),
                    ]);
                    return Response::from_parts(parts, Body::from_stream(read.chain(stream)));
                }
                buf.extend_from_slice(&chunk);
            }
        }
    }
    let bytes = axum::body::Bytes::from(buf);
    let conformant = if ver < NATIVE { ver } else { NATIVE };
    let mut altered = false;
    let bytes = match serde_json::from_slice::<Value>(&bytes) {
        Ok(mut doc) => {
            altered = amend_payload(&mut doc, ver);
            serde_json::to_vec(&doc).map(Into::into).unwrap_or(bytes)
        }
        Err(_) => bytes,
    };
    parts.headers.insert(
        "Preference-Applied",
        format!("ngsi-ld={}.{}", conformant.0, conformant.1)
            .parse()
            .expect("header value"),
    );
    if altered {
        // The response tables' "altered Entity" rows: 203 Non-Authoritative.
        parts.status = StatusCode::NON_AUTHORITATIVE_INFORMATION;
    }
    parts.headers.remove(header::CONTENT_LENGTH);
    let mut resp = Response::from_parts(parts, Body::from(bytes));
    resp.headers_mut().remove(header::TRANSFER_ENCODING);
    resp
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn version_parsing() {
        assert_eq!(parse_version("1.5"), Some((1, 5)));
        assert_eq!(parse_version("1.9.1"), Some((1, 9)));
        assert_eq!(parse_version("junk"), None);
        assert_eq!(preferred_version("ngsi-ld=1.6"), Some((1, 6)));
        assert_eq!(preferred_version("body=json, ngsi-ld=1.4"), Some((1, 4)));
        assert_eq!(preferred_version("body=json"), None);
    }

    #[test]
    fn new_attribute_types_fall_back_per_version() {
        let mk = || {
            json!({"id": "urn:a", "type": "T",
                "lp": {"type": "LanguageProperty", "languageMap": {"en": "hi"}},
                "jp": {"type": "JsonProperty", "json": {"k": 1}},
                "vp": {"type": "VocabProperty", "vocab": "V"},
                "list": {"type": "ListProperty", "valueList": [1, 2]},
                "lr": {"type": "ListRelationship", "objectList": ["urn:b"]}})
        };
        // 1.8 understands Json/Vocab/List*, not... everything stays but nothing
        // else: only versions below the introduction boundary reformat.
        let mut d = mk();
        assert!(!amend_entity(&mut d, (1, 9)), "native version: unchanged");
        let mut d = mk();
        assert!(amend_entity(&mut d, (1, 4)));
        assert_eq!(d["jp"], json!({"type": "Property", "value": {"k": 1}}));
        assert_eq!(d["vp"], json!({"type": "Property", "value": "V"}));
        assert_eq!(d["list"], json!({"type": "Property", "value": [1, 2]}));
        assert_eq!(
            d["lr"],
            json!({"type": "Relationship", "object": ["urn:b"]})
        );
        assert_eq!(
            d["lp"],
            json!({"type": "LanguageProperty", "languageMap": {"en": "hi"}}),
            "LanguageProperty exists since 1.4"
        );
        let mut d = mk();
        assert!(amend_entity(&mut d, (1, 3)));
        assert_eq!(
            d["lp"],
            json!({"type": "Property", "value": {"en": "hi"}}),
            "1.3 predates LanguageProperty"
        );
    }

    #[test]
    fn one_dot_zero_single_type_and_default_instance() {
        let mut d = json!({"id": "urn:a", "type": ["A", "B"],
        "speed": [
            {"type": "Property", "value": 1, "datasetId": "urn:ds:1"},
            {"type": "Property", "value": 2}
        ]});
        assert!(amend_entity(&mut d, (1, 0)));
        assert_eq!(d["type"], "A", "note 1: single type, first wins");
        assert_eq!(
            d["speed"],
            json!({"type": "Property", "value": 2}),
            "note 2: default instance preferred, datasetId gone (<1.3)"
        );
    }

    #[test]
    fn sub_member_removals_by_boundary() {
        let mut d = json!({"id": "urn:a", "type": "T", "expiresAt": "2030-01-01T00:00:00Z",
            "scope": "/a",
            "r": {"type": "Relationship", "object": "urn:b", "objectType": "B",
                   "observedAt": "2020-01-01T00:00:00Z",
                   "nested": {"type": "Property", "value": 1, "unitCode": "C"}}});
        let mut d13 = d.clone();
        assert!(amend_entity(&mut d13, (1, 3)));
        assert!(d13.get("scope").is_none(), "scope is 1.4");
        assert!(d13.get("expiresAt").is_none(), "entity expiresAt is 1.9");
        assert!(d13["r"].get("objectType").is_none(), "objectType is 1.8");
        assert!(
            d13["r"].get("observedAt").is_some(),
            "observedAt fine at 1.3"
        );
        assert!(
            d13["r"]["nested"].get("unitCode").is_some(),
            "unitCode fine at 1.3"
        );
        assert!(amend_entity(&mut d, (1, 0)));
        assert!(d["r"].get("observedAt").is_none());
        assert!(d["r"]["nested"].get("unitCode").is_none());
    }

    #[test]
    fn notification_data_is_amended() {
        let mut n = json!({"id": "urn:n:1", "type": "Notification",
            "data": [{"id": "urn:a", "type": "T",
                      "jp": {"type": "JsonProperty", "json": 1}}]});
        assert!(amend_payload(&mut n, (1, 6)));
        assert_eq!(n["data"][0]["jp"], json!({"type": "Property", "value": 1}));
    }
}
