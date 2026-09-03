# Storage drivers

The broker holds storage behind two object-safe traits in the
dependency-free `antares-store` crate (`crates/antares-store/src/lib.rs`,
ADR-0013): the core names the traits and no backend.

| Trait | Surface | Chosen by |
|---|---|---|
| `CurrentStateDriver` | create / get / delete / list / upsert, the batch operations, `query_entities`, `matching_registrations`, `sweep_expired`, tenant inventory and purge, `@context` documents, ping / close / version, commit-queue depth, change hook and outbox wiring. | `ANTARES_STORE` |
| `TemporalDriver` | `temporal_append`, `query_temporal`, `get_temporal`, the temporal delete paths (CIM 009 5.6.13 to 5.6.16), the temporal-entity documents, and the event intake (`event` / `event_list`) that the post-response drain feeds. `supported()` tells the API whether history exists at all, and `close()` / `version_info()` mirror the current-state seam — a temporal driver on a backend of its own has its own pool to drain and its own version to report. | `ANTARES_TEMPORAL` |

`AppState` carries one `Arc<dyn CurrentStateDriver>` and one
`Arc<dyn TemporalDriver>`. Both traits keep generic `mutate<T, E>` ergonomics
through the `*Ext` extension traits over a boxed `mutate_boxed`. Dynamic
dispatch runs a handful of times per request against a database round trip
or a JSON-LD expansion, so it costs nothing measurable.

## The store ladder

`memory → file → postgres → timescale` (ADR-0004): one binary, one
configuration value. The `mem/` and `pg/` folders under
`crates/antares-sql/src/store/` are the two implementations; `any.rs` is the
`AnyStore` dispatcher that implements both traits. The folder README
(`crates/antares-sql/src/store/README.md`) lists the steps for adding a
backend.

| Mode | Current state lives in | History lives in | Survives |
|---|---|---|---|
| `memory` | process maps | process maps | nothing; the unit-test default and the read-only-rootfs mode |
| `file` | the same maps, with a redb write-through shadow in `ANTARES_DATA_DIR/antares.redb` | the same file | process restart, `kill -9` after the 2xx (commit before ack) |
| `postgres` | one shared-schema database, `tenant_id` on every row under Row-Level Security | `attr_instances`, range-partitioned by `observed_at` | restart, replica failover, ordinary Postgres backup |
| `timescale` | the same schema | `attr_instances` as a hypertable (7-day chunks, native compression) | as `postgres` |
| browser (`wasm`) | the `memory` maps, or `AntaresBroker.persistentWithHandle(...)` over an OPFS sync-access handle | the same | the origin's private file system; the redb format is the native `file` one ([Browser & WebAssembly](wasm.md)) |

`file` is not a second store: the redb shadow holds one table per resource
family (`entities`, `subscriptions`, `csource_registrations`,
`csource_subscriptions`, `temporal_entities`, `jsonld_contexts`, `snapshots`,
`entity_map_docs`, `dist_subs`, `dead_letters`, plus `meta` for the format
version), keyed `tenant\0id`, value the expanded JSON. Every commit is
`Durability::Immediate` inside the store's write-critical section, so the
fsync completes before the HTTP answer leaves. A failed commit aborts the
process rather than acknowledge a write the file does not hold. Boot rebuilds
the maps from the file and refuses a format-version mismatch. redb keeps an
exclusive lock: one broker per volume, stop-copy backup, Recreate-only
rollouts. Measured on the dev box with 1.5 KB entities: about 3,100 fsynced
writes per second, commit p50 0.21 ms, p99 0.85 ms.

## Choosing the temporal driver

`ANTARES_TEMPORAL` names the history backend and defaults to following
`ANTARES_STORE`. Any pairing works: `file` current state with `timescale`
history, `postgres` current state with `memory` history. A backend different
from the store builds a second store instance used only for history; a
Postgres half runs its own maintenance and retention job wherever the
current state lives, and `/q/health` reports both halves (`store`,
`temporal`).

`ANTARES_TEMPORAL=none` installs the `NoTemporal` driver: `supported()` is
false, the recorder and the bookkeeping paths become no-ops, and every
client-facing temporal read answers `OperationNotSupported` with status 422
(CIM 009 Table 6.3.2-1):

```json
{
  "type": "https://uri.etsi.org/ngsi-ld/errors/OperationNotSupported",
  "title": "OperationNotSupported",
  "status": 422,
  "detail": "no temporal store is configured"
}
```

## What enters history

History is fed after the response, from a queue of `TemporalEvent`s the
entity endpoints produce. An event enters history only when every gate admits
it (`crates/antares-api/src/history.rs`):

