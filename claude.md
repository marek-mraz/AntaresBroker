# Antares — agent working rules

**An NGSI-LD Context Broker in Rust.** This file is the agent's contract:
rules, the audit loop, run policy. Everything else lives elsewhere —
architecture in `docs/deep-analysis.md` (§-references below point there),
irreversible decisions in `docs/adr/`, the conformance ledger in `docs/spec/`,
suite/spec defects in `error.md` + `ngsi-ld-test-suite/testsuite-doubts.md`,
the task list in `tasks.md`.

## 0. Ground rules

1. **Never touch the host Mac.** Do not use `/var/run/docker.sock` (or any
   Docker CLI/API against it), do not start/stop/inspect host containers, do
   not mount host paths, do not install anything Mac-side. Everything needed
   for development runs inside this sandbox. If a task appears to require
   host Docker (e.g. Postgres/NATS containers), stop and ask the user.
2. **Spec-first implementation.** Every feature is implemented from its ETSI
   CIM 009 V1.9.1 clause, NOT from ETSI test-suite failures. The Robot suite
   is the validation oracle run *after* a clause is implemented — never the
   requirements source (its 686 TPs cover only part of the normative
   surface; a broker built test-first ships the untested gaps broken).
3. **MemPalace is the spec oracle.** Any question about payload shapes,
   endpoint contracts, attribute semantics, temporal behaviour:
   `mempalace_search` + `mempalace_get_pdf_pages` FIRST, answer grounded in
   the returned clause text with the clause number cited. Never from memory.

## 0.3 GOAL — the conformance audit loop (STRICT, 2026-08-10)

**Goal: every clause file in `docs/spec/` audited — status earned with evidence, code annotated with its CIM 009 clause, Robot TPs (including edge cases) covering its normative surface.** The ledger was deliberately reset to zero (947 sections `not-implemented`, old `docs/ics.yaml` deleted — history in git). Statuses are EARNED by this loop, never assumed from the old ledger or from memory.

Work **file by file, in clause order** (`4.*` → `5.*` → `6.*` → `7.*` → annex `A`, `B`; annexes C–I are informative — mark `status: informative`, nothing to implement). Per file `docs/spec/<ch>/<clause>.md`:

1. **Read the clause text in the file, then ask MemPalace before writing a line.** The body IS the spec text; for comma-level wording read the PDF pages named in the frontmatter (`mempalace_get_pdf_pages`). Then `mempalace_search` the clause number/topic — prior decisions, gotchas, audit findings and testsuite-doubts for that surface live there, written by earlier sessions and other agents. Implementing a clause without that lookup repeats already-paid-for mistakes. Decisions and traps discovered during the clause work go BACK into the palace (`mempalace_add_drawer`, after `mempalace_check_duplicate`).
2. **Find the implementation and annotate it.** Every function implementing normative behaviour carries a doc comment citing **the CIM 009 clause number + a one-line summary of the rule**, e.g. `/// 5.6.6 Delete Entity: 204 on success, ResourceNotFound 404 if absent.` **FORBIDDEN as normative citation: internal documents** — never `(see docs/deep-analysis.md §9.3)`, never `claude.md §…`, never tasks.md. Internal docs may be cited for *architecture* decisions only; the requirement's source is always the spec clause. Existing comments citing internal docs as the requirement are fixed on touch.
3. **Verify the FULL normative behaviour** against the clause text: every SHALL, every error case (type + status per Table 6.3.2-1), every output member. A gap → implement it now (if it passes the rule-9 scope test), or set `status: partial` with the gap NAMED in `notes:`. Never mark `implemented` with a known gap.
4. **Unit tests for the clause's own rules and edge cases — and RUN them, scoped to the clause.** Boundary values, invalid input → the exact spec error type, empty/absent optional members, multi-instance `datasetId` where applicable, tenant isolation. Test names/doc comments cite the clause number. **Writing tests is not evidence — a green run is**, and the run is TARGETED: `cargo test -p <touched-crate> <filter> -j 2` for the tests of this clause/feature, not the whole workspace per clause (the full matrix is CI's job, §2). Every new test must be SEEN able to fail (invert the assertion or the guarded behaviour once) — a test that cannot fail proves nothing. Never mark a clause done from tests that were written but not run.
5. **Robot Framework TPs — check for existing ones FIRST, then dry-run what you write.** The file's `robot:` list + `grep -r "5_6_6" ngsi-ld-test-suite/TP/` (clause tag form). Only write a NEW TP for normative surface no existing TP covers — duplicating an ETSI TP is noise. New TPs go in the suite fork following its conventions: `[Documentation]` quotes the clause requirement briefly, `[Tags]` carries the clause number (`5_6_6` form) — that tag is what feeds `robot:` back into the ledger. **Edge-case TPs are mandatory**, not optional: error paths, boundary inputs, the cases the official 686 TPs skip. **Every new/changed TP is validated locally before commit** — this respects rule 8 (no broker) while catching broken TPs now instead of one CI round-trip later:
   first `--dryrun` while iterating (catches broken keywords/paths in seconds), then the REAL run per rule 8: the clause's TPs executed against one local broker. `python3 -m json.tool` over each new fixture. The full 4×8 matrix remains CI's job.
