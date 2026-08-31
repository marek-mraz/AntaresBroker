// SPDX-License-Identifier: EUPL-1.2
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
        // A temporal instance array (clause 5.2.5) is a different shape —
        // every instance carries instanceId — and Notes 2/3 do not cover it,
        // so its history survives intact instead of collapsing to one point.
        let temporal = instances.iter().any(|i| i.get("instanceId").is_some());
        if ver < (1, 3) && !temporal {
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
/// The 4.3.6.8 fallback tables describe Entity data. The other NGSI-LD
/// resources are served with the same id + type shape but have no version
/// fallbacks of their own, so they pass through untouched and keep their 200
/// rather than the 203 that marks an altered Entity. They are recognised by
/// their reserved clause 5.2 data-type name.
fn is_reserved_resource(o: &serde_json::Map<String, Value>) -> bool {
    const RESERVED: [&str; 6] = [
        "Subscription",
        "ContextSourceRegistration",
        "CSourceRegistration",
        "Notification",
        "EntityMap",
        "Snapshot",
    ];
    o.get("type")
        .and_then(Value::as_str)
        .is_some_and(|t| RESERVED.contains(&t))
}

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
            } else if o.contains_key("id") && o.contains_key("type") && !is_reserved_resource(o) {
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
        // RFC 9110 clause 5.6.2: a field name is case-insensitive, and RFC
        // 7240 lets a preference carry its own parameters after ';'.
        if !k.trim().eq_ignore_ascii_case("ngsi-ld") {
            return None;
        }
        parse_version(v.split(';').next()?.trim().trim_matches('"'))
    })
}

