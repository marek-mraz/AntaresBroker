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

/// One ISO 8601 duration — `P[nY][nM][nW][nD][T[nH][nM][nS]]` — read into
/// its components. Three NGSI-LD members carry this syntax and weigh it
/// differently: a Context Source registration's refresh rate (5.2.9) and an
/// EntityMap's `entityMapLifetime` (Table 6.4.3.2-1) want a span in seconds,
/// where a month is a nominal thirty days, while a temporal aggregation
/// period (4.5.19) keeps months as calendar months because that is what its
/// buckets are cut on. The scan is one function; the weighing belongs to
/// the caller.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct IsoDuration {
    /// `nY`.
    pub years: f64,
    /// `nM` before the `T` — calendar months.
    pub months: f64,
    /// `nW`.
    pub weeks: f64,
    /// `nD`.
    pub days: f64,
    /// `nH`.
    pub hours: f64,
    /// `nM` after the `T`.
    pub minutes: f64,
    /// `nS`.
    pub seconds: f64,
    /// Every component present is a plain digit run within `i64`: no
    /// fraction, no magnitude a whole-second span could not hold.
    pub whole: bool,
    /// No component at all — a bare `P`.
    pub empty: bool,
}

/// Read the syntax, in the designator order ISO 8601 fixes. `None` for
/// anything that is not it: a missing `P`, a component with no digit, a
/// designator out of order, repeated or in the wrong half, a number that
/// does not parse, digits with no designator to weigh them, or a `T` with
/// no time component after it.
pub fn parse_iso_duration(s: &str) -> Option<IsoDuration> {
    /// One half of the duration — the date designators or the time ones.
    /// `out` takes the values in `units` order; the answer is whether the
    /// half carried anything.
    fn scan(part: &str, units: &[char], out: &mut [f64], whole: &mut bool) -> Option<bool> {
        let mut p = part;
        // each designator is read at most once and in order: the search for
        // the next one starts after the last one matched
        let mut next = 0usize;
        let mut any = false;
        while !p.is_empty() {
            let i = p.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == ','))?;
            let (num, rest) = p.split_at(i);
            let unit = rest.chars().next()?;
            let slot = units.iter().skip(next).position(|u| *u == unit)? + next;
            next = slot + 1;
            if !num.bytes().any(|b| b.is_ascii_digit()) {
                return None;
            }
            *whole &= num.parse::<i64>().is_ok();
            // 4.6.3 leaves the fraction separator open, here as everywhere
            let value: f64 = num.replace(',', ".").parse().ok()?;
            if !value.is_finite() {
                return None;
            }
            out[slot] = value;
            any = true;
            p = &rest[unit.len_utf8()..];
        }
        Some(any)
    }

    let rest = s.strip_prefix('P')?;
    let (date, time) = match rest.split_once('T') {
        Some((d, t)) => (d, Some(t)),
        None => (rest, None),
    };
    let mut v = [0f64; 7];
    let mut whole = true;
    let date_any = scan(date, &['Y', 'M', 'W', 'D'], &mut v[..4], &mut whole)?;
    let time_any = match time {
        None => false,
        Some(t) => {
            // a `T` with nothing to designate is not a duration
            if !scan(t, &['H', 'M', 'S'], &mut v[4..], &mut whole)? {
                return None;
            }
            true
        }
    };
    Some(IsoDuration {
        years: v[0],
        months: v[1],
        weeks: v[2],
        days: v[3],
        hours: v[4],
        minutes: v[5],
        seconds: v[6],
        whole,
        empty: !date_any && !time_any,
    })
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

    /// One scan serves three weighings, so the syntax it accepts is the
    /// syntax all three accept: designators in ISO order, each at most once
    /// and in its own half, every component a number with a digit.
    #[test]
    fn a_duration_is_read_into_its_components() {
        let d = super::parse_iso_duration("P3Y6M4WT12H30M5.5S").expect("a duration");
        assert_eq!((d.years, d.months, d.weeks, d.days), (3.0, 6.0, 4.0, 0.0));
        assert_eq!((d.hours, d.minutes, d.seconds), (12.0, 30.0, 5.5));
        assert!(!d.whole, "a fractional component is not whole");
        assert!(!d.empty);
        // 4.6.3 leaves the fraction separator open
        assert_eq!(
            super::parse_iso_duration("PT0,5S").map(|d| d.seconds),
            Some(0.5)
        );
        // a bare P carries nothing to weigh, and is the only accepted shape
        // that carries nothing
        let bare = super::parse_iso_duration("P").expect("a bare P scans");
        assert!(bare.empty && bare.whole);
        for bad in [
            "",
            "PT",
            "P1DT", // a T with no time component after it
            "1Y",
            "P1",
            "PT1H1", // digits with no designator
            "P1X",
            "p1d",
            "P-1D",
            "P+1D",
            "P 1D",
            "PT1H ",
            " PT1H",
            "1PD",
            "P1H",
            "PT1D", // a designator in the wrong half
            "P1D2M",
            "PT1S1S", // out of order, and repeated
            "P,D",
            "P.D",
            "P..D",
            "P1.2.3D", // not a number
            "P\u{661}D",
            "P1D\u{0}",
        ] {
            assert_eq!(super::parse_iso_duration(bad), None, "{bad:?}");
        }
    }

    /// `whole` is what an EntityMap lifetime asks: the component is a plain
    /// digit run an `i64` of seconds can still hold.
    #[test]
    fn a_magnitude_past_i64_is_not_whole() {
        for s in ["PT99999999999999999999S", "PT1.5S", "PT1,5S"] {
            let d = super::parse_iso_duration(s).expect("it scans");
            assert!(!d.whole, "{s:?}");
        }
        for s in ["PT9223372036854775807S", "P0D", "PT0S"] {
            assert!(
                super::parse_iso_duration(s).expect("it scans").whole,
                "{s:?}"
            );
        }
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
