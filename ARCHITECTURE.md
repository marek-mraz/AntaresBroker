# Architecture

A map of the Antares NGSI-LD context broker for anyone who has to change
it: what lives where, how a request and a change travel, which invariants
hold the system together, and what to touch for a given kind of change.
The user-facing story is the book under `docs/src/`; irreversible
decisions are in `docs/adr/`; the clause-by-clause conformance ledger is in
`docs/spec/`. This file is the index that links them to code.

Every path below is relative to the repository root. Line counts are
approximate and mark weight, not authority.

## 1. Shape in one paragraph

One binary (`crates/antares-broker`) serves ETSI CIM 009 V1.9.1 over HTTP
(`crates/antares-api`, axum). Handlers negotiate the request
(`negotiate.rs`), call a synchronous store trait
(`crates/antares-store`), and return. The store fires a change hook on
every write; the hook records history synchronously and hands the change
to the notification pipeline (`notify.rs`), which matches subscriptions
from an in-memory mirror and delivers over HTTP or MQTT
(`crates/antares-notifier`). Storage is a ladder behind one trait:
process maps, redb file, PostgreSQL/PostGIS, TimescaleDB
(`crates/antares-sql`). With `ANTARES_BUS=nats` the same binary splits
into roles (api, matcher, notifier, temporal, registry) joined by a
JetStream stream fed from a transactional outbox. The browser build
(`crates/antares-wasm`) runs the same router behind a Service Worker.

## 2. Crates and their contracts

