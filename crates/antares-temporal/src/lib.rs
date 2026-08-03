//! Temporal store (docs/deep-analysis.md §8.2) — lands in phase 2.
//!
//! Contract fixed now: TWO first-class modes behind one trait,
//! `temporal.store = timescale | plain`, auto-detected via pg_extension.
//! Table shape and queries are identical; only DDL bootstrap and
//! maintenance jobs differ. Both modes are CI-tested.

/// Selected at startup; `Timescale` when the extension is present.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemporalMode {
    Timescale,
    Plain,
}
