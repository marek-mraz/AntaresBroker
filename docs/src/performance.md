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
| `perf-weekly` | ccx33 (8 dedicated vCPU, 32 GB), one hour | the request shapes other brokers publish, on the in-memory store; dispatched with `store=postgres` it adds the same tables against a PostgreSQL container on the box, at pool 20 and 100 (`pg-pool<N>/`) | Saturday |
| `scale-weekly` | ccx53 (32 dedicated vCPU, 128 GB) + volume, one hour | the design targets on PostgreSQL, at scale 0.01 | Sunday |

`scale-weekly` sizes the database to the box it rents: `shared_buffers` a
quarter of RAM, `maintenance_work_mem` a sixteenth, the shared memory
segment an eighth, and the broker's connection pool eight per core with
the eight-core value pinned to the 100 it was measured at. A number from
one server type therefore does not compare with a number from another;
the box is named next to every table.

## The shapes (`perf-weekly`)

Every script lives in `dev/perf/` and runs on a laptop the same way it
runs in CI; `k6` is the only tool it needs.

| table | script | method |
|---|---|---|
| startup and idle footprint | `startup.sh` | exec to the first `200` from `/q/health`, median of five, `VmRSS` right after, per store |
| throughput per shape | `shapes.sh` | 100 five-attribute entities; `GET /entities?type=Vehicle&limit=20` at 50 and 200 concurrent clients, `GET /entities/{id}` at 50 (`SPECS` picks other rows, the PostgreSQL dispatch runs 64, 256 and 1 024 clients); five seconds, median of three runs, p99 from the same runs. The `facade` and `facade-twin` shapes run the same pair only when the binary under test serves `/x/example/things` — a shipped build does not |
| core scaling | `core-scale.sh` | broker pinned to 1, 2, 4, 8 physical cores with `taskset`, load generator on the remaining cores; refuses a step it cannot isolate. `cores used` is the broker's CPU time over the window against the cores it was allotted; `peak threads` is the largest thread count of the process, which is what a store driver that parks threads instead of awaiting shows up in |
| saturation knee | `saturate.sh` | open model, +500 rps every 30 s until p99 passes 50 ms or errors pass 0.1 %; the knee is the last stage that held, the curve is a CSV; `cores used` and `peak threads` as in core scaling, over the whole sweep |
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

## The measured ceiling

Two `perf-weekly` dispatches. The first, on ccx33 (8 logical, 4 physical
cores, three passes) at commit `b55d554`, could isolate only the 1- and
2-core steps: pinning the broker to 4 cores needs 8 physical, and to 8
needs 16. The second, on ccx53 (32 logical, 16 physical cores, two passes)
at commit `41610be`, runs the whole ladder, and the tables below are that
run. `store=postgres` in both; the load generator shares the machine, so
every row is broker plus generator.

Core scaling, one row per store and pool: the broker pinned to N physical
cores (SMT siblings excluded), the generator on the rest, query shape at
50 concurrent clients. `cores used` is the broker's CPU time over the
window against the cores it was allotted.

| store | 1 core | 2 cores | 4 cores | 8 cores | efficiency at 2 / 4 / 8 | cores used at 8 | peak threads at 8 |
|---|---|---|---|---|---|---|---|
| memory | 3 076 req/s | 6 050 req/s | 11 342 req/s | 12 554 req/s | 98 % / 92 % / 51 % | 7.64 | 16 |
| postgres, pool 20 | 2 097 req/s | 3 809 req/s | 7 093 req/s | 7 238 req/s | 91 % / 85 % / 43 % | 6.47 | 73 |
| postgres, pool 100 | 2 026 req/s | 3 665 req/s | 7 157 req/s | 7 837 req/s | 90 % / 88 % / 48 % | 7.03 | 85 |

The same rows as CPU spent per request (`cores used` over req/s), which is
what the efficiency column is measuring underneath:

| store | 1 core | 2 cores | 4 cores | 8 cores |
|---|---|---|---|---|
| memory | 0.31 ms | 0.32 ms | 0.34 ms | 0.61 ms |
| postgres, pool 20 | 0.47 ms | 0.49 ms | 0.50 ms | 0.89 ms |
| postgres, pool 100 | 0.48 ms | 0.50 ms | 0.51 ms | 0.90 ms |

The saturation knee, whole box, open model:

| store | shape | knee | p99 at the knee | first failing stage | cores used | peak threads |
|---|---|---|---|---|---|---|
| memory | query | 5 000 rps | 1.2 ms | none reached | 0.96 | 39 |
| memory | write | 5 000 rps | 0.6 ms | none reached | 0.47 | 62 |
| postgres, pool 20 | query | 5 000 rps | 3.0 ms | none reached | 2.17 | 77 |
| postgres, pool 20 | write | 1 000 rps | 2.9 ms | 1 500 rps | 1.79 | 4 038 |
| postgres, pool 100 | query | 5 000 rps | 2.6 ms | none reached | 2.18 | 64 |
| postgres, pool 100 | write | 1 000 rps | 2.8 ms | 1 500 rps | 1.80 | 4 038 |

