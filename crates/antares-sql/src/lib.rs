//! AST → SQL compilation + schema (docs/deep-analysis.md §8).
//!
//! Phase-0 seed: the q= compiler emits parameterized SQL — structure from the
//! compiler, values as binds ONLY (§16.2). sqlx store implementations land in
//! phase 1; migrations live in `migrations/`.
#![cfg_attr(test, allow(clippy::unwrap_used))]

pub mod compile;
#[cfg(feature = "postgres")]
pub mod maintenance;
#[cfg(feature = "postgres")]
pub mod pg;
pub mod store;

/// The four store backends, decided ONCE at startup and threaded as a value —
/// never re-derived from strings or runtime probes, so a section gated on the
/// wrong mode is unrepresentable (user rule 2026-08-08).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum StoreMode {
    #[default]
    Memory,
    File,
    Postgres,
    Timescale,
}

impl StoreMode {
    pub fn as_str(self) -> &'static str {
        match self {
            StoreMode::Memory => "memory",
            StoreMode::File => "file",
            StoreMode::Postgres => "postgres",
            StoreMode::Timescale => "timescale",
        }
    }
    /// Shared-database modes — the only ones that can back multiple instances.
    pub fn is_pg(self) -> bool {
        matches!(self, StoreMode::Postgres | StoreMode::Timescale)
    }
}

impl std::str::FromStr for StoreMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "memory" => Ok(StoreMode::Memory),
            "file" => Ok(StoreMode::File),
            "postgres" => Ok(StoreMode::Postgres),
            "timescale" => Ok(StoreMode::Timescale),
            other => Err(format!(
                "unknown store mode {other} (memory|file|postgres|timescale)"
            )),
        }
    }
}

impl std::fmt::Display for StoreMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The transaction preamble that makes RLS effective (§3): always SET LOCAL,
/// never session-level SET.
pub const SET_TENANT_SQL: &str = "SELECT set_config('antares.tenant', $1, true)";
