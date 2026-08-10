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

1. **Read the clause text in the file** — the body IS the spec text. For comma-level wording read the PDF pages named in the frontmatter (`mempalace_get_pdf_pages`).
2. **Find the implementation and annotate it.** Every function implementing normative behaviour carries a doc comment citing **the CIM 009 clause number + a one-line summary of the rule**, e.g. `/// 5.6.6 Delete Entity: 204 on success, ResourceNotFound 404 if absent.` **FORBIDDEN as normative citation: internal documents** — never `(see docs/deep-analysis.md §9.3)`, never `claude.md §…`, never tasks.md. Internal docs may be cited for *architecture* decisions only; the requirement's source is always the spec clause. Existing comments citing internal docs as the requirement are fixed on touch.
3. **Verify the FULL normative behaviour** against the clause text: every SHALL, every error case (type + status per Table 6.3.2-1), every output member. A gap → implement it now, or set `status: partial` with the gap NAMED in `notes:`. Never mark `implemented` with a known gap.
4. **Unit tests for the clause's own rules and edge cases** — boundary values, invalid input → the exact spec error type, empty/absent optional members, multi-instance `datasetId` where applicable, tenant isolation. Test names/doc comments cite the clause number.
5. **Robot Framework TPs — check for existing ones FIRST.** The file's `robot:` list + `grep -r "5_6_6" ngsi-ld-test-suite/TP/` (clause tag form). Only write a NEW TP for normative surface no existing TP covers — duplicating an ETSI TP is noise. New TPs go in the suite fork following its conventions: `[Documentation]` quotes the clause requirement briefly, `[Tags]` carries the clause number (`5_6_6` form) — that tag is what feeds `robot:` back into the ledger. **Edge-case TPs are mandatory**, not optional: error paths, boundary inputs, the cases the official 686 TPs skip.
6. **Update the ledger file**: `status:` + `evidence:` (code/test anchors) + `notes:` (dates, named gaps, spec doubts); `python3 dev/spec.py robot` refreshes the TP list. Suspected suite/spec defect → prove it from the clause text first, log in `error.md` + `testsuite-doubts.md`, never hack the broker to a broken test.
7. **One clause = one commit**: code, unit tests, Robot TPs, ledger file — message cites the clause number (`5.6.6:` prefix).
8. **The audit loop starts no brokers and no stacks** (user rule, 2026-08-10): the clause's evidence is its unit tests; Robot TPs are validated by the standard pipeline (CI matrix / an explicitly requested `dev/etsi-pipeline.sh` run), never by ad-hoc local broker instances.

Ledger tooling: `python3 dev/spec.py split|robot|status|gaps` (see `docs/spec/README.md`). Status vocabulary: `not-implemented | partial | implemented | staged-v1x | informative`.

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
