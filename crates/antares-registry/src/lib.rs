// SPDX-License-Identifier: EUPL-1.2
//! CSR store + distributed operations.
//!
//! Contracts fixed now: candidate matching is SQL over csource_index (never a
//! scan of a tenant's registrations); forwarded queries are narrowed to the
//! registration's scope (4.3.6.1 — spec-mandated, do not "fix" away); fan-out
//! is bounded (semaphore + per-source timeout + aggregate deadline).

/// Registration modes (CIM 009 4.20 / getmode()).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegMode {
    Auxiliary = 0,
    Inclusive = 1,
    Redirect = 2,
    Exclusive = 3,
}
