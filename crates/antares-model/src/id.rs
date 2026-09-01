// SPDX-License-Identifier: EUPL-1.2
//! Validated id newtypes. Tenant scoping is threaded through the type system:
//! store methods take `&TenantId` as their first parameter.

use crate::error::NgsiError;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Tenant identifier from the `NGSILD-Tenant` header.
///
/// Token-safe by construction (also used as a NATS subject segment):
/// `[A-Za-z0-9_-]{1,64}`. The default tenant is `"default"`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct TenantId(String);

impl TenantId {
    /// The default tenant name, used when no `NGSILD-Tenant` header is sent.
    pub const DEFAULT: &'static str = "default";

    /// Validates a tenant name: `[A-Za-z0-9_-]{1,64}`, else BadRequestData.
    pub fn new(raw: &str) -> Result<Self, NgsiError> {
        let ok = !raw.is_empty()
            && raw.len() <= 64
            && raw
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
        if ok {
            Ok(Self(raw.to_owned()))
        } else {
            Err(NgsiError::BadRequestData(format!(
                "invalid NGSILD-Tenant value: {raw:?}"
            )))
        }
    }

    /// The tenant name as sent in the header.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for TenantId {
    fn default() -> Self {
        Self(Self::DEFAULT.to_owned())
    }
}

impl TryFrom<String> for TenantId {
    type Error = NgsiError;
    fn try_from(s: String) -> Result<Self, NgsiError> {
        Self::new(&s)
    }
}

impl From<TenantId> for String {
    fn from(t: TenantId) -> String {
        t.0
    }
}

impl fmt::Display for TenantId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One character of an Entity id.
///
/// RFC 3986 clause 2 fixes the ASCII repertoire a URI is written in:
/// unreserved, reserved (gen-delims + sub-delims) and "%" for percent-encoding.
/// Anything else — controls, DEL, space, `"`, `<`, `>`, `\`, `^`, backtick,
/// braces, pipe — is not a URI character. CIM 009 clause 5.2.1 widens every
/// "URI" in the document to an IRI as mandated by RFC 3987, so the id is not
/// confined to ASCII: RFC 3987 clause 2.2 adds the ucschar and iprivate ranges
/// (`urn:ngsi-ld:Ciudad:París` is a legal id). Admitting exactly those ranges,
/// rather than subtracting known-bad code points, also keeps out the Unicode
/// noncharacters, the C1 controls and the plane-14 tag/variation-selector block
/// that no IRI production covers.
///
/// The ucschar ranges themselves still admit characters that render as nothing
/// or reorder the text around them — the bidi controls, the zero-width and
/// format characters, and the fillers and blank patterns that Unicode classes
/// as ordinary letters or symbols yet paint no glyph. Two ids can then render
/// identically, and a log line, console or UI can be rewritten by the id it
/// carries, so those are excluded by name on top of the ranges.
fn is_id_char(c: char) -> bool {
    const URI_ASCII: &str = "-._~:/?#[]@!$&'()*+,;=%";
    let code = c as u32;
    // RFC 3987 clause 2.2 ucschar %xA0-D7FF / F900-FDCF / FDF0-FFEF and
    // iprivate %xE000-F8FF (F900 follows F8FF, so the two are one range here),
    // plus every supplementary plane except each plane's two trailing
    // noncharacters and the E0000-E0FFF block that ucschar's E1000-EFFFD skips.
    let iri_non_ascii = matches!(c,
        '\u{a0}'..='\u{d7ff}'
        | '\u{e000}'..='\u{fdcf}'
        | '\u{fdf0}'..='\u{ffef}')
        || (code >= 0x1_0000 && code & 0xffff <= 0xfffd && !(0xe_0000..=0xe_0fff).contains(&code));
    let invisible = matches!(c,
        '\u{00ad}'                // soft hyphen
        | '\u{061c}'              // arabic letter mark
        | '\u{115f}' | '\u{1160}' | '\u{3164}' | '\u{ffa0}' // hangul fillers: no glyph
        | '\u{180e}'              // mongolian vowel separator
        | '\u{200b}'..='\u{200f}' // zero-width space/joiners, bidi marks
        | '\u{2800}'              // braille pattern blank
        | '\u{2028}'..='\u{202e}' // line/paragraph separator, bidi overrides
        | '\u{2060}'..='\u{206f}' // word joiner, invisible operators, bidi isolates
        | '\u{fe00}'..='\u{fe0f}' // variation selectors
        | '\u{feff}'              // byte-order mark
        | '\u{fff9}'..='\u{fffb}' // interlinear annotation
    );
    (c.is_ascii_alphanumeric() || URI_ASCII.contains(c) || iri_non_ascii)
        && !c.is_whitespace()
        && !invisible
}

