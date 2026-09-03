# ADR-0022 — The storage drivers are async; nothing blocks on a store call

Date: 2026-09-03. Status: accepted, implemented.
Supersedes the synchronous-facade half of ADR-0005 and its amendment
(the read-modify-write transaction rule stands).

## Context

ADR-0005 kept the store surface synchronous so the Postgres cutover cost no
call-site churn: `PgBackend` ran sqlx under `block_in_place` +
`Handle::block_on`, and every consumer in `antares-api` kept its signature.
The amendment records what that cost. A parked `block_in_place` past tokio's
blocking-thread ceiling also parks the core it ran on; with every core
parked nothing polls the I/O and timer driver, so the pool acquires the
parked threads wait for can never complete and the process stops at zero CPU
(seen at 1 000 updates/s with 12 000 client connections). The composition
root answered by sizing `max_blocking_threads` at `ANTARES_MAX_CONNECTIONS +
1024`: one OS thread per served connection, 2–8 MB of stack each, and every
Postgres round trip a park and an unpark.

The measured alternative in that amendment — a Postgres runtime of its own —
was rejected because it makes every round trip a cross-thread wakeup (p99 at
500 updates/s: 49 ms to 2 s). Both arms of that choice exist only because the
call sites are synchronous. Removing the synchrony removes the choice.

## Decision

`CurrentStateDriver` and `TemporalDriver` declare `async fn`. Both traits are
`dyn`-compatible through `#[async_trait::async_trait]` (a boxed future per
call), which `Arc<dyn Trait>` in `AppState` requires; `async fn` in a trait
is not object-safe on its own.

The change hook is async too (`ChangeHook = Arc<dyn Fn(..) -> HookFuture>`).
The hook records temporal history through the temporal driver, so the driver
that fires it awaits it before its own write returns: a read that follows a
write still sees the history that write produced. A driver clones the hook
out of its lock before awaiting, since a `std::sync` guard cannot be held
across an await.

Query evaluation stays synchronous. `antares-ql` and `antares-matcher` take
no store: the 4.9 linked-entity terms (`attr{path}`) resolve through
`notify::linked_eval`, which runs the synchronous evaluator against a cache,
collects the URIs the pass missed, fetches them together, and runs again. The
first pass that misses nothing is the answer, so the pass count is the depth
of the link chain, not the number of entities it touches.

`block_in_place` survives in exactly one place: the memory/redb store's
`on_blocking`, which hands the worker's queue away for a `file`-mode commit —
genuinely blocking work with no future to wait for, serialized behind redb's
single writer. The request runtime therefore takes tokio's own bounds again.

## Consequences

- `wait()` and the 37 `block_in_place` + `block_on` bridges under
  `store/pg/` are gone. A caller waiting on the database holds no thread,
  and store parallelism is bounded by the connection pool rather than by a
  thread ceiling.
- `broker::runtime` takes no argument and sets no `max_blocking_threads`.
  `ANTARES_MAX_CONNECTIONS` still bounds served connections; it no longer
  sizes an OS-thread pool.
- `antares-api`'s in-crate `#[tokio::test]` suites can drive a Postgres
  `AppState`: nothing inside a driver calls `block_on`, so a current-thread
  runtime hosts it.
- A driver written outside this workspace implements async traits.
  `examples/plugin-example` is the proof it stays writable from outside.
- The wasm build wires its state through `now_or_never`: the memory arm
  never yields, and a JS constructor cannot await. Wiring that did not
  complete synchronously logs and falls back to the documented store scan
  per change.

## Confirmation

`antares-store`'s contract kit (`contract.rs`, both functions async) run
against every driver, `examples/plugin-example/tests/contract.rs` for the
out-of-workspace one, and `crates/antares-sql/tests/pg_entity.rs`
`a_write_waits_for_the_tenant_row_lock`, which drives a store call from a
spawned task while the row is locked.