/// Router middleware (6.3.6): honour `Prefer: ngsi-ld=` on JSON responses.
pub async fn prefer_version_layer(req: Request<Body>, next: Next) -> Response {
    // RFC 9110 clause 5.3: repeated field lines carry the same meaning as one
    // comma-separated list, so the preference is looked for on every line.
    let requested = req
        .headers()
        .get_all("prefer")
        .iter()
        .filter_map(|h| h.to_str().ok())
        .find_map(preferred_version);
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
            // The upstream body failed mid-stream, so the prefix already read
            // is not the response and must not be served under the handler's
            // 200. The transport error text stays server-side (5.5.6).
            Some(Err(_)) => {
                parts.status = StatusCode::INTERNAL_SERVER_ERROR;
                parts.headers.remove(header::CONTENT_LENGTH);
                return Response::from_parts(parts, Body::empty());
            }
            Some(Ok(chunk)) => {
                if buf.len() + chunk.len() > *crate::bounds::MAX_BODY_BYTES {
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
            // Re-serialize in the egress key order every other response path
            // uses (id and type first), not serde_json's own map order.
            crate::negotiate::ordered_vec(&doc).into()
        }
        Err(_) => bytes,
    };
    // Two integers and a dot are always a legal header value; if that ever
    // stopped being true, omitting the header beats taking the response down.
    if let Ok(v) = format!("ngsi-ld={}.{}", conformant.0, conformant.1).parse() {
        parts.headers.insert("Preference-Applied", v);
    }
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

    /// A version string that is not `major.minor` yields no version at all —
    /// the preference is then ignored rather than guessed at.
    #[test]
    fn version_parsing_rejects_malformed_input() {
        for bad in [
            "", "1", "1.", ".", ".5", "1.x", "-1.2", "1.-2", "v1.5", "1 . 5",
        ] {
            assert_eq!(parse_version(bad), None, "{bad:?} is not major.minor");
        }
        // out of u32 range on either part
        assert_eq!(parse_version("4294967296.0"), None);
        assert_eq!(parse_version("1.4294967296"), None);
        assert_eq!(parse_version(" 1.5 "), Some((1, 5)), "outer space trimmed");
    }

    /// RFC 7240: preference tokens are case-insensitive and may carry
    /// `;`-separated parameters; a preference that is not `ngsi-ld` (or whose
    /// value is not a version) leaves version negotiation off.
    #[test]
    fn prefer_header_forms() {
        assert_eq!(preferred_version("NGSI-LD=1.5"), Some((1, 5)));
        assert_eq!(preferred_version("Ngsi-Ld=\"1.5\""), Some((1, 5)));
        assert_eq!(preferred_version("ngsi-ld=1.5; foo=bar"), Some((1, 5)));
        assert_eq!(
            preferred_version("body=json, ngsi-ld=1.5;q=0.1"),
            Some((1, 5))
        );
        for none in [
            "",
            "ngsi-ld",
            "ngsi-ld=",
            "ngsi-ld=junk",
            "body=json",
            "respond-async",
        ] {
            assert_eq!(preferred_version(none), None, "{none:?}");
        }
    }

    /// Table 4.3.6.8-1/2/3 "Version Introduced" column: a member is only
    /// dropped BELOW the version that introduced it — at that version it must
    /// still be present.
    #[test]
    fn members_survive_at_their_introduction_version() {
        let mk = || {
            json!({"id": "urn:a", "type": ["A", "B"], "scope": "/s",
                "expiresAt": "2030-01-01T00:00:00Z",
                "p": [{"type": "Property", "value": 1, "datasetId": "urn:ds:1",
                       "observedAt": "2020-01-01T00:00:00Z", "unitCode": "C",
                       "valueType": "http://x/T", "expiresAt": "2030-01-01T00:00:00Z"},
                      {"type": "Property", "value": 2}],
                "lp": {"type": "LanguageProperty", "languageMap": {"en": "hi"}},
                "r": {"type": "Relationship", "object": "urn:b", "objectType": "B"}})
        };
        let mut d = mk();
        assert!(amend_entity(&mut d, (1, 8)));
        assert!(d["p"][0].get("datasetId").is_some(), "datasetId is 1.3");
        assert!(d["p"][0].get("objectType").is_none(), "attr has none");
        assert!(d["r"].get("objectType").is_some(), "objectType is 1.8");
        assert!(d["p"][0].get("valueType").is_none(), "valueType is 1.9");
        assert!(
            d["p"][0].get("expiresAt").is_none(),
            "attr expiresAt is 1.9"
        );
        assert!(d.get("expiresAt").is_none(), "entity expiresAt is 1.9");
        assert!(d.get("scope").is_some(), "scope is 1.4");
        assert_eq!(d["type"], json!(["A", "B"]), "multi-type is 1.3");
        assert!(d["p"].is_array(), "multi-instance is 1.3");
        assert_eq!(
            d["lp"]["type"], "LanguageProperty",
            "LanguageProperty is 1.4"
        );

        let mut d = mk();
        assert!(amend_entity(&mut d, (1, 3)));
        assert!(d["p"][0].get("observedAt").is_some(), "observedAt is 1.3");
        assert!(d["p"][0].get("unitCode").is_some(), "unitCode is 1.3");
        assert!(d.get("scope").is_none(), "scope only from 1.4");
        assert!(
            d["r"].get("objectType").is_none(),
            "objectType only from 1.8"
        );
        assert_eq!(
            d["lp"]["type"], "Property",
            "LanguageProperty only from 1.4"
        );
        assert!(d["lp"].get("languageMap").is_none(), "reformatted away");
    }

    /// Notes 2/3 collapse the datasetId-separated instances of an Entity
    /// attribute. The temporal representation's instance array (clause 5.2.5,
    /// every instance carrying `instanceId`) is a different shape and is not
    /// covered by the table — it must survive intact.
    #[test]
    fn temporal_instance_arrays_are_not_collapsed() {
        let mut d = json!({"id": "urn:a", "type": "T",
        "speed": [
            {"type": "Property", "value": 1, "observedAt": "2020-01-01T00:00:00Z",
             "instanceId": "urn:ngsi-ld:Instance:1"},
            {"type": "Property", "value": 2, "observedAt": "2020-01-02T00:00:00Z",
             "instanceId": "urn:ngsi-ld:Instance:2"}
        ]});
        amend_entity(&mut d, (1, 0));
        assert_eq!(
            d["speed"].as_array().map(Vec::len),
            Some(2),
            "temporal instances must not collapse to one"
        );
        assert!(d["speed"][0].get("observedAt").is_none(), "observedAt <1.3");
    }

    /// Payload shapes that carry no Entity data are left alone: a document
    /// without both `id` and `type`, a scalar, and a Notification with no
    /// `data` member.
    #[test]
    fn non_entity_payloads_are_untouched() {
        let mut v = json!({"title": "BadRequestData", "status": 400});
        assert!(!amend_payload(&mut v, (1, 0)));
        let mut v = json!("just a string");
        assert!(!amend_payload(&mut v, (1, 0)));
        let mut v = json!({"id": "urn:n", "type": "Notification"});
        assert!(!amend_payload(&mut v, (1, 0)));
        let mut v = json!([]);
        assert!(!amend_payload(&mut v, (1, 0)));
        // an already-conformant entity reports no change
        let mut v = json!({"id": "urn:a", "type": "T", "p": {"type": "Property", "value": 1}});
        assert!(!amend_payload(&mut v, (1, 0)));
    }
}

