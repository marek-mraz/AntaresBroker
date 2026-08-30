# Extending Antares

Antares grows in three layers. Each one has a fixed seam in the code; an
extension attaches to a seam, it never adds one. The rule behind every
seam: a boundary is crossed once per request or once per drained batch,
never once per attribute or per matched subscription. That keeps the cost
of an extension at one dynamic call per request instead of one
marshalling round per value.

| layer | what it is | how it is chosen |
|---|---|---|
| Component drivers | storage, temporal history, notification sinks, the event bus | a name in the environment at startup |
| Lifecycle hooks | five named phases in the request lifecycle | compiled in behind a cargo feature; settings are data |
| Dynamic tier | loadable or sandboxed code | not built; the two driver traits are the only coupling it would need |

## Cargo features

Every optional capability is one cargo feature plus a registration in
`antares-broker`. Removing a feature never touches a core crate.

| crate | feature | default | what it compiles |
|---|---|---|---|
| `antares-api` | `mqtt` | on | the MQTT notification binding (`antares-notifier/mqtt`) |
| `antares-api` | `postgres` | on | forwards `antares-sql/postgres` |
| `antares-api` | `sonic` | off | `sonic-rs` on the batch-ingest hot path (x86_64 / aarch64); `serde_json` is the compiled fallback |
| `antares-sql` | `postgres` | on | the Postgres and TimescaleDB backend (`sqlx`); off in the browser build |
| `antares-notifier` | `mqtt` | on | `rumqttc` + `rustls` |
| `antares-broker` | `console` | off | `tokio-console` support; only arms under `RUSTFLAGS="--cfg tokio_unstable"` |
| `antares-broker` | `mqtt` | on | forwards `antares-api/mqtt`; off, MQTT endpoints fail at subscription creation. Measured on one release build: 27.2 → 26.0 MB binary, 58.1 → 55.4 MiB idle RSS |

The browser artifact (`antares-wasm`) is the one build with `postgres`
off: it drives the same router over the memory store and the OPFS shadow.

No other capability gets a flag: the native binary serves every store
mode behind one `ANTARES_STORE` value, so `postgres` stays compiled in
(the browser build is the deployment that sheds it), and the NATS bus,
the telemetry stack and the admin routes are runtime switches that
allocate nothing until enabled. A flag earns its place with a measured
saving, and `cargo build -p antares-broker --no-default-features` is
checked in CI so the smallest build keeps compiling.

## Layer 1: component drivers

The core knows two storage traits, `CurrentStateDriver` and
`TemporalDriver` (`crates/antares-store/src/lib.rs`), and nothing about
redb, `sqlx` or TimescaleDB. `Arc<dyn Trait>` is the plugin interface.
The [Storage drivers](storage.md) chapter describes what each backend
persists; this section describes how one is selected and added.

### The registry

`build_drivers` in `crates/antares-broker/src/main.rs` is the registry:
a match from the configured names to constructors.

- `ANTARES_STORE` picks the current-state backend through `build_store`,
  one arm per backend.
- `ANTARES_TEMPORAL` picks the history backend. Absent, or the same name
  as the store, means one shared instance serves both seams. `none`
  installs `NoTemporal`: the recorder produces nothing and temporal reads
  answer `OperationNotSupported` 422. Any other backend name builds a
  second store used only through its temporal half.
- Every Postgres half, primary or temporal-only, gets its own maintenance
  loop (partitions, retention).

An unknown name is fatal at startup and lists what the binary was built
with:

```text
ANTARES_TEMPORAL: unknown backend "mongo"; built with memory|file|postgres|timescale (temporal also: none)
```

`/q/health` reports both choices as `store` and `temporal`.

### Notification sinks

`NotificationSink` (`crates/antares-notifier/src/lib.rs`) is the delivery
seam. A sink declares the URI schemes it serves, validates its own endpoints
at subscription creation (`parse_endpoint`), and delivers one prepared
notification (`deliver`). `SinkRegistry` keys sinks by scheme and is the only
way a binding is chosen: a subscription naming a scheme no sink serves is
rejected when it is created, with `BadRequestData` (400), and a stored row
that names one is dropped rather than delivered through some other binding.
HTTP is always registered; MQTT registers behind the `mqtt` feature. Add one
with `AppState::with_sink`. A WebSocket binding would be a sink registration
plus a router merge, with no change to a core crate.

The egress policy — allowlist, private-range and metadata-address deny,
per-destination circuit breaker — runs in the caller before `deliver`, so a
sink cannot step around it. A sink that opens no socket says so by returning
`false` from `network()`; every binding shipped here returns the default
`true`, and a unit test holds that.