Dependency direction runs one way — `broker → api → {bus, sql, store,
jsonld, ql, matcher, notifier, model}` — and cargo refuses a cycle in it.
What CI adds (`.github/workflows/workspace.yml`, "Shared crates
standalone") is the stronger rule for the five crates a gateway can take on
its own, `model`, `ql`, `jsonld`, `matcher` and `store`: each is built,
tested and documented alone, and its dependency tree may name neither the
broker nor a storage backend (`antares-api`, `antares-broker`,
`antares-sql`, `sqlx`, `axum`, `redb`).

| Crate | Lines | Owns | Must not |
|---|---|---|---|
| `antares-model` | 990 | CIM 009 types verbatim (`Entity`, `NgsiError` = Table 6.3.2-1), publishable | depend on anything Antares |
| `antares-ql` | 4 500 | `q=`, `scopeQ`, `geoQ` parsers → typed AST; the in-memory evaluator (`eval`) | know HTTP or SQL |
| `antares-jsonld` | 8 100 | `@context` loader with caches and pinned core contexts, expansion/compaction, structural validation, the one outbound `client_builder` | do business logic |
| `antares-matcher` | 380 | subscription vs entity: selector, conditions, activity, throttling | touch a store |
| `antares-store` | 2 600 | `CurrentStateDriver`, `TemporalDriver` (object-safe, `lib.rs:140`, `:385`), `Kind`, filters/paging (`filter.rs`) | pull a backend |
| `antares-sql` | 9 400 | AST → SQL compiler (`compile/`), migrations, the sqlx drivers (`store/pg/`), the memory/redb drivers (`store/mem/`), `AnyStore` facade (`store/any.rs`) | be called from handlers directly (see §7) |
| `antares-bus` | 760 | `ChangeEvent`, the JetStream bus, subjects | decide who consumes |
| `antares-notifier` | 1 700 | `NotificationSink` (schemes, `parse_endpoint`, `deliver`, `network`) chosen from `SinkRegistry` by endpoint scheme: http (`http.rs`), mqtt behind the feature, delivery policy, `Outbound` | match or store |
| `antares-api` | 44 500 | the HTTP binding: routers, handlers, negotiation, federation, notification pipeline, snapshots, bounds | own a backend or a transport |
| `antares-broker` | 3 500 | composition root: env → config, roles, bus wiring (`wiring.rs`), telemetry, shutdown | contain clause logic |
| `antares-wasm` | 500 | the router under a Service Worker, OPFS-backed file store | diverge from the native router |

## 3. Module map of `antares-api/src`

One module per NGSI-LD resource family; the clause range is in each
module's header comment.

| Module | Lines | Surface |
|---|---|---|
| `lib.rs` | 6 100 | the router (`/ngsi-ld/v1` nest, `/q/*` admin, `/info`), tenant purge, shared helpers |
| `negotiate.rs` | 2 000 | 6.3.4–6.3.6: tenant, Accept, Content-Type, `@context` resolution, parameter allow-lists (the query set of 6.4.3.2 included) and the RFC 7230 header-syntax checks. Every handler passes through `tenant_from`, `check_params`, `parse_body`, `request_context` |
| `entities.rs` | 3 500 | 5.6.1–5.6.6, 5.7.1–5.7.2 `/entities`, distributed write fan-out |
| `paging.rs` | 1 400 | 4.12, 6.3.10 limit/offset/count and the next/prev links, 4.23 ordering and ICU collation, 6.3.17 `NGSILD-Warning`, the query body of 5.2.23 lifted into parameters; every list operation pages through it |
| `attrs.rs` | 1 900 | 5.6.2–5.6.5 attribute operations |
| `batch.rs` | 2 000 | 5.6.7–5.6.10, 5.6.20 `/entityOperations/*` |
| `temporal.rs` | 4 100 | 5.6.11–5.6.16, 5.7.3–5.7.4 `/temporal/entities` |
| `temporalq.rs` | 200 | 5.2.21 `TemporalQ` from the timerel/timeAt/endTimeAt/timeproperty parameters and the 4.11 instance match; named by temporal, csource, federation and subscriptions |
| `types_attrs.rs` | 900 | 5.7.5–5.7.10 `/types`, `/attributes` |
| `subscriptions.rs` | 1 900 | 5.8, 5.11 `/subscriptions`, `/csourceSubscriptions` |
| `notify.rs` | 4 800 | 5.8.6 matching and delivery, the subscription mirror, interval sweeps, csource notifications |
| `distsub.rs` | 1 750 | 5.8.1.4 distributed subscriptions, consumer half |
| `csource.rs` | 2 200 | 5.9, 5.10 registrations, `csource_index` maintenance |
| `registry.rs` | 470 | matching over a registration document: `CsrSpec`, the 4.3.6.1 information match, csf and scope filters, the temporal interval and expiry of a registration, the 5.11.2 subscription match; named by federation, notify, csource and every resource module that forwards |
| `federation.rs` | 3 500 | 4.3.6, 5.12, 6.3.17–6.3.19 forwarding, fan-out, result merge |
| `entity_maps.rs` | 370 | 5.14 the `/entityMaps` resources (6.32, 6.34, 6.35): create from a query, retrieve, update, delete |
| `entity_map.rs` | 690 | one EntityMap document (5.2.39): store it under its tenant with a lifetime, read it back while it lives, take a page's candidate ids from it, merge what a distributed query reached, and serve a retrieve through a presented map |
| `snapshots.rs` | 1 600 | 5.16 snapshots under synthetic `snap-…` tenants |
| `contexts.rs` | 880 | 5.13 `/jsonldContexts` |
| `conformance.rs` | 760 | 6.3.21 version negotiation |
| `repr.rs` | 1 600 | 6.3.7, 4.5.4 representations: normalized, concise, keyValues, sysAttrs; 4.5.23 Linked Entity Retrieval (inline and flat, the lookup budget) and the 4.5.16 GeoJSON Feature/FeatureCollection shapes — what every module needs to render a document |
| `history.rs` | 260 | the producer side of temporal recording: the per-request change buffer and the 4.5.7/4.5.8 delete mirrors that record a deleted Entity or Attribute in history |
| `stamp.rs` | 50 | 4.8, 5.2.4 system attributes of a write: `createdAt`/`modifiedAt` on the entity and on every attribute instance |
| `mirror.rs` | 530 | the change event and the two document mirrors (`Change`, `DocMirror`, `SubMirror`, `TenantIndex`) that state, notify and history share, with the registration index (`reg_keys`, `bucketed`, `matching`) the federation read path selects candidates through; names no other module |
| `bounds.rs` | 500 | every cap: body, URI, JSON depth, batch, fan-out, in-flight, regex program size; reported by `/q/health` |
| `egress.rs` | 470 | SSRF wall and per-destination circuit breakers for notifications, forwards, `@context` fetches |
| `surface.rs` | 100 | `ApiSurface`: HTTP surfaces mounted beside the API root, on the reserved prefixes `/q` and `/x` |
| `policy.rs` | 996 | the policy seam (ADR-0020): `PolicyEngine`, `Subject`/`Operation`/`Decision`/`Filter`, the built-in `AllowAll` engine, the clauses a narrowing can reach and the refusal everywhere else, the fail-closed calls that deny on a panic or a timeout, and the stored subject a notification is decided under |
| `state.rs` | 930 | `AppState`: store, bus flag, mirror, HTTP clients, delivery policy, sinks, surfaces, hooks |

`geo.rs`, `qeval.rs` and `regexcache.rs` are not in that table because they
own almost nothing: each is a handful of lines re-exporting `antares_ql::geo`,
`::eval` and `::regex` so the broker-side paths (`crate::geo::GeoQuery`,
`crate::qeval::eval_q`, `crate::regexcache::compile`) stay stable while the
evaluation itself lives in the crate a gateway can use on its own. The one
exception is `qeval::expansion_list`, which settles the 4.9 precedence
between `expandValues` and `jsonKeys` — a rule about two query parameters
rather than about the query language, so it stays on the broker side.

## 4. A request, end to end

```
TCP accept (broker main.rs, TCP_NODELAY)
 → axum router (api lib.rs)
 → negotiate: tenant header → TenantId, Accept/Content-Type, params allow-list,
   Link/@context → antares_jsonld::Loader (cache, pinned core context)
 → bounds: body size/depth, URI length (rejects are counted in /q/health)
 → handler <resource>.rs::<operation>_inner
     expand (jsonld) → validate (model) → store call (CurrentStateDriver)
     for distributed operations: federation.rs::forward under FED_INFLIGHT
 → respond: compact, representation (repr.rs), paging Link headers, status
```

Handlers are synchronous with respect to the store: `antares-sql`'s
Postgres driver bridges sqlx through `tokio::task::block_in_place`
(`store/pg/entity.rs::wait`, 37 call sites under `store/pg/`). Every
Postgres round trip therefore parks a runtime worker, the composition root
sizes the blocking-thread ceiling at `ANTARES_MAX_CONNECTIONS + 1024`
(`broker/src/main.rs` `runtime`). The pool (`ANTARES_PG_POOL`, 20) answers
503 with `Retry-After` after its 5 s acquire timeout — the driver marks a
`PoolTimedOut` with `antares_model::error::DB_OVERLOADED` and
`negotiate::ApiError::Overloaded` renders it — and every transaction opens
through `store/pg/mod.rs::begin`, which times the wait into
`antares_pg_transaction_begin_seconds`; see §8.

## 5. A change, end to end

```
store write commits
 → ChangeHook (set at wiring; api role only)
     history.rs: temporal auto-recording, synchronous, same request
     buffer_change → per-request change buffer → change_flush at response
 → notify.rs::wire: bounded mpsc (CHANGE_QUEUE); a full queue drops and
   counts (antares_notification_changes_dropped_total, warn per thousand)
 → drain task: batch → process_changes
     match against SubMirror (in memory, per tenant; store list is the
     never-wired fallback) → group per subscription → JoinSet under
     DELIVERY_SLOTS (64)
 → deliver_as: one record_delivery per attempt (timesSent,
   lastNotification, lastSuccess, status), mirror updated from the document
   it returns,
   egress check + breaker for a binding that opens a socket, the
   NotificationSink the registry holds for the endpoint's scheme (never a
   fall-through, ADR-0016), retries as transport (ADR-0015), dead letter on
   exhaustion
```

With `ANTARES_BUS=nats` (`broker/src/wiring.rs`): the api role writes an
outbox row in the same transaction, a drain publishes to the
`ANTARES_CHANGES` stream with `Nats-Msg-Id` = outbox seq; matcher pods
consume, subscriptions travel through a KV bucket to every mirror;
interval firings are claimed through the store so one pod fires.

## 6. Storage

`ANTARES_STORE=memory|file|postgres|timescale` (ADR-0004), one trait
pair, one schema (`crates/antares-sql/migrations/`: `0001_init.sql`,
`0002_dead_letters.sql`, `0003_comma_seconds_fraction.sql`,
`0004_drop_entity_maps.sql`, `0005_service_escape_by_command.sql`).

- Tenancy: one shared schema, `tenant_id` on every row, Row-Level
  Security with `FORCE` on every tenant table (ADR-0001). RLS is a belt
  only when the role is not a superuser: `ANTARES_REQUIRE_RLS=1` refuses
  to boot otherwise, and the shipped manifests set it. The one hole in
  the belt is the transaction-scoped `antares.service` GUC, which the
  outbox drain and the two 4.22 reaps arm to work across tenants; it
  opens `SELECT` and `DELETE` on `entities`, `outbox` and (plain mode)
  `attr_instances`, and nothing else — the setting is not privileged, so
  the commands it reaches are the whole of its blast radius.
- Documents other than entities (subscriptions, registrations, entity
  maps, snapshots, dist-sub mappings, dead letters) are `Kind`-tagged
  JSON rows with the same trait surface (ADR-0012).
- Queries: `antares-sql/compile/` turns the AST into SQL that NARROWS;
  the in-memory evaluator (`antares-ql::eval`) is the arbiter
  (`qprefilter.rs`). A compiled predicate may be partial; it may never
  drop a candidate.
- Temporal: `attr_instances`, range-partitioned by `observed_at`
  (hypertable under timescale, no RLS there by columnstore constraint,
  ADR-0006); recording is synchronous in the write path (ADR-0007).
- Registrations: candidate matching goes through `csource_index`, never
  a scan of a tenant's registrations.

## 7. Invariants (break one and the suite tells you)

1. Spec names: Rust types and function names come from CIM 009 (`Entity`,
   `create_entity` = 5.6.1). Error variants = Table 6.3.2-1.
2. Doc comments on normative code cite the clause number and the rule.
3. Everything inbound is bounded (`bounds.rs`); everything outbound goes
   through `antares_jsonld::client_builder` and `egress.rs`.
4. No dynamic SQL outside the compiler (CI gate); `unsafe` forbidden;
   `unwrap`/`expect` denied outside tests.
5. Every env variable is in `KNOWN_KEYS` (`broker/src/main.rs`) and in
   `docs/src/configuration.md` (`dev/check-env-docs.sh`). Every version is
   in `[workspace.dependencies]`; the `wasm32` target sections, which
   cannot say `workspace = true` because cargo features are additive, are
   held equal to it by `dev/check-wasm-pins.py`.
6. `/q/health` lists every cap; a new cap is added to the health snapshot
   and its test (`bounds::tests`).
7. A store returns `NgsiError`, never a backend error; a handler never
   formats SQL.
8. Tests that wait for a delivery scale their wait by
   `antares_api::state::slow_factor()` (sanitizer builds run ten times
   slower).
9. The ETSI Robot suite is the oracle, never the requirements source;
   the clause text in `docs/spec/` is.

## 8. Known structural debts

Stated so they are not rediscovered. Each is measured, not guessed.

- The store trait is synchronous over async I/O (ADR-0005). Every
  Postgres call goes through `block_in_place`; the blocking-thread
  ceiling is sized from the connection cap (`antares-broker` `runtime`)
  so a parked caller always wakes; parallelism above the store is
  bounded by that ceiling, and a current-thread runtime cannot host the
  Postgres driver at all (`Handle::block_on` inside it panics), which is
  why the crate's own `#[tokio::test]` suites reach only the memory and
  redb stores. A dedicated Postgres runtime removes the ceiling but
  makes every round trip a cross-thread wakeup (measured p99 at 500
  updates/s: 49 ms to 2 s), so it is not the exit. This is the single
  largest architectural lever left; reversing it touches every driver
  and every store call in `antares-api` (116 `st.store.` and 21
  `st.temporal.` expressions), and the object-safe shape for it
  already exists in the tree: `antares-notifier`'s `DeliveryFuture`, a
  boxed `Send` future returned from a trait method.
- `antares-api` is acyclic: no module reaches itself through
  `crate::<module>` references, so the largest strongly connected
  component of its 31 modules is one. What two resources share lives in
  a leaf that names no resource module: the change event and the two
  document mirrors in `mirror.rs`, paging, ordering and the query-body
  lift in `paging.rs`, the request's temporal query in `temporalq.rs`,
  registration matching in `registry.rs`, the system attributes of a
  write in `stamp.rs`, one EntityMap document in `entity_map.rs`, and
  4.5.23 Linked Entity Retrieval with the 4.5.16 GeoJSON shapes in
  `repr.rs`. `dt_key`, `check_attr_name`, `scope_matches`,
  `type_selection_matches` and `redact_userinfo` are named from the
  crates that own them. The one call that pointed back up the pipeline —
  the delivery path handing a notification on the internal endpoint to
  `distsub` — is a seam: `AppState::csource_notification`, installed by
  `antares_api::wire`, absent and therefore dropped on a state nobody
  wired. `cargo modules`
  does not show such a cycle because it records call edges and not field
  types, so the source-level count is the one to measure: `dev/module-graph.py` prints
  it per crate and `xray.yml` ratchets the largest component of every
  crate against `dev/module-baseline.json` (`antares-jsonld` holds a
  second one, `context` ↔ `loader`, of two).
- `antares-api` names no storage backend in a normal build: `antares-sql`
  is an optional dependency behind the dev-only `test-kit` and `postgres`
  features (`AppState::new` over the built-in store, the Postgres-backed
  integration tests), the roots compose their drivers with
  `AppState::with_drivers`, and the temporal query's paging decision asks
  `TemporalDriver::q_pushdown_exact`, which only the Postgres arm answers
  from its prefilter. `workspace.yml` checks the dependency tree.
- `csource.rs` serialises registration writes on one process-wide
  `tokio::sync::Mutex` (`REGISTRATION_WRITE`) shared by every tenant, and
  the 5.9.2.4 overlap check of an idPattern-only registration walks up to
  `MAX_UNDECIDED_ROWS` under it.
- Every distributed write takes its registrations and the `Via` loop
  answer from `federation::write_plan` (`WritePlan::Forward` or
  `Answered`); the read-path prologue (csf, the two-binding rule) is
  still spelled per query path.
- Oversized functions carrying one clause's whole validation matrix:
  `purge_inner`, `batch_write`, `query_temporal_inner`,
  `normalize_subscription`, `expand_instance`, `normalize_registration`,
  `deliver_as`. Split by member on touch.
- Misplaced helpers: the error constructor `bad` lives in
  `snapshots.rs` and is used crate-wide.
- Per-notification cost is one row update. The Postgres arm writes it as
  a single statement (`pg::doc::record_delivery`), so the row lock is held
  for the statement rather than across a round trip and the Rust closure —
  at fan-out that hold time is what serializes delivery on a hot
  subscription. Batching several attempts into one statement is the next
  lever and is unbuilt: `set_config` is the RLS context, so a batch is
  per tenant by construction.
- Registration candidate selection reads on the order of a thousand
  rows per federated query on a 10 000-registration registry; the
  `csource_index` needs a type-first shape.
- Duplication has a measured floor rather than a target of zero.
  `dev/dup-check.sh` ratchets the workspace at 65 token clones over 779
  lines (`dev/dup-baseline.json`), 590 of them in `antares-api`, and at 12
  groups of functions sharing one signature. What the ratchet still allows:
  - 37 lines are `use` blocks in files serving neighbouring clauses. A
    `use` list is not logic, and a shared prelude would hide what each
    module depends on.
  - 52 lines are the Entity Type List and Attribute List discovery
    operations (5.7.5, 5.7.6). That parallel is the specification's: the
    two build different representations from different tables
    (5.2.24/5.2.25 against 5.2.27/5.2.28), so one generic over both would
    take more parameters than it removes and would let a change to one
    table reshape the other.
  - The largest remaining share is per-operation handler heads and tails,
    nine to fifteen lines each. What those share is already extracted --
    `tenant_from`, `check_params`, `request_context`,
    `ParsedBody::object`, `attach_paging`, `entity_maps::retrieve_with_map`
    -- and what remains is the part that differs: the parameter
    vocabulary, the error type of Table 6.3.2-1, the forwarded operation
    name. Fifty `check_params` call sites carry twenty-one distinct
    allowlists, and only two share the eleven-name query prefix; one
    allowlist for operations whose allowlists deliberately differ would
    widen what each endpoint accepts.
  - Two signature groups are coincidences of shape rather than shared
    logic: `check_ring`, `check_position` and `check_vertex_budget` all
    take a `&Value` and return `Result<(), String>` while validating three
    unrelated rules (RFC 7946 ring closure, WGS84 range, a vertex budget),
    and `temporal_key` and `pattern_regex` are both
    `(&str) -> Option<String>` over different vocabularies. Merging either
    would name one function after two rules.
  - One signature group is the reference plugin's, and stays: because
    `examples/plugin-example` implements the same two driver traits as the
    in-binary memory store, its `live`, `rows` and `emit` necessarily carry
    the memory store's signatures. That is the seam being demonstrated, so
    the ratchet holds it at twelve rather than eleven.
  - Three clusters are real debt, each reading one rule more than once:
    the temporal writes (80 lines inside `temporal.rs`, `add_temporal_attrs`
    against `upsert_temporal` and `delete_temporal_instance` against
    `delete_temporal_attr`), the federation entry points (72 lines,
    `fed_query`/`fed_query_temporal`, `fed_retrieve`/`fed_retrieve_temporal`
    and `import_entity`/`import_temporal`) and the snapshot document
    handlers (24 lines across `retrieve_snapshot`, `update_snapshot` and
    `clone_snapshot`).

## 9. To change X, touch Y

| Change | Touch | Also |
|---|---|---|
| a new query parameter | `negotiate.rs` allow-list, the handler, `antares-ql` if it has grammar | a `docs/spec/` ledger entry, a Robot TP |
| a new NGSI-LD resource | `<resource>.rs` + route in `lib.rs` | `Kind` in `antares-store` if it is stored |
| an error status | `antares-model` `NgsiError` | Table 6.3.2-1 in the doc comment |
| a cap | `bounds.rs` + env key + `docs/src/configuration.md` | `/q/health` test |
| a store backend | implement both driver traits in a new crate | a name on `store_shelf()` behind its feature (`examples/plugin-example` is the worked one); a built-in also takes a `StoreMode` value, an `AnyStore` arm and a CI cell |
| a notification transport | `NotificationSink` impl (own crate, or `antares-notifier`) | `SinkRegistry::register` at the composition root; `network()` false only if it opens no socket |
| outbound HTTP anywhere | `antares_jsonld::client_builder` only | egress policy and breaker; the DNS pin and the redirect cap live there and nowhere else, and `workspace.yml` fails on a client built any other way. reqwest is built provider-less, so a client built by another spelling panics rather than merely escaping the policy; `main` installs the provider before the OTLP exporter builds one of its own |
| a schema change | a new numbered migration; never edit an applied one | RLS policy + `FORCE` on the table |
| federation behaviour | `federation.rs` (`forward` is the one outbound chokepoint) | 4.3.6 narrowing is spec-mandated; keep it |
| the bus or roles | `broker/src/wiring.rs` | ADR-0002 |
| a role's HTTP surface | `lib.rs` router construction by `roles` | a worker must 404 the API |
| another standard's API beside NGSI-LD (SensorThings, OGC API, WFS) | an `ApiSurface` under `/x/…` in its own crate that calls `AppState::call` — the in-process handle over this broker's own router, built once per state on first use | never under `/ngsi-ld/`; every façade request is an NGSI-LD request, so negotiation, bounds and tenancy are not repeated. The handle carries `NGSILD-Tenant`, `NGSILD-Snapshot`, `Link` and the policy subject headers from the caller, so a façade cannot be the way around any of them |
| an authorization decision | no engine in the broker: `policy.rs` is the seam one attaches to, and the gateway keeps authentication and rate limiting (`docs/src/shared-crates.md`, "The PEP boundary") | conformance is asserted against the built-in allow-all engine, the way `surface.rs` refuses a surface under the API root |
| an NGSI-LD operation | one `policy::gate` call where the request enters its handler, naming the clause | exactly once per request: the shared query engines below a handler (`query_entities_inner`, `query_temporal_inner`) never gate, and `tests/policy_gate_seam.rs` walks every route of `router()` to prove both halves |
| a broker-held record of one query that outlives its request (an EntityMap, a Snapshot, a Subscription) | the subject it was made for, stored in `__subject` and read back through `policy::belongs_to` | the record is reused later, by a request the seam decides separately: a map's `entityMap` member is the id set a narrowing withholds, a snapshot's fill runs after its request is answered, and a notification has no request at all. The `__` prefix is the broker's — `policy::strip_internal` is the one serve-side filter, so a member added later is hidden by construction |
| a narrowing policy decision | the `Filter` the gate returned, applied where the request's own query and representation are built: `Filter::narrow_params` before the store is asked, `repr::narrow_projection` before the document is rendered | a served document is never re-walked to strip a member — the request's own 5.5.2 `pick`/`omit` machinery carries the policy's, so the two projections cannot disagree, and the `q` is conjoined on the `antares-ql` AST so the store push-down, the 5.7.2.4 re-check and the forwarded query read one query. `Filter::project` is the reference semantics the addon contract holds an engine against, not the served path |

## 10. Verification ladder

Unit (`cargo test -p <crate> <filter> -j 2`) → clause Robot TPs against
one local memory broker (`ANTARES_HTTP_PORT=9377 target/debug/antares`,
`ngsi-ld-test-suite/`) → `ci.yml` (dispatch: workspace tests, clippy
wall, quick ETSI matrix) → `full.yml` (seven store cells, twice weekly)
→ `strict.yml` (TSan, Miri, fuzz replay, coverage floor) →
`perf-weekly.yml` / `scale-weekly.yml` (rented hardware, `dev/perf/`).
`dev/code-xray.sh` writes module and call graphs under `results/x-ray/`.

## 11. Reading order for a newcomer

`docs/src/introduction.md` → this file → `docs/adr/` (0001, 0002, 0004,
0005, 0013, 0015) → `crates/antares-api/src/negotiate.rs` →
`entities.rs::create_entity` → `notify.rs::wire` →
`crates/antares-sql/src/store/pg/entity.rs` → `broker/src/wiring.rs`.
