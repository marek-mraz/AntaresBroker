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
`broker-file.yaml` (single-replica file mode, `Recreate` strategy). All pod
specs set `enableServiceLinks: false` (kubelet's injected `ANTARES_*`
service-link vars would otherwise trip the unknown-config check; the broker
also exempts those exact shapes). Proven by k8s-smoke.yml's `k8s-manifests`
kind smoke (dispatch): apply + every `rollout status` green.

- A worker pod serves **only** `/q/health`, `/q/ready`, `/q/metrics` — the
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
| `GET /q/tenants` | Every tenant with what it holds: `{tenant, createdAt, counts: {entities, subscriptions, csourceSubscriptions, registrations, snapshots, entityMaps, distSubs, attrInstances}}`. `createdAt` is present on Postgres, where the `tenants` table records it. |
| `DELETE /q/tenants/{tenant}` | Purge: every document of the tenant leaves the current-state backend and the temporal backend in one transaction each. 204 when done, 404 for a tenant that does not exist, 409 while the tenant still holds distributed subscriptions (unsubscribe them first), 400 for a name outside the tenant grammar. The default tenant is emptied and keeps existing. |

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
| `GET /q/dead-letters?tenant=&subscription=&limit=` | Letters of one tenant (default tenant when `tenant` is absent), newest first, `limit` 100 by default. Endpoint userinfo is redacted in the listing. |
| `POST /q/dead-letters/{id}/replay?tenant=` | One more attempt through the same binding under the egress policy of the moment: `204` and the letter is deleted, or `502` with the failure text and the letter kept (`attempts`, `lastError`, `lastAt` extended). |
| `DELETE /q/dead-letters/{id}?tenant=` | Drop the letter. `404` when the tenant holds no such letter. |

`/q/health` reports `deadLetters`, the letters this process wrote since
start; the letters themselves are rows, so they survive restarts on the
file, postgres and timescale stores and a tenant purge removes them with
the rest of the tenant. Egress-policy refusals (private ranges, blocked
schemes) are never retried and never dead-lettered: a policy verdict is
not a transport failure.

## Backup, per store mode

The README's store-mode table is the authority; the operational short form:

| Mode | Backup |
|---|---|
| `memory` | nothing to back up |
| `file` | **stop-copy only** — stop the broker, copy `antares.redb`, restart (redb holds an exclusive lock; a live copy can tear) |
| `postgres` / `timescale` | ordinary Postgres backup/PITR; the outbox, docs and temporal tables all live in the one database |

## Rolling update

`dev/rolling-update.sh` — one instance at a time against the HA compose
stack: SIGTERM → `/q/health` flips 503 → haproxy ejects within 400 ms →
in-flight requests finish → recreate on the current image → wait healthy +
rise window before the next instance. Preconditions and env are documented
in the script header. **file mode cannot roll** (redb allows one process per
volume — K10): use a `Recreate` strategy there, as `broker-file.yaml` does.

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

## Where the proofs run

| Claim | Workflow |
|---|---|
| ETSI conformance, per-commit gate (file/postgres/timescale × 8 suites) | ci.yml → etsi-matrix.yml `preset: quick` (every push) |
| ETSI conformance, FULL seven cells (memory/file/postgres/timescale + the two rolling role-fleet cells + wasm-file) × 8 suites | full.yml (twice a week + `v*` tags + dispatch); its bundle feeds [the report page](https://antares-ngsi-ld-demo.marek-mraz.com/reports/latest/) + per-cell badges |
| The browser artifact serves the full API from a container (file store, serial suites + IOP) | the `wasm-file` matrix cell (`WASM=1 WASM_DOCKER=1 STORE=file` through the one pipeline — Dockerfile.wasm, the same www/pkg bytes a page loads) |
| Zero-downtime rolling update | `roll-weekly` (Tue 04:17 UTC + dispatch) + the full-run `-nats` matrix cells (10-pod fleet rolling under the whole suite) |
| Role-pair exactly-once semantics (duplicated matcher/notifier/temporal/registry pods) | ci.yml nats job (`nats_e2e::role_pairs_exactly_once_semantics`, live PG + NATS) |
| NATS bus + role split e2e | ci.yml nats job (`nats_e2e`, live PG + NATS) |
| k8s manifests boot | k8s-smoke.yml kind smoke (dispatch) |
| Coverage | strict.yml coverage job (daily line-coverage floor) + etsi-coverage.yml (Mon 04:41 UTC) → merged lcov/html on the report page |
