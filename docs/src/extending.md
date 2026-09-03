# Extending Antares

Antares grows in three layers. Each one has a fixed seam in the code; an
extension attaches to a seam, it never adds one. The rule behind every
seam: a boundary is crossed once per request or once per drained batch,
never once per attribute or per matched subscription. That keeps the cost
of an extension at one dynamic call per request instead of one
marshalling round per value.

| layer | what it is | how it is chosen |
|---|---|---|
| Component drivers | storage, temporal history, notification sinks, the HTTP surfaces beside the API root | a name in the environment at startup |
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
| `antares-broker` | `plugin-example` | off | the reference plugin (`examples/plugin-example`): one more backend, surface, notification binding, policy engine and façade route, all from outside `crates/`. Never in a shipped build |

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
a match from the configured names to constructors. The shelf it selects
from is a list of NAMES, not an enumeration — `store_shelf()` chains the
built-in `StoreMode` values with whatever backends were compiled in from
outside the workspace, so adding one never edits a core crate.

- `ANTARES_STORE` picks the current-state backend through `build_store`,
  one arm per backend.
- `ANTARES_TEMPORAL` picks the history backend. Absent, or the same name
  as the store, means one shared instance serves both seams. `none`
  installs `NoTemporal`: the recorder produces nothing and temporal reads
  answer `OperationNotSupported` 422. Any other backend name builds a
  second store used only through its temporal half.
- Every Postgres half, primary or temporal-only, gets its own maintenance
  loop (partitions, retention).

Both traits carry the same two lifecycle methods. `version_info()` answers
what the driver runs on — engine, server version, extensions — from state
captured at startup, never by querying on the call: `/q/health` is polled,
and it prints the answer as `storeInfo` and `temporalInfo`. `close()` is the
drain: the shutdown path closes both seams, because a deployment whose
`ANTARES_TEMPORAL` names a second backend has two pools to close. One
instance can serve both seams, so it is closed through each of them and
`close()` must be idempotent.

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
sink cannot step around the verdict on the endpoint as written. What that
check cannot do is judge a name it does not resolve: under the default
`ANTARES_EGRESS_ALLOW_PRIVATE=true` an endpoint host given as a name passes
it, and the addresses the name stands for are judged where they are dialled.
A sink that opens its own socket therefore owes that filter —
`EgressPolicy::ip_is_metadata` and `ip_is_private` over the resolved answer,
before connecting, as the MQTT binding does in `connect_addr` and every
reqwest client does through `PolicyResolver`. A sink that opens no socket
says so by returning `false` from `network()`; every binding shipped here
returns the default `true`, and a unit test holds that.

### API surfaces

`ApiSurface` (`crates/antares-api/src/surface.rs`) is the routing seam: a
name, a prefix, an `axum::Router<AppState>` and a `version_info()` object
that `/q/health` prints under `surfaces`. The broker's own operational
routes are one such surface, `admin`, mounted at `/q`.

A surface may only claim a reserved prefix — `/q`, `/x`, or a path below
`/x`. The NGSI-LD API root belongs to the spec: a surface that could shadow
a spec resource would make conformance a function of deployment
configuration, so `AppState::with_surface` refuses any other prefix, and
refuses a prefix that overlaps one already mounted rather than leaving the
winner to route-matching order. Both refusals are startup errors.

`ANTARES_API_SURFACES` names the selection, comma-separated; absent means
`admin`. An unknown name is fatal at startup and lists the shelf, the same
way an unknown backend does. A selection that leaves `admin` out serves no
`/q` at all, health and readiness included, so an empty selection is
refused outright.

Adding one is a struct implementing the trait plus an entry in
`SURFACE_SHELF` (`crates/antares-broker/src/main.rs`); no core crate
changes.

### Policy engines

`PolicyEngine` (`crates/antares-api/src/policy.rs`) is the authorization
seam, and it is the narrowest of the five on purpose. Authentication, rate
limiting and request transforms belong to the gateway in front of the
broker (SECURITY.md says so, and ADR-0020 records why); what a gateway
cannot do from outside is narrow the query the store runs, project one
subscription's notification, and filter a federated result before it is
rendered. Those three are what an engine is for.

