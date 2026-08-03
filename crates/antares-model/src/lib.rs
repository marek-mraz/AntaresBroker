//! NGSI-LD data model (ETSI CIM 009 V1.9.1).
//!
//! Shapes and invariants only: no I/O, no clocks, no config
//! (see docs/deep-analysis.md §9.3).

pub mod error;
pub mod id;

pub use error::{NgsiError, ProblemDetails};
pub use id::{EntityId, TenantId};

/// The NGSI-LD core @context URL this broker targets.
pub const CORE_CONTEXT_URL: &str =
    "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";

/// API root path (CIM 009 clause 6.2).
pub const API_ROOT: &str = "/ngsi-ld/v1";