Throughput per shape at 64, 256 and 1 024 concurrent clients, whole box:

| store | shape | c64 | c256 | c1024 |
|---|---|---|---|---|
| memory | query | 19 272 req/s, p99 7.7 ms | 21 254 req/s, p99 36.6 ms | 21 566 req/s, p99 123.4 ms |
| memory | retrieve | 39 766 req/s, p99 3.8 ms | 43 805 req/s, p99 19.7 ms | 45 425 req/s, p99 68.1 ms |
| postgres, pool 20 | query | 12 418 req/s, p99 7.3 ms | 11 948 req/s, p99 23.2 ms | 11 448 req/s, p99 93.2 ms |
| postgres, pool 20 | retrieve | 12 432 req/s, p99 5.9 ms | 12 233 req/s, p99 22.3 ms | 11 766 req/s, p99 90.0 ms |
| postgres, pool 100 | query | 12 426 req/s, p99 7.8 ms | 11 823 req/s, p99 28.5 ms | 10 292 req/s, p99 104.0 ms |
| postgres, pool 100 | retrieve | 13 974 req/s, p99 6.0 ms | 14 144 req/s, p99 20.4 ms | 13 696 req/s, p99 75.7 ms |

The run predates ADR-0022: the Postgres driver still parked a thread per
in-flight store call, which is what the four-figure `peak threads` column
records. The knees and the per-request cost are what that shape delivered.

What the run says, in the order it matters:

- Scaling is close to linear to four cores (98 %, 92 % on memory; 91 %,
  85 % and 90 %, 88 % on the two pools) and loses half of that at eight.
- The eighth core is used, not idle: 7.64 of 8 on memory, 7.03 and 6.47
  on the two pools. What changes is the price of a request, which is flat
  from one core to four and then rises by about 80 % on all three stores.
  A cost that appears identically in the in-process store, which never
  parks a thread on a socket, is not the storage driver: it is contention
  above the store, in the path the three shapes share.
- The blocking pool is nowhere near its ceiling. The Postgres write shape
  parks 4 038 live OS threads at its knee against a ceiling of 11 024
  (`ANTARES_MAX_CONNECTIONS` plus 1 024, `main.rs`), and the query shapes
  park 64 to 77. Nothing deadlocks and no shape reaches the cap.
- Pool size barely moves anything on this box. Pool 100 is 8 % faster at
  eight cores and 10 % slower at 1 024 concurrent query clients; both
  pools hold the same 1 000 rps write knee and fail at the same 1 500.
  The ccx33 run's "a larger pool is worse" reads as an artefact of four
  physical cores, not a property of the pool.
- The write path is what bends first. Both Postgres pools hold 1 000 rps
  and fail at 1 500 while every query shape holds the harness ceiling of
  5 000 rps; the in-memory write path holds 5 000 rps on 0.47 cores.

### The update shape

The ladder above is the query shape. `perf-weekly` run `33797374897`, same
ccx53 box, runs it again against the update shape — the write path with the
notification pipeline behind it — once per store:

| store | 1 core | 2 cores | 4 cores | 8 cores | efficiency at 2 / 4 / 8 | cores used at 8 | peak threads at 8 |
|---|---|---|---|---|---|---|---|
| memory | 13 635 req/s | 19 498 req/s | 11 573 req/s | 12 538 req/s | 71 % / 21 % / 11 % | 6.78 | 44 |
| file | 3 870 req/s | 4 492 req/s | 3 744 req/s | 3 617 req/s | 58 % / 24 % / 12 % | 0.94 | 17 |

Updates do not merely scale worse than queries: past two cores they scale
backwards, and eight cores serve fewer requests per second than one while
burning 6.78 of them. Something serializes and the cores spend their time
arriving at it.

Not the store's map lock, which is what a single-tenant ladder would blame
and what tenant sharding would answer: this ladder drives one tenant, so
sharding by tenant would leave every request on the same shard and change
nothing. What the write path does that the query path does not is hand the
worker's queue to another thread — `block_in_place` — for every document it
writes, so the store can commit without stalling an async worker. In `file`
mode that commit is an fsync and the hop is the point. In `memory` mode
there is no commit: the write is a lock and a map insert, and the hop is the
whole cost, paid once per write and paid more the more cores there are to
hand work between. The hop is now taken only where something blocks; the
paths that hold the write section for a whole scan (the 4.22 sweep, the two
purges) still take it in either mode.

### The exit criterion