An engine answers two questions. `decide` is asked once per operation and
returns `Allow`, `Deny(reason)` — a `403` carrying
`urn:antares:error:AccessDenied`, this broker's own URN because Table
6.3.2-1 names no access-denied error — or `Filter`, the narrowing the
answer is built under. `pre_notify` is asked once per notification, just
before it is sent, and returns `Deliver`, `Drop` (no delivery attempt at
all: `timesSent` and `lastNotification` do not move) or a `Filter`
projecting the entities it carries.

The `Operation` an engine is given names the clause, the ids, types and
attributes the request selects, its `q`, `scopeQ` and geo query, and a
write's body — every name already expanded, so an engine never has to
carry a JSON-LD context. The `Subject` is the tenant plus the request
headers `ANTARES_POLICY_SUBJECT_HEADERS` names, and it stays in this
process: the seam strips those headers from every forwarded request and
keeps them out of notifications, logs and dead letters.

A `Filter` may only narrow. Its `q` and `scopeQ` are conjoined into the
condition the store already had, and its `pick`/`omit` project members out
of what is served; there is no member it can add and no row it can widen
the answer to. Set `restricted` and a narrowed read answers
`Antares-Results-Restricted: true`, so an operator can tell a short answer
from an empty one. Three limits are the seam's, not the engine's: an
operation over everything the tenant holds — delete-by-type, purge, the
whole-tenant snapshot clauses — is `Allow` or `Deny` and never a `Filter`,
because doing it to less than it says is a data-loss bug; a `Filter` on a
notification carrying a `q` is a `Drop`, because the broker cannot re-run a
query there and must not report a narrowing it did not apply; and an engine
that panics or overruns `ANTARES_POLICY_TIMEOUT_MS` is a `Deny`. Fail
closed is the whole posture: a deployment that wires in a broken engine
loses service, never its access rules.

`ANTARES_POLICY` names the engine, `allow-all` by default. That built-in
engine decides nothing, and it is the one the shipped image and every CI
gate run — conformance is asserted against it, never against an addon.
An unknown name is fatal at startup and lists the shelf the binary was
built with, so a typo cannot quietly serve every request wide open.

Adding one is a struct implementing the trait plus an entry in
`POLICY_SHELF` (`crates/antares-broker/src/main.rs`); no core crate
changes. Hold it to `antares_api::policy::run_policy_contract` (behind the
`test-kit` feature) from the crate's own tests: it asserts through the seam
the three things an engine can get wrong — it stops answering, it hands
back an answer the seam has to override, or it puts a member into an answer
that was not there.

`examples/plugin-example/src/policy.rs` is the worked one. Its rules are a
JSON document named by `ANTARES_POLICY_RULES`, one entry per tenant:

```json
{
  "acme": {
    "denyTypes": ["Secret"],
    "omit": ["price"],
    "q": "speed<100"
  }
}
```

A tenant with no entry is unrestricted. `denyTypes` refuses any operation
naming those Entity types — the request's own selector and a write's body
type, matched against the short name or the expanded IRI; `omit` drops
those Attributes from every document served and every notification sent;
`q` is conjoined into every query that tenant runs. Rules are read once, at
startup, and rules the engine cannot read make it refuse everything rather
than allow everything. `examples/plugin-example/tests/policy.rs` runs the
contract and then answers real requests through the router;
`ngsi-ld-test-suite/AntaresSpecificTests/policy_engine.robot` does the same
against a running broker, and `.github/workflows/examples.yml` runs the
conformance suite on `allow-all` and that folder on the engine, in one job.

### Façades for another standard

A SensorThings, OGC API, WFS or OData surface is an `ApiSurface` like any
other, with one extra rule: it answers by driving this broker's NGSI-LD API
in process, through `AppState::call`, and never by reaching the store. That
is the whole design. Every façade request becomes an NGSI-LD request, so
negotiation, the bounds wall, tenancy, the policy seam, history and
notifications happen once, in the code that already implements CIM 009 —
and there is no second data path to keep in step with the first.

```rust,ignore
async fn things(State(st): State<AppState>, headers: HeaderMap) -> Response {
    let req = Request::get("/ngsi-ld/v1/entities?type=Device&options=keyValues")
        .body(Body::empty())?;
    let resp = st.call(&headers, req).await;   // the broker answers
    // ...translate the answer into the standard's own shape
}
```

