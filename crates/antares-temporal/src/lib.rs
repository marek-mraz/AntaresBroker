//! Temporal store concerns (docs/deep-analysis.md §8.2).
//!
//! The two store modes (`timescale | plain`) live as the two DDL/maintenance
//! branches inside `antares-sql` (ADR-0005 deviation from the trait sketch).
//! This crate carries what sits ABOVE the store: the F8 recorder — the
//! durable change-stream consumer that owns auto-recording in bus=nats mode.

pub mod recorder;

/// Selected at startup; `Timescale` when the extension is present.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemporalMode {
    Timescale,
    Plain,
}