`perf-weekly` run `33683839528` sets what the runtime work has to hold. A
change to the request runtime, the drivers or the store keeps all of it:

- Efficiency at eight allotted physical cores, query at c50: at least
  51 % on memory and 43 % on either Postgres pool.
- CPU per request at eight cores: no more than twice its one-core value
  on any store. The run itself sits at 1.97 (memory), 1.89 (pool 20) and
  1.87 (pool 100).
- Saturation, whole box: the knee at 5 000 rps or better for every query
  shape and 1 000 rps or better for the Postgres write shape, p99 at the
  knee within 3.0 ms.
- Live OS threads at the write knee: 4 038 or fewer, against the 11 024
  ceiling.

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
| resident set | `rss.sh` | broker and Postgres backends sampled at 1 Hz for the whole run, peaks printed as the verdict table. Ceilings are opt-in through `BROKER_MIB` and `PG_GIB`, and only a run at scale 1.0 lets them fail the step; neither is set today, because a budget is read off these runs rather than asserted ahead of them |
| at load | `shapes.sh`, `saturate.sh` | throughput per tenant, the knee |
| subscriptions firing | `fire.sh` | update + delete streams over the loaded entities at 100, 200, 500, 1,000, 2,000 and 4,000 rps; every update fires each subscription of its tenant once, so the notifications due are known; the table shows due, delivered, the distinct subscriptions that fired, how long the sink kept receiving after the stream, failed operations by class (no HTTP answer / 4xx / 5xx) and the broker's own counters over the rate (changes the bounded matcher queue dropped, dead letters), so a delivery gap is attributed to the queue, the delivery policy or the receiver; the limit is the last rate that delivered 99% with no failed operation |
| per-class delivery | `fire.sh` → `fire-classes.md` | the subscriptions (10 000 by default, the `subs` dispatch input) fall into eight filter classes (type, q, watchedAttributes, idPattern, geoQ, scopeQ; `subs.md`) and every one is unique: p = k // tenants parametrises its q threshold, idPattern tail, polygon edge or scopeQ branch, and k6 evaluates the same rule, so due and delivered are reported per class |
| federated queries | `fed.sh` | five query shapes (type, q, geoQ, scopeQ, idPattern) on random tenants over the registrations (10 000 by default, the `regs` input; each with its own idPattern, polygon or scope) of eight classes (`csr.md`: mode, operations, csf properties, headers, expiry, location, scopes); every source is the sink; the row shows queries, failures, queries with a source warning, p99, source calls and calls per query |
| CPU and memory | `rss.sh` → `rss.csv`, `rss.md` | 1 Hz: broker and Postgres RSS, broker and Postgres CPU in cores, and whole-host busy cores against the core count — the saturation check; every fire.md / fed.md row carries the mean over its own window |
| PDF | `report.py` → `report.pdf` | the same tables plus the RSS/CPU timeline and the delivery curve, next to `index.html` and `perf.json` in the downloadable results folder |

`dev/perf/sink.py` is the other end of every subscription and
registration: it counts notifications and answers forwarded queries with
an empty list, so the fan-out over 100,000 registrations costs the broker
the matching and the HTTP round trips and nothing else.

