// SPDX-License-Identifier: EUPL-1.2
//! Subscription matching (CIM 009 5.8.6) against one in-memory entity: the
//! predicates the broker's notification path applies, usable without a
//! broker — a gateway or an edge component can answer "would this change
//! notify subscription X" with the broker's own semantics.
//!
//! Inputs are the stored (internal, expanded) forms: the subscription
//! document as created (5.2.12, selector entity types expanded) and the entity document as the broker
//! stores it (expanded attribute IRIs, `type` as an array). Matching is
//! index-shaped in the broker (candidate lookup by (tenant, type) /
//! (tenant, watched attribute)); every predicate here evaluates one
//! candidate self-contained.
#![cfg_attr(not(test), warn(clippy::expect_used))]
#![deny(missing_docs)]

use antares_jsonld::Context;
use antares_model::dt_key;
use antares_ql::eval::EntityLookup;
use antares_ql::geo::GeoQuery;
use antares_ql::type_selection_matches;
use serde_json::{Map, Value};

fn sub_str<'a>(sub: &'a Value, key: &str) -> Option<&'a str> {
    sub.get(key).and_then(Value::as_str)
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// Every predicate at once: active, entities selector (5.2.33), `q` /
/// `scopeQ` / `geoQ` conditions, throttling window. `lookup` resolves 4.9
/// linked-entity terms (`attr{…}`); `&|_| None` when no store is at hand.
pub fn would_notify(sub: &Value, doc: &Value, ctx: &Context, lookup: EntityLookup) -> bool {
    is_active(sub)
        && selector_match(sub, doc, ctx)
        && conditions_match(sub, doc, ctx, lookup)
        && !throttled(sub)
}

/// 5.8.1.4 / 5.2.12: `isActive` false or an `expiresAt` in the past means
/// the subscription notifies nothing.
pub fn is_active(sub: &Value) -> bool {
    if sub.get("isActive") == Some(&Value::Bool(false)) {
        return false;
    }
    // 5.8.1.4 auto-expiry; dt_key so fraction spellings cannot misorder
    // around the boundary second (4.11)
    !sub.get("expiresAt")
        .and_then(Value::as_str)
        .is_some_and(|e| dt_key(e) < dt_key(&now_iso()))
}

/// entities selector (5.2.33) against an internal entity doc.
pub fn selector_match(sub: &Value, doc: &Value, ctx: &Context) -> bool {
    let Some(sel) = sub.get("entities").and_then(Value::as_array) else {
        return true; // watchedAttributes-only subscription
    };
    let types: Vec<&str> = doc
        .get("type")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let id = doc.get("id").and_then(Value::as_str).unwrap_or("");
    sel.iter().any(|e| {
        let t_ok = e.get("type").and_then(Value::as_str).is_none_or(|t| {
            if t.contains(['|', ',', ';', '(']) {
                type_selection_matches(t, &types, ctx)
            } else {
                types.contains(&t)
            }
        });
        // Table 5.2.33-1: id is String or String[]; "id takes precedence
        // over idPattern" — a selector carrying id ignores its idPattern.
        let id_ok = match e.get("id") {
            None => true,
            Some(Value::String(i)) => i == id,
            Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).any(|i| i == id),
            Some(_) => false,
        };
        let pat_ok = e.get("id").is_some()
            || e.get("idPattern").and_then(Value::as_str).is_none_or(|p| {
                antares_ql::regex::compile(p).is_ok_and(|re| re.find(id).is_some())
            });
        t_ok && id_ok && pat_ok
    })
}

/// A subscription's `geoQ` (Table 5.2.13-1) in the parameter shape the 4.10
/// GeoQuery parser takes. The one reading of that table: every validator,
/// matcher and forwarder in the broker turns a `geoQ` object into query
/// parameters here, so `coordinates` is spelled the same way on all of them.
pub fn geo_params(g: &Map<String, Value>) -> std::collections::HashMap<String, String> {
    let mut params: std::collections::HashMap<String, String> = Default::default();
    for k in ["georel", "geometry", "geoproperty"] {
        if let Some(s) = g.get(k).and_then(Value::as_str) {
            params.insert(k.into(), s.to_owned());
        }
    }
    if let Some(c) = g.get("coordinates") {
        params.insert(
            "coordinates".into(),
            match c {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            },
        );
    }
    params
}

