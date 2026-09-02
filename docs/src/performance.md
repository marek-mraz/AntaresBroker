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
| `scale-weekly` | ccx33 (8 dedicated vCPU, 32 GB) + volume, one hour | the design targets on PostgreSQL, at scale 0.01 | Sunday |

## The shapes (`perf-weekly`)

Every script lives in `dev/perf/` and runs on a laptop the same way it
runs in CI; `k6` is the only tool it needs.

| table | script | method |
|---|---|---|
| startup and idle footprint | `startup.sh` | exec to the first `200` from `/q/health`, median of five, `VmRSS` right after, per store |
| throughput per shape | `shapes.sh` | 100 five-attribute entities; `GET /entities?type=Vehicle&limit=20` at 50 and 200 concurrent clients, `GET /entities/{id}` at 50 (`SPECS` picks other rows, the PostgreSQL dispatch runs 64, 256 and 1 024 clients); five seconds, median of three runs, p99 from the same runs |
| core scaling | `core-scale.sh` | broker pinned to 1, 2, 4, 8 physical cores with `taskset`, load generator on the remaining cores; refuses a step it cannot isolate. `cores used` is the broker's CPU time over the window against the cores it was allotted; `peak threads` is the largest thread count of the process, which is where a blocking-thread ceiling shows (a parked store call is one OS thread; the cap is the connection limit plus 1024) |
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

One `perf-weekly` dispatch on ccx33 (8 logical, 4 physical cores; three
passes; `store=postgres`) at commit `b55d554`. The load generator shares
the machine, so every row is broker plus generator on 4 physical cores.

Core scaling, one row per store and pool. `cores used` is the broker's
CPU time over the window against the cores it was allotted; the 4- and
8-core steps refuse to run on this instance type because isolating them
needs 8 and 16 physical cores.

| store | 1 core | 2 cores | efficiency at 2 | cores used at 2 | peak threads |
|---|---|---|---|---|---|
| memory | 2 481 req/s | 4 964 req/s | 100 % | 1.87 | 10 |
| postgres, pool 20 | 1 378 req/s | 2 809 req/s | 102 % | 1.82 | 88 |
| postgres, pool 100 | 1 471 req/s | 2 746 req/s | 93 % | 1.81 | 140 |

The saturation knee, whole box, open model:

| store | shape | knee | p99 at the knee | first failing stage | cores used | peak threads |
|---|---|---|---|---|---|---|
| memory | query | 5 000 rps | 4.2 ms | none reached | 1.83 | 17 |
| memory | write | 5 000 rps | 1.2 ms | none reached | 0.84 | 72 |
| postgres, pool 20 | query | 3 000 rps | 12.1 ms | 3 500 rps | 2.70 | 4 028 |
| postgres, pool 20 | write | 1 000 rps | 5.8 ms | 1 500 rps | 1.84 | 4 016 |
| postgres, pool 100 | query | 2 500 rps | 7.7 ms | 3 000 rps | 2.80 | 4 073 |
| postgres, pool 100 | write | — | — | 500 rps | 1.52 | 4 015 |

Throughput per shape at 64, 256 and 1 024 concurrent clients:

| store | shape | c64 | c256 | c1024 |
|---|---|---|---|---|
| memory | query | 7 274 req/s, p99 24.5 ms | 7 451 req/s, p99 73.2 ms | 7 359 req/s, p99 367.1 ms |
| memory | retrieve | 29 008 req/s, p99 7.8 ms | 30 114 req/s, p99 35.9 ms | 29 136 req/s, p99 93.4 ms |
| postgres, pool 20 | query | 3 207 req/s, p99 22.3 ms | 3 384 req/s, p99 73.8 ms | 3 909 req/s, p99 252.5 ms |
| postgres, pool 20 | retrieve | 4 548 req/s, p99 15.6 ms | 4 405 req/s, p99 56.9 ms | 4 458 req/s, p99 216.8 ms |
| postgres, pool 100 | query | 3 456 req/s, p99 26.3 ms | 3 244 req/s, p99 86.9 ms | 2 917 req/s, p99 337.7 ms |
| postgres, pool 100 | retrieve | 4 640 req/s, p99 18.4 ms | 4 247 req/s, p99 63.8 ms | 4 317 req/s, p99 231.5 ms |

What the run says, in the order it matters:

- The blocking pool carries the whole PostgreSQL path: about 4 000 live
  OS threads at saturation against a ceiling of 11 024 (the connection
  limit plus 1 024). The ceiling is never reached, so nothing deadlocks,
  but every store call is one parked thread.
- Past the knee p99 does not degrade, it steps: 12.1 ms at 3 000 rps to
  843.6 ms at 3 500 rps on pool 20, with the error rate still zero. The
  queue absorbs the overload and the client waits.
- A larger pool is worse, not better. Pool 100 holds 2 500 rps where
  pool 20 holds 3 000, and its write path fails one stage earlier, at
  500 rps.
- The in-memory write path holds 5 000 rps on 0.84 cores. It is not CPU
  bound; something ahead of the CPU serialises it.
- Scaling from 1 to 2 cores is linear (100 %, 102 %, 93 %). This
  instance type cannot measure the 4- and 8-core steps, which is where a
  blocking-thread design is expected to bend; `perf-weekly` takes a
  `server` input for the run that can.

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

## Setting up the rented runner (two repository secrets)

`perf-weekly` and `scale-weekly` rent a Hetzner Cloud server for the run
(ccx33 for both; the design targets add a volume),
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

A new project starts with small quotas. Both runs ask for a ccx33 (8
dedicated cores); `scale-weekly` at 1.0 would add a 500 GB volume, and Hetzner answers
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

## What the numbers are not

They are one machine, one request shape, one week. A different instance
type invalidates the whole history, which is why both workflows pin one.
No regression gate exists until the noise profile has ten runs on that
instance type; until then the runs report, and the report is the
evidence.
