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
| `perf-weekly` | ccx33 (8 dedicated vCPU, 32 GB), one hour | the request shapes other brokers publish, on the in-memory store | Saturday |
| `scale-weekly` | ccx33 (8 dedicated vCPU, 32 GB) + volume, one hour | the design targets on PostgreSQL, at scale 0.01 | Sunday |

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
| at load | `shapes.sh`, `saturate.sh` | throughput per tenant, the knee |
| subscriptions firing | `fire.sh` | update + delete streams over the loaded entities at 500, 1,000, 2,000 and 4,000 rps; every update fires each subscription of its tenant once, so the notifications due are known; the table shows due, delivered, the distinct subscriptions that fired and how long the sink kept receiving after the stream; the limit is the last rate that delivered 99% with no failed operation |

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