/// Entity id: a valid URI per CIM 009 clause 4.5.1 and Table 5.2.4-1, where
/// "URI" also means an IRI per clause 5.2.1. Invalid → BadRequestData, which
/// Table 6.3.2-1 maps to 400.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct EntityId(String);

impl EntityId {
    /// Validates an entity id as a URI/IRI (4.5.1, 5.2.1): a non-empty scheme,
    /// only URI-legal characters and no `.`/`..` path segment; else BadRequestData.
    pub fn new(raw: &str) -> Result<Self, NgsiError> {
        // Lazy URI check: a scheme followed by ':', over characters a URI or
        // IRI is allowed to contain. Full IRI validation happens during
        // JSON-LD expansion; this guards the id-shaped entry points, so that
        // no id reaches storage, a Location header or a downstream log/UI
        // carrying something a reader cannot see.
        let no_illegal = raw.chars().all(is_id_char);
        // An entity id is interpolated into the path of a forwarded request,
        // so a dot-segment in it climbs out of /entities/{id} and addresses a
        // different resource on the peer. Slashes stay legal (an http-scheme
        // id has them); only "." and ".." as whole segments are refused, in
        // their percent-encoded spellings too, since the peer decodes the path
        // it receives. RFC 3986 clause 3.3.
        let no_dot_segment = {
            let decoded = raw
                .to_ascii_lowercase()
                .replace("%2e", ".")
                .replace("%2f", "/");
            !decoded.split('/').any(|seg| seg == "." || seg == "..")
        };
        let scheme_ok = no_illegal
            && no_dot_segment
            && raw.split_once(':').is_some_and(|(s, rest)| {
                !s.is_empty()
                    && !rest.is_empty()
                    && s.chars()
                        .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
            });
        if scheme_ok {
            Ok(Self(raw.to_owned()))
        } else {
            Err(NgsiError::BadRequestData(format!(
                "entity id is not a valid URI: {raw:?}"
            )))
        }
    }

    /// The id as its original URI string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for EntityId {
    type Error = NgsiError;
    fn try_from(s: String) -> Result<Self, NgsiError> {
        Self::new(&s)
    }
}

impl From<EntityId> for String {
    fn from(e: EntityId) -> String {
        e.0
    }
}

