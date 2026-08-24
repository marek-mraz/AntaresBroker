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
