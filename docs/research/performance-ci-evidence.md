# Periodic performance testing on dedicated hardware — what the evidence supports

Adversarially-verified research pass (second of two; the first
spent its verification budget on architecture/plugin claims). Claims below
survived a 3-vote panel unless marked otherwise. Companion document:
`plugin-and-runtime-evidence.md`.

## 1. The ephemeral-runner pipeline is a standard, buildable pattern

Confirmed 3-0. One dedicated server per benchmark run, driven entirely from
a GitHub Actions workflow, in three jobs:

1. **create** — provision a Hetzner Cloud server and register it as a
   self-hosted runner; the action emits a `label` output (and `server_id`).
2. **benchmark** — `runs-on:` that emitted label.
3. **delete** — guarded by `if: ${{ always() }}` so the server dies even
   when the benchmark fails or is cancelled.

`Cyclenerd/hcloud-github-runner` implements exactly this shape (`mode`
create/delete, `label`/`server_id` outputs), and the same idiom — down to the
comment "required to stop the runner even if the error happened in the
previous jobs" — predates it in `machulav/ec2-github-runner`, which is what
makes it a pattern rather than one project's quirk. GitHub's own expression
reference confirms `always()` runs even when cancelled, and that a job-level
`if` is evaluated independently of `needs` results.

**Scope limit recorded by the verifiers, and it matters for cost:**
`always()` is not an absolute guarantee. GitHub force-terminates after a
~5-minute cancellation window, and a create job that provisions the server
*before* failing to emit its output leaks it. Mature setups therefore add a
server TTL or a scheduled janitor that sweeps by label. Build the janitor;
do not trust `always()` alone with a paid resource.

## 2. Dedicated vCPU (CCX), not shared (CX/CPX/CAX)

Confirmed 3-0, from Hetzner's own documentation. Shared instances use
hypervisors "to distribute usage rights of CPU resources among several
virtual servers" and "provide a baseline CPU performance and have the option
to temporarily burst"; dedicated (CCX) instances have "CPU resources
exclusively", where "one vCPU equals one thread of a physical CPU core".
Hetzner positions dedicated as "ideal for CPU intensive applications or
research calculations". The ephemeral-runner action passes `server_type`
straight through with no family allowlist, so any CCX type works (ccx13,
2 cores / 8 GB, through ccx63, 48 cores / 192 GB).

**Two caveats the sources do not cover and we must not paper over:** CCX
exclusivity is CPU-only — L3 cache, memory bandwidth, network and disk stay
shared — and Hetzner publishes no steal-time SLO or variance figure. Vendor
positioning is not a measurement. Our own variance baseline has to be
measured on the instance we pick (see §5).

## 3. Noise control: the LLVM checklist

Confirmed 3-0 on the individual knobs (2-1 on the two-core cpuset rule).
LLVM's official benchmarking guidance treats frequency scaling, Turbo Boost,
ASLR and CPU sharing as first-order noise sources, with concrete knobs:

- performance governor on every core;
- `echo 1 > /sys/devices/system/cpu/intel_pstate/no_turbo`;
- `echo 0 > /proc/sys/kernel/randomize_va_space`;
- cpuset shielding (`cset shield -c N1,N2 -k on`), reserving at least two
  cores so the measurement tool and the program under test do not share one.

Apply whichever knobs the guest actually exposes — inside a VM, `no_turbo`
and governor control frequently are not available, which is precisely why
§5's own-variance measurement is not optional.

Refuted (0-3): LLVM's "<0.1% run-to-run variation" as a checkable target.
Do not quote that number as an acceptance criterion.

## 4. Load generation: open model, or explicit CO correction

Confirmed 3-0 (merged from four claims). A latency-sensitive broker must be
driven by an **open workload model**, because closed models self-throttle
exactly when the system degrades — a slow response delays the next request,
so the load generator quietly stops applying the load it claims to. That is
coordinated omission.