### How to add a storage backend

1. Copy `crates/antares-sql/src/store/mem/` to a new folder under
   `store/`. One folder holds the whole backend.
2. Implement `CurrentStateDriver` and `TemporalDriver` for it. Methods
   the backend does not support keep the trait defaults, which return an
   unsupported error instead of panicking.
3. Add an arm to `AnyStore` (`store/any.rs`) and a value to `StoreMode`
   (`crates/antares-store/src/lib.rs`).
4. Add the `build_store` arm in `crates/antares-broker/src/main.rs`,
   naming the environment variables the backend reads in its doc comment;
   `dev/check-env-docs.sh` requires a row for each in
   `docs/src/configuration.md`. Extend the `BUILT_WITH` listing.
5. If the backend needs background jobs, add an arm next to the expiry
   sweep and the maintenance job in `main.rs`.
6. Add the restart-survival and per-kind tests the other backends carry
   (`crates/antares-broker/tests/store_combos.rs` has one row per
   store × temporal pairing). The API test suite itself runs once per
   built-in store: `AppState::new` composes a fresh store per state from
   `ANTARES_TEST_STORE` (`memory` by default, `file` for a redb directory
   under the system temp dir), and `workspace.yml` runs
   `cargo nextest -p antares-api` under each value. A backend that wants
   the same proof adds its arm there and one more CI configuration.
7. Add a cell to the matrix in `.github/workflows/etsi-matrix.yml`. The
   full preset runs seven cells today: memory, file, postgres, timescale,
   postgres-nats, timescale-nats and wasm-file; every cell must pass the
   whole suite before the backend is part of a release.

SQL assembled at runtime stays inside `crates/antares-sql/src/`.
`workspace.yml` fails the build on `AssertSqlSafe` or an `sqlx` query built
with `format!` anywhere else under `crates/` or `examples/`. Integration
tests under a `tests/` directory are exempt: a test that creates its own
scratch database has no bind-parameter alternative. Everything a request
supplies reaches the database as a bind parameter, and a new backend keeps
that property.

## Layer 2: lifecycle hooks

Five phases exist. Extensions attach to a phase; they never define one.

| phase | fires | seam in code | failure policy |
|---|---|---|---|
| `on_request` | after parse and validation, before the operation | tower layer on the router | fail-closed |
| `on_change` | after commit, with the before/after documents | `ChangeHook` (`antares-store`); on Postgres the same change rides the transactional outbox to the bus | fail-open |
| `temporal_event` | in the post-response drain, with the whole request's events | `history::drain` and the gate chain in `crates/antares-api/src/history.rs` | fail-open |
| `pre_notify` | notification built, before send | `NotificationSink` | fail-open |
| `on_response` | render and annotate | tower layer | fail-closed |

Failure policy follows the hook's role. An observer (metrics, audit)
fails open: a broken observer loses its own data and the request
completes. A gate fails closed: a broken gate refuses, it never waves a
request through. A failed temporal drain is the worked example: the
write keeps its 2xx, the failure is logged, counted in
`antares_temporal_drain_errors_total` and shown as `temporalDrainErrors`
on `/q/health`.

The history gate chain shows the granularity rule. `GATES` is an ordered
list of `fn(&AppState, &TemporalEvent) -> bool`; every event of a request
passes the chain once, in the drain, and the surviving events reach the
temporal driver in one `event_list` call. Adding a gate is one entry in
that list. Producers and drivers do not change.

Which hooks are active, and their settings, are data: they can be
reloaded at runtime from the stores the broker already has. Hook code is
a cargo feature. Nothing in the typed NGSI-LD path dispatches through a
generic plugin chain, so conformance stays a property of the binary, not
of a deployment's configuration.

HTTP-level concerns that belong to a gateway (authentication,
authorization, rate limiting, request transforms) stay in the gateway in
front of the broker. The shared crates give a gateway the broker's own
parsing, expansion and matching for that job; see
[Shared crates](shared-crates.md).

## Layer 3: dynamic loading

Not built. Rust has no stable ABI, so a loadable driver needs a C ABI
with a version check that turns a mismatch into a link error, and native
modules cannot be sandboxed. When a third party who cannot recompile
Antares needs a driver, the shape is either `#[repr(C)]` vtables over the
same two traits or a WebAssembly component driver for untrusted code.
Until then the traits stay the only coupling, which keeps that loader the
small half of the work.
