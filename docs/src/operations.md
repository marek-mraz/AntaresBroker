# Antares operations runbook

Everything here re-states what the code, manifests and workflows already
enforce — no claim without a test behind it (the workflow proving each claim
is named inline).

## Deploy

**Docker (single node):** see the README quickstart — `memory` needs zero
infrastructure; `file` needs a mounted volume; `postgres`/`timescale` need
`ANTARES_DATABASE_URL`. Compose stacks in `compose-files/`
(`docker-compose.yml` one broker + PostGIS; `docker-compose-ha.yml` adds a
second broker1 replica + haproxy + NATS — the rolling-update shape;
`docker-compose-roles.yml` is the TRUE role split: 5 roles × 2 replicas =
10 broker containers — api×2 behind haproxy, matcher/notifier/temporal/
registry ×2 as ops-only worker pods — one shared PG, `ANTARES_BUS=nats`;
these stacks are what CI's ETSI cells run).

**Kubernetes (reference manifests, `deploy/k8s/`):** `namespace.yaml`,
`nats.yaml` (3-replica JetStream), `postgres-dev.yaml` (dev-only single PG;
production uses a CNPG cluster, `postgres-cnpg.yaml` is lint-only),
`broker-postgres.yaml` (an `antares-api` Deployment with `ANTARES_ROLES=api`
and an `antares-worker` Deployment with matcher/notifier/temporal/registry),
`broker-file.yaml` (single-replica file mode, `Recreate` strategy),
`networkpolicy.yaml` (deny-by-default ingress in the namespace, one allow
per flow the other manifests use; egress stays open because notification
endpoints, Context Sources and `@context` URLs are client data, gated by
`ANTARES_EGRESS_ALLOW_PRIVATE` and the scheme allowlist rather than by a
CIDR list). Both broker pod specs set `enableServiceLinks: false` (kubelet's injected
`ANTARES_*` service-link vars would otherwise trip the unknown-config check;
the broker also exempts those exact shapes). Proven by k8s-smoke.yml's `k8s-manifests`
kind smoke (dispatch): apply + every `rollout status` green.

- A worker pod serves the `/q` admin surface and nothing else — health,
  readiness, metrics, the tenant calls and the dead-letter admin. The
  NGSI-LD API exists only on pods with the `api` role.
- `ANTARES_BUS=nats` requires a shared store (`postgres`/`timescale`) and
  refuses to boot otherwise; `bus=local` requires all roles in one process.
- Production gates: set `ANTARES_REQUIRE_RLS=1` (refuses a DB role that
  bypasses RLS) and require auth on NATS (the broker logs a loud warning on
  an unauthenticated JetStream).

## Health, readiness, metrics

| Endpoint | Meaning |
|---|---|
| `/q/health` | Liveness + store mode and the temporal backend (`temporal`: `memory`, `file`, `postgres`, `timescale` or `none`), file-mode commit queue, resource limits + rejection counters, jemalloc heap, and under `bus=nats` the bus state `{mode, connected, reconnects}`. 503 = DRAINING (a roll in progress). |
| `/q/ready` | Readiness: not draining ∧ store answers (`SELECT 1` on Pg) ∧ bus connected. The k8s `readinessProbe` polls this; liveness stays on `/q/health` (a restart does not fix a lost DB). |
| `/q/metrics` | Prometheus text (`antares_` prefix; every metric in the [admin API](admin-api.md#get-qmetrics)). |

The NATS-outage contract (proven by `nats_e2e::nats_outage_flips_health_and_recovers`):
during an outage the API keeps serving (writes land in the transactional
outbox), `/q/ready` goes 503, and on reconnect the outbox drains — the
outage-time notifications arrive, none lost.

## Sizing the connection pool

`ANTARES_PG_POOL` (default 20) is how many PostgreSQL connections one broker
process may hold. It is a ceiling on concurrent database work, not a
throughput dial, and the measured runs say the dial does very little: on a
16-physical-core box pool 100 is 8 % faster than pool 20 at eight allotted
cores and 10 % slower at 1 024 concurrent query clients, and both pools hold
the same 1 000 rps write knee and fail at the same 1 500
([performance](performance.md#the-measured-ceiling)). Raise it to buy
concurrency the database can actually serve, never to buy speed.

Size it from the database, not from the broker:

```text
ANTARES_PG_POOL  <=  (max_connections - superuser_reserved_connections - other clients)
                     / number of broker replicas
```

A pool larger than the server's share does not fail at startup — it fails
later, at the first burst, as `FATAL: sorry, too many clients already` from
whichever client asks last. Leave headroom for the migration job, the
replicas rolling during an update, and anything else on the same database.

Three signals say the pool is the constraint:

| Signal | Where | Reading |
|---|---|---|
| `antares_pg_transaction_begin_seconds` | `/q/metrics` | time to get a pooled connection and open a transaction. Sub-millisecond when the pool is idle; it grows toward the 5 s acquire timeout as the pool empties. |
| `antares_pg_pool_timeouts_total` | `/q/metrics` | requests that waited the whole acquire timeout and got nothing. Any non-zero rate means clients are being turned away. |
| `storeInfo.poolSize`, `storeInfo.poolAcquireTimeoutSeconds` | `/q/health` | what this process was configured with, so a dashboard does not have to trust the deployment manifest. |

When the pool has nothing to give inside its acquire timeout, the request is
answered **503** with a `Retry-After` header and no body. The operation was
never attempted, so the client may retry the same request unchanged; a batch
that had already written part of its array reports the failure per item
instead, and never as a 503 the client would retry into duplicates. This is
an Antares decision, not a CIM 009 requirement: Table 6.3.2-1 has no error
type for an overloaded server, and clause 6.3.2 requires the HTTP binding's
own status codes beside it.

## Observability

Three signals, one switch (`ANTARES_TELEMETRY`):

| Signal | Where it goes |
|---|---|
| Traces | OTLP/HTTP to `ANTARES_OTLP_ENDPOINT` (`…/v1/traces`), batch exported. |
| Metrics | Prometheus text at `/q/metrics`, scraped. |
| Logs | Stdout always; with the endpoint set, also OTLP/HTTP to its `…/v1/logs` twin, same `service.name` resource, batch exported from a bounded queue on its own thread. A collector that does not answer drops records and never slows a request. |

Log records exported while a request span is open carry that span's
trace id, so a collector joins the three signals per request.

## Tenants

Tenants come to exist implicitly (CIM 009 5.5.10): the first create
operation carrying an `NGSILD-Tenant` header creates the tenant, and the
default tenant always exists. The NGSI-LD API has no operation to list or
remove tenants; the admin surface has both.

| Endpoint | Meaning |
|---|---|
| `GET /q/tenants` | The tenant names, sorted, and nothing else: the customer accounts, never the tenants the broker mints for its own bookkeeping. A deployment runs up to 10 000 tenants (ADR-0001); counting every kind for every one of them on a list call is a cost the list does not pay. |
| `GET /q/tenants/{tenant}` | What one tenant holds: `{tenant, createdAt, counts: {entities, subscriptions, csourceSubscriptions, registrations, snapshots, entityMaps, distSubs, attrInstances}}`. 404 for a tenant that does not exist, 400 for a name outside the tenant grammar. `createdAt` is present on Postgres, where the `tenants` table records it. |
| `DELETE /q/tenants/{tenant}` | Purge: every document of the tenant leaves the current-state backend and the temporal backend in one transaction each. 204 when done, 404 for a tenant that does not exist, 409 while a distributed subscription of the tenant still holds a copy at a Context Source (delete those subscriptions first, which removes the copies at their source), 400 for a name outside the tenant grammar. The default tenant is emptied and keeps existing. |

The path names the tenant; an `NGSILD-Tenant` header on these calls is
ignored. Like the rest of `/q/*`, the routes sit outside `/ngsi-ld/v1` and
belong behind the gateway. What the gateway owns instead: who may create a
tenant, quotas and rate limits per tenant, authentication.

## Notification delivery

CIM 009 5.8.6 books every notification once: `timesSent` moves by one,
`lastNotification` is stamped, then either `lastSuccess` or `lastFailure`
plus `status: "failed"`. The broker sends once by default. Retries are an
operator choice (`ANTARES_NOTIFY_ATTEMPTS`, `ANTARES_NOTIFY_BACKOFF_MS`,
`ANTARES_NOTIFY_MAX_AGE_SECS`, [configuration](configuration.md#notification-delivery))
and are transport under that one notification: the first attempt is booked
as above the moment it resolves, the retries run on their own task (a slow
endpoint never delays another subscription), a retry that succeeds sets
`lastSuccess` and `status: "ok"` without touching `timesSent` or
`timesFailed`, and an exhausted policy leaves a **dead letter**: the exact
request (endpoint, headers, payload) plus the attempt history, stored under
the subscription's tenant in every store mode.

| Call | Effect |
|---|---|
| `GET /q/dead-letters?tenant=&subscription=&limit=` | Letters of one tenant (default tenant when `tenant` is absent), newest first, `limit` 100 by default. Endpoint userinfo and every credential the letter carries (`receiverInfo`, `notifierInfo`, the rendered `headers` of an older letter) are blanked in the listing; the stored letter keeps them for a replay. |
| `POST /q/dead-letters/{id}/replay?tenant=` | One more attempt through the same binding under the egress policy of the moment: `204` and the letter is deleted, or `502` with the failure text and the letter kept (`attempts`, `lastError`, `lastAt` extended). |
| `DELETE /q/dead-letters/{id}?tenant=` | Drop the letter. `404` when the tenant holds no such letter. |

`/q/health` reports `deadLetters`, the letters this process wrote since
start; the letters themselves are rows, so they survive restarts on the
file, postgres and timescale stores and a tenant purge removes them with
the rest of the tenant. Egress-policy refusals (private ranges, blocked
schemes) are never retried and never dead-lettered: a policy verdict is
not a transport failure.

The notifications in flight at one moment are bounded broker-wide, and one
tenant may hold only a share of that bound. A subscription belongs to one
tenant, and a delivery to an endpoint that accepts the connection and never
answers holds its slot until `endpoint.timeout` expires — up to 30 seconds.
Without the per-tenant share, one tenant pointing enough subscriptions at
dead endpoints would hold every slot for that long and nothing would leave
the broker for anybody else. The share is a fraction of the bound rather
than an equal split of it, so a tenant delivering alone still runs several
notifications at once.

## Backup and restore, per store mode

| Mode | Backup |
|---|---|
| `memory` | nothing to back up; the process is the data |
| `file` | stop-copy only: stop the broker, copy `antares.redb`, restart |
| `postgres` / `timescale` | ordinary Postgres backup or PITR; entities, subscriptions, registrations, outbox, dead letters, entity maps and the temporal tables all live in the one database |

**postgres.** A custom-format dump restores with `--clean`, so the same
command works on an empty and on a populated database:

```bash
pg_dump  -U postgres -Fc antares > antares.dump
pg_restore -U postgres -d antares --clean --if-exists antares.dump
```

Drill on a database holding four entities: `SELECT count(*) FROM entities`
answers 4, `DELETE FROM entities` brings it to 0, `pg_restore` exits 0 and
the count is 4 again. Stop the brokers before restoring; they cache
nothing, but a write during the restore lands in a table that is about to
be replaced.

**timescale.** The same tools; wrap the restore in TimescaleDB's
`SELECT timescaledb_pre_restore();` and `SELECT timescaledb_post_restore();`
so the hypertable catalog is restored with the data.

**file.** redb holds an exclusive lock, so a copy of a running broker's
file can tear. Stop, copy, restart:

```bash
kill -TERM $(pidof antares)          # drains, then exits
cp -r "$ANTARES_DATA_DIR" /backup/antares-$(date +%F)
```

Drill: an entity created with an `observedAt`, broker stopped with SIGTERM,
directory copied, a broker started on the copy answers
`GET /entities/urn:ngsi-ld:Vehicle:f:1` and its temporal history. A second
broker on a directory that is already open fails at startup:

```text
Error: "open …/antares.redb: Database already open. Cannot acquire lock."
```

## Background jobs

One sweep loop per process, every `ANTARES_SWEEP_SECS` (default 900):

- Expired entities (`expiresAt`, 4.22) are deleted.
- Registrations, snapshots and entity maps (5.14; one hour by default, a
  client-set `expiresAt` capped at 24 hours) carry their own expiry and
  are deleted by the same loop. A read never deletes one: it refuses the
  expired document and leaves the row for the sweep, so a broker pointed
  at a read replica serves GET without writing.
- With `ANTARES_TEMPORAL_RETENTION_DAYS` set, attribute instances older
  than the horizon are pruned from the postgres or timescale temporal
  half, wherever that half lives (a `file` store with `postgres` history
  runs the job too). Drill with `ANTARES_TEMPORAL_RETENTION_DAYS=30
  ANTARES_SWEEP_SECS=2`: an entity with one instance observed seven
  months back and one observed this hour shows both before the sweep and
  only the recent one after it.

The outbox drainer (`ANTARES_OUTBOX_DRAIN`) is the other loop; it hands
committed changes to the matcher and can be moved to a dedicated process.

A change whose document exceeds the bus message ceiling (256 KB) travels as
a claim-check reference, and its outbox row is kept instead of deleted: that
row holds the bodies the message dropped, and the matcher reads them back by
the event's sequence number. Such rows carry a `published_at` stamp, sit out
of the drain's page, and the maintenance pass frees them 24 hours later. A
matcher lagging further behind than that resolves nothing — the change is
logged and counted on `antares_claim_check_unresolved_total`. A non-zero
counter means the consumer side, not the store, needs attention.

## Egress breaker

Every broker-initiated request (notification, forward, `@context` fetch)
passes the egress policy: scheme allowlist, private-range rule
(`ANTARES_EGRESS_ALLOW_PRIVATE`), redirect cap, DNS pinning and response
size caps. On top of it a per-destination breaker tracks timeouts: five
consecutive timeouts trip the destination open; while open, one probe per
30 seconds is admitted (half-open) and a success closes it again. A
destination that answers, even with an error status, never trips; only
silence does. At most 4096 destinations are tracked. A refused or tripped
delivery is booked on the subscription as a failure (`lastFailure`,
`status: failed`) and, with a delivery policy configured, is not retried.

An `@context` fetch is the one broker-initiated request that gets a second
attempt: a connection carrying no response at all — refused, or dropped
before a status line — is sent once more before the client is answered
`LdContextNotAvailable`. A timeout, a redirect-cap breach and any response
that did arrive are answers rather than accidents, so none of them is
repeated, and the two attempts share one fetch deadline.

## Drain

On SIGTERM the broker flips `/q/health` to 503, keeps serving for
`ANTARES_DRAIN_DELAY_MS` (default 2000) so the load balancer notices, then
waits up to `ANTARES_DRAIN_DEADLINE_SECS` (default 20) for in-flight
requests before exiting. Set the container's stop grace period above the
sum. The rolling-update section below relies on exactly this sequence.

## Bulk load (postgres, timescale)

`dev/bulk-load.sh` loads entities straight into the `entities` table for
initial loads and migrations. It bypasses the broker: no notification
fires, no history is recorded, and the secondary indexes are dropped for
the duration, so run it against a database no broker is serving.

```bash
DATABASE_URL=postgres://postgres:postgres@localhost:5432/antares \
  dev/bulk-load.sh vehicles.ndjson            # tenant "default"
DATABASE_URL=… dev/bulk-load.sh vehicles.ndjson odpady
```

Input is NDJSON, one entity per line in the store's internal form:
attribute names as expanded IRIs, each attribute an array of instances,
`id`/`type`/`scope` short. `type` and `scope` may be a string or an
array; the loader stores both as arrays, which is the shape the query
evaluator reads. A line may name its own tenant as a prefix separated by
the byte `0x02` (`t42<0x02>{"id":…}`), which lets one file load many
tenants; a bare JSON line lands in the tenant given as the argument. The
file may be a FIFO, so a generator can stream straight into the loader:

```bash
python3 dev/perf/gen.py --entities 1000000 --tenants 100 > /tmp/e.fifo &
DATABASE_URL=… dev/bulk-load.sh /tmp/e.fifo
```

```json
{"id":"urn:ngsi-ld:Vehicle:bulk:1","type":"https://uri.etsi.org/ngsi-ld/default-context/Vehicle","https://uri.etsi.org/ngsi-ld/default-context/speed":[{"type":"Property","value":42}],"https://uri.etsi.org/ngsi-ld/location":[{"type":"GeoProperty","value":{"type":"Point","coordinates":[19.15,48.73]}}]}
{"id":"urn:ngsi-ld:Vehicle:bulk:2","type":"https://uri.etsi.org/ngsi-ld/default-context/Vehicle","scope":"/city/east","https://uri.etsi.org/ngsi-ld/default-context/speed":[{"type":"Property","value":7}]}
```

The script, step by step:

1. `\copy` the file into an `UNLOGGED` staging table; the `jsonb` cast is
   the only parser the payload meets.
2. Drop the five secondary indexes (`i_entities_location`,
   `i_entities_jsonb`, `i_entities_types`, `i_entities_loc_ambiguous`,
   `i_entities_expires`); the primary key stays because the insert needs
   it.
3. Derive the columns the store derives on write: `types`, `scopes`,
   `created_at`/`modified_at` (the document's values, else `now()`),
   `expires_at`, and `location` from the default GeoProperty when it has
   exactly one instance holding a GeoJSON geometry; any other shape with
   the GeoProperty present sets `location_ambiguous`, and geo queries
   then judge that row in the broker instead of the index.
4. `INSERT … ON CONFLICT (tenant_id, id) DO NOTHING`: a row that already
   exists is left as it is, the loader never overwrites API-written data.
5. Rebuild the five indexes and `ANALYZE entities`.

Three lines offered, one of them an id the API had already created:

```text
COPY 3
DROP INDEX
INSERT 0 2
DROP TABLE
CREATE INDEX
…
ANALYZE
bulk load done: 3 lines offered into tenant 'default'
```

Verify through the broker once it is back up; every query kind must see
the loaded rows next to the API-written ones:

```bash
curl "$U/entities?type=Vehicle&q=speed>20&options=keyValues"
curl "$U/entities?type=Vehicle&georel=near;maxDistance==1000&geometry=Point&coordinates=[19.15,48.73]&options=keyValues"
curl "$U/entities?type=Vehicle&scopeQ=/city/%23&options=keyValues"
```

or in SQL, before restarting the broker:

```sql
SELECT id, types[1], scopes, ST_AsText(location), location_ambiguous
FROM entities WHERE tenant_id = 'default' ORDER BY id;
```

## Rolling update

`dev/rolling-update.sh` — one instance at a time against the HA compose
stack: SIGTERM → `/q/health` flips 503 → haproxy ejects within 400 ms →
in-flight requests finish → recreate on the current image → wait healthy +
rise window before the next instance. Preconditions and env are documented
in the script header. **file mode cannot roll** (redb allows one process per
volume): use a `Recreate` strategy there, as `broker-file.yaml` does.

**Role fleet:** `ROLES_SPLIT=1 dev/rolling-update.sh` rolls all 10 pods of
the role-split stack in role-group order — the same-group peer must be
healthy before its twin goes down, so no role ever has 0 live pods (api
pods gate on `/q/health` + the LB rise window; workers on `/q/ready`).
Measured: full roll ≈ 43 s (the api pod pays the
~21 s drain, workers ~2 s each), 52/52 LB requests answered 200 across the
whole roll.

Proven: the `roll-weekly` workflow (Tue 04:17 UTC + dispatch) runs
the FULL ETSI suite through the LB while the replicas roll in a loop — the
suite has no retries, so any red TP is a real drain bug. The per-push
`postgres-nats`/`timescale-nats` matrix cells do the same over the
10-container role fleet. On k8s the same contract holds via the readiness
probe + `terminationGracePeriodSeconds` exceeding drain delay + deadline.

## State reset (test/staging discipline)

API-level delete PAIRED with DB truncate — `dev/reset-broker.sh` plus the
suite's `clean_db.sh`; never raw-SQL-truncate or container-restart alone.
Federation/temporal state is only truly cleared by a volume-wiping teardown.
After a reset, restart the broker before measured runs (in-VM subscription
maps survive an external clean).

## Upgrades

Minor versions roll in place under the rolling-update contract above.
Major upgrades go blue/green: deploy the new version EMPTY, replay
declarative state (entities, subscriptions, registrations) through the
standard NGSI-LD API from your configuration source of truth, verify with
smoke queries, switch traffic. Because the broker is vanilla CIM 009, the
replay needs no Antares-specific tooling — any GitOps/city-as-code plane
that speaks the standard API can drive it (this is requirement CC-50/51 of
the companion configuration-plane spec). Temporal history is NOT part of
the replay — restore it from database backup (per-store recipes above).
The `file` store carries a format version: a downgraded or corrupted file
is refused at startup rather than partially served.

Rolling a minor version in place is proven on every release tag rather than
asserted: `upgrade-path` builds the previous tag's binary and this one, lets
the old binary write an entity, its history and a subscription through the
standard API, then points the new binary at that same `file` and `postgres`
store and requires all three back — the entity at its last value, the
history intact, and the stored subscription firing on a write the new binary
accepts. Run it against any two binaries with
`dev/upgrade-path.sh OLD NEW` (`ANTARES_TEST_DATABASE_URL` adds the postgres
half).

## Where the proofs run

| Claim | Workflow |
|---|---|
| ETSI conformance, per-commit gate (file/postgres/timescale × 10 suites) | ci.yml → etsi-matrix.yml `preset: quick` (every push) |
| ETSI conformance, FULL seven cells (memory/file/postgres/timescale + the two rolling role-fleet cells + wasm-file) × 10 suites | full.yml (twice a week + `v*` tags + dispatch); its bundle feeds [the report page](https://antares-ngsi-ld-demo.marek-mraz.com/reports/latest/) + per-cell badges |
| The browser artifact serves the full API from a container (file store, serial suites + IOP) | the `wasm-file` matrix cell (`WASM=1 WASM_DOCKER=1 STORE=file` through the one pipeline — Dockerfile.wasm, the same www/pkg bytes a page loads) |
| Zero-downtime rolling update | `roll-weekly` (Tue 04:17 UTC + dispatch) + the full-run `-nats` matrix cells (10-pod fleet rolling under the whole suite) |
| Role-pair exactly-once semantics (duplicated matcher/notifier/temporal/registry pods) | ci.yml nats job (`nats_e2e::role_pairs_exactly_once_semantics`, live PG + NATS) |
| NATS bus + role split e2e | ci.yml nats job (`nats_e2e`, live PG + NATS) |
| Data written by the previous release is served by this one (file + postgres) | full.yml `upgrade-path` on every `v*` tag: the two release binaries are built and pointed at one store in turn (`dev/upgrade-path.sh`) |
| k8s manifests boot | k8s-smoke.yml kind smoke (dispatch) |
| Coverage | strict.yml coverage job (daily line-coverage floor) + etsi-coverage.yml (Mon 04:41 UTC) → merged lcov/html on the report page |
