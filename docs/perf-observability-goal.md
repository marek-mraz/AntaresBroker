# Perf & observability goal — Robot metrics + performance TPs (drafted 2026-08-14, AntaresBroker)

Research base: Robot Framework 7.4.1 Listener v3 (result-model mutation:
per-test messages, suite metadata, sidecar files), `robotframework-dashboard`
(SQLite + cross-run trends from output.xml), pabot / rfswarm surveyed.
Standing conclusions this checklist encodes:

- **Conformance TPs stay serial forever** — fixed URNs, exact-count
  assertions, pattern teardowns, one fixed mock host:port. Never reuse
  them for load.
- **Perf TPs are new, concurrency-safe by construction**: worker-suffixed
  ids, one tenant per worker (`NGSILD-Tenant` isolation), assertions only
  on self-created data, latency thresholds as generous tripwires (this
  sandbox is noisy), never global counts.
- Robot measures *latency and concurrent correctness*; raw throughput vs
  the §1 targets belongs to a load generator (k6/oha), not Robot.
- §2 rule applies doubly: never build while a measured run is in flight.

## The /goal prompt

```
/goal Work docs/perf-observability-goal.md top-to-bottom, sandbox-side only.
First copy the checklist below into tasks.md as "## Perf & observability
2026-08-14", then one item = one commit with full §0.3 discipline where it
applies (MemPalace first, TEST-FIRST red run, negative assertions, rule-8
local Robot validation, claude.md §6 + ledger notes updated, minimal
diffs throughout). Perf TPs are repo EXTENSIONS, not conformance — tag `perf`,
keep them out of every conformance run. DONE = every checkbox [x] in
tasks.md with commit hash + green-run evidence.

[ ] 1. Metrics listener: ngsi-ld-test-suite/listeners/broker_metrics.py —
      Listener v3, zero new deps. Find broker pid by /proc/*/cmdline scan
      (arg-configurable pattern, default "antares"); 1 Hz background thread
      samples /proc/<pid>/statm RSS + /proc/<pid>/stat utime/stime.
      end_test appends "[perf] rss=… cpu=…" to result.message; end_suite
      sets suite metadata (Broker peak RSS / CPU avg); close writes
      broker-metrics.csv (test longname, timestamps, samples). Self-check:
      assert-based __main__ against a spawned dummy process. Fallibility:
      run one small TP file with the listener, verify log.html/report.html
      carry the numbers AND verdicts are bit-identical to a listener-less
      run (negative assertion: listener must not flip any status).
[ ] 2. Wiring: opt-in METRICS=1 in dev/etsi-local.sh + dev/etsi-pipeline.sh
      adds --listener; default OFF so measured conformance runs stay
      undisturbed. Document the flag in the scripts' headers.
[ ] 3. Trends: pip install robotframework-dashboard into /workspace/.venv;
      dev/robot-dashboard.sh ingests matrix output.xml files (incl. the
      unpacked ETSI-matrix-results zips) into one SQLite db + dashboard
      html; verify per-TP duration drift is visible across matrices (6)-(8).
[ ] 4. Perf TP foundation: ngsi-ld-test-suite/TP/NGSI-LD/Performance/
      perf_resources.robot — ${WORKER} variable (default 0), unique-id
      keyword (urn:ngsi-ld:perf:${WORKER}:${counter}), per-worker tenant
      header, per-worker teardown (tenant-scoped delete). Tag `perf` on
      everything; add --exclude perf to conformance run scripts and assert
      (grep) no conformance path picks Performance/ up.
[ ] 5. Latency tripwire TPs: create/retrieve/query/delete + one
      subscription-notify path, thresholds generous (e.g. p95 create
      < 250 ms local memory store), measured via keyword elapsed time;
      every TP also asserts response correctness (perf run that returns
      wrong data must fail on the data, not just the clock).
[ ] 6. Concurrent-correctness smoke: pabot (pip install robotframework-pabot)
      running the Performance/ suite with N=4 workers × distinct WORKER/
      tenant against ONE local memory broker; assertions purely
      self-scoped; listener attached; a red here means a concurrency bug
      (the 5814_01_01 class), not a perf miss. Keep it out of CI for now —
      sandbox-local tool.
[ ] 7. Throughput harness (non-Robot): dev/perf/ k6 or oha scenario for
      create+query against the local broker, wired to read
      broker-metrics.csv afterwards and FAIL if peak RSS ≥ 500 MB (§1
      contract row). Report-only latency percentiles; document that
      absolute numbers from this sandbox are indicative only.
[ ] 8. Docs + palace: claude.md §6 loop-position note, tasks.md evidence,
      MemPalace drawer (perf-tp posture + listener recipe + the
      "conformance TPs are not concurrency-safe" finding, check-duplicate
      first). List Mac-side pushes at the end instead of doing them.
```

Deliberately out of scope: rfswarm (own agent/manager stack — revisit only
if virtual-user realism is ever needed), pabot-izing the conformance
matrix (per-worker brokers/clean_db rewrite — separate project), any
CI-side changes beyond keeping `perf` excluded.
