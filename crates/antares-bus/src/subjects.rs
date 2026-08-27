// SPDX-License-Identifier: EUPL-1.2
//! Subject encoding: `changes.{tenant}.{type_hash}.{id_hash}`.
//!
//! Entity types and ids are IRIs/URNs containing `.` and `:` — illegal or
//! ambiguous as NATS subject tokens — so both segments are FNV-1a 64 hashes
//! in hex. Tenant names are validated token-safe at creation (`TenantId`), so
//! the tenant travels verbatim and consumers can filter `changes.{tenant}.>`.
//! FNV-1a is spelled out here because it must stay bit-stable across Rust
//! releases (std's DefaultHasher is not) — a subject is a wire contract.
//!
//! The tenant is taken as a `TenantId`, not a `&str`: it is the only segment
//! that is not hashed, so the validated newtype is what keeps a `.`, a `*` or
//! a `>` out of the subject.

use antares_model::TenantId;

/// FNV-1a 64 (public-domain constants). Stable forever by construction.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// The subject one `ChangeEvent` publishes to.
pub fn change_subject(tenant: &TenantId, first_type: &str, entity_id: &str) -> String {
    format!(
        "changes.{tenant}.{:016x}.{:016x}",
        fnv1a64(first_type.as_bytes()),
        fnv1a64(entity_id.as_bytes())
    )
}

/// Registration CUD deltas (`ANTARES_REGISTRY`): broadcast, per tenant.
pub fn registry_subject(tenant: &TenantId) -> String {
    format!("registry.{tenant}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_the_published_fnv1a_vector() {
        // FNV-1a 64 test vectors ("" and "a") from the reference spec
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
    }

    fn tenant(raw: &str) -> TenantId {
        TenantId::new(raw).expect("token-safe tenant")
    }

    #[test]
    fn subject_tokens_never_carry_iri_punctuation() {
        let s = change_subject(
            &tenant("acme"),
            "https://uri.etsi.org/ngsi-ld/default-context/Vehicle",
            "urn:ngsi-ld:Vehicle:A1",
        );
        let mut parts = s.split('.');
        assert_eq!(parts.next(), Some("changes"));
        assert_eq!(parts.next(), Some("acme"));
        for token in parts {
            assert!(!token.is_empty() && token.chars().all(|c| c.is_ascii_hexdigit()));
        }
        assert_eq!(s.split('.').count(), 4);
    }

    /// The tenant is the one segment that travels verbatim, so it must not be
    /// able to add tokens or a `>`/`*` wildcard to the subject. The subject
    /// builders take a `TenantId`, which is the only way to construct one, so
    /// the escape is refused at the type level and again at validation.
    #[test]
    fn a_hostile_tenant_cannot_escape_the_subject_encoding() {
        for hostile in ["a.b.>", ">", "*", "a b", "a.b"] {
            assert!(TenantId::new(hostile).is_err(), "should reject {hostile:?}");
        }
        let s = change_subject(&tenant("a-b_1"), "T", "urn:x:1");
        assert_eq!(s.split('.').count(), 4, "tenant must stay one token: {s}");
        assert!(!s.contains('>') && !s.contains('*'), "no wildcards: {s}");
        assert_eq!(registry_subject(&tenant("a-b_1")), "registry.a-b_1");
    }

    /// Hostile types and ids never reach the wire: both are hashed, so
    /// separators and wildcards cannot re-shape the subject either.
    #[test]
    fn hostile_types_and_ids_stay_hashed() {
        let s = change_subject(&tenant("acme"), "*.>", "urn:x:1.>.*\r\n");
        assert_eq!(s.split('.').count(), 4);
        assert!(s.starts_with("changes.acme."));
        assert!(s
            .split('.')
            .skip(2)
            .all(|t| t.len() == 16 && t.chars().all(|c| c.is_ascii_hexdigit())));
    }
}
