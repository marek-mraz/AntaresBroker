// SPDX-License-Identifier: EUPL-1.2
//! AST → SQL compilation + schema.
//!
//! The q= compiler emits parameterized SQL — structure from the
//! compiler, values as binds ONLY. The sqlx store implementations live in
//! `store`; migrations live in `migrations/`.
#![cfg_attr(test, allow(clippy::unwrap_used))]

pub mod compile;
#[cfg(feature = "postgres")]
#[cfg(feature = "postgres")]
/// Re-export for integration tests that need raw SQL against the pool.
#[cfg(feature = "postgres")]
pub use sqlx;
pub mod store;

pub use antares_store::StoreMode;

/// The transaction preamble that makes RLS effective: always SET LOCAL,
/// never session-level SET.
pub const SET_TENANT_SQL: &str = "SELECT set_config('antares.tenant', $1, true)";
