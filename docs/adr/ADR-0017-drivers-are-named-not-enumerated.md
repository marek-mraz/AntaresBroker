# ADR-0017 — A driver is identified by its name, not by an enum value

Date: 2026-08-30. Status: accepted, implemented.
Supersedes the `fn mode(&self) -> StoreMode` consequence of ADR-0013.

## Context

ADR-0013 split storage into `CurrentStateDriver` and `TemporalDriver` and
made `Arc<dyn Trait>` the coupling. One thread of the old design survived
the split: the identity of the chosen backend. ADR-0013's Consequences
proposed `fn mode(&self) -> StoreMode` on the trait so `/q/health` could
report the concrete backend without a downcast. That method was never
written. What shipped instead put the enum one level out, in the state:
`AppState` carried `store_mode: StoreMode` and `temporal_mode:
Option<StoreMode>`, set by the broker's composition root, and `/q/health`
rendered them.

Either shape closes the seam at the same place. `StoreMode` is a closed
enum in `antares-store` with one value per shipped backend, so a driver
written outside this workspace has no way to say what it is. It can
implement both traits in full, pass the contract kit, serve every request
— and still not be namable by the binary that runs it. The traits were
pluggable; the identity was not, and identity is on the path from
`ANTARES_STORE` to a running driver.

The reference plugin is what made this concrete rather than theoretical:
`examples/plugin-example` implements both traits from outside `crates/`,
and could not be selected at startup at all.

## Decision

A driver's identity is its name, a `String`, chosen by whoever built it.

- `AppState` holds `store_name: String` and `temporal_name:
  Option<String>`. `with_store` and `with_drivers` take `&str`.
  Nothing in `antares-api` branches on the value; it is carried and
  printed.
- The shelf the broker resolves a name against is a list, not an
  enumeration. `store_shelf()` chains `StoreMode::ALL` with a const array
  of the plugin names compiled into this binary; `build_store` dispatches
  on the name, with the built-in arms behind `build_builtin`, and
  `temporal_choice` compares names. An unknown name is fatal at startup
  and prints the shelf.
- `StoreMode` stays, and stays closed. It is the built-in ladder — the
  backends this repository ships, tests in the ETSI matrix and supports —
  not the set of backends that can exist. `is_pg()` answers the one
  question the broker still asks of a built-in (whether the backend holds
  shared state, the precondition for the NATS bus).
- What a driver runs on is `version_info()` (ADR-0013's other lifecycle
  method), free-form JSON the driver fills in. `/q/health` prints the name
  as `store`/`temporal` and the object as `storeInfo`/`temporalInfo`.

## Consequences

- A backend from outside the workspace is one crate plus one cargo
  feature. No core crate learns its name at compile time.
- No code may branch on the store's identity. The last such branch was
  the transient-entity sweep, gated on `matches!(store, AnyStore::Mem(_))`
  because only the memory store reaped `expiresAt` in-process; it now runs
  for every driver, and a backend whose collection happens elsewhere — the
  Postgres maintenance loop — answers 0 from `sweep_expired`. A driver
  that cannot answer a question answers it through the trait, never by
  having the caller recognise it.
- `/q/health` reports a string the broker was handed rather than a value
  it can exhaustively match. A deployment can therefore print a backend
  name this repository has never heard of, which is the point.
- The API test harness is unchanged and still bounded to the built-ins:
  `AppState::new` composes a store from `ANTARES_TEST_STORE`, inside
  `antares-api`, so a plugin that depends on `antares-api` cannot be
  reached from there without a dependency cycle. An outside driver proves
  itself through the conformance suite against a running broker instead.

## Confirmation

`crates/antares-api/tests/api_surface.rs
a_store_from_outside_the_shelf_can_back_the_state` composes an `AppState`
over a driver defined in the test crate under a name no `StoreMode` has.
`crates/antares-broker/src/main.rs` holds
`a_plugin_backend_is_on_the_shelf_and_builds_both_seams` and
`only_the_database_backends_hold_shared_state`. The end-to-end proof is
the `plugin-example` job in `.github/workflows/examples.yml`: a broker
built on `examples/plugin-example`, asserting `store`, `temporal`,
`surfaces` and `notificationSchemes` on `/q/health` and running the
conformance suite through that driver.
