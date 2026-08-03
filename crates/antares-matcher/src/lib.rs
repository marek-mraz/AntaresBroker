//! Subscription matcher (docs/deep-analysis.md §9.3) — lands in phase 2.
//!
//! Contract fixed now: matching is index-shaped (candidate lookup by
//! (tenant, type) / (tenant, watched-attr)), each ChangeEvent is evaluated
//! self-contained, and expired subscriptions are filtered at the mirror's
//! single yield point (§4.1).