impl fmt::Display for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tenant_accepts_token_safe() {
        assert!(TenantId::new("city-01_A").is_ok());
        assert_eq!(TenantId::default().as_str(), "default");
    }

    #[test]
    fn tenant_rejects_unsafe() {
        for bad in ["", "a.b", "a b", "ü", &"x".repeat(65)] {
            assert!(TenantId::new(bad).is_err(), "should reject {bad:?}");
        }
    }

    #[test]
    fn entity_id_requires_uri() {
        assert!(EntityId::new("urn:ngsi-ld:Vehicle:A123").is_ok());
        assert!(EntityId::new("not a uri").is_err());
        assert!(EntityId::new(":noscheme").is_err());
    }

    /// The id lands in the path of a forwarded request, so a dot-segment in it
    /// reaches a different resource on the peer: an id of
    /// `urn:a/../../csourceRegistrations` turns a write on one entity into a
    /// write on the peer's registration collection.
    #[test]
    fn entity_id_rejects_dot_segments() {
        for bad in [
            "urn:a/../../csourceRegistrations",
            "urn:a/..",
            "urn:a/./b",
            "urn:a/%2e%2e/b",
            "urn:a/%2E%2E/b",
            "urn:a%2f..%2fb",
        ] {
            assert!(EntityId::new(bad).is_err(), "should reject {bad:?}");
        }
        // slashes and dots that are not a whole segment stay legal: an
        // http-scheme id has a path, and versioned names carry dots
        for ok in [
            "http://example.org/entities/1",
            "urn:ngsi-ld:Vehicle:A1.2",
            "http://example.org/v1.0/e..1",
        ] {
            assert!(EntityId::new(ok).is_ok(), "should accept {ok:?}");
        }
    }

    #[test]
    fn entity_id_rejects_control_chars_and_space() {
        assert!(EntityId::new("urn:has space").is_err());
        assert!(EntityId::new("urn:x\r\nX-Injected:1").is_err());
        assert!(EntityId::new("urn:x\ttab").is_err());
        assert!(EntityId::new("urn:x\u{7f}").is_err());
        assert!(EntityId::new("urn:ngsi-ld:ok-1").is_ok());
    }

    /// 4.5.1 and Table 5.2.4-1: "id" shall be a valid URI, and clause 5.2.1
    /// widens every "URI" in the document to an IRI (RFC 3987). Invisible and
    /// bidi-control characters pass a Unicode-category control test (they are
    /// Cf/Mn/Zl/Zp/Lo/So, not Cc) and most sit inside the RFC 3987 ucschar ranges,
    /// yet a reader cannot see them — an id that renders as another id spoofs
    /// logs, UIs and audit trails. Each is rejected by name, with the error
    /// type Table 6.3.2-1 mandates: BadRequestData, 400, never a 500.
    #[test]
    fn entity_id_rejects_non_uri_characters() {
        for (bad, what) in [
            ("urn:x\u{202e}gpj.exe", "U+202E right-to-left override"),
            ("urn:x\u{202d}y", "U+202D left-to-right override"),
            ("urn:x\u{2066}y", "U+2066 left-to-right isolate"),
            ("urn:x\u{200b}y", "U+200B zero width space"),
            ("urn:x\u{200e}y", "U+200E left-to-right mark"),
            ("urn:x\u{feff}y", "U+FEFF byte-order mark"),
            ("urn:x\u{2028}y", "U+2028 line separator"),
            ("urn:x\u{2029}y", "U+2029 paragraph separator"),
            ("urn:x\u{00a0}y", "U+00A0 no-break space"),
            ("urn:x\u{2060}y", "U+2060 word joiner"),
            ("urn:x\u{00ad}y", "U+00AD soft hyphen"),
            ("urn:x\u{061c}y", "U+061C arabic letter mark"),
            ("urn:x\u{180e}y", "U+180E mongolian vowel separator"),
            ("urn:x\u{fe0f}y", "U+FE0F variation selector 16"),
            ("urn:x\u{e0100}y", "U+E0100 variation selector 17"),
            ("urn:x\u{e0001}y", "U+E0001 language tag"),
            ("urn:x\u{e0041}y", "U+E0041 tag latin capital A"),
            ("urn:x\u{115f}y", "U+115F hangul choseong filler"),
            ("urn:x\u{1160}y", "U+1160 hangul jungseong filler"),
            ("urn:x\u{3164}y", "U+3164 hangul filler"),
            ("urn:x\u{ffa0}y", "U+FFA0 halfwidth hangul filler"),
            ("urn:x\u{2800}y", "U+2800 braille pattern blank"),
            ("urn:x\u{fdd0}y", "U+FDD0 noncharacter"),
            ("urn:x\u{fffe}y", "U+FFFE noncharacter"),
            ("urn:x\u{1fffe}y", "U+1FFFE noncharacter"),
            ("urn:x<script>", "angle brackets"),
            ("urn:x\"y", "double quote"),
            ("urn:x`y", "backtick"),
            ("urn:x\\y", "backslash"),
            ("urn:x^y", "caret"),
            ("urn:x|y", "pipe"),
            ("urn:x{y}", "braces"),
        ] {
            let e = EntityId::new(bad).expect_err(what);
            assert_eq!(e.kind(), "BadRequestData", "{what} must be 400 data error");
            assert_eq!(e.status(), 400, "{what}");
        }
    }

    /// The over-tightening guard for the character predicate: RFC 3986 admits
    /// its whole ASCII repertoire including percent-encoding, and clause 5.2.1
    /// admits the non-ASCII characters of an IRI (RFC 3987 clause 2.2), so none
    /// of these may be refused.
    #[test]
    fn entity_id_accepts_uri_and_iri_forms() {
        for ok in [
            "urn:ngsi-ld:Vehicle:A123",
            "urn:ngsi-ld:Vehicle:A%20B",      // percent-encoded space
            "urn:ngsi-ld:Vehicle:%E2%82%AC1", // percent-encoded UTF-8
            "http://example.org/entities/%E2%82%AC", // percent-encoded in a path
            "https://ex.org/a-b_c.d~e/f?g=h&i#j%20k[l]@m!$'()*+,;=",
            // the non-ASCII characters RFC 3987 admits in an IRI: the suite's
            // own Relationship objects carry them
            "urn:ngsi-ld:Ciudad:París",
            "urn:ngsi-ld:城市:1",
            "urn:ngsi-ld:Δήμος:1",
            "urn:ngsi-ld:Vehicle:🚗1", // plane 1, inside ucschar
        ] {
            assert!(EntityId::new(ok).is_ok(), "should accept {ok:?}");
        }
    }

    /// The rejection message must not echo the id back unescaped — a
    /// rejected id lands in logs, so any control byte stays quoted.
    #[test]
    fn entity_id_rejection_message_is_escaped() {
        let e = EntityId::new("urn:x\r\nX-Injected:1").expect_err("rejected");
        let msg = e.to_string();
        assert!(!msg.contains('\r') && !msg.contains('\n'), "{msg}");
    }

    /// Deserialization is an entry point of its own: change events arriving
    /// off the bus are turned into these types by serde, and `try_from`
    /// routes that through the same validation the HTTP path uses.
    #[test]
    fn deserialization_validates_and_serialization_stays_a_bare_string() {
        assert!(serde_json::from_str::<TenantId>("\"a.b\"").is_err());
        assert!(serde_json::from_str::<TenantId>("\"\"").is_err());
        assert!(serde_json::from_str::<EntityId>("\"urn:x\\u202ey\"").is_err());
        assert!(serde_json::from_str::<EntityId>("\"noscheme\"").is_err());
        let id: EntityId = serde_json::from_str("\"urn:ngsi-ld:Vehicle:A1\"").expect("valid");
        assert_eq!(
            serde_json::to_string(&id).expect("serialize"),
            "\"urn:ngsi-ld:Vehicle:A1\"",
            "the newtype must not add a wrapper object"
        );
    }

    /// A tenant is also the first half of the `file`-mode redb key
    /// (`tenant \0 id`, `antares-sql store/mem/redb.rs`), whose split takes
    /// the FIRST NUL. A separator or control byte in a tenant name would make
    /// two different (tenant, id) pairs one key, and one tenant's document
    /// would be written over another's.
    #[test]
    fn tenant_rejects_the_file_mode_key_separator() {
        for bad in ["a\0b", "\0", "a\nb", "a\tb", "a/b", "a:b"] {
            assert!(TenantId::new(bad).is_err(), "should reject {bad:?}");
        }
    }

    /// A tenant travels verbatim as a NATS subject token, so the wildcard
    /// and separator characters must never pass validation.
    #[test]
    fn tenant_rejects_subject_metacharacters() {
        for bad in ["a.b", "*", ">", "a>b", "a*"] {
            assert!(TenantId::new(bad).is_err(), "should reject {bad:?}");
        }
        assert!(TenantId::new(&"x".repeat(64)).is_ok(), "64 is the boundary");
    }
}
