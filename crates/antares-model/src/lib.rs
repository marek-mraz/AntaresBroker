// SPDX-License-Identifier: EUPL-1.2
//! NGSI-LD data model (ETSI CIM 009 V1.9.1).
//!
//! Shapes and invariants only: no I/O, no clocks, no config.
#![deny(missing_docs)]

pub mod error;
pub mod id;

pub use error::{NgsiError, ProblemDetails};
pub use id::{EntityId, TenantId};

/// The NGSI-LD core @context URL this broker targets.
pub const CORE_CONTEXT_URL: &str =
    "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";

/// API root path (CIM 009 clause 6.2).
pub const API_ROOT: &str = "/ngsi-ld/v1";

/// Egress key order for a served payload: every object serializes `id`
/// then `type` first, recursively (an attribute object leads with
/// `"type": "Property"`, a GeoJSON Feature with `id`/`type` — the order the
/// spec's own examples print). Cosmetic only: RFC 8259 objects are unordered
/// and CIM 009 4.5.1 mandates presence, not position. Applied ONLY at egress
/// (responses and notifications) — internal serialization (storage, temporal
/// diff) stays byte-stable alphabetical and must not use this.
pub struct SpecOrder<'a>(pub &'a serde_json::Value);

impl serde::Serialize for SpecOrder<'_> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        use serde_json::Value;
        match self.0 {
            Value::Object(m) => {
                let mut map = s.serialize_map(Some(m.len()))?;
                for k in ["id", "type"] {
                    if let Some(v) = m.get(k) {
                        map.serialize_entry(k, &SpecOrder(v))?;
                    }
                }
                for (k, v) in m {
                    if k != "id" && k != "type" {
                        map.serialize_entry(k, &SpecOrder(v))?;
                    }
                }
                map.end()
            }
            Value::Array(a) => s.collect_seq(a.iter().map(SpecOrder)),
            other => other.serialize(s),
        }
    }
}

/// Serialize a response or notification payload in egress key order
/// (serializing a `Value` cannot fail).
pub fn ordered_vec(v: &serde_json::Value) -> Vec<u8> {
    serde_json::to_vec(&SpecOrder(v)).unwrap_or_default()
}

/// Canonical lexicographic comparison key for a 4.6.3 DateTime: the trailing
/// `Z` dropped and the optional seconds fraction (`.` or the request-side `,`
/// separator) zero-padded to six digits, so string order equals temporal
/// order across spellings of the same instant. Non-DateTime input is
/// returned as-is (callers validated at write/parse time).
pub fn dt_key(s: &str) -> String {
    let Some(body) = s.strip_suffix('Z') else {
        return s.to_owned();
    };
    if !body.is_char_boundary(19) {
        return s.to_owned();
    }
    let (base, frac) = body.split_at(19);
    let digits = frac
        .strip_prefix('.')
        .or_else(|| frac.strip_prefix(','))
        .unwrap_or("");
    format!("{base}.{digits:0<6}")
}