- **k6** implements the open model through exactly two executors:
  `constant-arrival-rate` and `ramping-arrival-rate`. `constant-vus` and the
  rest are closed models. Choosing the wrong executor silently invalidates
  the tail-latency numbers.
- **wrk2** takes the other route: it corrects coordinated omission post hoc,
  timestamping each response against when the request *should* have been
  sent under the configured constant rate.

Refuted (1-2): the specific characterisation of wrk2's `-R` flag as
mandatory with a default of 1000. Check the tool's own help output rather
than citing that.

## 5. Gating: statistical, two-gate, and against the benchmark's own history

This is where most performance CI goes wrong, and the evidence is unusually
concrete.

**Criterion.rs — the two-gate model (confirmed 3-0).** It bootstrap-resamples
the current and prior runs, computes a T score, derives a p-value from the
fraction of more-extreme bootstrapped T scores, and then applies a
**separate, configurable noise threshold (default 1%)**. A change must be
both statistically significant **and** larger than the noise threshold to be
reported. Significance answers "is this real", effect size answers "do we
care". A fixed-percentage ratchet answers neither, which is why it produces
false positives on a noisy runner.

**rustc-perf — threshold-free gating (confirmed 3-0).** A result counts as
significant only when the relative change is an outlier **against that
benchmark's own historical run-to-run deltas**, by interquartile-range
fencing: `result > Q3 + (interquartile_range * 3)`. So a noisy benchmark
needs a bigger move to trip, and a quiet one trips on less, without anyone
hand-tuning per-benchmark thresholds. Reporting is then **tiered by
context**: PR-triggered "try" runs report at "somewhat relevant", while the
periodic triage report only reports at "definitely relevant".

**ClickHouse — a counter-example to the weekly premise (confirmed 3-0).**
It runs ~3,000 end-to-end queries across ~200 suites on **every pull
request**, with the reference server and the tested server running
**simultaneously on the same machine, queries alternated between them**. Host
drift then hits both arms equally instead of being attributed to the change.

That last one is worth taking seriously before building a weekly job. A
same-machine A/B comparison cures noise more cheaply than any amount of
tuning, because it removes the need to compare across time and hardware at
all. The two are complementary: A/B per change for regression detection,
absolute numbers on a schedule for tracking real capacity against the §1
targets.

## 6. What did not survive

Do not carry these into a design document (all refuted):

- TestFlows-GitHub-Hetzner-Runners' claimed one-job-per-server model and
  unconditional power-off/delete teardown (0-3).
- GitHub officially recommending against autoscaling with persistent
  runners, and the guarantee that an ephemeral runner receives exactly one
  job (0-3 each).
- LLVM's "<0.1% run-to-run variation" as a target (0-3).
- wrk2's mandatory `-R` / default-1000 characterisation (1-2).

## 7. The design this implies

1. Weekly scheduled workflow, three jobs (create / benchmark / delete with
   `always()`), plus a label-sweeping janitor for leaked servers.
2. A CCX instance, pinned to one type forever — changing instance type
   invalidates the history.
3. Apply whatever of the LLVM checklist the guest exposes; record which
   knobs were actually available in the run metadata.
4. **Measure our own variance first.** Before any gate exists, run the same
   commit N times and compute the per-benchmark IQR. That distribution is
   the gate's input, and without it every threshold is a guess.
5. k6 with `constant-arrival-rate` (open model) for the HTTP-level numbers;
   Criterion for in-process micro-benchmarks.
6. Gate the rustc-perf way (IQR fencing against each benchmark's own
   history) with Criterion's two-gate discipline for the micro side. Tier
   the reporting; do not fail the build on a single noisy sample.
7. Consider ClickHouse's same-machine A/B as the primary regression signal,
   with the weekly absolute run for capacity tracking.

Cost note: nothing in the evidence quantifies the euro cost, which is a
function of instance type and runtime — compute it from Hetzner's price list
for the chosen CCX type at the expected job duration, and put a hard TTL on
the server regardless.
