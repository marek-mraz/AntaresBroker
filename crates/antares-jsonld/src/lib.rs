//! JSON-LD layer (docs/deep-analysis.md §6.3) — hand-rolled NGSI-LD-subset
//! processor (risk #1 escape hatch): context processing, expansion with
//! structural validation, compaction, caching loader with pinned core
//! contexts.

pub mod compact;
pub mod context;
pub mod expand;
pub mod loader;

pub use compact::{compact_entity, compact_entity_shallow, compact_types};
pub use context::{Context, DEFAULT_VOCAB, NGSI_LD_BASE};
pub use expand::{expand_entity, expand_types, is_ngsi_null, parse_datetime, ExpandOpts};
pub use loader::{Loader, CORE_CONTEXT};
