// SPDX-License-Identifier: EUPL-1.2
//! NGSI-LD data model (ETSI CIM 009 V1.9.1).
//!
//! Shapes and invariants only: no I/O, no clocks, no config.
#![cfg_attr(not(test), warn(clippy::expect_used))]
#![deny(missing_docs)]

pub mod error;
pub mod id;
pub mod operations;

pub use error::{NgsiError, ProblemDetails};
pub use id::{EntityId, TenantId};

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

/// 5.2.4 Entity, Table 5.2.4-1, with the common members of Table 5.2.2-1:
/// the members of an Entity document that are not Attributes. Every other
/// member is a Property or a Relationship (`location` and the two other
/// default GeoProperties included), so this list is what every layer that
/// has to tell an attribute from an entity member reads — the query
/// projection, the notification diff, the temporal split, the outbox event.
/// A layer with its own copy is a layer that will disagree with the others
/// about what an attribute is.
///
/// `@context` is in the list because a stored or rendered document carries
/// it and it is not an Attribute either. It is the one member Table 5.2.4-1
/// does not name, which is why [`is_meta`] — the Entity's OWN members, the
/// question 5.2.4 asks — leaves it out.
pub const ENTITY_META_KEYS: &[&str] = &[
    "id",
    "type",
    "scope",
    "createdAt",
    "modifiedAt",
    "deletedAt",
    "expiresAt",
    "@context",
];

/// The members of an Entity that are not Attributes — `id`, `type`,
/// `scope`, `expiresAt`, `createdAt`, `modifiedAt`, `deletedAt`. See
/// [`ENTITY_META_KEYS`] for the document-level list this narrows.
pub fn is_meta(k: &str) -> bool {
    k != "@context" && ENTITY_META_KEYS.contains(&k)
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

/// Attribute names in paths must be valid terms/IRIs (4.6.2) — 400 otherwise.
pub fn check_attr_name(attr: &str) -> Result<(), NgsiError> {
    // 4.6.2 supported names: no '@' (keyword territory), no parens/quotes/etc.
    let ok = !attr.is_empty()
        && attr
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_:.#/%-+".contains(c))
        && !has_dot_segment(attr);
    if ok {
        Ok(())
    } else {
        Err(NgsiError::BadRequestData(format!(
            "invalid attribute name {attr:?}"
        )))
    }
}

/// A 4.6.2 name begins with a letter, so no valid Attribute name is a relative
/// path dot-segment (RFC 3986 clause 5.2.4). The name is interpolated into the
/// request URLs of forwarded operations, where a `.`/`..` segment addresses a
/// different resource of the registration endpoint — `/entities/{id}/attrs/..`
/// is that endpoint's Entity resource, and a URL parser resolves the segment
/// before the request leaves this process. Percent triplets are folded once
/// first, because the endpoint decodes the path it is given.
///
/// The name a client sends is checked by [`check_attr_name`], but that is not
/// the only form that reaches a path: 4.3.6.6 compacts the name again with a
/// registered `@context`, which is client-supplied and may bind any term. The
/// compacted form is held to this same rule before it is written into a
/// forwarded URL.
pub fn has_dot_segment(attr: &str) -> bool {
    attr.to_ascii_lowercase()
        .replace("%2e", ".")
        .replace("%2f", "/")
        .split('/')
        .any(|seg| seg == "." || seg == "..")
}

#[cfg(test)]
mod tests {
    use super::dt_key;
    #[test]
    fn meta_members_are_the_non_attribute_members_of_an_entity() {
        for k in [
            "id",
            "type",
            "scope",
            "createdAt",
            "modifiedAt",
            "deletedAt",
            "expiresAt",
        ] {
            assert!(super::is_meta(k), "{k}");
        }
        for k in [
            "",
            "v",
            "Type",
            "location",
            "observationSpace",
            "operationSpace",
            "speed",
            "@context",
            "observedAt",
            "datasetId",
        ] {
            assert!(!super::is_meta(k), "{k}");
        }
    }

    /// 4.6.3 DateTime: only a DateTime has a canonical key — anything else
    /// is returned unchanged, including a multi-byte string that ends in
    /// `Z` and is long enough to reach the seconds position in bytes.
    #[test]
    fn non_datetime_input_is_returned_unchanged() {
        for s in ["", "Z", "not-a-date", "ααααααααααZ", "urn:ngsi-ld:nullZ"] {
            assert_eq!(dt_key(s), s, "{s:?}");
        }
        // a real DateTime still normalizes to its comparison key
        assert_eq!(dt_key("2026-05-01T00:00:00Z"), "2026-05-01T00:00:00.000000");
        assert_eq!(
            dt_key("2026-05-01T00:00:00,5Z"),
            "2026-05-01T00:00:00.500000"
        );
    }

    /// The two views of the same list stay one list: `is_meta` answers
    /// Table 5.2.4-1's question — the Entity's OWN members — and
    /// `ENTITY_META_KEYS` answers the document's, which is the same set plus
    /// the `@context` a stored or rendered document carries.
    #[test]
    fn the_document_list_and_the_entity_list_agree() {
        for k in super::ENTITY_META_KEYS {
            assert_eq!(
                super::is_meta(k),
                *k != "@context",
                "{k} is in the document list"
            );
        }
        assert_eq!(
            super::ENTITY_META_KEYS.len(),
            8,
            "seven Entity members plus @context"
        );
        for k in [
            "location",
            "speed",
            "https://uri.etsi.org/ngsi-ld/default-context/x",
            "",
        ] {
            assert!(!super::is_meta(k), "{k:?} is an Attribute, not a member");
            assert!(!super::ENTITY_META_KEYS.contains(&k), "{k:?}");
        }
    }
}
