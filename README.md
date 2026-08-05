# Antares

**An NGSI-LD Context Broker in Rust.**


Antares is the brightest star in the Scorpius constellation — and a rust-red
supergiant. It follows the NGSI-LD broker naming tradition (Orion, Scorpio,
Stellio) and reimplements the broker in Rust with hard resource targets.

> Status: the full store ladder (`memory → file → postgres → timescale`),
> NATS JetStream scale-out with split roles, HTTP + MQTT notifications,
> federation and the security wall are implemented, with the ETSI Robot suite
> green in every store mode. Architecture:
> [docs/deep-analysis.md](docs/deep-analysis.md); remaining work and its
> hardware/decision blockers: [tasks.md](tasks.md).

## Targets (v1 contract)

| Dimension | Target |
|---|---|
| Entities | 10,000,000 in one PostgreSQL |
| Tenants | 1,000 — **one shared schema**, `tenant_id` + Row-Level Security |
| Subscriptions | 10,000 active (HTTP + MQTT delivery) |
| CSource registrations | 1,000+ per tenant (broad federation) |
| Broker memory | < 500 MB RSS (CI gate: 350 MiB during the ETSI suite) |
| Postgres memory | < 16 GB, PostGIS required, **TimescaleDB optional** (two temporal modes) |
| Compliance | ETSI CIM 009 V1.9.1, gated on the ETSI Robot test suite |

## Architecture in one paragraph

One binary (`antares --roles api,matcher,notifier,temporal,registry`), one
Postgres, decoupling via NATS JetStream (or `bus=local` for single-node: no
infrastructure beyond Postgres). Durable state lives in Postgres only; change
events fan out on the `ANTARES_CHANGES` stream; subscriptions mirror through a
JetStream KV bucket — there is no instance-sync protocol to break. Business
logic is Rust with parameterized DML (no PL/pgSQL, no ORM). Full rationale,
Scorpio reference mapping, and the improvement catalogue:
[docs/deep-analysis.md](docs/deep-analysis.md).

## Store modes

One binary, one config value (`ANTARES_STORE`), same API in every mode:

| Mode | Backend | Durability | Extra config | Backup |
|---|---|---|---|---|
| `memory` (default) | in-memory maps | none — state dies with the process | — | n/a |
| `file` | memory + [redb](https://www.redb.org/) write-through shadow | fsync **before** every ack (commit-before-ack; a `kill -9` after a 201 loses nothing) | `ANTARES_DATA_DIR` (must be a mounted volume) | **stop-copy only**: stop the broker, copy `antares.redb`, restart. A live copy of the open file is unsupported (redb holds an exclusive lock; a mid-commit copy can tear) |
| `postgres` | PostgreSQL + PostGIS | WAL | `ANTARES_DATABASE_URL` | ordinary Postgres backup/PITR (`pg_dump`, base backup + WAL) |
| `timescale` | postgres + TimescaleDB temporal | WAL | `ANTARES_DATABASE_URL` | as `postgres`; chunk-aware tooling applies to `attr_instances` |

`file` mode notes: queries and subscription matching still run on the
in-memory maps — redb is durability only, so working-set RAM grows with the
dataset (measured 2026-08-04: ~19 KB RSS per typical entity — expanded doc +
temporal mirror as `serde_json::Value` — rule of thumb: ~10k entities
(~200 MB, comfortably inside the 350 MiB gate); beyond that, move up a rung to
`postgres`). The on-disk file carries a format
version and the broker refuses to start on a mismatch or corruption rather
than serve partial data. Measured cost on a dev box: ~3.1k fsynced writes/s,
commit p50 0.21 ms (the cost is the fsync, not redb); batch operations commit
once per entity write, and redb has a single writer, so this is a per-process
ceiling (tasks.md B13).

## Build & test

```bash
cargo test --workspace          # unit + property tests, no services needed
cargo run -p antares-broker     # serves http://0.0.0.0:9090
curl -s localhost:9090/q/health
```

## Observability

```bash
curl -s localhost:9090/q/health    # liveness + store mode; 503 DRAINING during a roll
curl -s localhost:9090/q/metrics   # Prometheus text format, antares_ prefixed
```

Metrics follow Prometheus conventions with the `antares_` prefix and unit
suffixes: `antares_http_requests_total` (method + status class),
`antares_http_request_duration_seconds`, `antares_notifications_sent_total` /
`_failed_total` (by sink scheme), `antares_change_lag_seconds` (bus=nats:
publish → matcher processing), `antares_draining` (a rolling update is
visible on a dashboard for its whole duration), plus jemalloc heap gauges and
the I2 bounds-wall rejection counters.

Distributed traces export over OTLP/HTTP when `ANTARES_OTLP_ENDPOINT` is set
(e.g. `http://collector:4318/v1/traces`); unset — the default — costs
nothing. For `tokio-console` in dev, build the broker with
`--features console` and `RUSTFLAGS="--cfg tokio_unstable"`.

## ETSI conformance suite

```bash
dev/etsi-local.sh                       # local gate: workspace tests + ONE store mode (default memory)
STORE=timescale dev/etsi-local.sh       # the mode you are touching
STORE=postgres STOP_ON_ERROR=1 dev/etsi-pipeline.sh   # single suite/mode while debugging
```

**Locally: one store mode — the one you are touching.** A dev box runs the
cells serially, so all four modes cost ~4× wall-clock for a signal CI already
produces. **CI runs all four modes in parallel** (`.github/workflows/ci.yml`,
a 4 × 8 store × suite matrix, `fail-fast: false`, one image build feeding
every cell) and is the authority; `:latest` publishes only when all 32 cells
are green. `STORE=all dev/etsi-local.sh` reproduces the full matrix locally
when you actually need it.

The [ngsi-ld-test-suite](https://forge.etsi.org/rep/cim/ngsi-ld-test-suite)
is vendored at `ngsi-ld-test-suite/` (override with `SUITE=...`) — same
serial-run recipe as ScorpioBroker's `dev/etsi-serial.sh`, which this repo
uses as its reference implementation. Per-suite pass count is the only
progress metric.

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
docs/                     deep analysis + ADRs
dev/                      run/test scripts        compose-files/  local stack
```

## License

BSD-3-Clause (same family as Scorpio).
