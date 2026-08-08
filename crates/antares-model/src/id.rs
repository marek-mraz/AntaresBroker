//! Validated id newtypes. Tenant scoping is threaded through the type system:
//! store methods take `&TenantId` as their first parameter (§9.3).

use crate::error::NgsiError;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Tenant identifier from the `NGSILD-Tenant` header.
///
/// Token-safe by construction (also used as a NATS subject segment, §7):
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
        // Control chars, DEL and spaces are illegal in a URI (RFC 3986) and are
        // rejected here so a CRLF/`<script>`/space never reaches storage, a
        // Location header, or a downstream log/UI.
        let no_illegal = !raw
            .chars()
            .any(|c| c.is_control() || c == ' ' || c == '\u{7f}');
        let scheme_ok = no_illegal
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

    #[test]
    fn entity_id_rejects_control_chars_and_space() {
        assert!(EntityId::new("urn:has space").is_err());
        assert!(EntityId::new("urn:x\r\nX-Injected:1").is_err());
        assert!(EntityId::new("urn:x\ttab").is_err());
        assert!(EntityId::new("urn:x\u{7f}").is_err());
        assert!(EntityId::new("urn:ngsi-ld:ok-1").is_ok());
    }
}