`examples/plugin-example/src/surface.rs` is the worked one, and
`examples/plugin-example/tests/facade.rs` is the contract it is held to.

**The tenant rule.** A façade maps its own notion of a caller — a path
segment, a subdomain, a header its standard defines — to `NGSILD-Tenant`,
and to nothing else. `AppState::call` copies `NGSILD-Tenant`,
`NGSILD-Snapshot`, `Link` and the policy subject headers from the outer
request when the inner one does not set them, so a façade that has no
tenant notion of its own inherits the caller's and cannot get it wrong. A
façade that DOES map its own sets the header on the inner request, and the
copy stands aside. What a façade must never do is derive a tenant from
anything the broker did not validate: `NGSILD-Tenant` is checked once, in
`negotiate`, and that check is the whole of 6.3.14.

**The representation to ask for.** Most of a mapping is already an option
on the NGSI-LD request, and asking for the right one is the difference
between a translation and a rewrite:

| the façade wants | ask for | clause |
|---|---|---|
| values without the NGSI-LD envelope | `options=keyValues` | 4.5.4 |
| the envelope, minus what is inferable | `options=concise` | 4.5.2.3 ff. |
| GeoJSON Features | `Accept: application/geo+json` | 6.3.15 |
| history rather than current state | the `/temporal/entities` resources | 6.18-6.22 |
| only some Attributes | `attrs=` | 6.4.3.2 |
| a count of what matched | `count=true` → `NGSILD-Results-Count` | 6.3.13 |

**The error table.** A façade keeps the broker's status — it is the verdict
on the operation, and the façade has no better one — and re-renders the
body in its own vocabulary. A caller of a SensorThings API is not expecting
Table 6.3.2-1 ProblemDetails, and a façade that passed them through would
be telling its clients to parse a second error model. What each type means
to a translation:

| NGSI-LD error (Table 6.3.2-1) | status | what the façade's caller did |
|---|---|---|
| `InvalidRequest`, `BadRequestData` | 400 | sent something the broker will not accept — a bad filter, a malformed body |
| `TooComplexQuery`, `TooManyResults` | 403 | asked for more than a cap allows; the façade's own paging is what avoids it |
| `ResourceNotFound`, `NonexistentTenant` | 404 | named an entity, or a tenant, that is not there |
| `AlreadyExists`, `Conflict` | 409 | created something twice, or a registration that clashes |
| `OperationNotSupported` | 422 | asked for an operation the broker does not offer on that resource |
| `NoMultiTenantSupport` | 501 | named a tenant a single-tenant deployment cannot serve |
| `InternalError` | 500 | nothing; the broker failed |
| `LdContextNotAvailable` | 504 | named an `@context` the broker could not fetch |

6.3.4's own statuses (411, 413, 414, 415, 406) carry no body at all, so a
façade fills the message from the reason phrase rather than from a payload
that is not there.

**The paging map.** The broker pages with `limit`/`offset` and answers with
RFC 8288 `Link` headers, `rel="next"` and `rel="prev"`, carrying the
response media type; `count=true` adds `NGSILD-Results-Count` (6.3.10,
6.3.13). Each standard renders the same two facts — where the next page is,
and how many there are — its own way:

| standard | next page | total |
|---|---|---|
| SensorThings 1.1 | `@iot.nextLink`, an absolute URL | `@iot.count` with `$count=true` |
| OData | `$skip`/`$top` on the next request | `@odata.count` |
| OGC API — Features | a `rel="next"` entry in `links` | `numberMatched`, `numberReturned` |

The façade rewrites the broker's `Link` into its own form rather than
re-deriving the offsets: the broker already knows whether there IS a next
page, and a façade that recomputed it would page differently from the API
it fronts.

**The write rule.** A façade writes through the NGSI-LD write resources —
`POST /entities`, the `/entityOperations/*` batch endpoints, the temporal
resources — and never through `AppState::store`. A store call skips
expansion, validation, the policy seam, the history recording layer and the
change hook that feeds subscriptions: an Entity written that way is in the
database, absent from every notification, and absent from history. The
batch endpoints are the right target for a façade that receives many
records at once (an STA `POST /Observations` array), because one batch
request is one pass through that machinery instead of N.

