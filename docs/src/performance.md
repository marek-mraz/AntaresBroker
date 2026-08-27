# Performance

Two scheduled runs on `master` produce every performance number this
project publishes; nothing in this chapter is typed in by hand. Both rent
a dedicated-vCPU machine at Hetzner for the run, register it as an
ephemeral GitHub runner, and delete it afterwards (`perf-janitor` sweeps a
leaked server or volume by its expiry label). Results land under
[`/reports/perf/latest/`](https://antares-ngsi-ld-demo.marek-mraz.com/reports/perf/latest/)
with the raw CSVs next to the tables.

| run | box | what it measures | cadence |
|---|---|---|---|
| `perf-weekly` | ccx23 (4 vCPU, 16 GB) | the request shapes other brokers publish, on the in-memory store | Saturday |
| `scale-weekly` | ccx43 (16 vCPU, 64 GB) + 500 GB volume | the design targets on PostgreSQL | Sunday |

## The shapes (`perf-weekly`)

Every script lives in `dev/perf/` and runs on a laptop the same way it
runs in CI; `k6` is the only tool it needs.

| table | script | method |
|---|---|---|
| startup and idle footprint | `startup.sh` | exec to the first `200` from `/q/health`, median of five, `VmRSS` right after, per store |
| throughput per shape | `shapes.sh` | 100 five-attribute entities; `GET /entities?type=Vehicle&limit=20` at 50 and 200 concurrent clients, `GET /entities/{id}` at 50; five seconds, median of three runs, p99 from the same runs |
| core scaling | `core-scale.sh` | broker pinned to 1, 2, 4, 8 physical cores with `taskset`, load generator on the remaining cores; refuses a step it cannot isolate |
| saturation knee | `saturate.sh` | open model, +500 rps every 30 s until p99 passes 50 ms or errors pass 0.1 %; the knee is the last stage that held, the curve is a CSV |
| noise profile | `variance.py` | the same commit measured ten times; the fence for a future regression gate is `Q3 + 3·IQR` of each metric's own history |

The load generator shares the machine with the broker, as in every
published broker table; the numbers describe that shape and nothing
else, and quadrupling the concurrency shows the queue, not the broker.
Reproduce them on your own box before quoting them:

```bash
cargo build --release -p antares-broker
dev/perf/startup.sh && dev/perf/shapes.sh && dev/perf/core-scale.sh && dev/perf/saturate.sh
python3 dev/perf/report.py results/perf     # index.html + perf.json
```

## The design targets (`scale-weekly`)

The README's target table is a design contract; this run is where each
row gets its measured column. `SCALE` scales every count linearly, so the
same rig runs at `0.0001` against a laptop's Postgres and at `1.0` on the
rented box:

| stage | script | at scale 1.0 |
|---|---|---|
| entities | `gen.py` streaming into `dev/bulk-load.sh` (one COPY stream) | 100,000,000 over 10,000 tenants, five attributes and a location each |
| subscriptions | `api-load.py subscriptions` | 100,000, one per tenant round-robin, HTTP to the sink, every tenth over MQTT |
| registrations | `api-load.py registrations` | 100,000, one id pattern each, endpoint at the sink |
| resident set | `rss.sh` | broker and Postgres backends sampled at 1 Hz for the whole run; the verdict asserts broker < 500 MiB and Postgres < 16 GiB only at scale 1.0 |
| at load | `shapes.sh`, `saturate.sh`, a 1,000 rps update stream | throughput per tenant, the knee, notifications delivered per second at the sink |

`dev/perf/sink.py` is the other end of every subscription and
registration: it counts notifications and answers forwarded queries with
an empty list, so the fan-out over 100,000 registrations costs the broker
the matching and the HTTP round trips and nothing else.

Dispatch `scale-weekly` with `scale=0.01` for a one-hour dry run of the
whole rig; the schedule runs `1.0`. Bulk load bypasses the broker
(no notifications, no history), which is the documented path for initial
loads in [Operations](operations.md#bulk-load-postgres-timescale).

## What the numbers are not

They are one machine, one request shape, one week. A different instance
type invalidates the whole history, which is why both workflows pin one.
No regression gate exists until the noise profile has ten runs on that
instance type; until then the runs report, and the report is the
evidence.
