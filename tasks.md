# Antares — task list

Pruned 2026-08-15: the A–N implementation ladder, Sequencing, and the
completed backlogs (2026-08-12, temporal q= 2026-08-13, IOP TP 2026-08-14)
are done and removed — full text in git history (last pre-prune tree:
commit `99023be`). Working discipline lives in `claude.md` (§0.3 loop,
rules 8/9); this file holds only OPEN work.

## The master /goal (scoped by the user 2026-08-15) — copy-paste this

**STATUS 2026-08-15: COMPLETE.** All four tracks done, everything ticked with
evidence below: (1) routing campaign 58/58 green (full IOP_TP 278/278;
broker fixes 416c274/8cb8256/0cb0447/edc57cc); (2) NATS health member +
outage e2e (a754272, live PG+NATS 4/4); (3) roll-weekly.yml (f273950) +
re-audit with six open findings fixed red-first
(25c59f2/7584be7/a86054e/c9aa821/8a8b39f, table in the audit doc); (4)
report page/badges/README/hygiene/docs/coverage
(2fa0eb2/6b2db61/3d72999/4eae426/d6e01f9). Final local gates: workspace
tests green (-j 2 fix eb234f4), official HTTP Subscription tree 125/125 vs
one memory broker (058_x MQTT = mosquitto DNS, CI-only as always).
Remaining actions are ONLY the "Mac-side / user" list below.

Scope: ONLY (a) the id/idPattern routing campaign, (b) NATS runtime
proof, (c) production testing across all four store types, (d) the
rolling-update (rollout) proof, (e) a production-looking repository
with all docs. Explicitly out: M1 crates.io prep, E8, J4/K11/L1/L2
(hardware), M3, anything Mac-side.

```
/goal Work ONLY these tracks, in this order, until every named box in
tasks.md is [x] with evidence (commit hash + green local run / rendered
page) recorded next to it:
(1) ETSI id/idPattern routing — "## Backlog 2026-08-15 — IOP
id/idPattern routing campaign": execute its own /goal prompt (end of
that section) VERBATIM, all 58 boxes.
(2) NATS — backlog-08-14 item 4: bus health field + NATS-outage e2e,
exactly as the item specifies (read bus.rs first, red-first, absence
asserted for bus=local).
(3) Production testing, all store types + rollout — backlog-08-14
items 5 and 6: the rolling-update/HA job on a weekly cadence, and the
production-readiness re-audit (every P0/P1 CLOSED-with-evidence or
appended as a new box and then worked). Store-type proof: each claim
tested in the mode it touches — memory/file locally per rule 8 and §2
(`STORE=<mode> dev/etsi-local.sh` where the sandbox allows), postgres/
timescale via targeted pg-gated tests on the private-dind recipe where
already sanctioned, and the CI 4×8 matrix as the authority for the
full grid — no claim marked proven for a store the evidence doesn't
cover.
(4) Production-look repository — backlog-08-14 items 1, 2, 3, 7, 8:
clickable ETSI report page, README pass with badges and corrected
targets table, hygiene files (LICENSE is the user's choice — ask once),
docs index + operations runbook, coverage surfaced.
SKIP: M1/E8/J4/K11/L1/L2/M3 and anything Mac-side (pushes, Pages
settings, first scheduled runs) — collect those on the "Mac-side /
user" list instead of blocking.
Full claude.md discipline throughout: MemPalace first, TEST-FIRST red
run as the fallibility proof, negative assertions, rules 8 and 9 hard,
minimal diffs, one item = one commit with its box ticked in the same commit;
CI workflow edits validated by actionlint/yaml parse + local dry-runs.
DONE = all 58 campaign boxes + 08-14 items 1-8 (plus any boxes item 6
appends) [x] with evidence and the Mac-side list written. Ask only for
genuine user decisions (license, rule-9 doubt).
```

## Backlog 2026-08-15b — role-split rolling NATS matrix + per-cell README badges

**Ask (user, 2026-08-15).** (a) A TRUE role-split fleet: 5 roles × 2
replicas = 10 broker containers (api×2 behind the LB, matcher×2,
notifier×2, temporal×2, registry×2), bus=nats, shared Postgres — with the
containers CONTINUOUSLY rolling/restarting for the entire ETSI suite.
(b) Per-push-to-main result cells: memory, file, postgres, timescale
(single-broker, bus=local — as today) PLUS postgres-nats and
timescale-nats (the 10-container rolling role fleet). (c) The per-cell
pass counts visible from the README like "file 1652/1652" — live shields
badges per cell, not a stale hand-edited table.

