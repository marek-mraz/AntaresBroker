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
    pub const DEFAULT: &'static str = "default";

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

/// Entity id: a URI per CIM 009 (clause 4.5.1 / Annex A).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct EntityId(String);

impl EntityId {
    pub fn new(raw: &str) -> Result<Self, NgsiError> {
        // Lazy URI check: a scheme followed by ':'. Full IRI validation happens
        // during JSON-LD expansion; this guards the id-shaped entry points.
        // Rejected, so that no id reaches storage, a Location header or a
        // downstream log/UI carrying something a reader cannot see:
        //   - controls, DEL and every whitespace character (CRLF injection,
        //     and RFC 3986 admits no space anywhere in a URI);
        //   - the ASCII characters RFC 3986 excludes from a URI — `<script>`,
        //     quotes, backslash, backtick, braces, pipe, caret;
        //   - the invisible and bidi-control characters (Unicode Cf, Zl, Zp).
        //     RFC 3987 lets an IRI carry non-ASCII characters
        //     (`urn:ngsi-ld:Ciudad:París` is a legal object id), so the id is
        //     not restricted to the RFC 3986 ASCII repertoire; but the ucschar
        //     production still admits U+200B, U+202E, U+FEFF and the tag
        //     block, which render as nothing or reverse the text around them.
        //     Two ids can then render identically and a log line can be
        //     rewritten by the id it carries, so they are listed out here.
        const NON_URI_ASCII: &str = "\"<>\\^`{}|";
        let no_illegal = !raw.chars().any(|c| {
            c.is_control()
                || c.is_whitespace()
                || c == '\u{7f}'
                || NON_URI_ASCII.contains(c)
                || matches!(c,
                    '\u{00ad}'                // soft hyphen
                    | '\u{061c}'              // arabic letter mark
                    | '\u{180e}'              // mongolian vowel separator
                    | '\u{200b}'..='\u{200f}' // zero-width space/joiners, bidi marks
                    | '\u{2028}'..='\u{202e}' // line/paragraph separator, bidi overrides
                    | '\u{2060}'..='\u{206f}' // word joiner, invisible operators
                    | '\u{feff}'              // byte-order mark
                    | '\u{fff9}'..='\u{fffb}' // interlinear annotation
                    | '\u{e0000}'..='\u{e007f}' // tag characters
                )
        });
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

    /// 4.5.1: "id" shall be a URI. Invisible and bidi-control characters pass
    /// a Unicode-category control test (they are Cf/Zl/Zp, not Cc) and are
    /// inside the RFC 3987 ucschar ranges, yet a reader cannot see them — an
    /// id that renders as another id spoofs logs, UIs and audit trails. The
    /// ASCII characters RFC 3986 excludes from a URI go the same way.
    #[test]
    fn entity_id_rejects_non_uri_characters() {
        for bad in [
            "urn:x\u{202e}gpj.exe", // bidi override: spoofs the id in logs/UIs
            "urn:x\u{200b}y",       // zero-width space
            "urn:x\u{feff}y",       // BOM
            "urn:x\u{2028}y",       // line separator
            "urn:x\u{2029}y",       // paragraph separator
            "urn:x\u{00a0}y",       // no-break space
            "urn:x\u{2060}y",       // word joiner
            "urn:x\u{00ad}y",       // soft hyphen
            "urn:x\u{061c}y",       // arabic letter mark: bidi control
            "urn:x\u{180e}y",       // mongolian vowel separator
            "urn:x\u{e0001}y",      // language tag
            "urn:x\u{e0041}y",      // tag latin capital A
            "urn:x<script>",
            "urn:x\"y",
            "urn:x`y",
            "urn:x\\y",
            "urn:x^y",
            "urn:x|y",
            "urn:x{y}",
        ] {
            assert!(EntityId::new(bad).is_err(), "should reject {bad:?}");
        }
        // the whole RFC 3986 repertoire stays legal
        assert!(EntityId::new("https://ex.org/a-b_c.d~e/f?g=h&i#j%20k[l]@m!$'()*+,;=").is_ok());
        // and so do the non-ASCII characters RFC 3987 admits in an IRI: the
        // suite's own Relationship objects carry them
        assert!(EntityId::new("urn:ngsi-ld:Ciudad:París").is_ok());
        assert!(EntityId::new("urn:ngsi-ld:城市:1").is_ok());
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