/// 5.8.6 notification matching: the subscription's q (4.9), scopeQ (4.19)
/// and geoQ (4.10) conditions against an internal entity doc; all present
/// conditions must hold.
pub fn conditions_match(sub: &Value, doc: &Value, ctx: &Context, lookup: EntityLookup) -> bool {
    if let Some(q) = sub_str(sub, "q") {
        // q values in subscription bodies may be percent-encoded (4.9, 046_05)
        let q = antares_ql::percent_decode(q.as_bytes());
        // parsed once per distinct q text, not once per event per candidate
        match antares_ql::regex::q_node(&q) {
            Some(node) => {
                if !antares_ql::eval::eval_q(&node, doc, ctx, lookup) {
                    return false;
                }
            }
            None => return false,
        }
    }
    if let Some(sq) = sub_str(sub, "scopeQ") {
        if !antares_ql::scope::scope_matches(sq, doc) {
            return false;
        }
    }
    if let Some(g) = sub.get("geoQ").and_then(Value::as_object) {
        // the geometry parse is shared per distinct geoQ member; the
        // serialization of the stored member is the key
        let key = serde_json::to_string(g).unwrap_or_default();
        let gq = antares_ql::regex::geo_query(&key, || {
            GeoQuery::from_params(&geo_params(g)).ok().flatten()
        });
        match gq {
            Some(gq) => {
                if !gq.matches(doc, ctx) {
                    return false;
                }
            }
            None => return false,
        }
    }
    true
}

