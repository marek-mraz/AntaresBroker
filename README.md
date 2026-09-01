# Antares

**An NGSI-LD Context Broker in Rust.**

[![ci](https://github.com/marek-mraz/AntaresBroker/actions/workflows/ci.yml/badge.svg)](https://github.com/marek-mraz/AntaresBroker/actions/workflows/ci.yml)
[![strict](https://github.com/marek-mraz/AntaresBroker/actions/workflows/strict.yml/badge.svg)](https://github.com/marek-mraz/AntaresBroker/actions/workflows/strict.yml)
[![roll-weekly](https://github.com/marek-mraz/AntaresBroker/actions/workflows/roll-weekly.yml/badge.svg)](https://github.com/marek-mraz/AntaresBroker/actions/workflows/roll-weekly.yml)
[![ETSI conformance](https://img.shields.io/endpoint?url=https%3A%2F%2Fantares-ngsi-ld-demo.marek-mraz.com%2Freports%2Fbadge.json)](https://antares-ngsi-ld-demo.marek-mraz.com/reports/latest/)
[![unit tests](https://img.shields.io/endpoint?url=https%3A%2F%2Fantares-ngsi-ld-demo.marek-mraz.com%2Freports%2Fbadge-unit.json)](https://antares-ngsi-ld-demo.marek-mraz.com/reports/unit/)
[![coverage](https://img.shields.io/endpoint?url=https%3A%2F%2Fantares-ngsi-ld-demo.marek-mraz.com%2Freports%2Fcoverage-badge.json)](https://antares-ngsi-ld-demo.marek-mraz.com/reports/coverage/)
[![license: EUPL-1.2](https://img.shields.io/badge/license-EUPL--1.2-blue)](LICENSE)
[![release](https://img.shields.io/github/v/release/marek-mraz/AntaresBroker?include_prereleases)](https://github.com/marek-mraz/AntaresBroker/releases)

Per-cell ETSI results, live from the latest matrix run on `master`
(single-broker cells, plus the two `-nats` cells where the 10-container
role-split fleet rolls continuously under the whole suite):

[![memory](https://img.shields.io/endpoint?url=https%3A%2F%2Fantares-ngsi-ld-demo.marek-mraz.com%2Freports%2Fbadge-memory.json)](https://antares-ngsi-ld-demo.marek-mraz.com/reports/latest/)
[![file](https://img.shields.io/endpoint?url=https%3A%2F%2Fantares-ngsi-ld-demo.marek-mraz.com%2Freports%2Fbadge-file.json)](https://antares-ngsi-ld-demo.marek-mraz.com/reports/latest/)
[![postgres](https://img.shields.io/endpoint?url=https%3A%2F%2Fantares-ngsi-ld-demo.marek-mraz.com%2Freports%2Fbadge-postgres.json)](https://antares-ngsi-ld-demo.marek-mraz.com/reports/latest/)
[![timescale](https://img.shields.io/endpoint?url=https%3A%2F%2Fantares-ngsi-ld-demo.marek-mraz.com%2Freports%2Fbadge-timescale.json)](https://antares-ngsi-ld-demo.marek-mraz.com/reports/latest/)
[![postgres-nats](https://img.shields.io/endpoint?url=https%3A%2F%2Fantares-ngsi-ld-demo.marek-mraz.com%2Freports%2Fbadge-postgres-nats.json)](https://antares-ngsi-ld-demo.marek-mraz.com/reports/latest/)
[![timescale-nats](https://img.shields.io/endpoint?url=https%3A%2F%2Fantares-ngsi-ld-demo.marek-mraz.com%2Freports%2Fbadge-timescale-nats.json)](https://antares-ngsi-ld-demo.marek-mraz.com/reports/latest/)
[![wasm-file](https://img.shields.io/endpoint?url=https%3A%2F%2Fantares-ngsi-ld-demo.marek-mraz.com%2Freports%2Fbadge-wasm-file.json)](https://antares-ngsi-ld-demo.marek-mraz.com/reports/latest/)



Antares is the brightest star in the Scorpius constellation — and a rust-red
supergiant. It follows the NGSI-LD broker naming tradition (Orion, Scorpio,
Stellio) and reimplements the broker in Rust with hard resource targets.

## Why Antares

Three properties, each backed by a number CI reproduces on every full run:

1. **Footprint** — ~35 MiB average RSS (38–59 MiB peak) while running the
   complete ETSI conformance suite, ~9 MiB idle. The full store ladder fits
   where a JVM broker's heap alone would not.
2. **Conformance** — 1812/1812 ETSI CIM 009 V1.9.1 test cases green in
   every one of the six native store cells, and 1800/1800 in the browser
   cell, where the twelve MQTT cases have no broker socket to run against
   — including the two cells where a 10-container role-split fleet rolls
   continuously under the suite
   ([per-store report with Robot drill-down](https://antares-ngsi-ld-demo.marek-mraz.com/reports/latest/)).
   The methodology is a per-clause ledger over the whole spec text
   (`docs/spec/`, 947 clause files), not only the official TP list.
3. **The browser build** — the same broker compiles to a 4.05 MB wasm
   artifact (1.55 MB gzipped) and serves `/ngsi-ld/v1/*` from a Service
   Worker inside a web page: an NGSI-LD broker with zero installation, for
   demos, edge devices and offline-first tooling. No other NGSI-LD broker
   has this.

## 60-second quickstart

```bash
docker run --rm -p 9090:9090 ghcr.io/marek-mraz/antares-broker:dev
```

Create an entity and query it back (no infrastructure — the default is the
in-memory store):

```bash
curl -i -X POST localhost:9090/ngsi-ld/v1/entities \
  -H 'Content-Type: application/ld+json' \
  -d '{"id":"urn:ngsi-ld:TemperatureSensor:001","type":"TemperatureSensor",
       "temperature":{"type":"Property","value":21.5,"unitCode":"CEL"},
       "@context":"https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld"}'
# HTTP/1.1 201 Created
# Location: /ngsi-ld/v1/entities/urn:ngsi-ld:TemperatureSensor:001

curl -s 'localhost:9090/ngsi-ld/v1/entities?type=TemperatureSensor'
# [{"id":"urn:ngsi-ld:TemperatureSensor:001","type":"TemperatureSensor",
#   "temperature":{"type":"Property","unitCode":"CEL","value":21.5}}]
```

No Docker? `cargo run -p antares-broker` serves the same API on :9090
(MSRV: Rust 1.97).

> Status: the full store ladder (`memory → file → postgres → timescale`),
> NATS JetStream scale-out with split roles, HTTP + MQTT notifications,
> federation and the security wall are implemented, with the ETSI Robot suite
> green in every store mode ([the conformance report, per store, with each
> run's Robot drill-down](https://antares-ngsi-ld-demo.marek-mraz.com/reports/latest/)).
> Documentation: the [book](https://antares-ngsi-ld-demo.marek-mraz.com/docs/),
> sections listed below.

## Documentation

Maintainers start at [ARCHITECTURE.md](ARCHITECTURE.md): the crate and
module map, the request and change flows, the invariants, and what to
touch for a given kind of change.

The book is published at <https://antares-ngsi-ld-demo.marek-mraz.com/docs/>
and built from `docs/src`:

| section | what it covers |
|---|---|
| [Getting started](docs/src/getting-started.md) | install, first entity, first subscription, first federation pair |
| [Configuration](docs/src/configuration.md) | every `ANTARES_*` variable, with defaults |
| [Deployment](docs/src/deployment.md) | Docker, compose stacks, Kubernetes manifests, the role split |
| [Subscriptions and notifications](docs/src/subscriptions.md) | HTTP and MQTT delivery, formats, grouping, retries, dead letters |
| [Temporal API](docs/src/temporal.md) | recording, querying, aggregation, pagination, retention |
| [Federation](docs/src/federation.md) | registrations, forwarding, loop protection, partial failures |
| [Operations](docs/src/operations.md) | health, observability, tenants, backup and restore, bulk load, rolling updates |
| [Browser & WebAssembly](docs/src/wasm.md) | the in-page broker |
| [Admin API](docs/src/admin-api.md) | the `/q/*` routes |
| [Storage drivers](docs/src/storage.md) | the store ladder, the temporal driver, measured costs, migrations |
| [Conformance](docs/src/conformance.md) | the ledger, the matrix, how to run a suite, upstream raises |
| [Performance](docs/src/performance.md) | the weekly shape and scale runs, how each number is produced, how to reproduce them |
| [Shared crates](docs/src/shared-crates.md) | the parser, query engine and matcher as libraries for a gateway |
| [Extending Antares](docs/src/extending.md) | features, the driver registry, hooks, adding a backend |
| [Decisions](docs/adr/README.md) | the architecture decision records |

## Targets (the design contract)

These are the DESIGN targets the architecture is sized for. What CI proves
today is the ETSI conformance matrix, the resource gate and the
rolling-update drill; load rigs at the 100M-entity scale need hardware the
project does not have yet.

| Dimension | Target |
|---|---|
| Entities | 100,000,000 — current-state, one PostgreSQL cluster |
| Tenants | 10,000 — **one shared schema**, `tenant_id` + Row-Level Security |
| Subscriptions | 100,000 per broker (HTTP + MQTT delivery) |
| CSource registrations | 100,000+ per broker — matching stays index-shaped, fan-out bounded |
| Storage | PostgreSQL with PostGIS, **TimescaleDB optional** (two temporal modes) |
| Compliance | full NGSI-LD (ETSI CIM 009 V1.9.1), gated on the ETSI Robot suite + this repo's extension TPs |
| HA | stateless broker pods, NATS JetStream, Postgres primary/replica |

The weekly `scale-weekly` run on `master` measures a fixed fraction of
these rows: 1 % of the entity and tenant targets (1 M entities over 100
tenants) with 10,000 subscriptions and 10,000 registrations, on one rented
box, broker and Postgres resident set and CPU sampled per phase. The full
targets need more than the one hour that box lives, so no run has reached
them. The tables and CSVs of the newest run are at
<https://antares-ngsi-ld-demo.marek-mraz.com/reports/perf/latest/>, and the
[Performance](docs/src/performance.md) chapter says how each number is made.

## Architecture in one paragraph

One binary (`antares --roles api,matcher,notifier,temporal,registry`), one
Postgres, decoupling via NATS JetStream (or `bus=local` for single-node: no
infrastructure beyond Postgres). Durable state lives in Postgres only; change
events fan out on the `ANTARES_CHANGES` stream; subscriptions mirror through a
JetStream KV bucket — there is no instance-sync protocol to break. Business
logic is Rust with parameterized DML (no PL/pgSQL, no ORM). The rationale
behind each irreversible choice is written down in [docs/adr/](docs/adr/).

## Store modes

One binary, one config value (`ANTARES_STORE`), same API in every mode.
Rule of thumb: `memory` for tests and demos, `file` for a durable single
node without a database (edge boxes, small deployments), `postgres` for
production, `timescale` when temporal queries dominate. Two orthogonal
switches on top: `ANTARES_BUS` (`local` single-node default, `nats` for
multi-pod scale-out and HA) and MQTT notifications (built in; enabled per
subscription endpoint, `mqtt[s]://` URI). Operations detail:
[docs/src/operations.md](docs/src/operations.md).

| Mode | Backend | Durability | Extra config | Backup |
|---|---|---|---|---|
| `memory` (default) | in-memory maps | none — state dies with the process | — | n/a |
| `file` | memory + [redb](https://www.redb.org/) write-through shadow | fsync **before** every ack (commit-before-ack; a `kill -9` after a 201 loses nothing) | `ANTARES_DATA_DIR` (must be a mounted volume) | **stop-copy only**: stop the broker, copy `antares.redb`, restart. A live copy of the open file is unsupported (redb holds an exclusive lock; a mid-commit copy can tear) |
| `postgres` | PostgreSQL + PostGIS | WAL | `ANTARES_DATABASE_URL` | ordinary Postgres backup/PITR (`pg_dump`, base backup + WAL) |
| `timescale` | postgres + TimescaleDB temporal | WAL | `ANTARES_DATABASE_URL` | as `postgres`; chunk-aware tooling applies to `attr_instances` |

`file` mode notes: queries and subscription matching still run on the
in-memory maps — redb is durability only, so working-set RAM grows with the
dataset (measured: ~19 KB RSS for a SMALL entity of a few hundred bytes of
compact JSON — expanded doc + temporal mirror as `serde_json::Value` — rule
of thumb: ~10k of those, ~200 MB, comfortably inside the 350 MiB gate). The
cost scales with the document: the 1.5 KB entity the
[storage chapter](docs/src/storage.md#measured-storage-cost) measures costs
37.6 KB, so size from your own payload rather than from that rule of thumb;
beyond it, move up a rung to `postgres`. The on-disk file carries a format
version and the broker refuses to start on a mismatch or corruption rather
than serve partial data. Measured cost on a dev box: ~3.1k fsynced writes/s,
commit p50 0.21 ms (the cost is the fsync, not redb); batch operations commit
once per entity write, and redb has a single writer, so this is a per-process
ceiling.

## Run with Docker

```bash
docker run --rm -p 9090:9090 ghcr.io/marek-mraz/antares-broker:dev
curl -s localhost:9090/q/health
```

Defaults need zero infrastructure: `memory` store, `local` bus, all roles.
Other store modes via env:

```bash
# file store — durable, no Postgres; the data dir must be a mounted volume
docker run --rm -p 9090:9090 \
  -e ANTARES_STORE=file -e ANTARES_DATA_DIR=/data \
  -v antares-data:/data \
  ghcr.io/marek-mraz/antares-broker:dev

# postgres / timescale store
docker run --rm -p 9090:9090 \
  -e ANTARES_STORE=postgres \
  -e ANTARES_DATABASE_URL=postgresql://antares:antares@db:5432/antares \
  ghcr.io/marek-mraz/antares-broker:dev
```

Or the local compose stacks (broker + PostGIS + NATS + mosquitto):

```bash
docker compose -f compose-files/docker-compose.yml up          # one broker, postgres store
docker compose -f compose-files/docker-compose-ha.yml up       # 2 replicas + haproxy + NATS (the rolling-update shape)
```

**Role-split fleet** (the scale-out shape): 5 roles × 2 replicas = 10 broker
containers from the same binary — `api`×2 behind haproxy (the only pods
serving the NGSI-LD API), `matcher`×2 + `notifier`×2 on one shared JetStream
durable, `temporal`×2 and `registry`×2 as ops-only pods — one shared
Postgres, `ANTARES_BUS=nats`:

```bash
STORE=postgres docker compose -f compose-files/docker-compose-etsi.yml \
  -f compose-files/docker-compose-roles.yml --profile db up -d
dev/roles-smoke.sh        # fleet ready + notify chain fires EXACTLY once
STORE=postgres ROLES_SPLIT=1 bash dev/rolling-update.sh   # roll all 10, role-group order
```

This is the shape the CI `postgres-nats`/`timescale-nats` cells gate on
every push: the full ETSI suite through the LB while the fleet rolls
continuously in role-group order (never 0 healthy pods per role), plus the
`nats_e2e` pair semantics — one change → exactly one notification across
the duplicated matcher/notifier pods, single-winner interval firings,
exactly-once temporal recording.

Tags: `:dev` = latest green master, `:dev-<run>` = a specific CI run,
`:latest` = latest release. Pre-1.0 there is no release, so `:latest` does
not resolve yet and every command here pulls `:dev`. Images are multi-arch
(linux/amd64 + linux/arm64).
The amd64 image is the exact bytes the ETSI gates tested; the arm64 half is
built natively but not gated (the workspace tests run natively on arm).
Idle RSS ≈ 9 MiB.

## Browser build (WebAssembly)

The same broker compiles to `wasm32-unknown-unknown` and runs entirely in a
web page — no server, no install (ADR-0008). A module Service
Worker answers `/ngsi-ld/v1/*` in-tab; `www/index.html` is the demo (create
entities, subscribe, watch notifications arrive in-page).

```bash
./dev/install-wasm-tools.sh   # wasm-bindgen (lockfile-matched) + wasm-opt
./dev/wasm-build.sh           # → www/pkg (≤8 MB raw / ≤3 MB gzip budget; 4.05/1.55 today)
node www/node-shim.mjs 9090   # the SAME .wasm behind a real TCP port (Node ≥18)
./dev/wasm-test.sh            # Node smoke + headless-Chromium page test
```

Scope: memory store + local bus only — no NATS, MQTT, Postgres or roles in a
page. Conformance: the **Node tier** (shim) is the gate for the serial ETSI
suites; in-browser, federation and external HTTP callbacks are structurally
out of reach (no inbound sockets, CORS) and stay covered by the Node tier.

## Build & test

```bash
cargo test --workspace          # unit + property tests, no services needed
cargo run -p antares-broker     # serves http://0.0.0.0:9090
curl -s localhost:9090/q/health
```

## Observability

```bash
curl -s localhost:9090/q/health    # liveness + store mode (+ bus state under bus=nats); 503 DRAINING during a roll
curl -s localhost:9090/q/ready     # readiness: store ping + bus connected — what the k8s readinessProbe polls
curl -s localhost:9090/q/metrics   # Prometheus text format, antares_ prefixed
```

Metrics follow Prometheus conventions with the `antares_` prefix and unit
suffixes: `antares_http_requests_total` (method + status class),
`antares_http_request_duration_seconds`, `antares_notifications_sent_total` /
`_failed_total` (by sink scheme), `antares_change_lag_seconds` (bus=nats:
publish → matcher processing), `antares_draining` (a rolling update is
visible on a dashboard for its whole duration), plus jemalloc heap gauges and
the bounds-wall rejection counters.

Distributed traces export over OTLP/HTTP when `ANTARES_OTLP_ENDPOINT` is set
(e.g. `http://collector:4318/v1/traces`); unset — the default — costs
nothing. For `tokio-console` in dev, build the broker with
`--features console` and `RUSTFLAGS="--cfg tokio_unstable"`.

## ETSI conformance suite

The latest per-store results are a click away — stats on the page, each
suite linking Robot's own drill-down:
**<https://antares-ngsi-ld-demo.marek-mraz.com/reports/latest/>** (rebuilt
with every Pages deploy from the newest `etsi-matrix` bundle). How to read
the matrix — cells, the ledger methodology, caveats:
[the conformance chapter](https://antares-ngsi-ld-demo.marek-mraz.com/docs/conformance.html).

```bash
dev/etsi-local.sh                       # local gate: workspace tests + ONE store mode (default memory)
STORE=timescale dev/etsi-local.sh       # the mode you are touching
STORE=postgres STOP_ON_ERROR=1 dev/etsi-pipeline.sh   # single suite/mode while debugging
```

**Locally: one store mode — the one you are touching.** A dev box runs the
cells serially, so every mode costs its own wall-clock for a signal CI
already produces. **CI gates every commit on the QUICK preset** — file,
postgres, timescale (`ci.yml` → `etsi-matrix.yml`, one image build feeding
the cells) — and **runs the FULL seven-cell matrix twice a week**
(`full.yml`, also on `v*` tags and manual dispatch): those three plus
memory, `postgres-nats`/`timescale-nats` — the 10-container role-split
fleet rolling continuously under the whole suite — and `wasm-file`: the
BROWSER artifact, five dockerized Node shims over the redb file store,
serial suites + IOP with MQTT structurally excluded. The report page and
the per-cell badges always render the newest FULL run. `STORE=all
dev/etsi-local.sh` reproduces the store matrix locally when you actually
need it; `STORE=postgres ROLES_SPLIT=1 ROLL_DURING_RUN=1 dev/etsi-pipeline.sh`
reproduces a rolling-fleet cell; `WASM=1 WASM_DOCKER=1 STORE=file
dev/etsi-pipeline.sh` reproduces the wasm cell.

The [ngsi-ld-test-suite](https://forge.etsi.org/rep/cim/ngsi-ld-test-suite)
is vendored at `ngsi-ld-test-suite/` (override with `SUITE=...`) — same
serial-run recipe as ScorpioBroker's `dev/etsi-serial.sh`, which this repo
uses as its reference implementation. Per-suite pass count is the only
progress metric. The vendored copy is a fork: it carries a handful of
test-side fixes (fixtures and keywords the clause text contradicts), each
with its rationale and the issue text ready to file upstream in
[`docs/upstream/etsi-raises.md`](docs/upstream/etsi-raises.md); no test is
weakened to fit the broker. The suite as published by ETSI is not
aligned with CIM 009 V1.9.1: some TPs assert behaviour the clause
text contradicts and some do not run as shipped, so a 100 % pass rate is
only meaningful against a fork that names every test-side change.

## Repository layout

```
crates/antares-model      NGSI-LD types (publishable as ngsild-model)
crates/antares-ql         q=/scopeQ/geoQ parsers -> AST (publishable as ngsild-ql)
crates/antares-jsonld     @context cache + core-context fast path
crates/antares-sql        AST -> parameterized SQL + migrations
crates/antares-bus        ChangeEvent bus: in-process ring or NATS JetStream
crates/antares-api        HTTP binding (axum), thin handlers
crates/antares-{matcher,notifier,registry}            domain crates
crates/antares-broker     composition root -> the `antares` binary
docs/                     conformance ledger + ADRs + user book source
dev/                      run/test scripts        compose-files/  local stack
```

## How Antares compares

Architecture facts, each checkable in the respective project's docs; the
Antares numbers are measured by this repo's CI (links above). Conformance
claims for other brokers are theirs to make — check each project's own
reporting.

| | Antares | [Scorpio](https://github.com/ScorpioBroker/ScorpioBroker) | [Orion-LD](https://github.com/FIWARE/context.Orion-LD) | [Stellio](https://github.com/stellio-hub/stellio-context-broker) | [coraine](https://github.com/seamware/coraine) |
|---|---|---|---|---|---|
| Language / runtime | Rust, one native binary | Java (Quarkus, JVM) | C | Kotlin (Spring, JVM) | C, core + `dlopen` plugins ([plugin-architecture.md](https://github.com/seamware/coraine/blob/main/doc/plugin-architecture.md)) |
| Primary storage | memory / redb file / PostgreSQL+PostGIS / TimescaleDB | PostgreSQL+PostGIS ([README](https://github.com/ScorpioBroker/ScorpioBroker/blob/development/README.md)) | MongoDB, with PostgreSQL+PostGIS+TimescaleDB as the temporal sink ([troe.md](https://github.com/FIWARE/context.Orion-LD/blob/develop/doc/manuals-ld/troe.md)) | PostgreSQL + PostGIS + TimescaleDB ([docker-compose.yml](https://github.com/stellio-hub/stellio-context-broker/blob/develop/docker-compose.yml)) | plugin: MongoDB (libmongoc) or in-memory, TimescaleDB plugin for temporal ([plugin-architecture.md](https://github.com/seamware/coraine/blob/main/doc/plugin-architecture.md)) |
| Message bus | none (`local`) or NATS JetStream | Kafka ([README](https://github.com/ScorpioBroker/ScorpioBroker/blob/development/README.md)) | none | Kafka ([docker-compose.yml](https://github.com/stellio-hub/stellio-context-broker/blob/develop/docker-compose.yml)) | none |
| Minimum footprint | one binary, zero infrastructure (`memory`/`file`) | JVM + PostgreSQL | broker + MongoDB | JVM + PostgreSQL + Kafka | broker + in-memory plugin (no persistence) |
| Measured RSS under the full ETSI suite | ~35 MiB avg / 59 MiB peak (CI, every matrix run) | JVM heap-sized | — | JVM heap-sized | — |
| Browser/wasm build | yes — 4.05 MB artifact, full API in a Service Worker | no | no | no | no |
| Conformance evidence | [public per-store matrix, Robot drill-down](https://antares-ngsi-ld-demo.marek-mraz.com/reports/latest/) | see project | see project | see project | see project |

All five speak the same ETSI CIM 009 API — that is the point of the
standard. Antares is a compliant peer of the FIWARE-ecosystem brokers, not
a fork of any of them; pick per deployment constraints and verify with the
suite.

## License

[EUPL-1.2](LICENSE) — the European Union Public Licence: copyleft,
EU-institution vetted, compatible with public-sector procurement across
member states. Commercial licensing: contact@marek-mraz.com.
