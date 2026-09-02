// SPDX-License-Identifier: EUPL-1.2
//! JSON-LD layer — hand-rolled NGSI-LD-subset processor: context processing,
//! expansion with structural validation, compaction, caching loader with
//! pinned core contexts.
#![cfg_attr(not(test), warn(clippy::expect_used))]
#![deny(missing_docs)]
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
    expand_attr_fragment, expand_attr_name, expand_entity, expand_types, expanded_id,
    expanded_object, is_deletion_instance, is_ngsi_null, is_ngsi_null_langmap, parse_datetime,
    reject_first_level_nulls, valid_scope_value, ExpandOpts, RESERVED_MEMBERS,
};
pub use loader::{
    allow_private_egress, client_builder, core_context, http_interaction, io_deadline, slow_factor,
    with_timeouts, wrap_client, CtxUsage, EgressPolicy, HttpClient, Loader, CORE_CONTEXT,
    INTERNAL_FETCH_HEADER, MAX_CONTEXT_URLS, MAX_REDIRECTS,
};