/// 5.2.12 `throttling`: true while the last notification is younger than the
/// throttling window, so no further notification is due yet.
pub fn throttled(sub: &Value) -> bool {
    let Some(secs) = sub.get("throttling").and_then(Value::as_f64) else {
        return false;
    };
    let Some(last) = sub
        .get("notification")
        .and_then(|n| n.get("lastNotification"))
        .and_then(Value::as_str)
    else {
        return false;
    };
    chrono::DateTime::parse_from_rfc3339(last).is_ok_and(|t| {
        (chrono::Utc::now() - t.with_timezone(&chrono::Utc)).num_milliseconds()
            < (secs * 1000.0) as i64
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Arc;

    const DC: &str = "https://uri.etsi.org/ngsi-ld/default-context";

    /// The core context once per process, with no loader behind it: the
    /// interpreter-run of these tests (Miri) must not pay for a cache and an
    /// HTTP client it never uses.
    fn ctx() -> Arc<Context> {
        static CORE: std::sync::OnceLock<Arc<Context>> = std::sync::OnceLock::new();
        CORE.get_or_init(|| Arc::new(antares_jsonld::core_context()))
            .clone()
    }

    /// A stored entity in its internal form: the broker's own expansion of
    /// the API payload (expanded IRIs, `type` as an array, instance arrays).
    fn expand(doc: Value) -> Value {
        antares_jsonld::expand_entity(
            doc.as_object().expect("object"),
            &ctx(),
            antares_jsonld::ExpandOpts::default(),
        )
        .expect("a valid entity")
    }

    fn vehicle(speed: f64, lon: f64) -> Value {
        expand(json!({
            "id": "urn:ngsi-ld:Vehicle:A1",
            "type": "Vehicle",
            "speed": {"type": "Property", "value": speed},
            "driver": {"type": "Relationship", "object": "urn:ngsi-ld:Person:P1"},
            "location": {"type": "GeoProperty",
                "value": {"type": "Point", "coordinates": [lon, 40.4]}}
        }))
    }

    /// A subscription as the broker stores it: entity types in the selector
    /// are already expanded (5.2.33), `q`/`geoQ` as sent.
    fn sub() -> Value {
        json!({
            "id": "urn:ngsi-ld:Subscription:1",
            "type": "Subscription",
            "entities": [{"type": format!("{DC}/Vehicle"), "idPattern": "^urn:ngsi-ld:Vehicle:.*"}],
            "q": "speed>25",
            "geoQ": {"georel": "near;maxDistance==2000", "geometry": "Point",
                     "coordinates": "[-3.7,40.4]"},
            "notification": {"endpoint": {"uri": "http://x/n"}}
        })
    }

    /// 5.8.6: every predicate holds → the change would notify.
    #[test]
    fn matching_change_notifies() {
        assert!(would_notify(&sub(), &vehicle(30.0, -3.7), &ctx(), &|_| {
            None
        }));
    }

    /// Each predicate alone refuses: q (4.9), geoQ (4.10), selector type,
    /// idPattern (5.2.33), `isActive`, `expiresAt` (5.8.1.4), throttling.
    #[test]
    fn each_failing_predicate_refuses() {
        let c = ctx();
        let none = |_: &str| None;
        assert!(!would_notify(&sub(), &vehicle(20.0, -3.7), &c, &none), "q");
        assert!(
            !would_notify(&sub(), &vehicle(30.0, -4.7), &c, &none),
            "geoQ"
        );
        let mut s = sub();
        s["entities"][0]["type"] = json!(format!("{DC}/Bicycle"));
        assert!(!would_notify(&s, &vehicle(30.0, -3.7), &c, &none), "type");
        let mut s = sub();
        s["entities"][0]["idPattern"] = json!("^urn:ngsi-ld:Bicycle:.*");
        assert!(
            !would_notify(&s, &vehicle(30.0, -3.7), &c, &none),
            "idPattern"
        );
        let mut s = sub();
        s["isActive"] = json!(false);
        assert!(
            !would_notify(&s, &vehicle(30.0, -3.7), &c, &none),
            "isActive"
        );
        let mut s = sub();
        s["expiresAt"] = json!("2000-01-01T00:00:00Z");
        assert!(
            !would_notify(&s, &vehicle(30.0, -3.7), &c, &none),
            "expiresAt"
        );
        let mut s = sub();
        s["throttling"] = json!(3600);
        s["notification"]["lastNotification"] = json!(now_iso());
        assert!(
            !would_notify(&s, &vehicle(30.0, -3.7), &c, &none),
            "throttling"
        );
        assert!(throttled(&s));
        s["notification"]["lastNotification"] = json!("2000-01-01T00:00:00Z");
        assert!(
            !throttled(&s),
            "an old lastNotification is outside the window"
        );
    }

    /// 5.2.33 Table 5.2.33-1: `id` takes precedence over `idPattern`, and a
    /// selector without `type` matches every type.
    #[test]
    fn selector_id_precedence_and_typeless_selector() {
        let c = ctx();
        let doc = vehicle(30.0, -3.7);
        let s = json!({"entities": [{"id": "urn:ngsi-ld:Vehicle:A1", "idPattern": "^nothing$"}]});
        assert!(selector_match(&s, &doc, &c));
        let s = json!({"entities": [{"id": ["urn:ngsi-ld:Vehicle:B2"], "type": format!("{DC}/Vehicle")}]});
        assert!(!selector_match(&s, &doc, &c));
        let s = json!({"watchedAttributes": ["speed"]});
        assert!(
            selector_match(&s, &doc, &c),
            "watchedAttributes-only subscription"
        );
    }

    /// 4.9 EXAMPLE 13/14: a linked-entity term (`driver{name}`) resolves the
    /// Relationship object through `lookup`; without a store it cannot match.
    #[test]
    fn linked_entity_term_uses_the_lookup() {
        let c = ctx();
        let mut s = sub();
        s["q"] = json!("driver{name}==\"Ann\"");
        let doc = vehicle(30.0, -3.7);
        let store = |uri: &str| {
            (uri == "urn:ngsi-ld:Person:P1").then(|| {
                expand(json!({"id": uri, "type": "Person",
                       "name": {"type": "Property", "value": "Ann"}}))
            })
        };
        assert!(conditions_match(&s, &doc, &c, &store));
        assert!(
            !conditions_match(&s, &doc, &c, &|_| None),
            "no store, no match"
        );
    }

    /// 4.19: `scopeQ` is a condition like `q`; a percent-encoded `q` (as a
    /// subscription body may carry it) decodes before evaluation.
    #[test]
    fn scope_q_and_percent_encoded_q() {
        let c = ctx();
        let mut doc = vehicle(30.0, -3.7);
        doc["scope"] = json!(["/Madrid/Centre"]);
        let mut s = sub();
        s["scopeQ"] = json!("/Madrid/#");
        s["q"] = json!("speed%3E25");
        assert!(conditions_match(&s, &doc, &c, &|_| None));
        s["scopeQ"] = json!("/Paris/#");
        assert!(!conditions_match(&s, &doc, &c, &|_| None));
    }
}