6. **Update the ledger file**: `status:` + `evidence:` (code/test anchors) + `notes:` (dates, named gaps, spec doubts); `python3 dev/spec.py robot` refreshes the TP list. Suspected suite/spec defect → prove it from the clause text first, log in `error.md` + `testsuite-doubts.md`, never hack the broker to a broken test.
7. **One clause = one commit — committed on a green TARGETED run**: code, unit tests, Robot TPs, ledger file, message citing the clause number (`5.6.6:` prefix). The gate is the clause's own unit tests green (`cargo test -p <touched-crate> <filter> -j 2`) **plus the clause's Robot TPs green against the local broker (rule 8)** plus `cargo fmt` on touched crates — NOT a full workspace run per clause; the whole-workspace suite, clippy wall and the 4×8 ETSI matrix run in CI on push (§2), which is where cross-clause regressions are caught. Run the workspace suite locally only when the change touches shared plumbing (store traits, expansion, negotiate/respond) whose blast radius is plainly wider than the clause.
8. **Every implementation is validated by Robot LOCALLY before commit** (user rule, 2026-08-10 — supersedes the earlier no-brokers rule). The sanctioned harness is ONE local memory-store broker + the clause's TP files, nothing more:
   ```
   cargo build -q -p antares-broker -j 2
   ANTARES_HTTP_PORT=9377 ./target/debug/antares &   # memory store, no docker
   cd ngsi-ld-test-suite && /workspace/.venv/bin/robot \
     --variable url:http://localhost:9377/ngsi-ld/v1 \
     --outputdir /tmp/…/robot-<clause> TP/path/to/<tp>.robot …
   # kill the broker afterwards — never leave it running
   ```
   Run the clause's NEW TPs plus the existing ones its `robot:` list names, wherever a single broker + the suite's own in-process mocks suffice (that covers provision/consumption/subscription/@context and single-broker DistributedOperations). MQTT TPs (docker mosquitto) and multi-broker IOP stay CI-only. Still forbidden: compose stacks, the 5-broker federation stack, anything on the host Docker. A red local Robot run blocks the commit exactly like a red unit test.
9. **Implement only what belongs in a Context Broker** (user rule, 2026-08-10). A clause earns broker code ONLY for observable API behaviour: validate input, store, serve, notify, federate, and return the mandated errors. Everything else is satisfied representationally or lives outside the core, and its ledger entry SAYS SO instead of growing code:
   - **Umbrella/conceptual clauses** (4.2.x information model, 4.3.x architectures, ontology figures, RDF/RDFS groundings) — no code of their own; the concepts exist as typed attributes validated at expansion, the SHALLs delegate to concrete clauses and are audited THERE. Precedents: 4.2.2 (meta-model = the required-member checks, nothing more), 4.2.3 (cross-domain ontology = delegation note, zero code).
   - **Semantics beyond the API**: no RDF store, no reasoning, no ontology/vocabulary management (SAREF, Smart Data Models — deployment content, not broker code), no domain-model validation.
   - **Out of core by standing decision** (deep-analysis §16): authn/authz, rate limiting, DID/VC/ODRL — the PEP's job; per-tenant quotas are a policy knob, not clause work.
   - The smell test: if the "implementation" would be a data model treatise, a figure, or a subsystem no HTTP request can observe — it is not broker surface. Write the delegation/posture note in the ledger and move on. When genuinely unsure whether a SHALL is broker-observable, ask the user before building it.

Ledger tooling: `python3 dev/spec.py split|robot|status|gaps` (see `docs/spec/README.md`). Status vocabulary: `not-implemented | partial | implemented | staged-v1x | informative`.

### The /goal prompt — copy-paste this to run the loop to completion

```
/goal Run the claude.md §0.3 audit loop until `python3 dev/spec.py
status` shows ZERO not-implemented. Repeat: take the first
not-implemented clause in order, do steps 1–9 exactly as written
(rules 8 and 9 are hard), commit as `<clause>:`, move on without
stopping. Ask only when rule 9 leaves you genuinely unsure. And always use ponytail. And Always check that in test response that can be for example something that should not be there. Write more tests, for the each implementaion to test different variations. TEST-FIRST: write the clause's tests BEFORE the implementation and run them — the red run on the missing behaviour IS the fallibility proof (and shows whether the implementation is even needed: already-green = already implemented, just annotate + ledger). Only tests that never saw red need one invert→FAILED→restore cycle, ONE per clause, not per test (each cycle costs ~3 full crate recompiles at -j 2). Use ponytail skill. Always write more tests in ngsi-ld-test-suite with different inputs so the ngsi-ld compliance ca be tested for 100 percent.
```