Grounding in what EXISTS (don't rebuild): docker-compose-ha.yml already
runs 2 all-roles replicas + haproxy + NATS with HA_BUS; rolling-update.sh
rolls ${ROLL_SERVICES}; etsi-pipeline.sh has the HA=1/ROLL_DURING_RUN=1
levers; etsi-matrix.yml is the reusable 3-cell matrix ci.yml runs per
push; dev/etsi-report-site.py + the wasm.yml fold publish /reports/ and
badge.json (item-1 mechanism); ops_router/readiness landed 2026-08-15
(workers serve /q/health,/q/ready,/q/metrics only — the probe basis for
rolling worker pods).

### Checklist

- [x] 1. **Role-split compose overlay** `docker-compose-roles.yml`:
      ✅ DONE 2026-08-15: compose config parses (STORE=postgres, both files,
      --profile db); live process-level fleet validated in-sandbox (10 broker
      processes with the overlay's exact env vs dind postgis+NATS): all 10
      pods ready (api /q/health, workers /q/ready), entity create → exactly
      ONE notification through the split matcher/notifier chain, 5/5 stable
      reruns, invert→FAIL→restore on the exactly-once count. Smoke flake
      root-caused and fixed IN the script: receiver must accept before the
      sub exists (delivery is ~ms and 5.8.6 failure is terminal — pinned by
      times_sent=1/last_failure in PG). Full 10-container compose run = CI.
      10 brokers — api1/api2 (ANTARES_ROLES=api) behind the existing K2
      haproxy, matcher1/2, notifier1/2, temporal1/2, registry1/2 — all
      ANTARES_BUS=nats against the overlay NATS, one shared PG
      (STORE=postgres|timescale). Worker healthchecks poll /q/ready (they
      serve ops endpoints only since P0-4). Bring-up green: every
      container healthy, one entity create via the LB notifies through
      the split matcher/notifier chain (smoke script, not a TP).
- [x] 2. **Role-pair semantics verified red-first** (the F7 class):
      ✅ DONE 2026-08-15 (item-1 commit 04f9261; this item's test commit
      below): nats_e2e.rs role_pairs_exactly_once_semantics — full 10-process
      fleet (api/matcher/notifier/temporal/registry ×2 each) vs live PG+NATS:
      one change → EXACTLY one notification (count==1 after settle);
      timeInterval=2 over 9 s fires 2..=6 single-winner (double-fire = ≥8);
      CSR via api-1 makes api-2 dial the source (F5 mirror consistent across
      the api pair — registry-role pods are ops-only, per the overlay's
      honest note); temporal write via either api records exactly 2 instances
      (negative recheck after settle); P0-4 negative (worker rejects API
      writes). Green first run (semantics already held); fallibility by
      invert→FAILED→restore on the exactly-once count; full nats_e2e 5/5.
      2 matchers on the shared durable a notification fires EXACTLY once
      (no duplicate delivery from the pair — negative assertion on the
      receiver count); interval subscriptions fire single-winner across
      the pair (§3.1.6 claim); registry×2 keeps federation mirrors
      consistent; temporal×2 keeps auto-recording exactly-once per write.
      Unit/e2e level (nats_e2e style, env-gated), BEFORE the suite runs.
- [x] 3. **Continuous role rolling**:
      ✅ DONE 2026-08-15: dev/rolling-update.sh gained ROLES_SPLIT=1 — all 10
      containers in role-group order, peer-of-same-group healthy before its
      twin goes down (api /q/health + LB rise window per K1; workers
      /q/ready). Validated LIVE in-sandbox on the containerized fleet (dind):
      full roll = 10 pods in ~43 s (antares1 23 s K1 drain, workers 2 s);
      availability probe every 0.5 s across the whole roll — 52/52 API
      requests through the LB answered 200, every role group ≥1 ready pod
      the entire time (group_fail=0); negative proven: peer stopped → "peer
      matcher2 unhealthy — aborting roll". file/memory refusals fire.
- [x] 4. **Pipeline lever** `ROLES_SPLIT=1`:
      ✅ DONE 2026-08-15: etsi-pipeline.sh lever wired (roles overlay,
      pg/ts-only + HA-exclusive guards proven to fire, results default
      results/roles-$STORE, roll loop passes ROLES_SPLIT through). Validated
      LIVE in-sandbox (dind compose allowed): STORE=postgres ROLES_SPLIT=1
      ROLL_DURING_RUN=1 SUITES=CommonBehaviours → gate PASS, 56/56 green
      through the real haproxy LB with the fleet rolling, results in
      results/roles-postgres/ with the standard shape (+ roll-loop.log);
      roles-smoke PASS via the LB. First attempt red-herring root-caused:
      sandbox .venv predated the suite's vendored HttpCtrl (ModuleNotFound
      → exitonfailure cascade), fixed by pip -r requirements.txt — a venv
      defect, not a fleet bug. Full 8-suite fleet runs are CI's (item 5).
- [x] 5. **Six cells per push to main**:
      ✅ DONE 2026-08-15: etsi-matrix.yml matrix include grows to six cells —
      memory/file/postgres/timescale (bus=local, as today) + postgres-nats/
      timescale-nats (ROLES_SPLIT=1 + ROLL_DURING_RUN=1, RESULTS_DIR per
      cell, 120-min timeout vs 90). One image build feeds all six;
      fail-fast: false kept; aggregate sets STORES to all six for the fold.
      Validated: yaml parse + actionlint 1.7.7 clean; etsi-matrix-summary.py
      dry-run over six synthetic cells → exit 0 all-green, 6 sections in
      matrix-summary.md, missing-cell → red exit (sandbox cannot trigger
      GitHub runs; the first real 6-cell wall-clock is the watch item —
      cadence-vs-scope is the USER's call if too slow, on the Mac-side list).
      etsi-matrix.yml matrix grows to
      memory, file, postgres, timescale (single-broker bus=local, as
      today) + postgres-nats + timescale-nats (item-4 fleet, rolling).
      One image build feeds all six; fail-fast: false; the aggregate
      summary + ETSI-matrix-results bundle fold all six cells (STORES
      env in etsi-matrix-summary.py/etsi-report-site.py). Watch runner
      wall-clock: the nats cells are the K8 job cost — if a per-push
      6-cell wall is too slow, the DECISION (user) is cadence vs scope,
      not silent dropping.
- [x] 6. **Per-cell badges**:
      ✅ DONE 2026-08-15: etsi-report-site.py emits badge-<cell>.json per
      cell (label=cell, message "ok/total", green on gate PASS / red
      otherwise) next to badge.json, and now AUTO-DISCOVERS cells from the
      bundle when STORES is unset — so the Pages fold renders whatever the
      newest matrix ran (an old 3-cell bundle keeps rendering green instead
      of inventing missing cells). README gets the six-badge live row
      (memory/file/postgres/timescale/postgres-nats/timescale-nats), each
      linking /reports/latest/. Validated: script run over 6-cell and old
      3-cell synthetic bundles — correct badges both ways.
      dev/etsi-report-site.py additionally emits
      badge-<store>.json per cell ("1652/1652" green / "1640/1652" red)
      next to the combined badge.json; README gets one shields endpoint
      badge per cell (file, memory, postgres, timescale, postgres-nats,
      timescale-nats) linking the report page — the "1024/1024 file"
      view, live.
- [ ] 7. **Badges track main automatically**: a pages.yml with
      `workflow_run` on ci.yml completion (main only) that rebuilds the
      site (www artifact reuse or rebuild) + folds the fresh bundle and
      deploys Pages — so a push to main updates the README numbers
      without a manual wasm dispatch. NOTE: this OVERTURNS the 2026-08-07
      "Pages deploys are manual-only" decision — flag it in the commit
      and keep wasm.yml's manual dispatch working for playground-only
      deploys.
- [ ] 8. **README + runbook updated**: the badge row (item 6), a
      role-split deployment paragraph (10-container shape, what rolls,
      what the cells prove), docs/operations.md gains the roles fleet;
      negative check — no README claim a cell does not actually gate.

### The /goal prompt — copy-paste to run this campaign

```
/goal Work the "Backlog 2026-08-15b — role-split rolling NATS matrix +
per-cell README badges" checklist in tasks.md top-to-bottom until every
box is [x] with evidence (commit hash + green run / rendered artifact)
recorded next to it. One item = one commit. Full claude.md discipline:
MemPalace first, TEST-FIRST red run as the fallibility proof (item 2's
exactly-once negatives especially), negative assertions everywhere,
ponytail. Rule 8 nuance: compose stacks for THIS campaign run only where
sanctioned — prefer the private/sibling-container recipe from the palace
for live PG+NATS e2es, keep the full 10-container fleet runs in CI, and
validate workflow edits with actionlint/yaml parse + script dry-runs
(the sandbox cannot trigger GitHub runs). Anything needing a push, Pages
settings, or a cadence/cost decision goes on the "Mac-side / user" list
instead of blocking. Ask only for genuine user decisions (per-push
6-cell wall-clock cost if it proves too slow, the Pages auto-deploy
overturn if unwanted).
```

## Blocked / user-side (carried from the completed ladder)

- [ ] E8. ⛔ NEEDS-YOU (policy decision, one line): the CI `publish` job
      pushes `:latest` + `:v<run>` to GHCR automatically on every green
      master run. Decide: keep it automatic, or gate it behind a GitHub
      Environment with required approval. Until you decide it stays
      automatic.
- [ ] J4. 🖥 NEEDS-HARDWARE (16 GB Postgres box; not a free CI runner).
      10M-row synthetic benchmark (xtask seeder): q=/geo/type query p95s;
      decide the extracted-attribute side table (named lever,
      deep-analysis §8.1) on numbers, not vibes.
- [ ] K11. 🖥 NEEDS-HARDWARE for the real drill (kind/compose covers the
      rehearsal). Postgres primary kill (replica promotion, broker
      reconnects without dropping acked writes), NATS node kill (R3 stream
      survives, consumers resume, at-least-once holds), broker pod kill
      between commit and publish (outbox covers it) (§10, §13 phase 4).
- [ ] L1. 🖥 NEEDS-HARDWARE (you provide the box + 24 h of it).
      `xtask load-rig`: 10M entities / 1,000 tenants seeded; 24 h soak;
      all §1 targets measured and held (broker <500 MB, PG <16 GB).
- [ ] L2. 🖥 NEEDS-HARDWARE (1,000 mock sources exceeds a CI runner).
      Federation rig: 1 tenant × 1,000 CSRs (20 % expired, mixed modes)
      vs mock sources with injected latency/failures; p95 bounded by the
      aggregate deadline, 207 correctness, mirror memory in budget (§16.7).
- [ ] M1. ⛔ NEEDS-YOU. Publish `ngsild-model` + `ngsild-ql` to crates.io;
      reserve the `ngsild` facade name (§9.1 — checked free 2026-08-03).
      Needs your crates.io token, and a published name cannot be taken
      back. An agent may prepare the crates (metadata, README,
      `cargo package` dry-run, `--dry-run` publish) and then stop.
- [ ] M3. ⏳ BLOCKED-EXTERNAL. WS binding stays deferred (§11): keep the
      seams (sink registry, outbox, one matcher). Unblocks only when ETSI
      TC DATA issue #8 produces a TS — implementing ahead of it risks
      divergence.

(H2 snapshots was on this list but is DONE — the Snapshot API shipped
2026-08-12, ledger 5.16 `implemented`, TP 5161_01; box removed.)

## Backlog 2026-08-14 (production-ready)

Goal: the repo reads as a production-ready project from the GitHub front
page — badges, a CLICKABLE ETSI conformance report (stats like 1652/1652
visible without downloading any artifact), current docs, and the two
operational claims (rolling update, NATS bus) proven by tests that run on
a cadence, not only on `v*` tags. Grounded in the 2026-08-14 repo audit:
what already EXISTS is `dev/rolling-update.sh` + `dev/k6-continuity.py` +
k8s kind smoke (all in full.yml, tag/dispatch-only), NATS bus tests in
ci.yml, per-store `GITHUB_STEP_SUMMARY` tables + `ETSI-matrix-results`
artifact in etsi-matrix.yml, and a GitHub Pages site owned by wasm.yml
(www playground). Don't rebuild those — surface and extend them.

### The /goal prompt — copy-paste this to run the backlog to completion

```
/goal Work the "## Backlog 2026-08-14 (production-ready)" checklist in
tasks.md top-to-bottom until every box is [x] with evidence (commit hash +
green run / rendered page) recorded next to it. One item = one commit.
Full claude.md discipline: MemPalace first, TEST-FIRST red run as the
fallibility proof, negative assertions, rule 8 (one local broker, no
compose stacks, no host Docker) and rule 9 stay hard; keep diffs minimal throughout.
CI workflow edits are validated by actionlint/yaml parse + local script
dry-runs (the sandbox cannot trigger GitHub runs); anything that needs a
GitHub push, Pages settings, or a license choice goes on the "Mac-side /
user" list at the end of the section instead of blocking. Ask only when an
item forces a genuine user decision (license, Pages mechanics if both
options fail).
```

### Checklist

- [x] 1. **ETSI report clickable from the README (the headline ask).**
      ✅ DONE 2026-08-15 (2fa0eb2): dev/etsi-report-site.py renders the
      ETSI-matrix-results bundle to a static page + shields badge.json;
      wasm.yml folds it under /reports/ in the ONE Pages deployment (gh api
      newest-bundle download, placeholder when absent); report.html added to
      cell/roll uploads. Local validation: generated over results/g4-* (badge
      "4×122 green"), served in-sandbox on :42040; actionlint clean.
      etsi-matrix.yml's summary job already builds
      `cells/_combined/matrix-summary.md` + per-store `run-summary.md` and
      robot `report.html`/`log.html`. Publish them: assemble a static
      `site/reports/latest/` (index.html rendering the matrix summary with
      per-store pass/total, linking each store's robot report.html) and
      fold it into the ONE GitHub Pages deployment. Constraint: Pages has
      one deployment source and wasm.yml currently owns it — recommended
      shape: ONE pages deploy job that combines the www playground artifact
      + the latest reports (e.g. wasm.yml's site assembly downloads the
      newest `ETSI-matrix-results` via `gh api`, or a single pages.yml
      triggered by both). Also emit `site/reports/badge.json` in shields
      endpoint schema ("ETSI 4×1652 green" / red). Acceptance: a README
      link → a page showing the stats, zero downloads. Local validation:
      run `dev/etsi-matrix-summary.py` over an existing `results/` cell
      set + open the generated index in-sandbox (serve on 42040 for the
      user to click).
- [x] 2. **README production pass.**
      ✅ DONE 2026-08-15 (6b2db61): badges (ci, strict, roll-weekly, ETSI
      endpoint, coverage endpoint); targets table = claude.md §1 with a
      targets-vs-proven note; compose quickstart; /q/ready; report page +
      docs index linked; stray /goal line removed; negative check applied. Badges at top (ci, etsi-matrix, the
      shields endpoint badge from item 1, coverage); FIX the stale targets
      table (still the pre-2026-08-10 10× smaller contract — claude.md §1
      is the authority: 100M entities / 10k tenants / 100k subs);
      quickstart section (docker run + compose, the four store modes in
      two lines); link the conformance report page and a docs index.
      Negative check: no claim in the README that CI does not actually
      prove.
- [x] 3. **Repo hygiene files.**
      ✅ DONE 2026-08-15 (3d72999): LICENSE = BSD-3-Clause per the STANDING
      declaration in Cargo.toml `license` + the README License section (NOT a
      new choice — flag on the user list to swap if unwanted); SECURITY.md
      (contact + §16 posture + both audits); CONTRIBUTING.md (-j 2, spec-
      first, TEST-FIRST, one-clause-one-commit, local Robot gate);
      CHANGELOG.md seeded (no v* tag exists yet — noted). LICENSE is MISSING (deny.toml gates dep
      licenses; the repo itself has none) — license choice is the USER's,
      ask once and add. SECURITY.md (contact + the §16 security-wall
      posture, link docs/security-audit-2026-08-04.md), CONTRIBUTING.md
      (build, `-j 2` linker rule, test protocol, one-clause-one-commit),
      CHANGELOG.md seeded from the `v*` tags.
- [x] 4. **NATS visible + tested at runtime ("check NATS is working").**
      ✅ DONE 2026-08-15 (commit: health bus member + outage e2e): /q/health
      `bus:{mode,connected,reconnects}` via bus_stats provider (absent for
      bus=local, unit-asserted + invert-proven); NatsBus connected()/
      reconnects() over ConnectOptions event_callback;
      nats_outage_flips_health_and_recovers e2e (killable TCP proxy fronting
      real NATS): health flips false, API keeps serving during the outage,
      reconnect + outbox drain + notifications resume, at-least-once held —
      nats_e2e 4/4 green vs live PG+NATS (sibling-container recipe).
      `/q/health` reports store mode and file commit-queue but NOTHING
      about the bus — add `bus: {mode, connected, reconnects}` when
      ANTARES_BUS=nats (red-first unit on the health payload; assert the
      field is ABSENT for bus=local). Then a NATS-outage e2e (tests/,
      gated on ANTARES_TEST_NATS_URL like the existing F9 bus tests, live
      NATS via the private-dind recipe in the palace): kill NATS mid-run →
      health flips connected:false, broker keeps serving API traffic,
      restart NATS → reconnect + subscription notifications resume, no
      panic. Assert the DEFINED semantics, whatever the current
      implementation's contract is — read bus.rs first.
- [x] 5. **Rolling-update proof on a cadence, not only on tags.**
      ✅ DONE 2026-08-15: .github/workflows/roll-weekly.yml — schedule Tue
      04:17 UTC (offset from etsi-coverage Mon 04:41) + dispatch; build job
      = etsi-matrix build shape (shared type=gha cache), roll job = full.yml
      etsi-roll verbatim (postgres, HA=1, ROLL_DURING_RUN=1, PASS gate).
      Validated: yaml parse + actionlint 1.7.7 clean (sandbox cannot trigger
      GitHub runs). README badge + reports-page line land with items 1-2;
      first scheduled run = Mac-side list.
      full.yml's K1/K3 job (haproxy + `dev/rolling-update.sh` +
      `k6-continuity.py` under load) runs only on `v*` push/dispatch — add
      `schedule:` (weekly, offset from etsi-coverage's Mon 04:41) to
      full.yml or split the HA/rolling job into a scheduled workflow
      reusing the same steps. Surface the result: workflow badge in README
      + one line on the reports page. Sandbox validation: yaml/actionlint
      + a dry parse of the job's script steps; the run itself is CI's.
- [x] 6. **Production-readiness re-audit.**
      ✅ DONE 2026-08-15: every P0/P1 walked at HEAD; status table written
      into docs/production-readiness-audit-2026-08-09.md ("Re-audit
      2026-08-15"). Closed-with-evidence: P0 2/5/8/9/10, P1 all but one
      (one refuted by the audit itself, one closed-by-decision with the
      doc-comment anchor). Still-open items appended below and WORKED:
      - [x] 6a. P0-1 k8s manifests boot (reproduced first) — 25c59f2
      - [x] 6b. P0-3 /q/ready readiness (store ping + bus gate) — 7584be7
      - [x] 6c. P0-4 roles gate the API (ops_router on workers) — 25c59f2
      - [x] 6d. P0-6 outbox exact-seq ack (loss reproduced red on live
            PG, then pinned) — a86054e
      - [x] 6e. P0-7 throttling no-op (mirror bookkeeping) — c9aa821
      - [x] 6f. P1 temporal types frozen at first touch — 8a8b39f
      All red-first with units/pg-gated tests; nats_e2e 4/4 live,
      antares-api + antares-sql suites green. Walk every P0/P1 in
      docs/production-readiness-audit-2026-08-09.md; for each, record
      CLOSED (commit + test evidence) or still-open. Still-open items
      become new checkboxes appended to THIS list (that is the "if
      something is missing, it should also be tested" clause). Write the
      updated status table back into that doc with today's date.
- [x] 7. **Docs index + operations runbook.**
      ✅ DONE 2026-08-15 (4eae426): docs/README.md tree map + live artifacts;
      docs/operations.md runbook (deploy, health/ready/metrics with the
      proven NATS-outage contract, per-store backup, rolling update + K10
      file-mode rule, state reset, claims→workflows table). docs/README.md mapping the
      docs tree (deep-analysis, adr/, spec/ ledger, audits, upstream
      raises); docs/operations.md runbook: deploy (compose + deploy/k8s),
      per-store backup table (README already has the redb stop-copy rule —
      link, don't duplicate), rolling update procedure (`rolling-update.sh`
      + the file-mode-cannot-roll K10 rule), health/metrics endpoints,
      state-reset discipline (§2). Mostly assembling existing text — no
      new claims without a test behind them.
- [x] 8. **Coverage surfaced.**
      ✅ DONE 2026-08-15: merged genhtml in the etsi-coverage-merged
      artifact; wasm.yml publishes /reports/coverage/ + coverage-badge.json
      (% from merged.info); README badge wired. Display only — no floor. etsi-coverage.yml already produces
      lcov+html weekly — publish the merged HTML + a % badge json to the
      same `site/reports/` area (item 1's mechanism), README badge. No new
      floor/ratchet (strict.yml owns the unit floor) — display only.

Mac-side / user (consolidated 2026-08-15 — nothing below blocks anything):
1. Push master (~25 commits: routing campaign fixes, audit closures, NATS
   health, workflows, docs/hygiene) and the suite fork (7 IDR TP commits).
2. Watch the next 4×8 matrix — with edc57cc/8cb8256/0cb0447 aboard all
   cells should be 1652+/green incl. the 58 new Routing TPs.
3. Pages: verify the source is "GitHub Actions", then dispatch wasm.yml
   with deploy_pages ✔ — that publishes the playground + /reports/latest/
   + /reports/coverage/ and lights the two shields badges in the README.
4. Trigger/verify the first scheduled runs: roll-weekly (Tue 04:17 UTC)
   and etsi-coverage (Mon 04:41 UTC) — or dispatch both once now.
5. LICENSE: added as BSD-3-Clause following the STANDING declaration in
   Cargo.toml + the README — say the word if you want a different license.
6. Stale branch delete: `git push origin --delete fed-alias-tenant`.

## Backlog 2026-08-15 — IOP id/idPattern routing campaign (this goal)

**Why.** ADR-001 (Urbivita URN standard) claims a CSR with an anchored
`idPattern` prefix (`^urn:ngsi-ld:{Typ}:{Razidlo}:{Evidencia}:.*$`) gives
(a) exact routing of `GET /entities/{id}` to the ONE owning source and
(b) pruning — the broker never dials a source whose pattern cannot match,
so federation cost scales with matching CSRs, not total CSRs. Verified
live 2026-08-15 (AntaresBroker, two memory brokers): the exact ADR CSR
routed retrieve + type-query to B2 (dark entity, `local=true` 404), and a
`sk_presov` id 404'd with NO `NGSILD-Warning` while B2 was down — B1 never
dialed it. Code anchor: `entity_info_matches` (csource.rs:1160, clause
5.12) — exact-eq on `EntityInfo.id`, `regex::find` (UNanchored substring)
on `EntityInfo.idPattern`; this is exactly the full-match-vs-substring
divergence ADR-001's mandatory `^...$` neutralizes. Spec anchors: 5.12
(pp. 241-242, the FIVE EntityInfo match conditions — no-id/pattern,
ids-vs-id, ids-vs-idPattern, pattern-vs-id, both-patterns-present — plus
the attribute-overlap and datasetId-common-value conditions and the
4.3.6.4 previously-encountered exclusion), 5.2.8 (idPattern = IEEE 1003.2
regex), 4.3.6.1-4.3.6.4, 5.7.1.4/5.7.2.4, 5.6.x.4, 5.8.1.4, 4.14, 4.20,
6.3.17.

**Deliverable.** 58 new Robot TP cases in the suite fork under
`IOP_TP/NGSI-LD/Interoperability/Routing/` (7 files, IOP_EXT_IDR_01..07),
following IOP_TP conventions: `${b1_url}..${b5_url}` variables,
`InteropUtils.resource` keywords, `Setup Interop Ids`/`Cleanup Interop
Fixtures`, `[Tags]` carrying the clause (`5_12`, `4_3_6` form), broker
fleet via `dev/run-five.sh`, HttpCtrl mocks where "zero forwarded
requests" must be asserted. Every case carries at least one NEGATIVE
assertion (what must NOT be in the response / which mock must record NO
request). ADR-shaped URNs (`urn:ngsi-ld:WasteContainer:sk_banskabystrica:
odpady:...`) are the fixture id vocabulary throughout. Broker gaps found
by a red TP are fixed red-first per §0.3 (one clause = one commit).

### IOP_EXT_IDR_01 — retrieve-by-id routing via EntityInfo id/idPattern (B1→B2)

Evidence (1-10): suite fork commit a2cbe01, IOP_EXT_IDR_01.robot 10/10 green
on the run-five memory fleet 2026-08-15; fallibility cycle on case 03.

- [x] 1. Anchored ADR prefix pattern routes `GET /entities/{id}` B1→B2;
      entity exists only in B2 → 200 via B1; `local=true` still 404 (5.7.1.4, 5.12).
- [x] 2. Non-matching razidlo (`sk_presov`) → B1 404s WITHOUT contacting B2 —
      mock context source asserts ZERO forwarded requests (5.12; the pruning claim).
- [x] 3. Exact `EntityInfo.id`, no pattern: that one id routes; a sibling id
      under the same prefix is NOT forwarded (5.12 cond 2).
- [x] 4. UNanchored pattern `sk_banskabystrica:odpady` matches by substring
      (regex find) — id carrying it mid-URN forwards; documents why ADR-001
      mandates `^...$` (5.2.8, 5.12).
- [x] 5. EntityInfo with type only (no id/idPattern) matches every id of that
      type (5.12 cond 1).
- [x] 6. idPattern matches but EntityInfo type ≠ requested type → NOT
      forwarded (5.12 type-selector gate).
- [x] 7. Multiple EntityInfo elements in one RegistrationInfo — any-of: id
      matching only the second element still forwards (5.12).
- [x] 8. Two CSRs with disjoint razidlo prefixes → retrieve dials EXACTLY one
      endpoint; the other mock records zero hits (5.12, 4.3.6.2).
- [x] 9. Invalid regex in idPattern → 400 BadRequestData at registration,
      nothing registered (5.2.8; csource.rs:143 already validates — assert).
- [x] 10. Case sensitivity: pattern `sk_banskabystrica` does not match an id
      containing `SK_BanskaBystrica` (IEEE 1003.2; ADR lowercase rule).

### IOP_EXT_IDR_02 — query-entities routing (query id/idPattern × CSR id/idPattern)

Evidence (11-19): suite fork commit (IDR_02) 9/9 green on run-five 2026-08-15;
found TWO broker gaps, fixed red-first in 416c274 (query-side idPattern into
CsrSpec; forwarded id-list narrowing via FedReg::can_match_id) with
tests/id_routing_5_12.rs 3/3 and antares-api suite green.

- [x] 11. `?id=A,B` where only A matches the CSR pattern → forwarded query
      narrowed to A (mock asserts), B answered locally (5.7.2.4, 4.3.6.1).
- [x] 12. Query `?idPattern=` matching `EntityInfo.id` → forwarded (5.12 cond 4).
- [x] 13. Query pattern + CSR pattern both present → assumed compatible,
      forwarded (5.12).
- [x] 14. Query pattern anchored to a foreign razidlo vs CSR ids of another
      razidlo → NOT forwarded, zero mock hits (5.12).
- [x] 15. Type-only query against an id-restricted CSR → forwarded (broker
      cannot exclude; assert current Antares behaviour and cite 5.12).
- [x] 16. Fan-out merge: 3 brokers, one razidlo each; type query via B1
      returns the exact union, no duplicate ids (5.7.2.4, 4.5.5).
- [x] 17. `local=true` query never forwards regardless of pattern match (5.5.13).
- [x] 18. Dark entity: exists ONLY behind the CSR; query via B1 includes it;
      `local=true` query does not (4.3.6.2).
- [x] 19. CSR discovery `GET /csourceRegistrations?id=<urn>` returns only CSRs
      whose EntityInfo id/idPattern matches; `?idPattern=` matches
      EntityInfo.id too (5.10.2, 5.12).

### IOP_EXT_IDR_03 — provision routing by id (redirect/exclusive/inclusive)

Evidence (20-29): suite fork commit (IDR_03) 10/10 green on run-five
2026-08-15; cases 24/28/29 ran red and drove broker fix 8cb8256 (batch items
gated by EntityInfo idPattern via can_match_id; covers_item + batch-delete
sent_ids), unit covers_item_honours_entityinfo_id_patterns, crate suite green.

- [x] 20. redirect CSR, anchored pattern: POST /entities with matching id is
      created at B2, NOT held at B1 (`local=true` 404) (5.6.1.4, 4.3.6.2).
- [x] 21. Create with non-matching id stays local at B1; redirect mock records
      zero requests (5.6.1.4).
- [x] 22. `DELETE /entities/{id}` routed by pattern → 204, gone at B2;
      non-matching delete never leaves B1 (5.6.6.4).
- [x] 23. PATCH / partial-attribute-update routed by id through the pattern
      (5.6.2.4, 5.6.3.4).
- [x] 24. Batch create with MIXED razidlos: matching subset forwarded, rest
      local; success arrays carry ALL ids exactly once (5.6.7.4, 5.6.8.5).
- [x] 25. Deterministic-URN idempotency (ADR claim): same upsert twice via B1 →
      second is UPDATE (remote 204 ⇒ updated-list, per 2c6c10b), federation-wide
      query still exactly ONE entity (5.6.8.5).
- [x] 26. Exclusive CSR without explicit EntityInfo.id + attributes → rejected
      at registration (5.2.9, 4.3.6.2; validate_exclusive).
- [x] 27. Exclusive CSR by exact id: update routes ONLY to the remote; a local
      shadow copy is never created nor consulted (4.3.6.2).
- [x] 28. Registration narrowing on forwards: mock asserts the forwarded
      request carries only the registered id scope — no broadening (4.3.6.1).
- [x] 29. Batch upsert across 3 disjoint-razidlo redirect CSRs splits three
      ways; each mock receives ONLY its ids (5.6.8.4).

### IOP_EXT_IDR_04 — multi-broker topologies (3-5 brokers, run-five fleet)

Evidence (30-39): suite fork commit (IDR_04) 10/10 green on the five-broker
run-five fleet 2026-08-15; fallibility cycle on case 34 (localOnly).

- [x] 30. Star B1→{B2..B5}, four razidlos: retrieve dials exactly one; unique
      per-broker marker attribute proves the source; others unhit (5.7.1.4).
- [x] 31. Same star: type query = exact union of four remotes + B1-local
      (5.7.2.4).
- [x] 32. Cascade B1→B2→B3 (B2 holds a CSR for B3's narrower prefix):
      retrieve via B1 resolves through the chain (4.3.6.4).
- [x] 33. Loop B1↔B2 with overlapping patterns: Via header terminates the
      cycle, request completes with correct data (4.3.6.3, 6.3.18).
- [x] 34. `localOnly=true` on B1's CSR → B2 answers from its own data only,
      does NOT cascade to its B3 registration (4.3.6.4, 5.2.34).
- [x] 35. Overlapping redirect CSRs — two endpoints both match one id: the
      operation is distributed to ALL matching (4.3.6.3).
- [x] 36. Prefix shadowing: coarse `Typ:Razidlo:.*` CSR→B2 + fine
      `Typ:Razidlo:odpady:.*` CSR→B3 — both match, both consulted, merged
      result correct (5.12, 4.5.5).
- [x] 37. Matched endpoint down: retrieve returns 404/partial WITH
      `NGSILD-Warning` 199; a non-matching id returns 404 WITHOUT the warning
      (6.3.17; verified live 2026-08-15 — pin it).
- [x] 38. Registration timeout+cooldown honoured: second request inside the
      cooldown fails fast, mock sees exactly ONE dial (5.2.34).
- [x] 39. Routing follows the live CSR set: delete the CSR → next retrieve
      404s and the mock records zero new hits (5.9.4, 5.12).

### IOP_EXT_IDR_05 — subscriptions & notifications routed by id

Evidence (40-45): suite fork commit (IDR_05) 6/6 green on run-five
2026-08-15; cases 42/45 ran red -> broker fix 0cb0447 (inbound remote
notifications re-filtered by the original entities selector; red-first unit
clause_5_2_33_inbound_notification_refiltered_by_selector). NOTE box 41
corrected in the TP: disjoint sub-pattern vs CSR-PATTERN still matches per
5.12 (both patterns => compatible); the decidable no-match half is sub
pattern vs exact EntityInfo id, asserted as written.

- [x] 40. Subscription at B1 with `entities:[{type,idPattern}]` overlapping a
      CSR → distributed sub created at B2; create at B2 → notification via B1
      (5.8.1.4).
- [x] 41. Subscription pattern disjoint from the CSR pattern → NO remote sub
      created at B2 (assert absence via B2's subscription list) (5.8.1.4).
- [x] 42. Create matching + non-matching ids at B2 → exactly ONE notification;
      the non-matching id absent from every payload (5.8.6, negative).
- [x] 43. csourceSubscription with EntityInfo idPattern: registering a matching
      CSR notifies, a disjoint CSR does not (5.11.3 — ADR's discovery
      automation).
- [x] 44. Remote entity deleted → default notificationTrigger excludes
      deletions; with entityDeleted requested the notification fires (5.2.12).
- [x] 45. Subscription with exact id list: only the listed id notifies across
      the federation; id takes precedence over idPattern (5.2.33, 5.8.1.4).

### IOP_EXT_IDR_06 — ADR-001 URN grammar edge cases on the wire

Evidence (46-50): suite fork commit (IDR_06) 5/5 green on run-five
2026-08-15; fallibility cycle on case 50 (tenant isolation half).

- [x] 46. Hierarchy colons in the local segment
      (`...odpady:kontajner:sektor-a:0042`) route through the prefix pattern;
      full ADR validator regex accepted end-to-end (5.12).
- [x] 47. Crockford-base32 random-suffix ids route identically to natural
      keys (5.12).
- [x] 48. Unescaped-dot trap: pattern `sk.banskabystrica` (dot = any char)
      ALSO matches `sk_banskabystrica` ids — TP documents why razidlo uses
      `_` and patterns escape metacharacters (5.2.8).
- [x] 49. Multi-type entity (`["Device","Camera"]`): CSR pattern registered
      with the supertype still matches after the specialization type is
      appended; retrieve via B1 works (4.5.2, 5.12).
- [x] 50. CSR `tenant` member (4.14): forward to B2 uses the REGISTRATION's
      tenant, the client tenant never propagates; and with
      `operations:["federationOps"]` a write through that CSR must NOT reach
      B2 (4.20 query-op gate, 826afac) — the ADR's read-only agent-tenant
      posture, both halves asserted.

### IOP_EXT_IDR_07 — negative routing: the spec's must-NOT-forward surface

Verbatim grounds (verified against the PDF 2026-08-15): p.41 — brokers
"shall respect" a source's limited operations "to avoid unnecessarily
sending distributed operation requests which are always guaranteed to
fail"; 4.3.6.2 — auxiliary registrations "are limited to context
information consumption operations (see clause 5.7)"; 4.3.6.4 — "no
registration shall match if the CSourceRegistration contextSourceAlias
can be found within the listing of previously encountered Context
Sources" and "each Tenant … shall be considered separately"; 5.12 —
attribute-overlap and datasetId-common-value match conditions. Every case
here asserts the mock records ZERO requests (the routing decision itself
is the subject under test).

Evidence (51-58): suite fork commit (IDR_07) 8/8 green on run-five
2026-08-15; cases 51/53 ran red -> broker fix edc57cc (query-side attrs into
CsrSpec per the 5.12 attribute conditions + the should-level datasetId
common-value gate, red-first units in id_routing_5_12.rs); case 54 pins the
offset-timestamp edge as 400 at input (4.6.3 Z-only), closing the audited
string-compare path; case 52 red was a vendored-stub artifact (attrs-aware
stub needs the attribute in its body), fixed in the TP. Full Routing tree
58/58.

- [x] 51. Attribute-scope mismatch: request `?attrs=speed`, RegistrationInfo
      lists only `propertyNames:["fillLevel"]` → CSR not matched, NOT
      forwarded even though the idPattern matches (5.12 attribute conditions).
- [x] 52. Over-pruning guard (the opposite bound): RegistrationInfo with
      EMPTY propertyNames/relationshipNames (entities only) DOES match any
      `?attrs=` — must still forward (5.12: empty combination = match).
- [x] 53. datasetId disjoint: request `datasetId=urn:a`, CSR
      `datasetId:["urn:b"]` → not matched, no forward; only ONE side
      specifying datasetId → match, forwarded (5.12, should-level — note it).
- [x] 54. Expired CSR (`expiresAt` in the past) never matches: retrieve/query
      with a matching id does not dial it, discovery omits it (5.2.9;
      reg_expired csource.rs:604 — also pins the audited L-finding that
      string-compared timestamps; use a non-Z offset timestamp as the edge).
- [x] 55. Operation gating on reads: CSR with `operations:["createEntity"]`
      (or updateOps only) must NOT be dialed for `GET /entities/{id}` or
      query even when the idPattern matches (4.3.6.1 p.41 guaranteed-to-fail
      rule, 4.20; query_op gate 826afac).
- [x] 56. Auxiliary CSR never receives provision ops: create/update/delete
      with a matching id stays local, auxiliary mock records zero requests;
      the SAME auxiliary CSR is consulted for retrieve (4.3.6.2 —
      consumption-only, both halves asserted).
- [x] 57. Tenant separation: CSR registered under tenant A must not route
      tenant B's request for a matching id — B gets a clean 404, zero mock
      hits (4.3.6.4 "each Tenant … considered separately", 4.14).
- [x] 58. Previously-encountered exclusion: a request arriving WITH a Via /
      encountered-sources listing containing the CSR's contextSourceAlias →
      that CSR shall not match, no re-forward (4.3.6.4; sharpens case 33
      from the receiving broker's side).

### The /goal prompt — copy-paste to run this campaign

```
/goal Work the "Backlog 2026-08-15 — IOP id/idPattern routing campaign"
checklist in tasks.md top-to-bottom until all 58 boxes are [x] with
evidence (commit hash + green local run) recorded next to each. Per case:
MemPalace FIRST (5.12 pp.241-242, 5.2.8, 4.3.6.x via mempalace_search +
mempalace_get_pdf_pages — cite the clause in [Documentation]); check
existing TPs for overlap before writing (grep the official TP/ + IOP_TP/
trees — never duplicate an ETSI TP); write the TP in the suite fork under
IOP_TP/NGSI-LD/Interoperability/Routing/ per IOP_TP conventions
(b1_url..b5_url, InteropUtils keywords, clause tags like 5_12/4_3_6,
ADR-shaped fixture URNs, at least one negative assertion per case —
what must NOT appear / which mock must record ZERO requests); --dryrun
while iterating, then the REAL run per rule 8 against the dev/run-five.sh
fleet (memory store, no docker; kill the brokers afterwards). A red TP
against the broker = prove it from the clause text: broker bug → fix
red-first with unit tests per §0.3 and commit `<clause>:` separately;
suite/spec defect → error.md + testsuite-doubts.md, never hack the broker.
Group commits per IDR file; tick boxes with evidence as you go; ledger
notes on touched clauses updated. Rules 8 and 9 stay hard; keep diffs minimal;
Mac-side pushes are out of scope — list them at the end. Ask only when
rule 9 leaves you genuinely unsure.
```
