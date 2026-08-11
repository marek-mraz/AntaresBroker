//! JSON-LD layer (docs/deep-analysis.md §6.3) — hand-rolled NGSI-LD-subset
//! processor (risk #1 escape hatch): context processing, expansion with
//! structural validation, compaction, caching loader with pinned core
//! contexts.
#![cfg_attr(test, allow(clippy::unwrap_used))]

pub mod compact;
pub mod context;
pub mod expand;
pub mod loader;
#[cfg(target_arch = "wasm32")]
pub(crate) mod minicache;

pub use compact::{compact_entity, compact_entity_shallow, compact_types};
pub use context::{Context, DEFAULT_VOCAB, NGSI_LD_BASE};
pub use expand::{
    expand_attr_fragment, expand_entity, expand_types, is_deletion_instance, is_ngsi_null,
    is_ngsi_null_langmap, parse_datetime, valid_scope_value, ExpandOpts,
};
pub use loader::{
    client_builder, http_interaction, io_deadline, with_timeouts, wrap_client, CtxUsage,
    EgressPolicy, HttpClient, Loader, CORE_CONTEXT, MAX_REDIRECTS,
};