### The follow-up /goal prompt — the full-completion backlog (added 2026-08-12)

The §0.3 audit loop is COMPLETE (zero not-implemented since 2026-08-12).
This second prompt drives everything the ledger and §6 still leave open:

```
/goal Finish EVERYTHING the Antares ledger and §6 still leave open, sandbox-side.
First create "## Backlog 2026-08-12 (this goal)" in tasks.md with the checklist
below, then work it top-to-bottom — one item = one commit (clause-prefixed where
clause work), each with the full §0.3 discipline: MemPalace first, TEST-FIRST red
run as the fallibility proof, negative assertions (assert what must NOT be in the
response), extra ngsi-ld-test-suite TPs with varied inputs, rule-8 local Robot
validation, ledger + claude.md §6 updated. Use ponytail throughout. The goal is
DONE only when every checkbox is [x] with evidence (commit hash + green run)
recorded next to it in tasks.md:

[ ] 1. Close partial 4.10 — real metric geo distances in the in-memory store
      (haversine is enough; PostGIS stays the authority), ledger 4.10 → implemented.
[ ] 2. Close partial 5.8.6 — implement the splitEntities=true inbound-notification
      merge block, ledger 5.8.6 → implemented (keep the deployment default off).
[ ] 3. Snapshot ceilings: federated snapshot fills (5.16.1.4 via 5.7.2.4 dist path)
      + temporal fill paginates past max_limit ("all pages") + priority-ordered
      resource-pressure eviction (5.5.15). TPs for each.
[ ] 4. Durable state for HA (§1 contract): promote snapshots, entity_maps and
      dist_subs from per-process maps to the store trait (memory backend keeps
      today's behaviour; pg/timescale survive restart). ADR if the shape is
      irreversible. Restart-survival asserted in tests.
[ ] 5. Upstream raises drafted: write docs/upstream/etsi-raises.md with ready-to-file
      issue texts for D018_01, the 504 fixture fix, the 4.3.6.3 _exc TPs, and the
      5.3.4 naming/snapshotReady doubt (filing itself is user-side).
[ ] 6. MemPalace re-mine: add docs/spec/ to mempalace.yaml excludes, back up
      hand-filed drawers, run mempalace mine /workspace.
Rules 8 and 9 stay hard; Mac-side pushes stay out of scope (list them for the user
at the end instead). Ask only when rule 9 leaves you genuinely unsure.
```

Scope notes (decided with the user 2026-08-12): items 1-2 deliberately
OVERTURN the two standing "deliberate posture" partials — that is what
full completion means here; item 4 is the biggest diff (store-trait
surgery across three subsystems) and exists because per-process maps
contradict the §1 stateless-pods HA row. Dropped from the backlog by the
user 2026-08-12: the §2 capacity re-derivation and the dev/spec.py-check
CI enforcement (+ spec.py next/burndown).

## 1. Targets (the contract — raised 10× on 2026-08-10)

| Dimension | Target | Notes |
|---|---|---|
| Entities | 100,000,000 | current-state, one Postgres cluster |
| Tenants | 10,000 | **one shared schema**, `tenant_id` on every row (ADR-0001) |
| Subscriptions | 100,000 per context broker | HTTP callback + MQTT delivery |
| WebSocket connections | 100,000 per context broker | **DEFERRED — not in v1** (ADR-0003), design stays WS-ready |
| CSource registrations | 100,000+ per context broker | matching stays index-shaped, fan-out bounded |
| Broker memory | < 500 MB RSS | per broker process, at full load |
| Postgres memory | < 16 GB | PostGIS required, TimescaleDB optional (both store modes CI-tested) |
| Compliance | full NGSI-LD (ETSI CIM 009 V1.9.1) | the ETSI Robot suite + this repo's extension TPs |
| HA | yes | stateless broker pods, NATS JetStream, Postgres primary/replica |

> The capacity budgets behind 500 MB / 16 GB were derived for the ORIGINAL
> (10× smaller) targets — see `docs/deep-analysis.md` §2, flagged stale
> there. Re-deriving them against these numbers is an open, MEASURED task
> (e.g. 100k compiled subs ≈ 300 MB in the current mirror design).

## 2. Validation & run policy

- Locally run **one store mode — the one your change touches**:
  `STORE=<memory|file|postgres|timescale> dev/etsi-local.sh`, or
  `STORE=<mode> STOP_ON_ERROR=1 dev/etsi-pipeline.sh` for the tight debug
  loop. **CI is the authority**: `.github/workflows/ci.yml` fans out the
  4 × 8 store × suite matrix per push (`fail-fast: false`, one image build);
  a `v*` tag adds the serial all-suites job and publishes `:<version>` +
  `:latest`.
- **Never build (cargo or docker) while a measured ETSI run is in flight** —
  CPU contention manufactures phantom mock-502 and notification-timeout
  failures.