1. **Value change**, in the producer: an attribute instance whose value did
   not change never becomes an event.
2. **`ANTARES_TEMPORAL_RECORD`**: `all` (the default) admits everything;
   `observed` keeps only instances carrying `observedAt`, so a write without
   one updates current state and leaves no history; `none` records nothing
   from the entity endpoints while the temporal API itself still stores what
   it is given.

`observed` is a narrowing, not a tidying: an Entity created through the Core
API with no `observedAt` has no temporal evolution at all under it, and a
temporal query over such an Entity answers empty rather than answering with
its values. That is why `all` is the default — five of the ETSI temporal
conformance tests create their fixtures without an `observedAt` and expect to
read the history back. Choose `observed` only where every producer stamps its
own observation times.

A drain that fails leaves the client's 2xx standing and increments
`temporalDrainErrors` on `/q/health`. `ANTARES_TEMPORAL_RETENTION_DAYS` prunes
attribute instances older than the window from the maintenance job on the
temporal half; unset keeps everything. The migration sets no retention on
purpose: a schema that silently drops data is the wrong surprise.

## Measured storage cost

Measured on a 3.9 KB expanded entity (1.5 KB compact),
and on attribute instances of the same shape:

| Where | Per entity | Per attribute instance |
|---|---|---|
| Postgres, plain | 3.1 KB, 938 B of it the whole-document GIN index | 1,387 B |
| TimescaleDB after columnstore compression | as plain | 120 B, read latency unchanged |
| Process memory (`serde_json::Value`, all modes) | 37.6 KB resident | as the entity |

Two consequences shape the design. The in-memory `Value` costs nine times its
text, so memory-mode capacity is bounded by RAM, not by the file. And
expanded JSON-LD costs 4.8 times the compact text but only 1.8 times on disk,
because Postgres compression absorbs the repeated IRIs; storing the compact
form would buy nothing.

## Migrations

`crates/antares-sql/migrations/`, applied by the first process to start
unless `ANTARES_MIGRATE=0` hands the job to an init container.

| Migration | Adds |
|---|---|
| `0001_init` | The PostGIS and `btree_gin` extensions; `tenants`; `entities` with the GiST location index, the `jsonb_path_ops` GIN serving `q=`, the tenant-scoped type index and the expiry index; `subscriptions`, `csource_subscriptions`, `csource_registrations` with the `csource_index` match table; `jsonld_contexts`; `entity_maps` and `entity_map_docs` (5.5.9.3 distributed pagination); `outbox`; `snapshots`; `dist_subs`; `temporal_entities`; `maintenance_jobs`; `attr_instances` as a hypertable when TimescaleDB is present, otherwise range-partitioned with a default partition, plus the lookup index and the idempotent-upsert key `(tenant_id, entity_id, attr_id, instance_id, observed_at)`. Row-Level Security with a `tenant_isolation` policy on every tenant-scoped table. |
| `0002_dead_letters` | `dead_letters` (`tenant_id`, `id`, `doc`), same RLS belt, read through the [admin API](admin-api.md#dead-letters). |
| `0003_comma_seconds_fraction` | Rewrites `try_timestamptz` to accept the comma decimal separator 4.6.3 permits in requests. The stamps live in `jsonb` as the client wrote them, and a NULL from this function means "no expiry" to `NOT_EXPIRED` and to the 4.22 instance reap — so a comma-stamped `expiresAt` made the document immortal instead of raising. |
| `0004_drop_entity_maps` | Drops the `entity_maps` row store `0001_init` created. EntityMaps (5.14) are documents in `entity_map_docs`, which is what every read and write path uses; nothing ever wrote a row to the table. |
| `0005_service_escape_by_command` | Splits the `tenant_isolation` policy on `entities`, `outbox` and (plain mode) `attr_instances` into one policy per command. `0001_init` wrote a single `FOR ALL` policy whose `USING` clause named the `antares.service` escape, and `USING` is what PostgreSQL applies to the existing row of an `UPDATE`: a role that armed the escape could move another tenant's row into its own, because the `WITH CHECK` only sees the new row. The escape now reaches `SELECT` and `DELETE`, which is all the outbox drain and the two 4.22 reaps use. |
| `0006_context_tenant` | Gives `jsonld_contexts` a `tenant_id` column and the RLS belt every other tenant-bearing table carries (ADR-0021). The column is `GENERATED` from the row: `NULL` for a `Cached` copy of a public document, which belongs to no tenant and is readable by all of them, and otherwise the `owner` member the row already carried — the default tenant for a row written before that member existed. No `antares.service` escape: nothing about a stored `@context` is cross-tenant work. |
