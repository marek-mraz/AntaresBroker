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
stopping. Ask only when rule 9 leaves you genuinely unsure.
```

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

**Loop position:** `python3 dev/spec.py status` → 6 implemented (4.2.2,
4.2.3, 4.8, 5.2.40, 5.15.1, 6.3.18) · 1 partial (6.3.17 — gaps:
cacheDuration caching/warn 110, cooldown) · 121 informative · 819
not-implemented. **Next clause: 4.2.4**, then 4.2.5, 4.3.x (architecture
umbrellas — expect rule-9 delegation verdicts, no code).

**Pending Mac-side (no ssh in the sandbox):**
- `git -C /workspace/ngsi-ld-test-suite push origin main` — origin/master
  references suite commits that are NOT on the suite remote; every fresh
  clone has a broken submodule until this push happens.
- `git -C /workspace push origin --delete fed-alias-tenant` — stale branch
  with a pre-amend submodule pointer.
- Local `master` may be ahead of origin — an external auto-pusher exists
  but is not this sandbox; verify.

**Open engineering tasks (in rough priority):**
1. §2 capacity budgets re-derivation for the 10× targets — MEASURED
   (100k compiled subs ≈ 300 MB in the current mirror; likely per-tenant
   lazy load + LRU). See the §1 warning.
2. 4.3.6.3 validation gap: exclusive CSR shall name BOTH entity id and
   attributes; overlapping exclusive/redirect for the same combination
   shall be rejected — `csource.rs` checks neither. Known conformance
   hole on a WRITE path; the loop reaches it late, consider pulling it
   forward.
3. CI enforcement of §0.3 (`dev/spec.py check`): clause-prefix commit
   must touch `docs/spec/`; robot-list drift; forbidden internal-doc
   citations in `crates/`; frontmatter enum validation; split idempotence.
4. `dev/spec.py next` + per-chapter burndown in the CI summary.
5. ETSI D018_01 (`mode=inclusive` asserting 508) — raise upstream; fork
   already fixed, error.md 2026-08-10 has the full argument.
6. MemPalace re-mine after the doc split — FIRST add `docs/spec/` to
   `mempalace.yaml` excludes (it duplicates the already-indexed PDF).

**Loose ends:** `ETSI-matrix-results (5).zip` untracked in `/workspace`
(analysis input, delete freely); `results/`, `results-proc/`, `www/`
untracked as before. The federation Via/502/207 fixes, tenant-alias
work (ADR-0011) and the ledger reset are all committed and documented.
