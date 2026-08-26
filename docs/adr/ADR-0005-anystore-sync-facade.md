# ADR-0005: AnyStore enum + synchronous Pg facade

Date: 2026-08-04 · Status: accepted · enum seam superseded by ADR-0013
(the sync facade and the mutate transaction rule stand)

The store seam is a closed enum, not a trait object: `AnyStore { Mem, Pg }`
in `antares-sql`, exposing the v0 memory store's 12-method surface with
`Result`-returning signatures. Two deliberate deviations from the original
design sketch:

1. **Enum, not `dyn EntityStore`.** The closure-based
   `mutate<T, E>(FnOnce(&mut Value) -> Result<T, E>)` is generic — it cannot
   be a trait-object method. A generic trait would force every consumer
   generic over the store; the enum keeps call sites monomorphic and the
   backend set is closed by design (memory/file share one arm, postgres/
   timescale the other). Adding a backend = adding an arm, which the
   compiler then enforces everywhere.
2. **Synchronous facade over sqlx.** `PgBackend` runs its async sqlx calls
   under `block_in_place` (`store/pg/entity.rs::wait`), so all 60+ call
   sites in `antares-api` kept their signatures at the Postgres cutover — no
   async-ification churn, and the closure-based `mutate` maps 1:1 onto the
   `SELECT … FOR UPDATE` → apply-in-Rust → version-bump transaction.

Read-modify-write rule (learned the hard way, ETSI 047_06): **every mutate —
entities AND doc kinds — is one transaction under the row lock, and a
missing row is `None`, never an insert.** A get+upsert mutate lets a
bookkeeping writeback racing a DELETE resurrect the deleted row; the
suite's leftover-subscription failure was exactly that. Regression test:
`antares-sql/tests/pg_doc.rs::mutate_never_resurrects_a_deleted_row`.

Consequence: the facade is the compatibility layer, not the destination —
per-op SQL (UNNEST batches, compiled q= pushdown) lands inside the
Pg arm without touching consumers.

## Confirmation

`crates/antares-store/src/lib.rs typed_mutate_round_trips_through_the_boxed_seam` (the mutate-transaction rule survives the trait seam); the enum half is confirmed by ADR-0013.
