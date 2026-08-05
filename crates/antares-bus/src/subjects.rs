//! Subject encoding (§6.4/§9.1): `changes.{tenant}.{type_hash}.{id_hash}`.
//!
//! Entity types and ids are IRIs/URNs containing `.` and `:` — illegal or
//! ambiguous as NATS subject tokens — so both segments are FNV-1a 64 hashes
//! in hex. Tenant names are validated token-safe at creation (`TenantId`), so
//! the tenant travels verbatim and consumers can filter `changes.{tenant}.>`.
//! FNV-1a is spelled out here because it must stay bit-stable across Rust
//! releases (std's DefaultHasher is not) — a subject is a wire contract.

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
pub fn change_subject(tenant: &str, first_type: &str, entity_id: &str) -> String {
    format!(
        "changes.{tenant}.{:016x}.{:016x}",
        fnv1a64(first_type.as_bytes()),
        fnv1a64(entity_id.as_bytes())
    )
}

/// Registration CUD deltas (§7 `ANTARES_REGISTRY`): broadcast, per tenant.
pub fn registry_subject(tenant: &str) -> String {
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

    #[test]
    fn subject_tokens_never_carry_iri_punctuation() {
        let s = change_subject(
            "acme",
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
}