/// Middleware behaviour of the `Prefer: ngsi-ld=` layer (6.3.6): what it
/// amends, what it must leave byte-identical, and the status it answers with.
#[cfg(test)]
mod prefer_layer {
    use super::*;
    use axum::http::HeaderValue;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn app(path: &'static str, payload: &'static str) -> axum::Router {
        axum::Router::new()
            .route(
                path,
                axum::routing::get(move || async move {
                    ([(header::CONTENT_TYPE, "application/json")], payload)
                }),
            )
            .layer(axum::middleware::from_fn(prefer_version_layer))
    }

    async fn get(
        app: axum::Router,
        uri: &str,
        prefer: Option<&str>,
    ) -> (StatusCode, String, String) {
        let mut b = Request::builder().uri(uri);
        if let Some(p) = prefer {
            b = b.header("Prefer", p);
        }
        let resp = app
            .oneshot(b.body(Body::empty()).expect("request"))
            .await
            .expect("response");
        let status = resp.status();
        let applied = resp
            .headers()
            .get("Preference-Applied")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_owned();
        let bytes = resp.into_body().collect().await.expect("body").to_bytes();
        (
            status,
            applied,
            String::from_utf8_lossy(&bytes).into_owned(),
        )
    }

    /// The 4.3.6.8 fallbacks describe Entity data. A Subscription (5.2.12)
    /// has no version fallbacks: its members — including `expiresAt`, which
    /// is a Subscription member since 1.0 — and its string arrays must come
    /// back exactly as served, with 200 rather than the 203 that marks an
    /// altered Entity.
    #[tokio::test(flavor = "multi_thread")]
    async fn subscription_payload_is_never_amended() {
        let sub = r#"{"id":"urn:ngsi-ld:Subscription:1","type":"Subscription","expiresAt":"2030-01-01T00:00:00Z","notification":{"attributes":["a","b"],"format":"normalized"}}"#;
        let (status, applied, body) = get(
            app("/ngsi-ld/v1/subscriptions/{id}", sub),
            "/ngsi-ld/v1/subscriptions/urn:ngsi-ld:Subscription:1",
            Some("ngsi-ld=1.0"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "not an altered Entity: {body}");
        assert_eq!(applied, "ngsi-ld=1.0");
        assert_eq!(body, sub, "subscription served byte-identical");
    }

    /// An amended Entity keeps the egress key order (`id`/`type` first) and
    /// answers 203; an Entity that needed no amendment stays byte-identical
    /// and 200.
    #[tokio::test(flavor = "multi_thread")]
    async fn amended_entity_keeps_key_order_and_reports_203() {
        let ent = r#"{"id":"urn:a","type":"T","attr":{"type":"JsonProperty","json":{"k":1}}}"#;
        let (status, applied, body) = get(
            app("/ngsi-ld/v1/entities/{id}", ent),
            "/ngsi-ld/v1/entities/urn:a",
            Some("ngsi-ld=1.4"),
        )
        .await;
        assert_eq!(status, StatusCode::NON_AUTHORITATIVE_INFORMATION);
        assert_eq!(applied, "ngsi-ld=1.4");
        assert!(body.starts_with(r#"{"id":"urn:a","type":"T","#), "{body}");
        assert!(!body.contains("JsonProperty"), "reformatted away: {body}");
        assert!(!body.contains("\"json\""), "the 1.8 member is gone: {body}");

        // native/newer preference: nothing to amend, nothing to reorder
        let (status, applied, body) = get(
            app("/ngsi-ld/v1/entities/{id}", ent),
            "/ngsi-ld/v1/entities/urn:a",
            Some("ngsi-ld=2.0"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(applied, "ngsi-ld=1.9", "the version actually conformed to");
        assert_eq!(body, ent);
    }

    /// Without the preference the layer is inert, and it never touches a
    /// non-JSON body or a non-200 response.
    #[tokio::test(flavor = "multi_thread")]
    async fn layer_is_inert_without_a_usable_preference() {
        let ent = r#"{"id":"urn:a","type":"T","attr":{"type":"JsonProperty","json":1}}"#;
        for prefer in [None, Some("body=json"), Some("ngsi-ld=junk")] {
            let (status, applied, body) = get(
                app("/ngsi-ld/v1/entities/{id}", ent),
                "/ngsi-ld/v1/entities/urn:a",
                prefer,
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            assert!(applied.is_empty(), "no preference was applied: {prefer:?}");
            assert_eq!(body, ent);
        }

        let app = axum::Router::new()
            .route(
                "/ngsi-ld/v1/entities/{id}",
                axum::routing::get(|| async {
                    (
                        StatusCode::CREATED,
                        [(header::CONTENT_TYPE, "text/plain")],
                        "not json",
                    )
                }),
            )
            .layer(axum::middleware::from_fn(prefer_version_layer));
        let (status, applied, body) =
            get(app, "/ngsi-ld/v1/entities/urn:a", Some("ngsi-ld=1.0")).await;
        assert_eq!(status, StatusCode::CREATED);
        assert!(applied.is_empty(), "non-JSON non-200 is passed through");
        assert_eq!(body, "not json");
    }

    /// A body that fails mid-stream must not be served as a complete 200.
    #[tokio::test(flavor = "multi_thread")]
    async fn broken_body_stream_is_not_a_success() {
        let app = axum::Router::new()
            .route(
                "/ngsi-ld/v1/entities/{id}",
                axum::routing::get(|| async {
                    let s = futures_util::stream::iter(vec![Err::<axum::body::Bytes, _>(
                        std::io::Error::other("upstream gone"),
                    )]);
                    Response::builder()
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from_stream(s))
                        .expect("response")
                }),
            )
            .layer(axum::middleware::from_fn(prefer_version_layer));
        let (status, _, body) = get(app, "/ngsi-ld/v1/entities/urn:a", Some("ngsi-ld=1.0")).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(
            !body.contains("upstream gone"),
            "the transport error must not reach the client: {body}"
        );
    }

    /// RFC 9110 5.3: repeated field lines are equivalent to one
    /// comma-separated list, so the version preference is honoured whichever
    /// line carries it.
    #[tokio::test(flavor = "multi_thread")]
    async fn version_preference_found_on_a_second_prefer_line() {
        let ent = r#"{"id":"urn:a","type":"T"}"#;
        let mut req = Request::builder().uri("/ngsi-ld/v1/entities/urn:a");
        if let Some(h) = req.headers_mut() {
            h.append("Prefer", HeaderValue::from_static("body=json"));
            h.append("Prefer", HeaderValue::from_static("ngsi-ld=1.5"));
        }
        let resp = app("/ngsi-ld/v1/entities/{id}", ent)
            .oneshot(req.body(Body::empty()).expect("request"))
            .await
            .expect("response");
        assert_eq!(
            resp.headers()
                .get("Preference-Applied")
                .and_then(|v| v.to_str().ok()),
            Some("ngsi-ld=1.5")
        );
    }
}
