# ADR-0013: storage drivers — current-state and temporal as separate traits

Date: 2026-08-24 · Status: accepted (supersedes the enum half of ADR-0005;
the sync-facade half of ADR-0005 and all of ADR-0007's no-bus-recorder
decision stand)

## Context

The store seam is the closed enum `AnyStore { Mem, Pg }`
(`antares-sql/src/store/any.rs`), fusing current-state and temporal
storage in one type. ADR-0005 chose the enum because `mutate<T, E>` is
generic (dyn-incompatible) and the backend set was closed by design. Three
pressures have outgrown it:

1. **Every new backend is a new arm across ~30 match sites.** The wasm
   OPFS backend already lives outside the enum behind cfg, and the fused
   type forbids useful deployments outright: postgres current-state with
   no temporal, memory current-state with timescale temporal.
2. **Current-state and temporal are different products.** They scale
   differently, are configured differently, and coraine demonstrates the
   working shape: core knows two driver interfaces (`DbDriver`,
   `TroeDriver`, `doc/plugin-architecture.md`) and no storage code;
   temporal intake is an event queue drained after the response.
3. **Optional surface needs graceful degradation.** Temporal OFF is today
   a broker flag consulted at call sites; it should be a driver whose
   methods answer "unsupported", the way coraine maps a NULL driver
   function to one well-known error.

## Decision

Two object-safe traits, in a dependency-free crate the core can name
without pulling any backend:

- **`CurrentStateDriver`** — the non-temporal surface of `AnyStore`:
  create/get/delete/list/upsert, the batch ops, query_entities,
  matching_registrations, sweep_expired, tenant surface, context
  documents, ping/close, commit_queue, change hook and outbox wiring.
- **`TemporalDriver`** — temporal_append, query_temporal, get_temporal,
  the temporal delete paths (CIM 009 5.6.13–5.6.16), tenant
  setup/migration hooks, ping; plus the event intake
  (`event`/`event_list`, the bulk form defaulting to a loop) that the
  post-response drain feeds.

Wiring: `AppState.store` becomes `Arc<dyn CurrentStateDriver>` and a
separate `Arc<dyn TemporalDriver>`; one registry function in
`antares-broker` maps `ANTARES_STORE` / `ANTARES_TEMPORAL` names to
constructors, each arm behind its cargo feature. `ANTARES_TEMPORAL`
defaults to following `ANTARES_STORE`, so existing deployments see no
change. A name whose feature is compiled out is a startup error listing
what this binary carries.

**Why traits and not the enum now.** The enum's virtue (exhaustiveness)
became its cost: a backend cannot exist outside `antares-sql`, and every
cross-cutting feature touches every arm at every site. Dynamic dispatch
is called a handful of times per request against work that is a DB round
trip or a full JSON-LD expansion — the indirection is noise (measured
nanoseconds against sub-millisecond floors). Call sites keep monomorphic
ergonomics through an extension trait (below).

**Object-safety for the generic methods.** `mutate<T, E>` and
`batch_mutate<E>` become closure-boxed on the trait:

```rust
fn mutate(&self, tenant: &TenantId, kind: Kind, id: &str,
          f: Box<dyn FnOnce(&mut Value) -> Result<Value, NgsiError> + Send>)
          -> Result<Option<Result<Value, NgsiError>>, NgsiError>;
```

A non-dyn extension trait (`CurrentStateDriverExt`, blanket impl over
`dyn CurrentStateDriver`) restores the typed `mutate<T, E>` sugar so the
~60 call sites keep their signatures. The 047_06 rule from ADR-0005 is
part of the trait contract, not the backend's discretion: every mutate is
one transaction under the row lock, and a missing row is `None`, never an
insert.

**Why not dlopen / shared objects.** Rust has no stable ABI: a mismatch
between host and plugin is not a link error, it is silent memory
corruption. Cargo features are the shelf — a backend is one crate/feature
plus one registry arm, and third parties who cannot recompile do not
exist for this project today. If they materialise, `#[repr(C)]` vtables
(abi_stable/stabby) or a wasm component over these same two traits are
the loader's job; the traits stay the only coupling either way.

**Unsupported operations degrade, never panic.** Default trait method
bodies return an error instead of being required. The HTTP mapping is
decided once, here:

- Optional NGSI-LD surface (temporal API on a deployment without a
  temporal driver): `NgsiError::OperationNotSupported` → 422 with the
  `OperationNotSupported` problem type, per CIM 009 Table 6.3.2-1.
- Admin/internal surface with no spec error type: plain 501.

`NoTemporal` is the canonical instance: every read/write answers
unsupported, the recorder produces nothing, and
`AppState.record_locally` reduces to "is the loaded temporal driver
NoTemporal".

**The temporal seam is an event queue, not direct store calls.** The
write path produces `TemporalEvent`s into a per-request buffer; the
drain runs after the response is queued and hands the whole request's
events to the driver (`event_list`), so a bulk writer (COPY, a columnar
cold tier) sees one call per request. ADR-0007 is untouched: recording
stays in-process on the same pod, ordered per request, with no bus
consumer and no durable — the two defects that killed the bus design
(unbounded read-your-writes lag, late-replay resurrection) do not apply
to an in-process drain. What changes against ADR-0007's letter is the
transaction boundary: recording moves from inside the write transaction
to immediately after the response. The read-your-writes window shrinks
from "whenever the consumer catches up" to the microseconds before the
drain runs; the ETSI temporal suites are the oracle that this window is
invisible at HTTP round-trip granularity. A driver error in the drain is
logged and counted (visible in /q/health), never converted into a late
error for a response already sent.

Implementation note (`antares-api/src/history.rs`): the drain sits in a
router layer and runs after the handler returned its response but before
that response leaves the process. The batch shape is exactly the one above
— one `event_list` per request — while the read-your-writes window is
closed by construction rather than by timing, which keeps the temporal
tests deterministic at any store latency. Moving the drain past the
response flush is a one-line change to make on a measured latency need.

## Non-goals

- No runtime .so loading, no wasm host for drivers.
- No new storage backend in this phase — memory, redb-file, postgres,
  timescale, and the wasm OPFS backend are ported, nothing added.
- No behaviour change per backend: the memory store keeps its semantics,
  `PgBackend` keeps its SQL, the timescale/partitioned split stays
  inside the postgres temporal driver.

## Consequences

- `antares-api` loses its compile-time knowledge of the backend set;
  /q/health keeps reporting the concrete mode via `fn mode(&self) ->
  StoreMode` on the trait (no downcast).
- The memory store and `PgBackend` each split into a current-state half
  and a temporal half (same crates). Deployment matrices that were
  impossible become configuration.
- Each driver documents its own env vars next to its constructor;
  `dev/check-env-docs.sh` keeps the table honest.
- Prior art: coraine `src/lib/db/DbDriver.h`, `src/lib/troe/TroeDriver.h`,
  `doc/plugin-architecture.md` — the seams are copied, the dlopen loader
  deliberately is not.