Every run is capped at one hour: the server's TTL, the job timeout and
the box's own shutdown timer agree on it. Scale `0.01` (1,000,000
entities, 100 tenants, 1,000 subscriptions, 1,000 registrations) fits with
margin and is what the schedule runs; `1.0` does not fit in an hour and
is a deliberate dispatch on a bigger box with the TTL raised in the
workflow. Bulk load bypasses the broker
(no notifications, no history), which is the documented path for initial
loads in [Operations](operations.md#bulk-load-postgres-timescale).

### The measured run

`scale-weekly` run `33863032274` on a ccx33, `SCALE=0.01` with the
subscription and registration counts raised to 10,000 each: 1,000,000
entities over 100 tenants. The load took 367 s for the entities (one COPY
stream), 8 s for the subscriptions and 159 s for the registrations.

| store | ready in (median of 5) | RSS after start |
|---|---|---|
| memory | 39 ms | 18 MiB |
| file | 51 ms | 19 MiB |
| postgres | 358 ms | 59 MiB |

| store | shape | concurrency | req/s | p99 |
|---|---|---|---|---|
| memory | query | c50 | 7 431 | 19.97 ms |
| memory | query | c200 | 7 821 | 70.19 ms |
| memory | retrieve | c50 | 29 913 | 7.47 ms |
| postgres | query | c50 | 912 | 61.37 ms |
| postgres | query | c200 | 933 | 207.85 ms |
| postgres | retrieve | c50 | 1 841 | 29.08 ms |

| store | shape | knee (rps held) | p99 at knee | first failing stage | broker cores | peak threads |
|---|---|---|---|---|---|---|
| postgres | query | 3 000 | 10.5 ms | 3 500 | 2.56 | 13 |
| postgres | write | 1 000 | 5.2 ms | 1 500 | 1.64 | 13 |

Subscriptions firing, over 10,000 subscriptions on 101 tenants:

| rate (rps) | due | delivered | delivered % | POSTs/s | dropped by broker | dead letters | PATCH p99 | broker cores | host busy |
|---|---|---|---|---|---|---|---|---|---|
| 100 | 96 492 | 96 492 | 100.0 | 1 637.1 | 0 | 0 | 25.2 ms | 1.6 | 3.5 |
| 200 | 193 406 | 193 406 | 100.0 | 3 250.8 | 0 | 0 | 28.1 ms | 3.0 | 6.0 |
| 500 | 481 526 | 40 330 | 8.4 | 561.3 | 26 398 | 0 | 1 184.1 ms | 4.5 | 7.1 |

Against run `33835261405`, the same shape on the same server type before the
per-drain `@context` memo landed:

| rate (rps) | delivered % | POSTs/s | dropped | PATCH p99 | quiet after |
|---|---|---|---|---|---|
| 200 before | 99.9 | 2 742.7 | 14 | 69.6 ms | 4 s |
| 200 after | 100.0 | 3 250.8 | 0 | 28.1 ms | 0 s |
| 500 before | 12.7 | 847.9 | 25 359 | 645.7 ms | 4 s |
| 500 after | 8.4 | 561.3 | 26 398 | 1 184.1 ms | 2 s |

Federated queries, over 10,000 registrations, every source the sink:

| rate (rps) | queries | failed (conn/4xx/5xx) | with a source warning | GET p99 | source calls | calls per query | broker cores | host busy |
|---|---|---|---|---|---|---|---|---|
| 50 | 1 487 | 0 (0/0/0) | 0 | 1 821.5 ms | 50 675 | 34.08 | 1.8 | 5.9 |
| 100 | 2 346 | 0 (0/0/0) | 0 | 12 697.0 ms | 80 502 | 34.31 | 2.2 | 7.9 |
| 200 | 3 615 | 0 (0/0/0) | 0 | 33 731.2 ms | 130 096 | 35.99 | 2.2 | 7.9 |
| 500 | 1 928 | 0 (0/0/0) | 0 | 53 754.9 ms | 159 943 | 82.96 | 1.9 | 8.0 |

Resident set and CPU over the whole run, 1 085 samples about 1.3 s apart:

| measure | value | ceiling |
|---|---|---|
| broker RSS peak | 3 181 MiB | no ceiling set |
| Postgres RSS peak | 11.86 GiB | no ceiling set |
| broker CPU peak / mean | 5.3 / 1.1 cores | of 8 |
| Postgres CPU peak / mean | 6.5 / 2.1 cores | of 8 |
| host busy peak / mean | 8.0 / 4.7 cores | of 8: saturated when peak ≈ 8 |

What the run says:

- Delivery is exact to 200 rps of writes: all 193 406 due notifications
  arrive, nothing is dropped, and the sink goes quiet in the same second the
  writes stop. The run before the memo delivered 99.9 % of the same shape,
  dropped 14 changes and took four seconds to drain. Write latency at that
  rate falls with it, 69.6 ms to 28.1 ms at the 99th percentile, because the
  matcher drain is no longer competing with the write path for a core.
- At 500 rps both runs are past the knee the run itself reports, and the
  newer one is further past it: the drop count barely moves (25 359 to
  26 398) but what survives is delivered more slowly (848 to 561 POSTs/s).
  Cheaper matching feeds the delivery stage faster, so the queue behind it
  fills sooner. The limit line reads 200 rps in both runs; what changed is
  that 200 rps is now met exactly instead of nearly.
- The registry narrowing holds. Each tenant carries 100 registrations and a
  `type=Vehicle` query reaches 34 of them — the three Vehicle-typed classes
  of the eight in `csr.md` — so the index decides the fan-out and the
  forward path is not a broadcast. The ratio is flat from 50 to 200 rps.
- The distributed path returns no 5xx, no 4xx and no `NGSILD-Warning` over
  9 376 queries: every one of the 421 216 source calls was answered and
  folded in.
- Where the broker bends, it is not out of CPU. At the Postgres query knee
  it holds 3 000 rps on 2.56 of 8 cores and at the write knee 1 000 rps on
  1.64; on the federated path it sits at 2.2 cores while Postgres takes 5.1
  and the host runs out at 7.9. The component that saturates is the
  database or the machine, never the broker alone.
- A federated query costs about eight times the Postgres work of a direct
  query of the same shape (42.5 against 5.5 mcores). The fan-out is 34
  source calls and a registry match over 10,000 rows; the direct row is one
  local read, so the two are not the same unit of work and the ratio is the
  price of federation, not a regression against a like-for-like baseline.
  The broker side of that comparison is not available from this run. The
  sampler matched broker processes against the whole of the first broker's
  `argv[0]`, and the `shapes`, `saturate` and `startup` stages start their
  own brokers from the same binary by a different path form, so it counted
  none of them and reported the idle first process instead — zero cores
  through windows serving 912 req/s. `saturate.md` disagrees with `rss.csv`
  for the same window (2.56 cores against zero) because that stage measures
  the process it started itself. The sampler now matches on the last two
  path components, so a later run carries a broker CPU column for every
  stage; the rows above are left as they were measured.

## How each measurement works

No rig script is a unit test. Every one starts a real broker, drives it
over HTTP, and folds what came back into one Markdown table. The pieces
are the same in all of them:

```text
  dev/perf/<script>.sh
        │
        ├── starts ──► antares  (release binary, one store)  :9090
        │                 │
        │                 └── store: memory | file | postgres (docker)
        │
        ├── drives ───► k6 (a dev/perf/k6-*.js scenario)  or  python3
        │                 └── constant-arrival-rate: offered load, not
        │                     closed-loop, so a slow broker shows up as a
        │                     growing queue instead of a slower client
        │
        ├── receives ─► dev/perf/sink.py  :9800
        │                 ├── POST /…      one notification, counted
        │                 └── GET  /csr/k  one forwarded query, answered []
        │
        └── samples ──► dev/perf/rss.sh    1 Hz → rss.csv
                          reads /proc for broker, Postgres, k6, sink, host
```

`$OUT/phase` is the thread that ties them together: each script writes the
stage name into it before it starts, and `rss.sh` copies that string into
every sample it takes, so any row of `rss.csv` can be attributed to the
stage that caused it.

### `startup.sh` — how long a cold broker takes to answer

Runs the binary, polls `GET /q/health` until the first `200`, records the
elapsed time and reads `VmRSS` out of `/proc/<pid>/status` immediately
after. Five times per store, median reported. Nothing else is running, so
the number is the process itself: binary load, config parse, store open,
listener bind.

### `shapes.sh` — throughput per request shape

Starts one broker, seeds 100 five-attribute entities through the API, then
runs `k6-shapes.js` closed-loop at a fixed number of clients: a list query
at 50 and 200, a single retrieve at 50. Three runs of five seconds, median
reported, p99 from the same runs. Closed-loop on purpose — this table
answers "how fast is one shape", not "where does it break".

### `core-scale.sh` — does the broker use the cores it is given?

Pins the broker to 1, 2, 4 and 8 physical cores with `taskset` (SMT
siblings excluded) and keeps k6 on the cores left over, refusing any step
where the two would share one. The in-memory store on purpose: the
question is whether the broker's own work parallelises, so nothing waits
on a database.

### `saturate.sh` — where the knee is

Open model. `k6-saturate.js` raises the arrival rate by 500 rps every
stage and keeps going until p99 passes `P99_MS` or the error rate passes
`ERR`. The knee is the last stage that held both. A run that never fails a
stage reports `none reached`, which means the ladder is shorter than the
box — the answer is more stages, not a bigger conclusion.

### `load.sh` — building the dataset

The only stage that does not go through k6:

```text
load.sh
  ├─ sink.py 9800 8        one front door, eight worker processes behind it
  │                        (one CPython process tops out near 5 000 req/s)
  ├─ gen.py | bulk-load.sh entities, one COPY stream straight into Postgres,
  │                        bypassing the broker: no notifications, no history
  ├─ api-load.py subscriptions   through the API, eight filter classes
  └─ api-load.py registrations   through the API, eight CSR classes
```

Entities bypass the broker because a hundred million of them through the
API would measure the loader. Subscriptions and registrations do not: they
have to pass validation and land in the matcher's index, which is what the
later stages exercise.

### `fire.sh` — do the subscriptions fire, and up to what rate

```text
k6-fire.js                broker :9090                    sink.py :9800
    │                          │                                 │
    │ PATCH …/attrs (update)   │                                 │
    │ DELETE …/entities/{id}   │                                 │
    ├─────────────────────────►│                                 │
    │              204         │ 1. write the change              │
    │◄─────────────────────────┤ 2. match it against this         │
    │                          │    tenant's subscriptions        │
    │                          │ 3. queue it (changeQueue, 1024)  │
    │                          │ 4. deliver, deliveryWidth (64)   │
    │                          │    in flight, 8 per tenant       │
    │                          │                                  │
    │                          │  POST /  {"data": [entity, …]}   │
    │                          ├─────────────────────────────────►│
    │                          │                          204     │
    │                          │◄─────────────────────────────────┤
    │                          │                                  │
    └── k6 stops ──────────────┴── fire.sh polls /stats until the │
                                   count stops moving for 5 s     │
```

The count due is not measured, it is derived: `k6-fire.js` knows which
subscription classes each write should trigger and evaluates the same rule
`api-load.py` used to create them, so `delivered / due` is a real ratio and
not a guess. `quiet after` is how long the sink kept receiving once the
stream stopped — the drain. `dropped by broker` comes from the broker's own
`changesDropped` counter, read from `/q/health` before and after, so a
delivery gap can be charged to the queue, to the delivery policy or to the
receiver rather than left ambiguous.

The limit is the last rate that delivered 99 % with no failed operation,
and the ladder stops at the first rate that misses it.

### `fed.sh` — federated queries over the registrations

The registrations point at `sink.py`, which answers every forwarded query
with an empty array. That is deliberate: an empty answer costs the broker
the index lookup, the fan-out and the HTTP round trips and nothing else,
so the row measures the federation machinery rather than a source.

```text
k6-fed.js                    broker :9090                        sink.py :9800
    │                             │                                      │
    │ GET /entities?type=Vehicle  │                                      │
    │ NGSILD-Tenant: t42          │                                      │
    ├────────────────────────────►│                                      │
    │                             │ 1. expand the query against @context │
    │                             │                                      │
    │                             │ 2. csource_index lookup (5.12): one  │
    │                             │    SQL query, narrowing on entity     │
    │                             │    type and entity id alone. It may  │
    │                             │    only REMOVE registrations the     │
    │                             │    matcher would reject anyway, so   │
    │                             │    a NULL dimension always survives  │
    │                             │                                      │
    │                             │ 3. the matcher decides the rest in   │
    │                             │    Rust: geoQ, scopeQ, csf, the      │
    │                             │    intervals, the idPattern regex,   │
    │                             │    the Via chain                     │
    │                             │                                      │
    │                             │ 4. fold registrations naming the     │
    │                             │    same source into one request      │
    │                             │    (5.2.9: same endpoint, mode,      │
    │                             │    tenant, alias, headers, localOnly)│
    │                             │                                      │
    │                             │ 5. narrow each forward to what its   │
    │                             │    registration declares (4.3.6.1)   │
    │                             │                                      │
    │                             │ 6. fan out, ANTARES_FED_FANOUT (8)   │
    │                             │    in flight, each bounded by the    │
    │                             │    registration's timeout            │
    │                             │                                      │
    │                             │  GET /csr/17/entities?type=…&attrs=… │
    │                             │  Via: 1.1 broker-a                   │
    │                             ├─────────────────────────────────────►│
    │                             │                        200  []       │
    │                             │◄─────────────────────────────────────┤
    │                             │      × N per query (`calls per       │
    │                             │        query` in the table)          │
    │                             │                                      │
    │                             │ 7. book the outcome on each          │
    │                             │    registration: timesSent,          │
    │                             │    timesFailed, lastSuccess,         │
    │                             │    lastFailure, status               │
    │                             │    (Table 5.2.9-2)                   │
    │                             │                                      │
    │                             │ 8. merge the halves (4.5.5),         │
    │                             │    paginate, answer                  │
    │       200 + entity list     │                                      │
    │◄────────────────────────────┤                                      │
```

`calls per query` is the number that matters: it is the fan-out the
registry narrowing left behind, and it multiplies everything downstream. A
query that reaches 34 sources costs 34 HTTP round trips, 34 timeout
budgets and 34 bookkeeping writes.

Step 7 is where the federated path meets the database, once per source
call rather than once per query:

```text
one forwarded request
      │
      └─► note_forward (federation.rs)
             └─► CurrentStateDriver::record_forward
                    └─► record_forward_via_mutate (antares-store)
                           └─► PgDocStore::mutate(Kind::Registration)
                                  ├─ SELECT … FOR UPDATE   the row lock
                                  ├─ UPDATE csource_registrations
                                  └─ csource_index: rebuilt only when the
                                     write moved a member the index is
                                     built from — the counters are not
```

The `failed (conn/4xx/5xx)` and `with a source warning` columns are split
because they are different faults: a failed query is the broker not
answering, while a warning is the broker answering after a source did not
(6.3.17), which is the documented outcome and not an error.

### `rss.sh` — the CPU and memory column on every other table

One background sampler for the whole run. It resolves the broker by the
last two components of its `argv[0]`, the Postgres container by name, and
k6, the sink and mosquitto by their own names, then writes RSS and CPU for
each at 1 Hz along with whole-host busy cores. Every `fire.md` and `fed.md`
row folds the samples that fall inside its own window, which is why a
saturation claim can be checked against the phase that made it.

### `report.py` — the artefacts

Folds every table into `index.html`, `perf.json` and `report.pdf`, next to
the raw CSVs, so a later run can be diffed against this one without
rerunning anything.

## Measuring delivery without the rented runner

`fire.sh` needs k6, so until now the notification pipeline could only be
measured by dispatching `scale-weekly`. `dev/perf/deliver.py` drives the
same path with the standard library alone, against a memory-store broker
and `sink.py`:

```bash
ANTARES_STORE=memory ANTARES_EGRESS_ALLOW_PRIVATE=true antares &
python3 dev/perf/sink.py 9800 8 &
python3 dev/perf/deliver.py --seed --tenants 10 --entities 500
python3 dev/perf/api-load.py subscriptions --count 240 --tenants 10 \
  --sink-workers 8 --broker http://127.0.0.1:9090 --sink http://127.0.0.1:9800
python3 dev/perf/deliver.py --tenants 10 --entities 500 --rate 2000
```

The number to read is `matches_per_second`. One match is one
(subscription, entity) pair: it is what the matcher evaluates and what a
notification carries, so it is the unit both halves of the pipeline scale
with. `changes_per_second` divides that by how many subscriptions each
change fires, so it moves whenever the subscription set changes even
though the pipeline is doing identical work.

What it measures, on twelve cores with a release build, at an offered
2 000 changes per second over ten tenants and 500 entities:

| subscriptions over 10 tenants | matches/s | broker cores | changes dropped |
|---|---|---|---|
| 60 | 2 201 | 0.91 | 0 |
| 120 | 6 056 | 1.24 | 0 |
| 240 | 11 781 | 1.28 | 0 |
| 480 | 25 547 | 1.89 | 0 |
| 960 | 44 818 | 1.90 | 1 191 |

Matches per second rise with the subscription count, because a change is
evaluated against more subscriptions and fires more of them. The broker
carries the whole offered change rate up to 480 subscriptions over ten
tenants, and drops 0.15 per cent of it at 960.

Holding the subscription count at 240 and raising the offered rate
instead:

| offered changes/s | achieved | matches/s | broker cores | changes dropped |
|---|---|---|---|---|
| 2 000 | 2 000 | 12 074 | 1.72 | 0 |
| 3 000 | 3 001 | 17 998 | 2.18 | 0 |
| 4 000 | 4 004 | 23 948 | 2.48 | 0 |
| 6 000 | 5 131 | 30 683 | 2.74 | 0 |

The last row measures the harness rather than the broker: `deliver.py`
runs its writers as threads in one interpreter, and it cannot produce the
6 000 changes per second it was asked for. The broker absorbed every
change offered at every rate, dropped none of them, and held 2.74 of
twelve cores, so the table gives a floor for the delivery bound rather
than the bound itself. Reaching that bound needs the writers in separate
processes.

`ANTARES_DELIVERY_WIDTH` is not what governs this. Over a hundred-tenant
shape at 1 000 changes per second, widths of 8, 64, 256 and 1 024 deliver
7 026, 7 028, 7 026 and 7 028 matches per second, none of them dropping a
change: the offered load is absorbed whole at every width, so the knob
has nothing to arbitrate.

The same ladder against PostgreSQL, which is what a deployment runs
(`synchronous_commit=off`, 1 GiB of shared buffers, a pool of 50, the
database sharing the box with the broker, the harness and the sink):

| offered changes/s | achieved | matches/s | broker cores | changes dropped |
|---|---|---|---|---|
| 500 | 500 | 3 051 | 0.75 | 0 |
| 1 000 | 1 000 | 6 062 | 1.25 | 0 |
| 2 000 | 1 671 | 10 108 | 1.39 | 0 |
| 4 000 | 1 140 | 6 927 | 1.26 | 0 |

A change costs an entity update and a temporal-history insert here rather
than a map write, so the knee arrives near 1 671 changes per second
against the memory store's 7 800. Two things about that knee matter more
than its position. The broker holds 1.39 cores at it while PostgreSQL
holds 2.29 on average and peaks at 4.12, so the broker is not the
component that runs out — which is why a criterion written as broker core
utilization at saturation cannot be met on a rig that shares one box.
And goodput goes backwards past the knee: offering 4 000 changes per
second delivers less than offering 2 000, with broker CPU falling as it
does, because nothing sheds write load before it drives the database past
what it can commit. Neither is a property of the delivery path; both are
the write path underneath it.

## Setting up the rented runner (two repository secrets)

`perf-weekly` and `scale-weekly` rent a Hetzner Cloud server for the run
(a ccx33 and a ccx53; the design targets add a volume),
register it as an ephemeral GitHub runner, and delete it afterwards;
`perf-janitor` sweeps anything past its `expiry` label. Until the two
secrets below exist, both workflows stop at "Create server" with an empty
token, so the weekly runs stay red and no perf report is published.

### 1. `HCLOUD_TOKEN` — Hetzner Cloud API token

1. Sign in at <https://console.hetzner.cloud/>.
2. Create a project of its own for this (for example `antares-perf`) so the
   token cannot touch anything else, and set a spending alert on the
   project (Project → *Billing* → *Alerts*).
3. In that project open *Security* → *API tokens* → *Generate API token*.
   Name it `github-actions`, permission **Read & Write** (the workflow
   creates and deletes servers and volumes and reads the price list).
4. Copy the token once; Hetzner never shows it again.

### 2. `RUNNER_PAT` — GitHub token that can register a runner

The workflow asks the GitHub API for a runner registration token
(`POST /repos/<owner>/<repo>/actions/runners/registration-token`), which the
default `GITHUB_TOKEN` is not allowed to do.

1. GitHub → *Settings* (your account) → *Developer settings* →
   *Personal access tokens* → *Fine-grained tokens* → *Generate new token*.
2. Resource owner: the account that owns this repository. Repository
   access: **Only select repositories** → this repository.
3. Repository permissions: **Administration: Read and write**. Nothing
   else.
4. Expiration: one year is the maximum; put the renewal date in your
   calendar, the workflow fails with HTTP 401 when it lapses.
5. Copy the token once.

A classic token with the `repo` scope works too, but grants far more than
the workflow needs.

### 3. Store both as repository secrets

Repository → *Settings* → *Secrets and variables* → *Actions* →
*New repository secret*, twice:

| Name | Value |
|------|-------|
| `HCLOUD_TOKEN` | the Hetzner token from step 1 |
| `RUNNER_PAT` | the GitHub token from step 2 |

The names must match exactly; the workflows read them as
`${{ secrets.HCLOUD_TOKEN }}` and `${{ secrets.RUNNER_PAT }}`.

### 4. Limits on a fresh Hetzner project

A new project starts with small quotas. `perf-weekly` asks for a ccx33
(8 dedicated cores) and `scale-weekly` for a ccx53 (32);
`scale-weekly` at 1.0 would add a 500 GB volume, and Hetzner answers
`dedicated core limit exceeded` / `volumes size limit exceeded` until the
limits are raised: Project → *Limits* → request more dedicated cores and
volume storage (a short form, usually approved within a day). The 0.01
dry run needs the core limit only; it shrinks the volume to 10 GB.

### 5. First run

Actions → *scale-weekly* → *Run workflow* with `scale` = `0.01` (about an
hour, a few euros); read the step summary for the cost line and the
tables. Then Actions → *perf-weekly* → *Run workflow*. Both schedules take
over from there (Saturday 03:17 UTC and Sunday 02:17 UTC), and `pages`
folds the newest bundles into `/reports/perf/latest/`.

If you would rather not rent hardware, disable the two workflows (Actions
→ workflow → *⋯* → *Disable workflow*) so the weekly runs stop going red.

## What a façade costs

A façade for another standard answers by driving this broker's own NGSI-LD
router in process ([Façades for another
standard](extending.md#façades-for-another-standard)). The seam's own cost
is a JSON round trip: the inner answer is serialized to bytes, parsed, and
re-serialized into the façade's envelope. Everything else about the request
happens exactly once.

Measured with the reference façade (`GET /x/example/things?kind=Vehicle`)
against the NGSI-LD request it wraps
(`GET /entities?type=Vehicle&options=keyValues`), both through the same
router in the same process, 200 calls each, medians of five repetitions,
release build:

| answer | façade | the request it wraps | the round trip |
|---|---|---|---|
| 100 five-attribute Entities | 265-297 µs | 199-226 µs | **66-71 µs** |
| nothing matched | 112-127 µs | 107-157 µs | **under 10 µs** |

The comparison is in process on purpose: the number is about a serialize
and a parse, and a socket between the two halves would measure the socket.
`dev/perf/shapes.sh` runs the same pair end to end against a built binary
(the `facade` shape, skipped unless the binary was built with the reference
plugin); the table above is the per-call figure.

What it decides: the seam has almost no fixed cost — an empty answer's
round trip does not clear the noise — and what it does cost is
proportional to the answer, about 0.7 µs per Entity on this shape. A typed
operations layer, one where a façade reached the handlers through Rust
types instead of through JSON, would save exactly that and nothing else.
Sixty-six microseconds on a hundred-Entity page is not a reason to build
and maintain a second, typed API surface beside the HTTP one, so that box
stays closed until a façade measures this as its ceiling rather than as its
rounding error.

## What the numbers are not

They are one machine, one request shape, one week. A different instance
type invalidates the whole history, which is why both workflows pin one.
No regression gate exists until the noise profile has ten runs on that
instance type; until then the runs report, and the report is the
evidence.