### How to add a storage backend

`examples/plugin-example` is the worked answer: a crate outside `crates/`
that implements `CurrentStateDriver`, `TemporalDriver`, one `ApiSurface`,
one `NotificationSink` and one `PolicyEngine`, and reaches a running broker
through one cargo feature. Read it first — it is short, and it is built and tested
with the workspace so it cannot drift from the seams it demonstrates.

A backend from outside the workspace:

1. A crate depending on `antares-store` (the two driver traits) and, if it
   also brings a surface or a binding, `antares-api` and
   `antares-notifier`. Nothing depends on it in return.
2. Implement `CurrentStateDriver` and `TemporalDriver` for one type.
   Methods the backend does not support keep the trait defaults, which
   return an unsupported error instead of panicking. A driver may
   over-return from `query_entities` — answering `decided: false` hands
   every predicate back to the API — but it may never drop a matching row
   and never cross a tenant.
3. Hold it to the driver contract. `antares-store`'s `test-kit` feature
   exports `run_current_state_contract` and `run_temporal_contract`
   (`crates/antares-store/src/contract.rs`): the rules `antares-api` writes
   against and no backend decides for itself — a missing row answers `None`,
   a mutate never inserts (ADR-0005, ETSI 047_06), a rejected mutate commits
   nothing, batch results align with the input ids, `upsert` and
   `batch_upsert` answer opposite polarities, a query never drops a matching
   row and never crosses a tenant. Call both from the crate's own tests, the
   way `examples/plugin-example/tests/contract.rs` does. A driver whose calls
   block on an async runtime needs a multi-threaded runtime context.
4. Register it in `antares-broker`: an optional dependency, one feature that
   turns it on, and the name in the three shelves it belongs to —
   `store_shelf()` for the backend, `SURFACE_SHELF` for a surface,
   `AppState::with_sink` for a binding, `POLICY_SHELF` for a policy engine.
   Each is one `#[cfg(feature = …)]` line. Name the environment variables the backend reads in its doc
   comment; `dev/check-env-docs.sh` requires a row for each in
   `docs/src/configuration.md`.
5. Prove it against the conformance suite, not only against the contract:
   run the broker with `ANTARES_STORE=<name>` and put the ETSI suite
   through it. `.github/workflows/examples.yml` does exactly that for the
   reference plugin.

A backend that wants to be a built-in instead — one of the arms the shipped
binary carries — takes the same first three steps and then joins the store
ladder: an arm in `AnyStore` (`crates/antares-sql/src/store/any.rs`), a
value in `StoreMode` (`crates/antares-store/src/lib.rs`), an arm in
`build_builtin`, a background job next to the expiry sweep if it needs one,
a row per pairing in `crates/antares-broker/tests/store_combos.rs`, and a
cell in `.github/workflows/etsi-matrix.yml`. The full preset runs seven
cells today: memory, file, postgres, timescale, postgres-nats,
timescale-nats and wasm-file; every cell must pass the whole suite before
the backend is part of a release. The API test suite runs once per built-in
store — `AppState::new` composes a fresh store per state from
`ANTARES_TEST_STORE`, and `workspace.yml` runs `cargo nextest -p
antares-api` under each value; that harness reaches only backends
`antares-api` can construct, which is why an outside driver proves itself
through the suite in step 5.

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

HTTP-level concerns that belong to a gateway (authentication, rate
limiting, request transforms) stay in the gateway in front of the broker.
The shared crates give a gateway the broker's own parsing, expansion and
matching for that job; see [Shared crates](shared-crates.md). Authorization
is the one concern that is split: the broker ships no policy engine, but it
carries the seam an engine attaches to, because a query has to be narrowed
before the store answers it and a notification has to be filtered on its
way out — neither is visible from in front of the broker (ADR-0020).

## Layer 3: dynamic loading

Not built. Rust has no stable ABI, so a loadable driver needs a C ABI
with a version check that turns a mismatch into a link error, and native
modules cannot be sandboxed. When a third party who cannot recompile
Antares needs a driver, the shape is either `#[repr(C)]` vtables over the
same two traits or a WebAssembly component driver for untrusted code.
Until then the traits stay the only coupling, which keeps that loader the
small half of the work.