- `cargo test --workspace` on this box needs `-j 2` — default parallelism
  OOM-kills the linker (ld signal 9).
- State reset between suites = API-level delete PAIRED with DB truncate
  (`dev/reset-broker.sh` + suite `clean_db.sh`); never raw-SQL-truncate or
  container-restart alone. Federation/temporal state may only be truly
  cleared by a volume-wiping teardown.
- Suspected ETSI-tool bug: prove it from the clause text (MemPalace), log in
  `error.md` + `testsuite-doubts.md`, fix the fork if warranted — never hack
  the broker to pass a broken test. Prefer `https` @context URLs; `http`
  only for local mocks.

## 3. Conventions (essentials — full rationale in deep-analysis §9)

- Names come from the spec: Rust types verbatim from CIM 009 §5.2
  (`Entity`, `CSourceRegistration`, `NgsiError` variants = Table 6.3.2-1);
  one public fn per spec operation (`create_entity` = 5.6.1); DB tables =
  spec resources snake_cased; banned suffixes: `Manager`, `Service`,
  `Util`, `Helper`.
- Doc comments on normative code cite the clause (rule §0.3.2). Commits for
  clause work carry the clause-number prefix.
- Workspace hygiene: versions only in `[workspace.dependencies]`;
  `unsafe_code = "forbid"`; `unwrap_used`/`expect_used` denied outside
  tests; `cargo deny` gates licenses/advisories.
- Every optional capability is one crate/feature + a registration in
  `antares-broker`; removing it must not touch a core crate.

## 4. Environment gotchas (this dev box)

- `pgrep`/`pkill`/`free` are absent — scan `/proc/[0-9]*/cmdline`.
- A backgrounded command that PREFIXES a kill-loop may never run its
  payload — kill in a separate foreground call first.
- Background long-running commands need an explicit `cd /workspace &&`.
- The sandbox `grep` is ugrep and silently skips some files — on an empty
  result retry with `/usr/bin/grep -a`.
- No `ssh` in the sandbox: pushes to `git@github.com:` remotes must happen
  Mac-side.
- The Mac-side auto-pusher also COMMITS uncommitted working-tree changes
  with its own generic message (seen 2026-08-13: c7cc448). Commit promptly
  after editing, or your change ships without its rationale.

## 5. Where things live

| What | Where |
|---|---|
| Architecture & design analysis (the former content of this file) | `docs/deep-analysis.md` |
| Conformance ledger — CIM 009 full text, one file per clause | `docs/spec/` + `dev/spec.py` |
| Irreversible decisions | `docs/adr/ADR-*.md` |
| Suite/spec defect log | `error.md`, `ngsi-ld-test-suite/testsuite-doubts.md` |
| ETSI suite fork (submodule) | `ngsi-ld-test-suite/` |
| Task list & progress | `tasks.md` |
| Spec PDFs (authoritative) | `etsi-cim-specs/`, via `mempalace_get_pdf_pages` |

## 6. State handoff — 2026-08-10 (prune as items resolve)

**Loop position (2026-08-13, AntaresBroker): TEMPORAL q= FOLLOW-UP CHECKLIST COMPLETE — all sandbox-side items [x] in tasks.md ("Temporal q= follow-up 2026-08-13") with commit + green-run evidence; `dev/spec.py check` green.** The one conformance bug is FIXED: 5.7.4.4 S4 scopeQ now applied validity-aware per 4.18/C.5.16 (commit 1ce6de9 — scope_match_intervals validity intervals, instance bounding, pre-window carry-in; S7 free post-merge; EntityMap inherits S4; 5.6.11 accepts instance-shaped scope) + 9dc7024 (scopeQ gates the lastN/paging pushdown — proven red on live PostGIS via in-sandbox docker pgdev, since removed). Perf: 72a8c9b geo prefilter (migration 0009 try_geomfromgeojson, geo_value filled per instance, compile_geo_instance windowed EXISTS, any geoproperty), 98818fa exact-q entity paging (text window predicate in qprefilter leaves + prefilter_exact gate; datasetId/pick added to the paging gate), 87a3e06 extension leaves (!= as NOT-of-Eq, [lang]/[*] languageMap wildcard, COLLATE "C" string ordering, deleted_at fill + NULL-tolerant bound). Raises: 5d1c25a lastN-vs-q doubt = etsi-raises #6; 8175693 + fork 952e61e bracket-less-LP doubt #19. Suite fork TPs 5744_03..06 (da7d728/826424c/dab0838): temporal tree 149/149 local. TRAPS: value_or_filter numbers binds itself (advanced offset = bind-count mismatch); a Ne+List mixing string and number knocks out fewer entities than it looks (p.92 mismatch MATCHES !=); heredoc python after `cd ngsi-ld-test-suite &&` runs in the fork dir. Mac-side: push master + suite fork (now 10+ TP commits incl. 5744_03-06), delete fed-alias-tenant, watch the 4×8 matrix (pg/timescale cells exercise migration 0009 + the new prefilter/paging paths).

**Loop position (2026-08-12d, ngsi-ld agent): BACKLOG 2026-08-12 COMPLETE — all six checkboxes [x] in tasks.md with evidence; 947 sections = 479 implemented / 468 informative / 0 partial / 0 staged-v1x; `dev/spec.py check` green.** Final item (4, durable state for HA, commit 602b236): Kind::Snapshot/EntityMap/DistSub added to the store trait — memory maps, redb tables (a pre-0008 file loads: missing table is skipped, format version unchanged), pg/timescale via the pg_doc doc-kind path (migration 0008, ADR-0001-shaped RLS). distsub.rs rewrote its three per-process maps into store docs: one doc per (tenant, own sub id) = {csr_sub, remotes}, plus an inbound index (remote subscriptionId → {tenant, own}) under the reserved `distsub-index` tenant — reserved-tenant encoding + collision posture recorded in **ADR-0012**. AppState lost entity_maps/dist_subs/snapshots fields. Restart survival asserted in tests/durable_state.rs (2/2; red seen by inverting the survival assertion; the test simulates process exit by dropping the first tokio runtime + breaking the notify-hook Arc cycle before reopening the redb dir). Targeted regressions 36/36, EntityMap+5161_01+5814_01 Robot TPs 22/22 local. TRAPS this session: the NGSILD-EntityMap response header carries the map's LOCATION path (6.4.3.2-2) — strip to the last segment before GET /entityMaps/{id} (a nested path 405s); a kill-loop that misses its target leaves the OLD broker holding the port — the new one dies AddrInUse and robot runs hit stale state (verify the pid actually died, then check broker.log). Remaining work is ONLY Mac-side pushes (repo master + suite fork with 6 unpushed TP commits + stale fed-alias-tenant branch delete) and watching CI (4x8 matrix must exercise the new doc-kind tables on pg/timescale).**

**Loop position (2026-08-12c, superseded): SNAPSHOT API BUILT (user request "implement snapshot api") — 947 sections = 477 implemented / 468 informative / 2 partial / 0 staged-v1x; `dev/spec.py check` green.** The whole 5.16 group + 5.2.41/5.2.42/5.3.4/5.5.15/6.3.22/6.36-6.38 flipped from staged-v1x in one commit. Implementation shape (crates/antares-api/src/snapshots.rs): each snapshot owns a synthetic internal tenant ("snap-<uuid>"); the 6.3.22 NGSILD-Snapshot header is resolved by a middleware (snapshot_layer, ahead of tenant_exists_layer) that rewrites the request tenant, so every Core/Temporal handler serves the frozen copy unchanged and all ops are implicitly local (5.5.15 — no CSRs under the synthetic tenant). Registry is per-process memory keyed (owner tenant → id → 5.2.41 doc with __tenant), lazy expiry + background data purge; fill executes snapshotQueries via filter_entities (unpaged = "all pages") and snapshotTemporalQueries via query_temporal_inner (capped at max_limit — named ponytail ceiling); status success/partial/empty/failure per 5.16.1.4 with per-query ExecutionResultDetails (5.2.42). Red→green this session: lastUsedAt (init at create + snap_touch on scoped use), purge-q restricted to Snapshot members (QNode::attribute_paths gate), notifications from snapshot-scoped subscriptions carry NGSILD-Snapshot + owner tenant (notify.rs deliver via snapshot_of_synth — synthetic tenant never leaks). SPEC DOUBT logged in error.md: 5.3.4 names the temporal details list temporalSnapshotQueriesDetails (5.2.41 says snapshotTemporalQueriesDetails; each table governs its own payload — asserted both ways) and references a snapshotReady member its table never defines. Evidence: tests/snapshots_5_16.rs 13/13, suite-fork TP 5161_01 (4 cases) 4/4 local. 5.5.15 MAY-eviction not implemented (per-process registry may drop on restart — the clause allows it; promote to store + sweep orphaned snap- tenants for durable snapshots). NEXT unchanged: capacity re-derivation (#1), Mac-side pushes (suite fork now has 6 unpushed TP commits), watch CI (4x8 matrix + the new snapshots_5_16 integration test).**

**Loop position (2026-08-12b, ngsi-ld agent): PARTIALS BURNDOWN COMPLETE — 947 sections = 444 implemented / 440 informative / 2 partial / 61 staged-v1x; `dev/spec.py check` green.** The 2 remaining partials are deliberate postures, NOT missing work-in-order: 4.10 (in-memory geo metric ceiling — PostGIS is the metric authority) and 5.8.6 (the splitEntities=true inbound-notification merge block; deployment posture disallows split entities). Burndown commits this stretch, each red->green tested + TP'd + tree-regressed locally: e61f6b9 EntityMap usage wired into retrieve + temporal (retrieve_entity_outer/retrieve_temporal_outer/query_temporal_outer + federation map_gate; TPs 5714_01/5734_01; RetrieveEntity+TemporalEntity trees 244/244) · 360731f csf applied to CSR matching (CsrSpec.csf gate in matching_regs via query_spec + purge; csf_matches no longer double-wraps attribute-form Context Source Properties; TP 5724_01) · 720fbbb 4.9 linked-entity q in notifications (store lookup in conditions_match) + expandValues on temporal (TPs 49_01/5743_01) · 4171ff4 ICU collation (icu_collator dep; entities.rs build_collator, -u-ks strength mapping; 5.2.43 collation member now honoured — stale refusal test updated; TP 4233_01) · 13f2183 distributed subscriptions CONSUMER half (new distsub.rs: internal CSR-sub with urn:antares:distsub in-process endpoint, csource_initial gives the initial newlyMatching for free, reduced-copy create/PATCH/DELETE per triggerReason gated on op support, AppState.dist_subs mappings, POST /ngsi-ld/ex/remote-notify remap + origin-csf gate, ANTARES_PUBLIC_URL; TP 5814_01; Subscription+RegistrationSubscription trees green). Also fixed: f78fe10 rust-1.97 clippy wall (CI toolchain bump) and 6788162 **P0: tenant_exists never bound $1 on the Pg store** — every non-default-tenant request 500'd on postgres; found via CI #66 nats_e2e timeout, reproduced with local docker pg+nats (recipe in the palace postmortem drawer). ponytail ceilings documented in code/ledger: dist_subs + entity_maps are per-process maps (HA restart loses remote-subscription bookkeeping — promote to store if needed); temporal map-in-use pays a second recheck query. NEXT: the §6 open engineering tasks (capacity re-derivation #1 is now the top item), Mac-side pushes (suite fork has 5 new TP commits), and watch the next CI run — the 4x8 matrix must confirm the internal CSR-sub visibility does not disturb exact-count fixtures.**

**Loop position (2026-08-12, ngsi-ld agent): AUDIT LOOP COMPLETE (2026-08-12): `dev/spec.py status` shows ZERO not-implemented — 947 sections = 421 implemented / 440 informative / 25 partial / 61 staged-v1x.** The 25 partials are NAMED gaps documented in their ledger notes (the standing set: 4.3.5, 4.3.6.7, 4.9 notify-linked-q, 4.10 in-memory geo ceiling, 4.23.x ICU collation, csf-not-applied-to-CSR-matching, plus per-clause optional members); the 61 staged-v1x files are the optional Snapshot API group + related types (intake contracts recorded). Chapter 6 finished this stretch: 6.3.17 (fed-404 keeps NGSILD-Warning — new TP 6317_01), 6.3.16 (s-maxage), 6.3.21 MAY posture, 6.3.22/6.36-6.38 snapshot-staged, and the 6.4-6.35 resource-binding sweep validated by re-running the full official TP trees locally (entities CRUD, attributes, batch, subscriptions, csource registrations+discovery, csourceSubscriptions, temporal provision, discovery, POST-query, jsonldContexts, entityMaps, tenancy, common-responses — all green). Chapter 7 MQTT audited (CI-only TPs per rule 8). Annexes A/B audited. Next work is the PARTIALS burndown + the §6 open engineering tasks (capacity re-derivation, spec.py check CI gate). 6.3.5 (@context resolution, mixing-matrix unit test), 6.3.6 (Prefer: body=json omits geo @context — respond_prefer at 3 geo sites, 5.2.29/5.2.30 partials CLOSED, TP 636_01 incl. 204-no-Content-Length), 6.3.7 (format/options audit), 6.3.8 (notification behaviour audit), 6.3.9 (csource notifications — same deliver path, 047_09/10), 6.3.10 (pagination links+limit rules), 6.3.11 (sysAttrs), 6.3.12 (temporalValues/aggregatedValues), 6.3.13 (count), 6.3.14 (tenant header; TRAP: 414_01_03 dist-ops needs context_source_host:127.0.0.1 + egress-allow-private or Wait For Request hangs) all committed. 6.3.15 (geo representation audit, 019_27) and 6.3.16 (s-maxage now parsed with shared-cache precedence — was max-age only) also committed. Ledger ~309 implemented / 340 informative / 227 not-implemented / 28 partial / 42 staged. This session: 5.7-5.11 headings informative; 5.13 @contexts chapter (valid_context_shape 400 on bad @context VALUE; loader refetch keeps the old copy on failed reload + maps invalid content to LdContextNotAvailable; new TPs 5132_01/5135_01); **6.3.2 fix: LdContextNotAvailable restored to 504** per Table 6.3.2-1 — it had been flipped to 503 to please V1.8-era fixtures; fork fixtures 043_01/028_07/051_05/053_05 now assert 504/Gateway Timeout (doubts #18 RESOLVED, error.md entry, raise upstream); **5.14 EntityMaps BUILT** (new crates/antares-api/src/entity_maps.rs + entities.rs query_entities_outer; bounded per-tenant in-memory store per 5.14.1.1 "or memory"; expiresAt not expiredBy — 5.2.39 wins the spec conflict; PATCH applies only expiresAt; NGSILD-EntityMap request header fixes the id set + prunes stale "@none" entries, expired/unknown map → recreate 201; fed arm fed_entity_maps for createEntityMapQueryEntity/…QueryTemporal; temporal candidates via internal query_temporal_inner call, Box::pin, capped max_limit; suite dir TP/NGSI-LD/ContextInformation/EntityMap/ 5141-5145; 4.5.25/5.2.39/5.5.9.3/5.5.14 partials CLOSED); 5.15.1 sourceIdentity audited; 5.16 Snapshots staged-v1x (38 files, optional API group not offered); 6-6.3.4 audited (6.3.4: 406 body now lists availableRepresentations — ApiError::NotAcceptable). Traps this session: [Teardown] override REPLACES the default teardown (leaked fixtures → 409 on reruns); test AppState needs antares_api::notify::wire(&mut st) for temporal auto-recording, and auto-recorded creates carry createdAt (query with timeproperty=createdAt); common-responses TP runs need --variable temporal_api_url:… too; kill-loop needs `2>/dev/null; true` tail or the Bash call "fails". Robot recipes and standing decisions are in the palace (audit-5.13, audit-5.14 drawers).

**Loop position (2026-08-11d, superseded):** 5.5.4-5.6.10.4 audited this session (ledger 173 implemented / 15 partial / 565 not-implemented). Next clause: 5.6.11 (Create or Update Temporal Evolution — temporal.rs; decide the allow_null-on-temporal-input question deferred at 5.5.4, see that ledger's notes). Batch chapter 5.6.7-5.6.10 DONE: batch_write support ladder covers Create/Upsert/Update arms, batch_delete has its own; null-item→400 in both parse paths; all asserted on the wire in federation_loop.rs clause_5_6_7/8/9/10 tests. 5.6.2-5.6.6 fixes: fed_attr_parts three-way op-support gate with status-0 sentinel parts (zip alignment) + combine_attr_parts 409 errors/Conflict on complete unsupported failure; ?type selector (4.17) wired into update/append/partial/delete-attr/delete-entity via attrs.rs matches_type_param (was accepted-but-ignored everywhere); 5.6.4 scope-target 400; delete-entity local delete gated so a wrong-type delete can't remove the entity. Session-d key fixes: 5.5.4 null placement (reject_first_level_nulls everywhere incl. subscription/CSR creates; nested-value nulls merge-only via ExpandOpts.merge; merge_value_object RFC7396 into compound values — sentinel never stored); 5.5.6 invalid-vs-unavailable remote @context split (invalid→BadRequestData); 5.5.7 scoped contexts rejected; 5.5.8 datasetId-null 400 everywhere; 5.5.10 NonexistentTenant via tenant_exists_layer middleware (implicit-create exemptions + /info/*+/jsonldContexts* tenant-free; 404 echoes tenant header); 5.6.1.4 op-support gate (redirect-unsupported→Conflict, inclusive-unsupported→skip; DEFAULT ops=federationOps has NO createEntity — topology test fixed). EntityMap named-gap now spans 4.5.25/5.2.39/5.5.9.3/5.5.14 → all build at 5.14.x. Traps: transient git loose-object corruption on this volume — retry commit after fsck (it healed itself); robot local runs need broker on 9090 for 050_04/051_03 (hardcoded URL). New TPs: 554_01 555_01 556_01 557_01 558_01 5510_01 5614_01 + tags on 019_31/466_01.

**Loop position (2026-08-11c, superseded):** 5.2.21–5.2.38 audited this session (18 clauses; ledger now 120 implemented / 11 partial / 661 not-implemented). Next clause: 5.2.39 (then 5.2.40-5.2.44 close out 5.2; 5.2.43 OrderingParams completes the 5.2.23 ordering-mapping named gap, 5.2.44 AggregationParams is already mapped by query_doc_params). Notable session fixes beyond the earlier list: 5.2.33 array-form selector id matched EVERY entity in the notify matcher + id-over-idPattern precedence in all three filter loops; 5.2.34 management member was swallowed unvalidated and management.localOnly never reached the forward flag (cooldown/timeout deliberately overridden by egress breaker + 8s deadline per the clause's own MAY); 5.2.32 LP valueType must be literal "langString". 5.2.35-5.2.38 were table-confirmations (green-at-audit, invert-proven). Session-c commits: 5.2.21 TemporalQuery JSON form (temporal_q_params, TP 021_27), 5.2.22 KeyValuePair value-must-be-String (TPs 028_10+033_12), 5.2.23 Query one-flatten query_doc_params for 6.23+6.24 (partial: ordering only orderBy → 5.2.43; TP 019_32), 5.2.24–5.2.28 discovery types (annotation + 5.2.26 attributeDetails restriction asserted, Discovery TPs 14/14), 5.2.29/5.2.30 Feature/FeatureCollection (partial: Prefer body=json @context omission → 6.3.6), 5.2.31 FeatureProperties, 5.2.32 LanguageProperty valueType=langString (TP 5232_01). New traps in palace: stale broker binary after crate edits (cargo test rebuilds lib NOT the broker bin — rebuild before rule-8 runs); backgrounded &&-chains with & apply to the whole chain.

**Audited this session (each one commit, code+tests+TPs+ledger):**
4.5.4 (simplified: wrapped languageMap/json/vocab + vocab term compaction,
TP 454_01) · 4.5.5.1–.3 (datasetId @none normalization, TP 455_01;
push_down_expires + entity-expiresAt intersection/max in merge_docs) ·
4.5.6 (Core-API scope changes recorded as temporal Property instance,
observedAt=modifiedAt, TP 456_01) · 4.5.7/4.5.8 (deletion-null instances,
tests) · 4.5.9 (temporalValues renderer first coverage, TP 459_01) ·
4.5.10–4.5.15 (discovery representations, tests/discovery.rs) · 4.5.16
(GeoJSON: default-instance geometry + null-for-invalid, TP 4516_01) ·
4.5.17 (simplified GeoJSON dataset-map @none geometry, TP 4517_01) ·
4.5.18 (LanguageProperty: empty tags + unitCode rejected) · 4.5.19
(aggregation: Relationship label fix, eligibility 400 test) · 4.5.20
(VocabProperty: unitCode rejected) · 4.5.21/4.5.22 (Lists: objectList URI
validation + both input forms + normalized {"object":URI} output wrap in
compact.rs + is_ngsi_null_list deletion form, TP 4522_01) · 4.5.23 (linked
retrieval: ListRelationship joins under entityList inline + flat) ·
4.5.24 (JsonProperty: json shape + unitCode/value prohibited, TP 4524_01) ·
4.5.25 (EntityMap → partial, deferred to 5.14.x) · 4.6.1 (UTF-8, TP 461_01)
· 4.6.2 (name grammar via valid_name, TP 462_01) · 4.6.3 (GeometryCollection
excluded, TP 463_01) · 4.6.4 (content verbatim, TP 464_01) · 4.6.5 (langmap
null gated to allow_null — was creatable, TP 465_01) · 4.6.6 (batch dup
order, TP 466_01) · 4.7.1–4.7.3 (string-encoded geometries normalized,
TP 471_01).

**Local Robot recipes (also drawers in the palace):** temporal TPs need
`--variable temporal_api_url:http://localhost:9377/ngsi-ld/v1`
(variables.py hardcodes scorpio1); notification TPs (046_x) need
`--variable notification_server_host:127.0.0.1` + broker env
`ANTARES_EGRESS_ALLOW_PRIVATE=true` (else they hang to timeout). Dist-ops:
`context_source_host:127.0.0.1`. Test-inversion (fallibility) via targeted
python string replace, never sed. Broker kill-loops exit 144 — harmless;
verify with `curl :9377/q/health` → 000.

**Session test protocol (user directive, updated 2026-08-10):** TEST-FIRST —
write the clause's tests before the implementation; the red run on missing
behaviour IS the fallibility proof. Tests that never saw red get ONE
invert→FAILED→restore cycle per clause (not per test — each cycle is ~3 full
crate recompiles at -j 2, the dominant time cost). Every test carries at
least one negative assertion (what must NOT be in the response).

**Pending Mac-side (no ssh in the sandbox):**
- `git -C /workspace/ngsi-ld-test-suite push origin main` — origin/master
  references suite commits that are NOT on the suite remote; every fresh
  clone has a broken submodule until this push happens.
- `git -C /workspace push origin --delete fed-alias-tenant` — stale branch
  with a pre-amend submodule pointer.
- Local `master` may be ahead of origin — an external auto-pusher exists
  but is not this sandbox; verify.

**Open engineering tasks:** SUPERSEDED 2026-08-12 by the full-completion
backlog in §0.3 ("The follow-up /goal prompt") — the upstream ETSI raises
(D018_01 + 504 fixtures + `_exc` TPs + 5.3.4 doubt, all argued in
error.md) and the MemPalace re-mine live there as items 5-6. Dropped by
the user 2026-08-12: the §2 capacity re-derivation and the CI enforcement
of `dev/spec.py check` (+ `spec.py next`/burndown). 4.3.6.3 validation
itself was RESOLVED 2026-08-10 (commit 0990944); only its upstream raise
remains (backlog item 5).

**Loose ends:** `ETSI-matrix-results (5).zip` untracked in `/workspace`
(analysis input, delete freely); `results/`, `results-proc/`, `www/`
untracked as before. The federation Via/502/207 fixes, tenant-alias
work (ADR-0011) and the ledger reset are all committed and documented.
