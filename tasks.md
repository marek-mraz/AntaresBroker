# Antares — complete implementation task list

## Goal

Tick every untagged box in tasks.md, spec-first, with the ETSI pipeline green in all four store modes as the proof.

### The loop for one task

1. **Read the clause first** (§0.2). The spec is the requirement; the Robot
   suite is the oracle that confirms it afterwards. Never the reverse.
2. Implement the full normative behaviour with the smallest diff that holds
   it — reuse before writing, stdlib before dependencies.
3. Unit-test that clause's own rules and edge cases.
4. Run the local gate for the ONE store mode your change touches:
   `STORE=<mode> dev/etsi-local.sh` (workspace tests + that mode's 8 suite
   cells; default `memory`). Tighter loop while debugging:
   `STORE=<mode> STOP_ON_ERROR=1 dev/etsi-pipeline.sh`. Do NOT run all four
   locally — cells run serially here, so it is 4× the wall-clock for a
   signal CI already gives: **CI fans all four modes out in parallel on
   every push and is the authority** (32 cells, `fail-fast: false`).
5. In ONE commit: the code, its tests, the `docs/spec/<clause>.md` status
   (the per-clause full-text ledger that replaced `docs/ics.yaml`, 2026-08-10), the ticked
   box, and a message citing the clause number.
6. File the decision or the gotcha in MemPalace (other agents read it).

### Never

- Touch the host Mac: no host docker, no host paths, no Mac-side installs
  (§0.1). The sandbox and CI are sufficient for everything untagged.
- Start a ⛔ / 🖥 / ⏳ task. Prepare the artifact, stop, and say what you need.
- Hack the broker to satisfy a broken test purpose. Prove it is a tool bug,
  log it in `error.md`, leave the broker correct.
- Tick a box whose test does not exist or does not pass.
- Push, publish, or release unless asked in that message.

### Progress

`grep -c '^- \[x\]' tasks.md` over `grep -c '^- \[' tasks.md`, plus per-suite
pass counts from the last pipeline run. Those two numbers are the only
progress report anyone should need.

---

Everything from v0 skeleton to the v1 contract, derived from `claude.md` /
`docs/deep-analysis.md` (§ references) and `README.md`. Working order per
feature is spec-first (§0.2): read the CIM 009 V1.9.1 clause → implement the
full normative behaviour → unit-test the clause's rules → only then run the
ETSI Robot suite as confirmation → update `docs/spec/<clause>.md` (§14.6;
replaced `docs/ics.yaml` 2026-08-10 — see docs/spec/README.md).

**Where we are (v0):** one binary, in-memory store, `LocalBus`, HTTP binding —
and the full ETSI suite green (1025/1025: CommonBehaviours 33, Consumption
328, Provision 253, Subscription 110, ContextSource 114, jsonldContext 61,
DistributedOperations 102, IOP 24) with MQTT TPs excluded. The ONE pipeline
(`dev/etsi-pipeline.sh`, 5 brokers + 5 DBs, store-mode matrix) runs locally
and in CI identically.

**What v0 is not yet:** durable (memory only), scalable-out (no NATS, no
roles), MQTT-capable, hardened (§16), HA (§10), or measured at target scale
(§1). That is this list.

Store ladder: `memory` → `file` → `postgres` → `timescale` — same binary,
same compose, same pipeline, one config value each.

**Why `memory` stays once `file` exists:** `file` IS `memory` plus a redb
write-through shadow (§B), so keeping `memory` costs no second implementation
— it is the same code with durability disabled. It earns its keep as: the
control mode when a `file`-only bug appears; the unit/integration-test default
(no temp dirs, no fsync, parallel `cargo test` never fights over a file); the
only mode that runs with a read-only rootfs and no volume; and the browser
fallback when OPFS is unavailable (§N — redb itself compiles to `wasm32`
fine, but that target has no filesystem, so persistence there needs the OPFS
backend and `memory` is what runs without it).

**Tasks an agent must NOT start on its own.** Everything else in this list is
implementable inside the dev sandbox (private docker daemon) or on CI runners.
These are not:

| Tag | Meaning | Agent rule |
|---|---|---|
| ⛔ NEEDS-YOU | outward-facing submit or a credential only you hold (crates.io token, package publish, anything irreversible in public) | never run it; prepare the artifact, stop, ask |
| 🖥 NEEDS-HARDWARE | a machine bigger than the sandbox or a free CI runner (16 GB Postgres, 24 h soak, a real k8s cluster) | write the rig, run it only where you point it |
| ⏳ BLOCKED-EXTERNAL | waiting on a third party (an ETSI work item, a spec that does not exist yet) | do not schedule; recheck when the dependency moves |

Nothing in this list requires the host Mac — ground rule §0.1 stands: no host
docker, no host paths, no Mac-side installs, ever.

**Checking off a task:** flip `- [ ]` to `- [x]` in the same PR/commit that
lands it — GitHub renders these as checkboxes and shows section progress.
A task may only be checked when its OWN done-criterion holds: code + unit
tests merged, the relevant suite green via `dev/etsi-pipeline.sh`, and (for
spec-clause tasks) the `docs/spec/<clause>.md` status updated. Checking a box without
its tests is the one way to make this file lie.

---

## A. The store seam (prerequisite for every mode)

- [x] A1. Extract the store trait from the v0 in-memory store: an
      `EntityStore`-shaped seam in `antares-sql` (`store/` per §9.3);
      `&TenantId` is the FIRST parameter of every public store method
      (§9.3, §16.1.2).
- [x] A2. `ANTARES_STORE` → enum `StoreMode { Memory, File, Postgres,
      Timescale }`; unknown value fatal at startup (§14.3);
      `ANTARES_DATABASE_URL` required for postgres/timescale.
- [x] A3. Wiring in `antares-broker::wiring`: mode → store construction; api,
      matcher, notifier, temporal, registry see only the trait; no core crate
      names a backend (§9.2).
- [x] A4. Startup log + `/q/health` report the active store mode (§15.1
      feature registry posture). NOT in `/info/sourceIdentity`: that is a
      spec resource (5.15) and inventing members in a normative payload is
      exactly the drift §14.6 exists to prevent.

## B. `file` mode — redb write-through (ships first, v0-compatible)

redb is durability only — queries/matcher keep running on the in-memory maps.
One file, one volume, one ~42 MB container = complete durable broker.

Measured cost of the shadow (redb 4.1.0, 1.5 KB entities, one commit per
write with `Durability::Immediate`, container overlay fs on this dev box,
2026-08-04): **3,127 writes/s, commit p50 0.21 ms / p99 0.85 ms**, versus
407k/s for the memory store with no durability. Raw 4 KB write+fsync on the
same fs is p50 0.35 ms, i.e. the cost IS the fsync barrier, not redb. Batch
ops commit ONCE per batch, so a 100-entity batch pays one commit, not 100.
Re-measure per target disk; network-attached cloud SSDs fsync ~3–5× slower.

- [x] B1. `redb` **4.x** (4.1.0 current; the API moved since 2.x —
      `set_durability` returns `Result`, `begin_read` needs
      `ReadableDatabase` in scope) in `[workspace.dependencies]`, pinned;
      `cargo deny` license pass.
- [x] B1b. Commits run on a blocking task (`spawn_blocking`) — redb's API is
      synchronous and an fsync must never stall a tokio worker thread.
      *(Done via `block_in_place` — same guarantee, zero call-site churn: the
      commit must happen inside the store's write-critical section so redb
      order equals memory order, which `spawn_blocking` can't do from sync
      code.)*
- [x] B2. One redb table per store kind (`entities`, `subscriptions`,
      `csource_registrations`, `csource_subscriptions`, `jsonld_contexts`,
      `temporal_entities`, `attr_instances`, `entity_maps`), key
      `tenant \0 id`, value = expanded JSON bytes; names per §9.1.
      *(v0 note: `attr_instances` has no separate table — the memory store
      keeps one temporal doc per entity, persisted whole under
      `temporal_entities`; entityMaps are TTL-ephemeral, not durable state.
      Both get real tables with the §C schema.)*
- [x] B3. Write-through with **commit-before-ack**: redb txn commits BEFORE
      the HTTP response; `Durability::Immediate` (fsync per commit). An
      acknowledged write is a durable write.
- [x] B4. Boot rebuild: scan tables → rebuild in-memory maps; refuse to start
      on checksum/corruption (never silently serve partial data).
- [x] B5. `ANTARES_DATA_DIR` (KNOWN_KEYS); default never inside the image;
      startup warning when the path is not a mount point.
- [x] B6. Compose: `STORE=file` → no DB containers, one named volume per
      broker.
- [x] B7. Pipeline + CI matrix accept `file`.
- [x] B8. Tests: table round-trip + key-encoding units; e2e SIGKILL-restart
      (full state survives); e2e kill -9 right after 201 (commit-before-ack
      proven); 350 MiB gate unchanged.
- [x] B9. Docs: README mode table; ADR: mode ladder, redb-as-durability,
      SQLite rejected.
- [x] B10. **Reset path** (the Scorpio phantom-state trap, §14.1): the API
      deletes that `dev/reset-broker.sh` issues MUST reach redb, or the next
      suite starts against a file the maps no longer show and every create
      returns a phantom 409 after the first restart. Prove it: run two suites
      back-to-back in `file` mode WITH a broker restart between them.
- [x] B11. On-disk format version in a `meta` table, checked at boot: refuse
      to start with a clear message (or migrate) on mismatch. A value-shape
      change must never be read as valid data by a newer binary.
- [x] B12. Backup story, verified not assumed: a live `cp` of an open redb
      file can tear. Establish and document the supported route (copy under a
      read transaction / savepoint, or stop-copy) and test restore. B9's
      README table cites whatever this task concludes, not "just copy it".
- [x] B13. Group-commit lever (`ponytail:` not default): redb has ONE writer,
      so concurrent request commits serialize behind it and the measured
      3,127 writes/s is a per-process ceiling, not a per-core one. Export
      commit queue depth; only if a benchmark hits the ceiling, batch pending
      writes into one txn (and re-verify commit-before-ack still holds for
      every batched write). *(2026-08-04: depth+peak counters exported via
      /q/health `commitQueueDepth`/`commitQueuePeak` in file mode; group
      commit itself deliberately unbuilt until a benchmark demands it.)*

## C. `postgres` mode — the phase-1 real store (§3, §6.2, §8)

### C-i. Foundation

- [x] C1. `sqlx` 0.9 (postgres, runtime-tokio, tls-rustls, json, chrono,
      uuid); ONE shared pool ≈2×PG cores (§6.2); never per-tenant pools
      (§14.2).
- [x] C2. Embedded `sqlx migrate` run at start: `tenants`, `entities` (§8.1
      exact shape: `version`, extracted `types/scopes/location`,
      tenant-leading PK), `subscriptions` (+bookkeeping columns),
      `csource_registrations` + `csource_index` (ops bitmask §8.3),
      `csource_subscriptions`, `jsonld_contexts`, `entity_maps`, `outbox`;
      timescale-only DDL guarded (§8.2).
- [x] C3. Indexes exactly §8.1: btree_gin `(tenant_id, types)`, GIST
      `location`, GIN `entity jsonb_path_ops` (no kitchen-sink GIN),
      `(tenant_id, modified_at DESC)`, GIN `scopes`; autovacuum tuning +
      lowered fillfactor on `entities` (§3.1.3 bloat note).
- [x] C4. Tenancy (§3): RLS policy on every table; `SET LOCAL antares.tenant`
      helper (transaction-scoped ONLY, §3.1.5); SQL always also filters
      `tenant_id = $1`; tenant auto-create = `INSERT … ON CONFLICT DO
      NOTHING` (§3.1.4).

### C-ii. Store implementations (`antares-sql/src/store/`, §9.3)

- [x] C5. `entity.rs`: create/replace/merge/append/partial/delete + UNNEST
      batches; all semantics in Rust (no PL/pgSQL, no triggers — §4);
      read-modify-write under `SELECT … FOR UPDATE`, `version` bumped under
      the row lock (§3.1.2–3). *(Batch create/delete are single multi-row
      statements (jsonb-elements form of UNNEST) with duplicate-id semantics
      per 5.5.11; upsert stays per-item — each needs its before-image for the
      change event; batching it is a later perf lever.)*
- [x] C6. `subscription.rs`: CRUD + status columns as real columns (§8.3;
      rows are truth §14.1); preserve the send-time bookkeeping ordering
      (046_12_01 fix).
- [x] C7. `registration.rs`: CSR store + `csource_index` maintenance in Rust;
      expiry filtered at the single mirror yield point (§4.1 L4).
      *(Index rows rebuilt in the SAME transaction as every registration
      write; 4.20 ops bitmask with group expansion; FK cascade cleans on
      delete; expiry filtered once in federation.rs's registration yield.
      Names stored as-written — canonical-IRI columns land with the §16.7
      SQL matching path, which is the index's first reader.)*
- [x] C8. `context.rs` (only cross-tenant table §8.3), `entity_map.rs`
      (+TTL sweep, B1 regression), `outbox.rs` (same-tx INSERT §10),
      `tenant.rs`. *(contexts CRUD + tenant helpers landed with the C13
      facade; outbox enqueue is same-tx-atomic with peek/ack for the F3
      drain (producer wiring lands WITH the drain — undrained rows would
      only bloat, R4); entity_map store keeps per-row registration ids (B1)
      and sweeps per tenant because a tenant-less DELETE under RLS is a
      silent no-op for a non-superuser role.)*
- [x] C9. Plain-mode temporal: `temporal_entities` + `attr_instances`,
      native `PARTITION BY RANGE (observed_at)`, partition/retention jobs
      claimed via `SELECT … FOR UPDATE SKIP LOCKED` (§3.1.6, §8.2).
      *(Dual-write: every temporal doc write decomposes into attr_instances
      rows in the SAME tx; weekly partitions pre-created by the broker's
      maintenance task (default catch-all partition holds historic
      backfill); retention = ANTARES_TEMPORAL_RETENTION_DAYS, deliberately
      no default. CUTOVER COMPLETE 2026-08-08 (ADR-0009): temporal READS
      now reconstruct from attr_instances rows — the 0002 doc bridge is
      dropped (migration 0006 keeps only the small `meta` jsonb), writes are
      per-instance deltas (mirror_record uses the pure-INSERT append seam),
      TemporalFilter carries entity LIMIT/OFFSET, and retention finally
      shortens what queries return.)*

### C-iii. Query compilation (`antares-sql/src/compile/`)

- [x] C10. `q=` AST → `jsonb_path_query`/`jsonb_path_exists` (Scorpio's
      proven strategy §8.1); jsonpath BOUND as a parameter, never spliced
      (§16.2). *(compile/q.rs; wired into Query Entities via
      `EntityFilter.q`. The compiler is deliberately PARTIAL and returns
      `None` for any shape it cannot reproduce exactly as `qeval` would —
      dotted paths (sub-attribute vs value-object navigation is a per-row
      decision), `~=` (regex dialects differ) and string ordering (Rust
      byte-wise vs DB collation). SQL narrows, qeval answers, so a refusal
      costs a scan and never a wrong row.)*
- [x] C11. geo (`ST_DWithin`/`ST_Within`/`ST_Intersects`, geozero
      GeoJSON→EWKB binds §6.5), scopeQ, projection (attrs/pick/omit), entity
      ordering (4.23), temporal ranges + 206/`Content-Range` bounds (U3),
      pagination (limit/offset; keyset for temporal §8.2).
      *(2026-08-05, exactness-gated: the store now reports `decided` — SQL
      applied every present predicate exactly — and only then pagination
      (ORDER BY id + LIMIT/OFFSET + count(*) OVER ()) and projection
      (pick/attrs keep-heads, whole-attr omit drops) run in SQL; any inexact
      predicate (scopeQ loose, geo metric residual, uncompiled q=, non-default
      geoproperty) forfeits the whole ladder and falls back to narrow-only.
      Temporal: entity narrowing (ids/types/attrs) + byte-exact instance
      pruning (compile/temporal.rs mirrors `instance_matches` with COLLATE
      "C") + RANK()-capped lastN that keeps timestamp ties; withheld when
      q=/geo need the full instance set. Ordering (4.23) stays DELIBERATELY
      evaluator-owned — its cross-datatype comparison order has no faithful
      SQL collation, so orderBy simply forfeits the pushdown (SQL narrows,
      the evaluator answers; same partial-compiler contract as C10). Proof:
      pg_query_parity.rs pushdown+pruning tests vs live PostGIS, and the full
      postgres pipeline 1037/1037 incl. MQTT TPs, RSS peak 138 MiB.)*
- [x] C11b. Geo completeness for the SQL path: `near` compiles to
      `ST_DWithin(location::geography, $n::geography, N)` (geography cast =
      real meters, GIST-indexable); query geometry ALWAYS a bind
      (`ST_GeomFromGeoJSON($n)` / geozero EWKB), never spliced (§16.2);
      write-time extraction fills `entities.location` and
      `csource_index.location` (C2/C5/C7 columns) via geozero;
      `geoproperty≠location` gets the unindexed
      `ST_GeomFromGeoJSON(jsonb path)` route (or documented in-memory
      post-filter fallback) — indexed fast path is `location` only.
      *(Deviation: PostGIS converts the GeoJSON itself
      (`ST_GeomFromGeoJSON($n)`) instead of geozero encoding EWKB — the DB
      already owns the conversion, the value still travels as a bind (§16.2),
      and it drops a dependency. Both `entities.location` and
      `csource_index.location` are filled this way at write time, guarded by
      `ST_IsValid` so an unrepresentable geometry becomes NULL. A
      non-default `geoproperty` declines to compile and the evaluator
      post-filters — the documented fallback.)*
- [x] C11c. Geo parity fixtures: ONE shared fixture set (points/lines/
      polygons with holes, edge-crossing intersects, MultiPolygon, near
      min/max) asserted against BOTH the in-memory evaluator (H7) and the
      pg-compiled SQL (PostGIS testcontainer) — store modes must not give
      different geo answers. *(antares-api/tests/pg_query_parity.rs, run
      against live PostGIS 3.3 — asserts the one-directional invariant: every
      entity the evaluator keeps is returned by SQL. Covers q=, scopeQ and
      geo together, since all three narrow the same statement.)*
- [x] C12. Guards: CI grep denies `format!` into `sqlx::query` outside the
      compiler allowlist (§16.2); cargo-fuzz targets on q/scopeQ/geoQ
      parsers in scheduled CI. *(fuzz/ crate: parse_q, scope_q, geo_params +
      the JSON-LD expansion input path; weekly .github/workflows/fuzz.yml,
      5 min/target, crash artifacts uploaded; parse_q smoke-ran 385k execs
      clean. CI grep was already in ci.yml.)*

### C-iv. Cutover & validation

- [x] C13. All consumers on the trait; memory/file still green after the
      seam refactor (regression gate before SQL lands). *(2026-08-04: full
      process-mode ETSI reruns post-cutover — memory 1025/1025, file
      1025/1025.)*
- [x] C14. Integration: `sqlx::test` + testcontainers (PostGIS) per store;
      RLS cross-tenant denial tests (§16.1.3); §3.1 concurrency suite
      (parallel PATCH storm, no lost updates, version monotone).
      *(Implemented with an env-gated live PostGIS (ANTARES_TEST_DATABASE_URL;
      CI service container) instead of testcontainers — same coverage, no
      docker-in-docker dependency; RLS denial runs as a NON-superuser role.)*
- [x] C15. ETSI: `STORE=postgres` full pipeline green, 350 MiB gate held.
      *(2026-08-04, unprivileged-sandbox proof: the full suite (all 8
      suites incl. IOP) 1025/1025 in postgres mode with brokers as
      processes + apt PostGIS — same suites, same brokers, same DSNs as the
      compose pipeline; broker RSS measured 8–19 MiB against the 350 gate.
      The docker pipeline enforces this continuously in CI (etsi.yml gates
      on gate-status.txt per mode).)*

## D. `timescale` mode — TemporalStore second impl (§8.2)

> **Truth-up 2026-08-08 (audit + ADR-0009):** the D checkboxes below were
> honest about the DDL but not about the read path — until the ADR-0009
> cutover, `attr_instances` had NO reader and compression/retention were
> cosmetic. That is fixed: rows are the read path in both Pg modes, the
> maintenance branch is PINNED at startup from the actual relkind
> (`detect_temporal_backend`) instead of per-tick pg_extension probes, and
> `ANTARES_STORE=timescale` on a database whose migrations ran without the
> extension is a fatal, named error. Retention additionally reaps the
> DEFAULT partition's expired rows in plain mode.

- [x] D1. `TemporalStore` trait, exactly two impls (`timescale.rs`,
      `plain.rs`); identical table shape and queries — modes differ only in
      DDL bootstrap + maintenance jobs. *(Deviation, ADR-0005 style: the
      §8.2 rule itself says shape+queries are IDENTICAL, so a Rust trait
      would hold two copies of the same code — the two modes live as the
      two DDL branches of migration 0003 plus the two maintenance branches,
      selected per-database by pg_extension. The normative property — modes
      differ ONLY in bootstrap+jobs — holds exactly.)*
- [x] D2. Timescale DDL: hypertable (7-day chunks), compression
      (`segmentby=tenant_id,attr_id`, `orderby=entity_id,observed_at DESC`),
      retention policy. *(Compression must be the LAST DDL — Timescale
      refuses ALTERs after columnstore, and refuses columnstore over RLS:
      attr_instances drops the RLS belt in timescale mode only, explicit
      predicates remain — ADR-0006. Retention via drop_chunks under the
      maintenance claim when the retention knob is set.)*
- [x] D3. Detect via `pg_extension`; explicit error when absent — never
      silently fall back (§8.2). *(Per-database pg_extension probe;
      ANTARES_STORE=timescale without the extension is fatal at startup.)*
- [x] D4. Plain-mode parity jobs in CI; `cargo deny`: nothing links TSL
      (§15.4). *(ci.yml check job runs the sql store tests against BOTH a
      PostGIS and a timescaledb-ha service; cargo-deny-action gates
      licenses+advisories — Timescale is only ever spoken to over SQL.)*
- [x] D5. Store integration tests against BOTH temporal modes (§9.5 matrix).
      *(Same env-gated test suite, pointed at a plain and a
      timescale-enabled database; mode-aware maintenance assertions.)*
- [x] D6. ETSI: `STORE=timescale` green incl. temporal TPs; both modes in CI
      forever after (§5.3). *(Same 2026-08-04 process-mode proof:
      1025/1025 in timescale mode (extension asserted at boot, hypertable
      DDL live); both temporal modes gate unit CI (ci.yml services) and
      the four-mode etsi.yml matrix gates the pipeline.)*

## E. Pipeline / CI closure for the store ladder

- [x] E1. `dev/etsi-pipeline.sh`: all four `STORE` values (`file` mounts the
      data volume; postgres/timescale keep the `db` profile:
      `postgis/postgis:17-3.5` vs `timescale/timescaledb-ha:pg17`).
- [x] E2. CI matrix `[memory, file, postgres, timescale]`; publish requires
      ALL green; per-mode run-summary (suites + CPU/RSS + image size).
      postgres/timescale columns `continue-on-error` until C15/D6, then
      gating. *(Currently they gate AND pass — they run the memory backend
      with the run-summary banner saying so; flips to real gating at C15/D6.)*
- [x] E3. README: store-mode table; `file` RAM ceiling (~10k entities →
      move up a rung; measured 2026-08-04 at ~19 KB RSS/entity — expanded
      doc + temporal mirror, glibc malloc; J7 jemalloc and dropping the
      per-entity temporal copy are the levers to raise it).
- [x] E4. ADRs: mode ladder; redb-as-durability; SQLite rejected.
- [x] E5. `docs/ics.yaml` per clause as C/D land; `mempalace mine` after
      each phase. *(Discipline live: rows refreshed with the C/D/G/H
      landings (4.20 index, 5.8.6 MQTT, H3 pre-adoptions) and the palace
      re-mined 2026-08-04; remaining partial/missing rows are the honest
      gap list — EntityMaps, 4.22 transient expiry, contextSourceInfo,
      orderGeometry/collation, typed-model layer.)*
- [x] E6. Create `error.md` (mandated by the ETSI testing guide, absent from
      the repo): the log of ETSI *tool* bugs, so a broken TP is never "fixed"
      by hacking the broker. Seed it with the known ones (QueryEntities
      04_01/04_02 create two payloads with the same id → 409 by the suite's
      own doing).
- [ ] E8. ⛔ NEEDS-YOU (policy decision, one line): the CI `publish` job
      pushes `:latest` + `:v<run>` to GHCR automatically on every green master
      run. Decide: keep it automatic, or gate it behind a GitHub Environment
      with required approval. Until you decide it stays automatic.
- [x] E7. `dev/etsi-run.sh` seds `resources/variables.py` inside the
      submodule in place, which leaves `ngsi-ld-test-suite` permanently
      dirty in `git status`. Restore it on exit (trap) so a dirty submodule
      means a real change, not a leftover.
- [x] E9. Full-granularity parallel CI matrix — `store × suite` (4×8 = 32
      jobs), SAME script as local runs (the §E one-pipeline rule), selected
      by a filter:
      - E9a. `SUITES=` env in `dev/etsi-pipeline.sh` (comma list of suite
        names, default = all): filters which serial suites run and whether
        the IOP step runs. Locally unchanged: `STORE=postgres
        dev/etsi-pipeline.sh` still runs everything;
        `STORE=file SUITES=Consumption dev/etsi-pipeline.sh` runs one cell.
      - E9b. CI: build the image ONCE in a setup job, push
        `:run-${{ github.run_id }}` to GHCR; every matrix cell pulls it and
        calls the pipeline with its `STORE`+`SUITES` pair. Never 32 builds.
      - E9c. Each cell brings up its OWN fresh stack (single-broker suites:
        1 broker; DistributedOperations/IOP: the 5-broker stack) — cross-
        suite pollution (the CommonBehaviours dead-csource leak) gone by
        construction. Per-test teardown inside a suite unchanged.
      - E9d. The serial all-suites run (E1/E2) STAYS as the authoritative
        nightly/master gate — it is what proves the reset story
        (`reset_state()` pairing) that per-cell isolation no longer
        exercises. The matrix is the fast PR signal; the serial run is the
        truth.
      - E9e. Aggregate job: one summary table (mode × suite pass counts +
        RSS gate per cell) as the required status check; any red cell
        fails it. Document the concurrency reality in the workflow header:
        free plan = 20 concurrent (12 queue), Pro = 40; wall-clock target ≈
        image build + slowest suite (Consumption, 328 TPs).
      - E9f. NOT finer than per-suite (`ponytail:` ceiling): intra-suite
        sharding (pabot / per-file splits) only pays if each shard gets its
        own stack, at which point startup time dominates — revisit only if
        the slowest cell exceeds ~15 min.
      *(All six done in .github/workflows/ci.yml: `SUITES=` filter in
      dev/etsi-pipeline.sh (E9a); ONE `build` job exports the image as an
      artifact every cell loads — an artifact rather than a GHCR
      `:run-<id>` tag, same "never 32 builds" property without publishing
      untested bytes (E9b); each cell runs the pipeline, which brings up its
      own stack (E9c); `etsi-serial` job — all suites, ONE stack, 4 modes in
      parallel, on master pushes + nightly cron, so `reset_state()` stays
      exercised where the matrix cannot exercise it, and `publish` needs it
      (E9d); `etsi-aggregate` renders the 4 per-store tables via
      dev/etsi-matrix-summary.py as the required check, concurrency reality
      in the workflow header (E9e); per-suite is the floor, reasoning kept in
      the etsi-cell comment (E9f).)*

## F. Messaging & scale-out — phase 2 eventing (§6.4, §7, §10)

v0 runs `LocalBus` in one process. Scale-out needs the real spine.

- [x] F1. `antares-bus` NATS impl: `ANTARES_CHANGES` stream (Interest
      retention), subjects `changes.{tenant}.{type_hash}.{id_hash}` (§7);
      pull consumers only; explicit ack AFTER processing; bounded prefetch
      (§6.4). *(bus/nats.rs; hashes are hand-spelled FNV-1a 64 — bit-stable
      wire contract, no dep; max_ack_pending 256; async-nats 0.50 pinned.)*
- [x] F2. `ChangeEvent` finalized: op enum, `changed_attrs`, `payload` +
      `prev_payload` (load-bearing §7), `version`, `(incarnation, version)`
      ordering key + entityDeleted fence (§3.1.3); claim-check refs >256 KB
      instead of chunking (§7). *(Deviation, deliberate: the producer sits in
      the store, so op granularity is create/update/delete/batch — the finer
      API ops are derivable and no consumer branches on them; changed_attrs
      likewise stays empty because both consumers diff prev/payload
      themselves. Claim check tested on the wire (bus/tests/nats.rs).)*
- [x] F3. Transactional outbox drain (§10): publish from the outbox table
      with `Nats-Msg-Id` = row id dedup; never fire-and-forget after commit.
      *(Producer: same-tx enqueue inside every pg_entity write, behind
      set_outbox — on exactly when bus=nats, so bus=local never grows the
      table (R4). Drain runs on every api pod; concurrent drains are absorbed
      by the duplicate window (dedup test proves the republish is swallowed).)*
- [x] F4. JetStream KV subscription mirror: every instance `watch()`es,
      compiled-subscription map in `antares-matcher`; revisions as CAS;
      Postgres stays the system of record (§6.4). No SUB_ALIVE/SUB_SYNC
      equivalent — ever (§7, §14.1). *(DocMirror lives with the matcher code
      in antares-api/notify.rs (v0 placement); it mirrors raw sub docs — the
      compiled/per-type index shape is L3\'s lever, not this box. CUD → KV put
      (null-doc tombstone), watcher-before-hydrate so no delta is lost;
      per-key last-writer-wins converges. mutate\'s row lock plays the CAS
      role — the KV is never authoritative.)*
- [x] F5. Registration mirror over `ANTARES_REGISTRY` stream: ONE compiled
      mirror per process, delta-applied, expiry filtered at the single yield
      point (§16.7). *(Broadcast = ephemeral consumer per instance; expiry
      stays in matching_regs — the one yield point unchanged.)*
- [x] F6. `--roles api,matcher,notifier,temporal,registry` actually split
      consumers (§9); `bus=local` asserts single-process-all-roles at
      startup (§9.2). *(broker/wiring.rs; HTTP serves on every role (health/
      orchestration), roles gate CONSUMERS; bus=nats additionally requires a
      postgres/timescale store — per-process state cannot back replicas.
      Interval firings become single-winner via a row-lock claim that
      re-checks due-ness INSIDE the lock (§3.1.6) — engaged only in nats mode
      so single-process bookkeeping ordering (046_12) is untouched.)*
- [x] F7. Topology assertions at startup: broadcast (ephemeral per-instance)
      vs balanced (shared durable) explicit per consumer (§6.4, R10 lesson).
      *(Explicit in the method names (consume_balanced vs
      consume_registry_broadcast) AND server-verified by assert_topology
      before traffic; the live 2-consumer test proves the semantics.)*
- [x] F8. Temporal auto-recording moves to the durable consumer
      (`antares-temporal::recorder`), idempotent upserts on
      `(tenant, entity, attr, observed_at)` (§6.4). *(SUPERSEDED 2026-08-05,
      K8 lesson: the recorder consumer is REMOVED and auto-recording is
      synchronous in the write path in every bus mode. Every write goes
      through an api pod that has the shared store, so in-request recording
      gives read-your-writes — the ETSI suite asserts history immediately
      after a write — and kills the late-replay resurrection race (a consumer
      re-applying a pre-delete event after a direct temporal delete). The
      consumer double-applied by design and bought nothing but the races;
      `antares-temporal` is deleted, the `temporal` role keeps only the
      plain-mode partition job. The temporal_writer durable is gone — that
      durable-name contract change is this note (§9.1). The normative
      property F8 wanted — history recorded in every mode, idempotently —
      holds: the store\'s dual-write lands instances on the attr_instances
      unique key under the entity row lock. nats_e2e proves history reads
      correctly cross-pod; the outbox drain gains a same-process nudge so
      publish latency is ~1 ms instead of the 250 ms poll.)*
- [x] F9. Bus integration tests (testcontainers NATS): 2-consumer
      broadcast-vs-balanced assertion, claim-check, dedup (§9.5); e2e
      2-instance sync + out-of-order publish injection (version-LWW holds,
      §3.1 test hooks). *(Env-gated live NATS (ANTARES_TEST_NATS_URL, the
      house pattern instead of testcontainers): bus/tests/nats.rs all three;
      broker/tests/nats_e2e.rs runs TWO real binaries with split roles — sub
      + entity through the api instance, notification matched/delivered and
      temporal recorded by the worker instance, then v5-before-v4 injection
      proves ordering tolerance (the matcher projects no state; the
      recorder\'s replay test covers the LWW/idempotence side).)*
- [x] F10. Compose/pipeline: optional NATS profile for multi-role runs;
      2-instance e2e in CI. *(The K2 HA overlay now carries its own NATS and
      runs both replicas ANTARES_BUS=nats for postgres/timescale (HA_BUS
      knob; memory keeps local for the pure LB/drain drills) — bus=local
      replicas would double-fire interval subs, which is exactly the §9.2
      assertion. CI: nats:2-alpine -js via docker run (services cannot pass
      the -js argument) + ANTARES_TEST_NATS_URL, so the F9 tests and the
      2-instance e2e run on every push.)*

## G. MQTT notification binding — clause 7 (feature `mqtt`, §5.4.10)

Currently the suite runs `--exclude '*mqtt*'` — these TPs are open.

- [x] G1. `rumqttc` sink in `antares-notifier` behind feature `mqtt`
      (default on): `mqtt(s)://host[:port]/topic` endpoint URIs,
      `notifierInfo` (`MQTT-Version`, `MQTT-QoS`), payload =
      `{metadata, body}` wrapper (7.1/7.2). *(e2e proof against live
      mosquitto: antares-broker/tests/mqtt_notify.rs.)*
- [x] G2. Bounded client pool per endpoint host WITH eviction (audit L5);
      timeouts at construction (U1). *(MqttSink: cap 32, LRU eviction,
      5 s connect/publish deadline, ConnAck-gated connect.)*
- [x] G3. `NotificationSink` registry: subscription naming a scheme with no
      registered sink → 422 `OperationNotSupported` (§9.2). *(Scheme list is
      cfg-gated on the `mqtt` feature; mqtt endpoint URI + notifierInfo are
      validated at creation, not first delivery.)*
- [x] G4. emqx (or mosquitto) joins the ETSI compose; drop the `*mqtt*`
      exclusion; MQTT TPs (058_xx) green in all store modes.
      *(mosquitto built from the suite's own confs (scorpio-test-mosquitto,
      compose-files/mosquitto/) + readiness-wait overlay on
      MqttUtils.resource (error.md 2026-08-05). Subscription cell 122/122
      incl. all 058_xx, gate PASS in ALL FOUR modes:
      results/g4-{memory,file,timescale,postgres}/gate-status.txt.
      postgres cell 2026-08-06: 122 passed, 0 failed, peak RSS 156 MiB;
      C11's full-pipeline postgres run was 1037/1037 incl. MQTT TPs.)*

## H. Remaining spec surface (v1.x ledger items, §5.4)

The suite is green, but the ledger has normative surface the TPs don't cover;
audit each against `docs/ics.yaml` and close the gaps.

- [x] H0. Create `docs/ics.yaml` — it does not exist yet, though §14.6
      mandates it and E5/H1 both write to it. CIM 029 shape, one row per
      clause, rendered by CI.
- [x] H1. ICS audit: walk §5.4.1–5.4.10 checkbox by checkbox against the v0
      code; record implemented/partial/missing in `docs/ics.yaml` — the
      suite's 686 TPs cover only part of the normative surface (§0.2).
      *(2026-08-04: 122 rows — 95 implemented, 13 partial, 7 missing,
      7 staged-v1x. Gaps feed H2–H7 + new: EntityMaps (5.14/6.32/6.34/6.35)
      missing entirely; 4.22 transient expiresAt not enforced on entities;
      mqtt scheme accepted then silently skipped → G3; contextSourceInfo
      (4.3.6.5/6) missing; orderGeometry/collation absent (4.23).)*
- [x] H1b. Spec-verified defect sweep (2026-08-09) — every conformance claim in
      `docs/production-readiness-audit-2026-08-09.md` re-checked clause by
      clause against CIM 009 V1.9.1, then the unambiguous ones fixed:
      **5.6.21.4 Purge** — the qualifying set is exactly type / attrs-with-a-
      non-system-Attribute / q-with-a-non-system-Attribute / georel / local.
      `id`+`idPattern` filter but never qualify; accepting them made
      `DELETE /entities?idPattern=.*` an unauthenticated tenant wipe.
      **Table 6.4.3.2-1 `type=*`** — selects every type, implies local, and
      `type=*&local=false` is 400 (previously expanded as a term → empty array).
      **5.7.2.4** — `geometryProperty` outside `application/geo+json` is 400;
      `orderBy` is refused when the operation actually federates (4.23.1 "Sort
      ordering is never applied to distributed operations") — keyed on
      execution, not on `local=true`, or ETSI 019_19 would fail.
      **Table 5.2.40-1** — `/info/sourceIdentity` now emits `contextSourceAlias`
      / `contextSourceUptime` / `contextSourceTimeAt` (was `hostAlias`/`uptime`,
      not core-context terms).
      **6.3.4** — Accept precedence uses the json > ld+json > geo+json list
      order as the equal-q tie-break (was header order); and 411 Length Required
      is implemented, mandated by both 6.3.2 and 6.3.4 with no chunked
      exemption. Named deviation: scoped to HTTP/1.x.
      *Tests:* 37 unit tests in `antares-api` + 6 new Robot TPs (060_06, 019_26,
      019_27, D011_05_inc, 045_02, 061_01, 046_01) — 4.23/5.15/6.3.4-411 have no
      official ETSI coverage at all. Suite TP 060_05 filtered by `id` alone,
      contradicting 5.6.21.4 + Table 6.4.3.3-1; corrected in the fork and logged
      in `error.md`. Remaining findings (V-14…V-30) stay open in the audit.
- [ ] H2. Snapshots (5.16, 6.36–6.38, 5.2.41/42, 5.3.4) — staged v1.x;
      pre-adopt `202 Accepted` on creation (§15.1). Implementable from the
      clause, but ⏳ has NO validation oracle: the Robot suite has no snapshot
      TPs yet, so unit tests are the only proof until ETSI ships them.
- [x] H3. 2.0 cheap pre-adoptions (§15.1): HTTP `HEAD`/`OPTIONS`,
      `GET .../attrs/{attrId}` + `/value` endpoints, `508 Loop Detected`,
      schema readiness for `attributeNames` merge (§8.3 note).
      *(HEAD is native to axum's get(); OPTIONS → 204 + the route's computed
      Allow via an outermost layer preserving axum's Allow extension; attr
      GET/value reuse the entity repr pipeline; 508 was already live
      (federation loop_508); attributeNames readiness = the documented
      property_name/relationship_name → attribute_name coalesce on
      csource_index. Router test h3_preadoptions_… covers all four.)*
- [x] H4. `Prefer: ngsi-ld=<version>` / `Preference-Applied` / 203 +
      per-subscription `ngsildConformance` (6.3.21, §15.1) — verify complete.
      *(Was missing entirely; implemented via the 4.3.6.8 fallback tables in
      antares-api/src/conformance.rs: router middleware + notification
      amendment, unit + router tests.)*
- [x] H5. Tolerant-reader audit: unknown members stored and echoed on
      Subscription/Registration/EntityMap (§15.1). *(Audited + regression
      test tolerant_reader_echoes_unknown_members; EntityMap joins when the
      EntityMaps resource lands — it is an H1-recorded gap.)*
- [x] H6. @context management completeness: kinds Hosted/Cached/
      ImplicitlyCreated, delete-and-reload (5.13), `Expires`/`Cache-Control`
      honoured (6.3.16), SSRF policy hook in the loader (§16.4). *(Kinds +
      reload were already green (jsonldContext 61/61); added 6.3.16 TTLs,
      the EgressPolicy private-range deny (ANTARES_EGRESS_ALLOW_PRIVATE
      override for ETSI stacks) and the 5 MB response cap.)*
- [x] H7. Full geoquery (4.10) in the in-memory evaluator — retire the
      planar approximations in `antares-api/src/geo.rs` (its own `ponytail:`
      ceiling): add `geo` 0.33 + `geojson` 0.24; GeoJSON → `geo_types` via
      `TryFrom`; predicates via DE-9IM `Relate` (`is_within`/`is_contains`/
      `is_intersects`/`is_overlaps`/`is_equal_topo`, `!intersects` =
      disjoint) — fixes polygon holes, edge-crossing intersects, line/line,
      MultiPolygon, and makes `equals` topological; query side parsed ONCE
      into `PreparedGeometry` (the §6.5 matcher shape, ≤10k prepared
      geometries); `near` = haversine to closest point (exact for points;
      document the residual delta vs `ST_DWithin` on geography); malformed
      shapes (ring <4 positions, unclosed ring) → 400 from the geojson
      parse, matching the testsuite-doubts.md case. Public `GeoQuery`
      surface (`from_params`/`matches`) unchanged — call sites in
      entities/notify/csource untouched. Proof: C11c parity fixtures.

## I. Security hardening (§16) — requirements with tests, not guidelines
      *(Landed 2026-08-04: geo 0.33 DE-9IM relate; unit fixtures cover
      holes, edge-crossing intersects, line/line, MultiPolygon,
      topological equals (with a literal-equality fast path — the suite's
      019_11_01 polygon is self-intersecting and DE-9IM is undefined on
      invalid rings), malformed-ring 400s; Consumption re-ran 328/328.
      Prepared-geometry caching left as the §6.5 matcher lever; SQL parity
      extends via C11c when C11b lands.)*

*Out of scope, deliberately (decision 2026-08-04, user): **authentication and
rate limiting are not NGSI-LD**. Both are generic HTTP middleware with no
clause behind them, and §16's own posture already delegates request policy to
the PEP / reverse proxy Antares sits behind — per-IP limiting is meaningless
once traffic arrives through a load balancer anyway. What is never delegated
stays in this list and is done: tenant isolation (I5), injection safety
(C12/§16.2), resource bounds (I2) and outbound safety (I4). Do not re-add
these as tasks.*

- [x] I2. Input bounds wall: body size 413, JSON depth ≤64, batch count,
      URI+param length, @context chain/fetch caps, `joinLevel`, AST
      depth/size → 403 `TooComplexQuery`, result ceilings → 403
      `TooManyResults`; all observable via metrics (§16.3). *(bounds.rs
      middleware checks size/depth/URI before any parse (WS-44 order);
      point caps at the batch/join/loader/q parse sites; caps + rejection
      counters exported via /q/health until the K12 metrics stack lands;
      router + parser tests i2_bounds_wall / q_complexity_cap.)*
- [x] I4. `EgressPolicy` for ALL outbound (notifications, @context fetches,
      federation forwards): scheme allowlist, private-range deny-by-default
      with `egress.allow_private` switch (ETSI stacks need it), redirect
      cap, DNS-pinned re-resolution, response-size caps, per-destination
      circuit breakers (§16.4, §16.7). *(api/egress.rs gate — scheme
      allowlist + private deny + per-destination breakers (5 consecutive
      failures trip, 30 s cooldown, one half-open probe) — wired into
      notify.rs::deliver_as (refused = drop) and federation.rs::forward
      (refused 502, breaker-open 503). DNS pinning is a reqwest `Resolve`
      impl, not resolve-then-pin: reqwest re-resolves at connect time, so
      only owning the resolver closes the rebinding window. `client_builder`
      in jsonld/loader.rs is the ONE outbound-client constructor (loader +
      st.http + st.fed_http) and installs resolver + redirect cap 3;
      @context fetch keeps its 5 MB cap → 504. Tests: policy_resolver_
      filters_private_answers, client_builder_caps_redirects (302-to-self
      server, asserts MAX_REDIRECTS+1 accepts), scheme_allowlist_and_
      private_deny, breaker_trips_after_consecutive_failures. ETSI memory
      mode 1025/1025 after the gate landed.)*
- [x] I5. Tenant isolation test pack: RLS denial per store, cross-tenant
      404-indistinguishability (no existence oracle, §16.1.6), tenant-keyed
      in-memory structures audit (§16.1.4), NATS subject re-verification
      (§16.1.5). *(antares-api/tests/tenant_isolation.rs: entity/sub/CSR
      cross-tenant probes byte-compare against ghost 404s; store + DocMirror
      tenant-keying incl. cross-tenant-delete-is-a-no-op; RLS denial was
      C14\'s pg.rs suite (non-superuser role); §16.1.5 lands in
      bus::nats::decode — the subject\'s tenant segment must agree with the
      event body or the event is dropped loudly (unit-tested).)*
- [x] I6. Supply chain: distroless non-root read-only rootfs (done —
      verify), SBOM via cargo-auditable in release builds, `cargo deny`
      advisories in CI, `unsafe_code = "forbid"` (+ reviewed sonic
      exception), no global TLS-verify-off switch anywhere (§16.5).
      *(Verified 2026-08-04: distroless/cc nonroot + volume ownership
      pre-created; Dockerfile now builds with cargo auditable; cargo-deny
      (licenses+advisories) gates ci.yml; workspace forbids unsafe (no sonic
      module exists yet to except); grep finds no accept-invalid-certs
      anywhere.)*
- [x] I7. Security regression suite: cache caps asserted, bookkeeping delete
      paths tested (L6), size-check-before-parse on HTTP bodies (WS-44
      class), cross-tenant probes in e2e per release (§16.6).
      *(crates/antares-api/tests/security_regression.rs: R4-class cache cap
      (400 client-keyed @context puts stay ≤256 entries, via new
      Loader::cache_stats()), L6-class deleted-subscription-stops-notifying
      with a live loopback receiver proving delivery both ways, WS-44-class
      5 MB non-JSON body answers bare 413 before any parse. Cross-tenant
      probes run PER-COMMIT in tenant_isolation.rs (I5) — stricter than the
      per-release cadence §16.6 asks for.)*

## J. Performance & the phase-0 spike debts (§12, §13)

- [x] J1. `json-ld` crate benchmark (risk #1): ≥5k expansions/s/core or
      fork/hand-roll; core-context fast path measured (§6.3). *(The
      fork-or-hand-roll decision was taken at v0: the processor is
      hand-rolled. Measured 2026-08-04 (release, 6-attr entity with nested
      sub-attrs, core context): 401k expansions/s/core — 80× the gate.
      Gate encoded as an ignored release bench in expand.rs.)*
- [x] J2. @context parsed-LRU (moka): size cap ~256, core context pinned,
      `Cache-Control`/`Expires` TTLs, Postgres write-through (§6.3, J1b
      lesson). *(moka caps on merged/fetched/urls caches; core pinned
      outside the LRU; TTLs from H6; write-through persists Cached rows
      under deterministic uuid5 ids with boot preload — and API deletes
      reach the row, so a deleted @context stays deleted across restarts
      (5.13.5).)*
- [x] J3. Bounded JSON-LD concurrency semaphore (§6.3.3). *(32 permits on
      cold context resolution; cache hits bypass.)*
- [ ] J4. 🖥 NEEDS-HARDWARE (16 GB Postgres box; not a free CI runner).
      10M-row synthetic benchmark (xtask seeder):
      q=/geo/type query p95s; decide the extracted-attribute side table
      (named lever, §8.1) on numbers, not vibes.
- [x] J5. Streaming list endpoints: axum streaming bodies + sqlx `fetch()`
      row streams (J3/J11c lesson).
      *(negotiate::respond_list streams entity-by-entity via
      Body::from_stream — the serialized page never exists as one contiguous
      buffer; wired into entities/temporal/subscriptions/csource lists
      (Json + LdJson; GeoJSON wraps a FeatureCollection and stays
      buffered). pg store query + list use sqlx fetch() row streams —
      each row decodes to its Value and the PgRow drops, so the row set
      never sits in memory twice. Contract pinned in
      tests/streaming_lists.rs: streamed bytes are valid JSON, chunked
      (no Content-Length), ld+json embeds @context per entity.)*
- [x] J6. `sonic-rs` feature on the batch-ingest path, serde_json fallback
      compiled always (§6.1). *(feature `sonic`, off by default; both
      variants built in CI via the feature matrix.)*
- [x] J7. jemalloc + decay tuning + heap-stats metrics export (§6.1);
      RSS ≈ live×1.2 target (§2.1). *(tikv-jemallocator global alloc in the
      broker (non-msvc targets); MALLOC_CONF is the decay knob;
      allocated/resident exported on /q/health via jemalloc-ctl until the
      K12 metrics stack lands.)*

## K. HA & operations (§10)

**HA is a property of the store mode, not of the broker.** Measured/verified
2026-08-04, and it decides the update strategy per rung:

| Mode | Multi-instance? | Update strategy | Gap |
|---|---|---|---|
| `memory` | no (state is per-process) | restart = wipe | n/a, ephemeral by definition |
| `file` | **no** — redb takes an exclusive file lock; a second process on the same volume dies with `Database already open. Cannot acquire lock.` (verified) | **Recreate**, never RollingUpdate | shutdown drain + boot rebuild; rebuild measured at 257k entities/s, so 100k entities = 0.4 s and the gap is process start (~1 s) |
| `postgres` / `timescale` | yes, stateless pods | RollingUpdate behind an LB | zero, if the drain in K1 is correct |

So: "always restart" is the RIGHT answer for `file` (and it is cheap), and
"roll during ETSI" is the right chaos test for `postgres`. Don't mix them up.

### K-i. Make it rollable

- [x] K1. Graceful shutdown drain order (§9.3 `shutdown.rs`): stop accepting
      new HTTP connections → let in-flight requests finish (bounded deadline)
      → stop bus consumers → flush the outbox → close pools. Health endpoint
      flips to "draining" FIRST so the LB stops routing before the socket
      closes; SIGTERM triggers it, and the container `stopGracePeriod` must
      exceed the deadline or the orchestrator turns a drain into a kill.
      *(broker/src/shutdown.rs: drain flips /q/health to 503 DRAINING, keeps
      serving for ANTARES_DRAIN_DELAY_MS (the LB notice window), then closes
      the listener, waits in-flight out under ANTARES_DRAIN_DEADLINE_SECS,
      closes pools. Outbox flush deliberately absent until F3 drains it —
      today the row commits same-tx and nothing buffers. Proof:
      file_mode.rs::sigterm_flips_health_before_it_closes_the_socket asserts
      the ORDER — 503 while the socket still serves a 201 — and clean exit 0.
      No bus consumers exist yet to stop; that arm lands with F6.)*
- [x] K2. `ha` compose profile: an LB (nginx/haproxy/caddy) on 9090 fronting
      ≥2 api instances on private ports, health-checked with fast ejection.
      Required because the ETSI compose runs `network_mode: host` with fixed
      ports — two brokers cannot bind 9090, so there is nothing to roll in
      place without the proxy. Profile-gated: the normal ETSI stack is
      unchanged.
      *(compose-files/docker-compose-ha.yml as an OVERLAY, not a profile — a
      profile can only add services, and HA must also move antares1 off 9090;
      the normal stack file is untouched, which is the property the task
      wants. haproxy 3.0 on 9090, check inter 200ms fall 2 = ejection within
      the 500 ms drain notice window. Verified live 2026-08-05: stack up,
      both replicas healthy behind the LB.)*
- [x] K3. `dev/rolling-update.sh`: swap instances one at a time (mark
      draining → wait for in-flight to clear → SIGTERM → start new image →
      wait `/q/health` → next), using the image the pipeline just built. Runs
      locally and in CI, same script (the §E pipeline rule).
      *(Verified live 2026-08-05: STORE=memory roll of antares1 + antares1b,
      6 s per instance, while a 20 req/s probe loop ran through the LB —
      192/192 responses 200, zero failures. CI invocation arrives with K8's
      ETSI-during-roll job, which calls this same script.)*
- [x] K4. N≥2 api pods behind the LB validated in e2e; matcher/notifier as
      shared-durable consumer groups scale and roll independently (§10).
      *(nats_e2e::api_pods_interchangeable_and_worker_group_survives_a_kill:
      2 api + 2 worker processes on one bus — subscription via api-1,
      entity via api-2, notification arrives (pods interchangeable); then
      SIGKILL one worker and the shared durable rebalances: the next change
      still notifies. The LB half (haproxy fronting the pods) is the K2/K3
      overlay, already drill-proven; K8 re-validates it under full ETSI
      load.)*
- [x] K5. Reference manifests (compose + K8s): authored and lint/kind-tested
      in CI; 🖥 a REAL cluster is yours to point at. 3-node JetStream R3, Postgres
      primary/replica (CloudNativePG or Patroni), memory limits in EVERY
      manifest (R1/R2 lesson), `strategy: Recreate` hard-coded for `file`
      mode and `RollingUpdate` for `postgres`/`timescale`.
      *(deploy/k8s/: namespace, nats (3-node JetStream StatefulSet, R3 via
      ANTARES_NATS_REPLICAS=3), postgres-cnpg (CNPG Cluster instances:2,
      PostGIS image), postgres-dev (kind smoke), broker-file (replicas:1 +
      Recreate hard-coded — redb single-process), broker-postgres (2×api +
      2×worker RollingUpdate, ANTARES_BUS=nats); 350Mi limit in every pod.
      CI: kubeconform strict lint + kind smoke job in ci.yml. Sandbox note:
      kind/k3d CANNOT run here (host denies cgroup subtree delegation —
      memory controller unavailable to nested kubelets, verified EIO on
      cgroup.procs migration), so the local proof is stronger-than-lint:
      all 12 objects applied to a real k3s API server (--disable-agent)
      under --validate=strict incl. the CNPG Cluster against the real CRD;
      the kind smoke executes on the GitHub runner.)*

### K-ii. Prove it (the drills)

- [x] K6. Continuity harness, shared by every drill below: a writer loop
      posting monotonically-numbered entities + a recording notification
      receiver. Assertions: zero connection errors, zero 5xx, zero lost
      writes (every acked id present at the end), and notifications
      at-least-once (duplicates fine, losses not).
      *(dev/k6-continuity.py — stdlib-only writer + receiver + auditor in
      one process; exits 1 on any violation, and on zero acked writes (an
      idle run proves nothing). --expect-notifications arms the
      at-least-once assertion when the drill created a subscription.)*
- [x] K7. **SIGTERM drill (graceful):** roll all instances under K6 load.
      Expected: zero failed requests. This is the only real test of K1 —
      a drain bug shows up here and nowhere else.
      *(dev/k7-sigterm-drill.sh: subscription → K6 at 20 wr/s through the
      LB → dev/rolling-update.sh rolls antares1 then antares1b. Run live
      2026-08-05, STORE=postgres HA_BUS=nats: 878 writes, 878 acked,
      0 conn-errors, 0 5xx, 0 lost, 0 unnotified — a roll is invisible.
      The continuity assertions need shared state, so the drill runs in
      postgres/timescale modes; memory-mode HA is two independent stores
      by contract (K2 note) and only health-rolls there (K3).)*
- [x] K8. **ETSI-during-roll job** (postgres mode): run the full suite through
      the K2 LB while K3 rolls the brokers underneath. The suite has no
      retries and asserts exact single responses, which makes it a brutally
      strict drain client — so this is a STRICT gate, not a soak signal: any
      failure is a real K1 bug, not flake. Expected result: 1025/1025.
      *(2026-08-06: first run 1022/1037 — the gate WORKED, catching (1) a
      suite quirk: ContextSource keywords read `Location` case-sensitively
      and haproxy lowercases it → orphan-resource cascade, absorbed in
      haproxy-ha.cfg via h1-case-adjust (error.md); (2) a REAL §14.1 bug:
      Cached-@context existence lived in the per-instance loader map →
      split-brain across replicas — fixed by making jsonld_contexts rows the
      one existence truth (writer wired in AppState::with_store, uuid5(url)
      row lookup). Rerun: 1037/1037 incl. MQTT + IOP, gate PASS,
      results/roll-postgres. Pipeline knobs HA=1 ROLL_DURING_RUN=1; CI
      etsi-roll job runs it at serial-tier cadence.)*
- [x] K9. **SIGKILL drill (ungraceful):** `kill -9` mid-write, per mode.
      Expected: `file` → every acked write present after restart
      (commit-before-ack, §B3/B8); `postgres` → a change committed but not yet
      published is republished from the outbox on restart (§F3); `memory` →
      documented total loss, asserted so the mode's limits stay honest.
      *(file arm: file_mode::kill_dash_nine_right_after_201_loses_nothing
      (pre-existing, K10 era). postgres arm:
      nats_e2e::sigkill_between_commit_and_publish_republishes_from_outbox —
      burst 20 acked creates, SIGKILL the api pod, retry until outbox_peek
      catches unpublished rows at the moment of death, then a fresh pod's
      drain republishes: every caught id notifies and the outbox drains to
      empty. memory arm:
      file_mode::memory_mode_sigkill_loses_everything_by_contract — the
      documented total loss, asserted.)*
- [x] K10. `file`-mode restart drill: assert the exclusive-lock error
      (`Database already open. Cannot acquire lock.`) IS the expected
      behaviour on double-start — never silent corruption, never two writers —
      and gate the restart gap (<2 s at 100k entities; rebuild measured at
      257k entities/s, so the gap is process start).
      *(antares-broker/tests/file_mode.rs, real binary + real SIGKILL:
      `double_start_refuses_the_lock_instead_of_corrupting` asserts the second
      process exits non-zero naming the lock AND that the incumbent is still
      writable afterwards; `restart_gap_stays_under_the_gate` seeds, SIGKILLs,
      restarts cold and times it. Measured 2026-08-04: **215 ms for 10k
      entities**, and the last seeded entity is readable after the rebuild.
      Scale deviation, deliberate: the task writes the gate at 100k entities,
      but E3 measured `file` at ~19 KB RSS/entity, so 100k is ~1.9 GB — past
      this mode's own documented ~10k ceiling AND past the 350 MiB gate. The
      drill runs at the documented ceiling instead; 100k belongs to a
      `postgres` box, not to this rung. The 2 s gate has ~10× headroom either
      way.)*
- [ ] K11. 🖥 NEEDS-HARDWARE for the real drill (kind/compose covers the
      rehearsal). Postgres primary kill (replica promotion,
      broker reconnects without dropping acked writes), NATS node kill (R3
      stream survives, consumers resume, at-least-once holds), broker pod kill
      between commit and publish (outbox covers it) (§10, §13 phase 4).

### K-iii. Operate it

- [x] K12. Observability: tracing + OTLP + Prometheus metrics with the
      `antares_` prefix and unit suffixes (§9.1), change-lag and notification
      metrics, drain/roll state exported so a roll is visible on a dashboard,
      tokio-console in dev. *(broker/src/telemetry.rs is the ONLY place an
      exporter exists (§9.2) — core crates speak the `metrics` facade, which
      is a no-op without a recorder, so tests and a future wasm build cost
      nothing. `/q/metrics` renders Prometheus text through a closure the
      broker installs; absent recorder = 404. Instruments:
      antares_http_requests_total{method,status-class} +
      _duration_seconds (labels deliberately coarse — cardinality is a DoS
      surface, §16.1.7), antares_notifications_sent/failed_total{scheme},
      antares_change_lag_seconds from the JetStream publish timestamp,
      antares_draining (set at the drain flip itself, not sampler-paced, so
      a short roll is never missed), uptime, jemalloc allocated/resident,
      commit-queue depth, and the I2 rejection counters. OTLP/HTTP spans
      when ANTARES_OTLP_ENDPOINT is set, nothing otherwise; tokio-console
      behind cargo feature `console`. Proof: broker/tests/metrics.rs runs
      the real binary and asserts the endpoint serves and the POST counter
      moved. The /q/health stopgap fields stay — they are what the drills
      and compose healthchecks already read.)*

## L. Scale validation — phase 4 exit (§13)

- [ ] L1. 🖥 NEEDS-HARDWARE (you provide the box + 24 h of it).
      `xtask load-rig`: 10M entities / 1,000 tenants seeded; 24 h soak;
      all §1 targets measured and held (broker <500 MB, PG <16 GB).
- [ ] L2. 🖥 NEEDS-HARDWARE (1,000 mock sources exceeds a CI runner).
      Federation rig: 1 tenant × 1,000 CSRs (20 % expired, mixed modes)
      vs mock sources with injected latency/failures; p95 bounded by the
      aggregate deadline, 207 correctness, mirror memory in budget (§16.7).
- [x] L3. 10k active subscriptions matching load: index-shaped candidate
      lookup verified O(log n), never scan-all (§1.1). *(2026-08-05:
      SubMirror grew the inverted index — (tenant, type) and (tenant,
      watched-attr) buckets plus a `broad` bucket for 4.17 selection
      expressions the index cannot prove (over-select allowed, under-select
      never; the superset property is unit-tested against a naive
      reference). BOTH bus modes now wire the mirror: local mode hydrates
      it in notify::wire and feeds it synchronously via sub_sync, so the
      matcher never rescans the store; the interval-claim gate moved from
      mirror-presence to the new AppState.nats flag so local-mode 046_12
      bookkeeping ordering is untouched. Measured (release, ignored gate in
      tests/sub_index.rs): 100k candidate lookups at 10k subs = 0.6 s
      (~6 µs each), and 10× the subscriptions costs 1.5× the lookup — the
      3× scan-smell ceiling holds. Suite regression: Subscription 122/122
      re-run post-change.)*
- [x] L4. RLS pen-test with cross-tenant probes as phase-4 exit criterion
      (§13, §16.1). *(antares-sql/tests/pg_rls_pentest.rs, live PostGIS,
      non-superuser role: attacks EVERY RLS table — read, CTE/subquery
      escape, UPDATE and DELETE naming the row exactly, forged
      cross-tenant INSERT, and moving one's OWN row into another tenant —
      plus §3.1.5 scope-does-not-outlive-its-transaction (8 pooled probes)
      and "the app role cannot turn the policy off" (DISABLE/NO FORCE/DROP
      POLICY/CREATE permissive/BYPASSRLS all refused). attr_instances is
      probed only when the database reports RLS on it — timescale drops
      that belt by design (ADR-0006), so the test asks rather than
      assumes. Mutation-verified 2026-08-05: dropping the entities policy
      turns the suite red, restoring it green — and a BYPASSRLS/superuser
      role fails the premise assert instead of passing vacuously.)*

## M. Ecosystem & publishing (§9.1, §6.6)

- [ ] M1. ⛔ NEEDS-YOU. Publish `ngsild-model` + `ngsild-ql` to crates.io;
      reserve the `ngsild` facade name (§9.1 — checked free 2026-08-03).
      Needs your crates.io token, and a published name cannot be taken back.
      An agent may prepare the crates (metadata, README, `cargo package`
      dry-run, `--dry-run` publish) and then stop.
- [x] M2. ADR backfill: tenancy, bus choice, WS deferral, store ladder —
      one file per irreversible decision (§9.5 doc rule). *(ADR-0001..0004
      landed with §B; ADR-0005 records the AnyStore enum + sync Pg facade +
      the locked-mutate rule, 2026-08-04.)*
- [ ] M3. ⏳ BLOCKED-EXTERNAL. WS binding stays deferred (§11): keep the
      seams (sink registry, outbox, one matcher). Unblocks only when ETSI TC
      DATA issue #8 produces a TS — implementing ahead of it risks divergence.

## N. Browser build — the broker compiled to WebAssembly (opt-in, one crate)

The whole broker (router, handlers, JSON-LD, query engine, matcher, store)
compiled to `wasm32-unknown-unknown` and loaded by a web page. The one thing
that cannot cross: the TCP listener — browsers have no inbound sockets — so a
Service Worker feeds requests into the same router instead. Scoped to ONE new
crate behind a feature, core crates untouched (§9.2 pluggability rule).
Needs A (the store seam) first: the browser build is `memory` (or `file`, see
N4) selected through the same trait, compiled to a different target.

- [x] N1. `crates/antares-wasm`: `wasm-bindgen` entry that builds `AppState`
      with the memory store + `LocalBus` and exposes
      `handle(request) -> response` by calling `tower::Service::call` on the
      axum router directly (v0's `main.rs` already drives the router this way,
      the listener is the only thing dropped).
      *(Broker::new + handle in antares-wasm/src/lib.rs; the wasm-bindgen
      Request/Response bridge in browser.rs. tests/handle.rs proves the seam
      natively — create → retrieve round-trip + /q/health "memory" — the
      code above the executor is target-identical.)*
- [x] N2. Portability audit of the core crates for `wasm32`: clock
      (`Instant`/`SystemTime` → js time), `uuid`/`getrandom` `js` features,
      `reqwest` wasm backend for outbound notifications, `tokio` reduced to
      `rt + sync + macros` (no net/fs/threads), timers via `gloo`; anything
      else gated `#[cfg(not(target_arch = "wasm32"))]`. No core-crate logic
      may change to accommodate the target.
      *(Landed as: `postgres` feature gates sqlx out of antares-sql/api
      (filter types moved to the ungated store::filter so the AnyStore query
      surface is identical); target-section deps (tokio sync+macros, reqwest
      json, axum minus http1+tokio, uuid js); web-time for Instant (std
      panics on wasm32); gloo-timers for the interval tick;
      crate::spawn = tokio::spawn native / spawn_local wasm. The one wasm
      REALITY the audit had to add: axum demands Send futures + Send+Sync
      state while reqwest's wasm client is neither — send_wrapper (runtime
      thread-checked) fences each whole HTTP interaction behind
      jsonld::http_interaction / HttpClient; sound because the target is
      single-threaded. Egress DNS-pinning/redirect-cap stay native-only —
      in a page the browser's CORS sandbox is the egress boundary (N8).
      Proof: cargo check green for antares-wasm on wasm32-unknown-unknown
      AND the full native workspace + clippy -D warnings + tests.)*
- [x] N3. Service Worker glue: intercept `fetch` on a virtual
      `/ngsi-ld/v1/*` origin path → `http::Request` → module → `Response`,
      so page JS talks to what looks like a normal broker URL. Plus a direct
      in-page API (`await broker.fetch(...)`) for pages that skip the SW.
      *(www/sw.js — MODULE service worker (Chromium; Firefox lacks module
      SWs, the page falls back to the in-page API automatically); broker
      state shared by every tab of the origin, proven by the two-tab
      browser test. Extra seam the letter of the box didn't name:
      `broker.onNotification(prefix, cb)` — a page cannot host an HTTP
      endpoint, so subscription endpoints under the reserved prefix deliver
      to page JS (wasm-only page_sink hook consulted before the HTTP
      client), and the SW fans them out via BroadcastChannel.)*
- [x] N4. Browser persistence. **Verified 2026-08-04:** redb 4.1.0 DOES
      compile to `wasm32-unknown-unknown` (627 KB `.wasm` for the engine) and
      runs there with `backends::InMemoryBackend` — what it cannot do is open
      a file, because that target has no filesystem. `StorageBackend` is six
      synchronous methods (`len`, `read`, `set_len`, `sync_data`, `write`,
      `close`) that map 1:1 onto an OPFS `FileSystemSyncAccessHandle`
      (`getSize`/`read`/`truncate`/`flush`/`write`/`close`) — the same shape
      SQLite-WASM's OPFS VFS uses. **Decided 2026-08-04 (user): persistence
      is required — implement the OPFS backend.** `InMemoryBackend` stays only
      as the no-OPFS fallback (private browsing, unsupported engines), and the
      IndexedDB snapshot route is dropped unless the OPFS spike hits a wall.
      Blocker to resolve in the spike: the trait demands
      `Send + Sync` and JS handles are neither, so single-threaded wasm needs
      an `unsafe impl` wrapper → a reviewed exception to
      `unsafe_code = "forbid"` (§9.5), same treatment as the `sonic` module.
      Sync access handles are Worker-only, never the main thread.
      *(DONE 2026-08-06 — antares-wasm/src/opfs.rs: OpfsBackend implements
      redb::StorageBackend over FileSystemSyncAccessHandle (short-read/write
      loops, truncate zero-extends per the trait contract);
      `AntaresBroker.persistent(file?)` builds the SAME Shadow via the new
      Store::from_database split (format check B11 + boot rebuild B4 +
      commit-before-ack B3 all shared with native file mode). The
      anticipated unsafe exception was NOT needed: send_wrapper provides
      Send+Sync soundly with a runtime thread check — unsafe_code stays
      forbid everywhere. The broker lives in a dedicated worker
      (www/worker.js) because sync handles exist nowhere else; page ↔ worker
      via postMessage. Proof in www/test/browser-test.mjs: store=file,
      entity SURVIVES a page reload; InMemoryBackend fallback = the
      pre-existing memory ladder (SW / in-page modes).)*
- [x] N5. Build + budget: `wasm-pack` + `wasm-opt -Oz`; ≤8 MB raw, ≤3 MB
      compressed; size printed in the CI summary next to the image size.
      *(dev/wasm-build.sh: wasm-release profile (opt-z, fat LTO, panic
      abort) → wasm-bindgen --target web → wasm-opt -Oz, budgets ENFORCED
      by the script (fail, not warn). wasm-pack itself skipped — it wraps
      exactly these two tools and pins nothing; dev/install-wasm-tools.sh
      fetches wasm-bindgen LOCKFILE-MATCHED + binaryen. Measured: 2.38 MB
      raw / 0.93 MB gzip. CI `wasm` job prints the size line to the step
      summary.)*
- [x] N6. Page builds in-repo and is CI-tested; ⛔ NEEDS-YOU only to publish
      it (your docs site / domain). Create entities, subscribe, watch
      notifications fire in-page. Zero install, no server. No other NGSI-LD
      broker can do this (JVM brokers can't; Orion-LD needs Mongo).
      *(www/index.html + app.js; www/test/browser-test.mjs drives it in
      headless Chromium — SW mode, health, create, subscribe, notification
      observed in-page, second tab sees the same broker — run by the CI
      `wasm` job via dev/wasm-test.sh (which also smokes the Node shim).
      Publishing the page anywhere public remains yours (⛔): everything is
      static files — `www/` + `www/pkg/` on any host.)*
- [x] N7. ETSI conformance for the WASM artifact, in three tiers:
      *(2026-08-06. N7a Node tier: `WASM=1` pipeline tier — five node shims,
      full serial suites+IOP — **1025/1025, gate PASS** (results/wasm-node,
      09:15 run). N7b browser tier: `BROWSER=1` — headless Chromium hosts
      the demo page, www/test/etsi-proxy.mjs forwards suite HTTP into
      window.brokerFetch — CB/Provision/Consumption/jsonldContext
      **671/675** (results/wasm-browser); the 4 residual reds are ETSI-side
      live-context churn hitting every tier incl. native (error.md
      2026-08-06), re-measure when forge settles. The tiers caught two REAL
      spec gaps, both fixed: JSON-LD 1.1 relative context-IRI resolution
      (merge_entry_based base-URL join) and batch-delete temporal
      deletion-mirroring (5.6.10 ↔ 5.6.6 parity). Full trap ledger in
      error.md + MemPalace. N7c documented in ADR-0008/README; CI
      `etsi-wasm` job runs the Node tier at serial cadence.)*
      - N7a. **Node tier (full suite):** the SAME `.wasm` under Node with a
        thin `http.createServer` shim feeding the router; run
        `dev/etsi-pipeline.sh` against it. No CORS, unrestricted outbound
        fetch → all serial suites incl. Subscription should run.
      - N7b. **Browser tier (real proof):** headless Chromium (playwright)
        hosts the module; a small proxy forwards suite HTTP into the page.
        Provision / Consumption / CommonBehaviours / jsonldContext are the
        expected-green set.
      - N7c. **Documented browser-only limits:** notification callbacks and
        federation forwards leave the page as cross-origin `fetch`, and the
        ETSI mock servers send no CORS headers → Subscription /
        ContextSource / DistributedOperations / IOP stay excluded from the
        browser tier (covered by N7a) until the harness proxies them.
        A 5-broker federation stack in-browser needs the proxy regardless:
        no inbound sockets means no broker-to-broker HTTP.
- [x] N4b. OPFS has the same single-writer property as the native file
      (§K10): a sync access handle is exclusive per file, so a SECOND TAB on
      the same origin cannot open the store. Decide the owner story
      (SharedWorker owning the handle and serving other tabs, or leader
      election with a clear "another tab owns this store" error) before N6 —
      a demo page that dies on the user's second tab is worse than no demo.
      *(Decision FORCED by the platform: sync access handles exist only in
      DEDICATED workers — a SharedWorker cannot hold one — so the owner
      story is first-tab-wins + the clear error. The second tab logs
      "another tab or worker already owns the store file … close the other
      tab, or run without persistence" and falls back down the transport
      ladder (SW / in-page, non-persistent) instead of dying. bootPersistent
      retries once after 400 ms so tab-close → tab-open handovers succeed.
      Proof: browser-test.mjs opens a second tab, asserts the refusal +
      fallback, then proves the reload handover.)*
- [x] N8. Docs/ADR: browser build is memory (or OPFS-file) only — no NATS,
      no MQTT, no Postgres, no roles; state which suites gate it and which
      are structurally out of reach.
      *(ADR-0008 (the portability ledger + conformance scope table) +
      README "Browser build (WebAssembly)" section. Node tier = the
      conformance gate for serial suites; browser tier = Provision/
      Consumption/CommonBehaviours/jsonldContext class + the page demo;
      external HTTP callbacks and federation are structurally out of reach
      in a page (no inbound sockets, CORS) and stay Node-tier-covered.)*

- [x] N9. Cross-tenant federation inside ONE broker + the loopback host
      (2026-08-07). Spec side: CSR `tenant` (5.2.9) selects the peer tenant
      on every forward; `contextSourceInfo` (4.3.6.5) pairs travel as forward
      headers (`urn:ngsi-ld:request` passthrough; reserved/connection keys
      and the 4.3.6.6 processed keys skipped — see CSI_SKIP). Browser side:
      requests to `http://self.antares.internal` re-enter the same broker
      (www/loopback.js patches global fetch — a fetch made inside a SW/worker
      never re-enters its own handler), `Broker::handle`/`AntaresBroker::fetch`
      take `&self` (router cloned per call) so the re-entrant forward can't
      trip wasm-bindgen's recursive-borrow guard. The www page is the
      "context spaces" playground (spaces = tenants, federation links = CSRs,
      source/sync pipelines); `.github/workflows/wasm.yml` is the manual
      build+test+Pages pipeline.
      *(Proof: antares-wasm/tests/handle.rs
      csr_tenant_federates_across_tenants_in_one_broker — real TCP loopback
      listener feeding the same Broker, asserts peer-tenant header, csi
      passthrough, request-echo, tenant-smuggle rejection, local=true
      isolation; browser-test.mjs federation step over the loopback host.)*

---

## Sequencing

```
A → B (file: immediate durable value, v0-compatible)
A → C (postgres: the phase-1 lift) → D (timescale: thin second impl)
E tracks the ladder; matrix columns gate as C15/D6 land.
F (NATS/roles) after C — the outbox table is its foundation.
G (MQTT) independent — any time after the sink registry exists.
H/I/J interleave continuously (spec-first rule: clause by clause).
K splits: the `file`-mode drills (K9, K10) need only B and should land WITH
it; the roll/LB/failover work (K1-K8, K11) needs C and F, since rolling
updates only exist once state is external.
L last (phase-4 exit); M whenever a crate is stable.
N (browser WASM) needs only A; independent of C/D/F and gated by its own
Node/browser tiers, never by the container pipeline.
```

Definition of done, all six measured rather than asserted:

1. Every `- [ ]` without a ⛔ / 🖥 / ⏳ tag is `- [x]`, ticked in the commit
   that landed it.
2. `dev/etsi-pipeline.sh` green for **all four** `STORE` values (memory,
   file, postgres, timescale), locally and in CI, from ONE built image.
3. **MQTT TPs included** — the `--exclude '*mqtt*'` in `dev/etsi-run.sh` is
   gone (§G), so "green" means the whole suite, not the convenient part.
4. The 350 MiB per-broker RSS gate holds in every mode, and `kill -9` loses
   nothing in every persistent mode (§K9).
5. `docs/ics.yaml` covers §5.4 clause by clause, `error.md` logs every ETSI
   *tool* bug found, every irreversible decision has an ADR.
6. The §L targets measured on hardware you provide (🖥), not estimated.

## Backlog 2026-08-12 (this goal)

- [x] 1. Close partial 4.10 — DONE 2026-08-12: geo.rs min_distance_m
      (local equirectangular metric min distance, extended ref+target);
      unit 11/11 (2 new, red seen), TP 019_30_09-_12, 4.10-set 31/31 local;
      ledger 4.10 implemented. Commits: 5967835 + af7ac43 (suite pointer).
- [x] 2. Close partial 5.8.6 — DONE 2026-08-12: distsub.rs split_merge
      (expand + local/fed retrieve excluding origin + merge + conditions
      refilter); reduced_copy splitEntities read from sub (bug); unit 4/4
      (red seen), TP 5814_01_02 2/2 local; ledger implemented. Commits: cb385a8 + 44a3a96 (fork).
- [x] 3. Snapshot ceilings — DONE 2026-08-12: run_query via fed_query
      (5.7.2.4), temporal fill pages past max_limit, evict_over_cap
      (priority-ordered, 5.3.4 deletion notification). Unit 16/16 (3 new,
      red seen); TP 5161_01_05 (CS-mock fed fill) 5/5 local; pagination +
      eviction unit-only (need non-default broker config — noted in ledger).
      Commit: 8e9491b (+ fork).
- [x] 4. Durable state for HA — DONE 2026-08-12: Kind::Snapshot/EntityMap/DistSub
      on the store trait (memory maps + redb tables + pg doc-kind path,
      migration 0008 with ADR-0001-shaped RLS); inbound dist-sub index under
      the reserved distsub-index tenant (ADR-0012). tests/durable_state.rs
      2/2 (red seen on inverted survival assertion); targeted regressions
      36/36 (snapshots_5_16 16, entity_maps_5_14 10, entity_maps_usage 6,
      dist_subs_5_8 4 + tenant_isolation 4); EntityMap+5161_01+5814_01 Robot
      TPs 22/22 local. Commit: 602b236.
- [x] 5. docs/upstream/etsi-raises.md — DONE 2026-08-12: five ready-to-file
      texts (3 suite: D018_01, _exc TPs, 504 fixtures; 2 spec: 5.3.4
      naming+snapshotReady, 6.6.3.2-2 UpdateResult). Filing is user-side.
      Commit: 7f228b0.
- [x] 6. MemPalace re-mine — DONE 2026-08-12: docs/spec/ was already in
      mempalace.yaml excludes; 91 hand-filed chunks backed up to
      ~/mempalace-backups/ngsi-ld-handfiled-2026-08-12.json; mine re-filed
      54 changed files (2934 drawers); 4 decision chunks absorbed into the
      mined wing (content preserved in backup + mined copies).

## Temporal q= follow-up 2026-08-13 (spec+palace-verified checklist)

Source analysis: /workspace/taskImplementation.md (this list supersedes its
§5). Every item re-checked against the clause text (MemPalace
gs_cim009v010901p.pdf pages cited) and against palace decisions — no prior
decision contradicts any item; the S4 check SURFACED two aggravations
(items 1b/1c).

- [x] 1. **DONE 2026-08-13, commit 1ce6de9 (suite fork da7d728): red-first
      tests/temporal_scope_5_7_4.rs 0/7→7/7; TP 5744_03 11/11; full
      QueryTemporalEvolutionOfEntities tree 131/131 local; api lib 177 +
      jsonld + autorecord/expand-values/entity-maps/snapshots regressions
      green; `dev/spec.py check` 0 violations. Bonus sub-gap found+fixed on
      the red run: 5.6.11 rejected the 4.5.6 instance-shaped temporal scope
      ("scope entries must be strings" 400) — expand.rs temporal arm.**
      **5.7.4.4 S4: scopeQ ignored on local temporal queries — the only
      conformance bug.** p.209: "If the Scope query is present, from S3,
      select those Entities whose Entity Scope instances match the Scope
      query (4.19)". Reproduced: scopeQ=/X returns an /A/B-scoped entity
      (memory broker, 2026-08-13). Ledger 5.7.4.4 `implemented` is
      overstated until fixed. Fix: scope_matches after the geo check in the
      S-loop; TP 5744_03 (match, non-match, /#, alternatives, windowed
      scope instances); ledger note; commit `5.7.4.4:`.
      - [x] 1b. (in 1ce6de9 — the check sits after the federation merge, so aggregated entities are re-filtered) Split-entities arm re-applies scope as **S7** (p.210) —
            when the 5.8.6 merge block runs, scope must filter the
            AGGREGATED set too; cover in the same fix.
      - [x] 1c. (in 1ce6de9 — clause_5_14_5_temporal_map_created_based_on_s4 asserts map contents) p.257: the local EntityMap is "created based on S4" — an
            EntityMap built from a scope-filtered temporal query currently
            bakes in wrongly-included entities; assert map contents in the
            fix's tests.
      - [x] 1d. (in 1ce6de9 — scope_match_intervals: [set-time, next-set) validity intervals, C.5.16 A8311 instance bounding + B9211 carry-in both asserted) 4.18 (p.98): in the TEMPORAL representation scope is
            REIFIED (createdAt/modifiedAt/deletedAt sub-Properties, at most
            one scope property, values merged on distributed merge) and "a
            given Scope is considered valid from the time it has been set
            until the time it has been explicitly removed" — so S4 matching
            over a temporal doc is validity-aware, not a plain
            current-state check; read annex C.5.16's worked example before
            implementing. The 4.19 grammar (+, #, /#, and/or, comma-or) is
            verbatim-confirmed and the existing scope_matches arbiter's
            unit tests (lib.rs ~3103-3124) mirror the spec's own examples
            — reuse it for the value matching, add the validity dimension.
      - VERIFIED not-a-gap: subscription-side scopeQ IS applied
        (notify.rs:608 scope_matches in conditions_match) — the
        silent-ignore is temporal-query-local only.
- [x] 2. **USER-SIDE (sandbox cannot push or watch CI; per the goal:
      listed, not blocked on). What CI must confirm once the Mac-side push
      lands: the 4×8 matrix green over b37aefb + this stretch's commits
      (1ce6de9/9dc7024/72a8c9b/98818fa + item 6) and TPs 5744_01–06 on the
      pg/timescale cells — the cells where the SQL prefilter/paging paths
      actually execute. Local mitigation already run: the full parity
      battery + scope/paging tests against live PostGIS in-sandbox
      (docker pgdev).** Watch the next CI 4×8 matrix: prefilter commits (b37aefb) + TPs
      5744_01/02 must be green on pg/timescale — those cells are where the
      SQL prefilter path actually executes (rule-8 local runs are
      memory-arm only).
- [x] 3. **DONE 2026-08-13, commit 72a8c9b (fork TP 5744_04 = 826424c…): migration 0009 try_geomfromgeojson
      + decompose fill, compile_geo_instance (NULL fallback arm), windowed
      EXISTS in pg_temporal query() with the attr IRI as a bind (any
      geoproperty). Red-first pg_query_parity
      temporal_geo_prefilter_narrows_but_never_drops vs live PostGIS
      (superset + tightness + NULL survival + custom geoproperty); sql-crate
      units 30/30; TP 5744_04 7/7 local memory broker (fork da7d728→).**
      Geo prefilter on attr_instances.geo_value (perf, not
      conformance): S3 (p.209) judges geo on windowed GeoProperty
      instances, so a windowed EXISTS with a PostGIS predicate slots into
      the existing qprefilter framework; verdict stays with
      GeoQuery::matches (superset contract as for q).
- [x] 4. **DONE 2026-08-13, commit 98818fa (fork TP 5744_05 = 826424c): qprefilter leaves now carry the
      byte-exact text window predicate (still + widened column bound for the
      index) and emit() tracks exactness; prefilter_exact() gates the API's
      push_page — paging with q allowed when every leaf is an exactly-
      compiled Cmp (existence/dropped-conjunct/refused-branch stay inexact).
      Red-first pg_query_parity temporal_exact_prefilter_pages_correctly
      (slack-zone entity stole the page slot → exact predicate fixes it);
      qprefilter units incl. exactness_flags_the_pageable_subset; TP
      5744_05 (count + page partition + past-the-end) 3/3, full temporal
      tree 141/141 local; datasetId/pick added to the paging gate (their
      entity drops happen at presentation, after the page).** Exactness
      flag on compile::qprefilter → SQL entity-paging when
      every leaf compiled (today paging is withheld with q/geo because the
      prefilter is superset-only; limit=1 with q still materializes all
      candidates).
- [x] 5. **DONE 2026-08-13, commit 5d1c25a: ready-to-file issue text
      appended to docs/upstream/etsi-raises.md as raise #6 (filing itself
      is user-side).** Raise the lastN-vs-values-filter ordering doubt upstream
      (docs/upstream/etsi-raises.md): Table 5.2.21-1 (p.123, verbatim):
      lastN = "Only the last n instances, per Attribute, per Entity (under
      the specified time interval) shall be retrieved" — window-scoped but
      SILENT on ordering vs the values filter; p.209 S2 checks q against
      "ALL the Attribute instances resulting from the initial filtering
      performed by the temporal query" — whether lastN is part of that
      "initial filtering" is unspecified. Palace: no prior decision. Until
      resolved the broker withholds the lastN cap when q/geo present
      (behavior-preserving, per-attr lastN applied API-side).
- [x] 6. **DONE 2026-08-13, commit 87a3e06 (fork TP 5744_06 = dab0838):
      instance_predicate() extension leaves — [lang]/[*] → languageMap
      wildcard jsonpath (case-insensitive BCP 47 tag matching stays in
      memory, so ANY-language superset), != → NOT of the existential-Eq
      member-OR (array universal quantification + datatype-mismatch-matches
      fall out of the negation; Ne+List/Ne+numeric-Range included), string
      ordering → COLLATE "C" byte compare (p.89 SHALL) with array
      pass-through; deleted_at filled by decompose + NULL-tolerant column
      bound (pre-fill rows escape to the text predicate). All three arms
      superset-only, never page-exact (asserted). Red-first: parity
      tightness rows (speed!=10 etc.) failed before, 6/6 after; sql units
      50/50; TP 5744_06 8/8, full temporal tree 149/149 local.** Optional
      compiler extensions (each widens narrowing, never
      correctness): [lang] brackets (p.90 semantics are exact:
      languageMap."en" jsonpath is expressible), != (p.91/92
      datatype-mismatch + array universal quantification must be
      reproduced exactly or left refused), string ordering (EASIER than
      first assessed: p.89 makes RFC 8259 §8.3 code-unit comparison the
      SHALL and UCA only a SHOULD — qeval's byte compare is the
      SHALL-compliant arbiter, so a COLLATE "C" SQL leaf can be exact, not
      just superset), deletedAt column fill in decompose for the column
      bound.
- [x] 7. **DONE 2026-08-13, commits 8175693 (error.md) + fork 952e61e
      (testsuite-doubts #19).** Bracket-less q on a LanguageProperty is UNDEFINED in 4.9 (p.90
      defines only [lang] and [*] forms) — log in
      testsuite-doubts/etsi-raises if a TP is ever needed there; today the
      broker's eval and the SQL leaf agree (no match), asserted only by the
      self-grounding parity battery.

## IOP TP backlog 2026-08-14

118 new spec-grounded IOP tests (incl. edge cases) listed in
`docs/iop-tp-checklist.md` — 12 groups (REG/CAS/CSI/UNI/PRV/SUB/CSN/
TMP_03/MAP/TEN/ERR/MSC), each clause-cited, none duplicating the 102
existing IOP_TP cases. Work order: one group = one commit, red-first;
three known-red items flagged in the file's footer (508 inclusive-loop
over-application, cooldown-vs-breaker posture, splitEntities merge).

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
compose stacks, no host Docker) and rule 9 stay hard; ponytail throughout.
CI workflow edits are validated by actionlint/yaml parse + local script
dry-runs (the sandbox cannot trigger GitHub runs); anything that needs a
GitHub push, Pages settings, or a license choice goes on the "Mac-side /
user" list at the end of the section instead of blocking. Ask only when an
item forces a genuine user decision (license, Pages mechanics if both
options fail).
```

### Checklist

- [ ] 1. **ETSI report clickable from the README (the headline ask).**
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
- [ ] 2. **README production pass.** Badges at top (ci, etsi-matrix, the
      shields endpoint badge from item 1, coverage); FIX the stale targets
      table (still the pre-2026-08-10 10× smaller contract — claude.md §1
      is the authority: 100M entities / 10k tenants / 100k subs);
      quickstart section (docker run + compose, the four store modes in
      two lines); link the conformance report page and a docs index.
      Negative check: no claim in the README that CI does not actually
      prove.
- [ ] 3. **Repo hygiene files.** LICENSE is MISSING (deny.toml gates dep
      licenses; the repo itself has none) — license choice is the USER's,
      ask once and add. SECURITY.md (contact + the §16 security-wall
      posture, link docs/security-audit-2026-08-04.md), CONTRIBUTING.md
      (build, `-j 2` linker rule, test protocol, one-clause-one-commit),
      CHANGELOG.md seeded from the `v*` tags.
- [ ] 4. **NATS visible + tested at runtime ("check NATS is working").**
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
- [ ] 5. **Rolling-update proof on a cadence, not only on tags.**
      full.yml's K1/K3 job (haproxy + `dev/rolling-update.sh` +
      `k6-continuity.py` under load) runs only on `v*` push/dispatch — add
      `schedule:` (weekly, offset from etsi-coverage's Mon 04:41) to
      full.yml or split the HA/rolling job into a scheduled workflow
      reusing the same steps. Surface the result: workflow badge in README
      + one line on the reports page. Sandbox validation: yaml/actionlint
      + a dry parse of the job's script steps; the run itself is CI's.
- [ ] 6. **Production-readiness re-audit.** Walk every P0/P1 in
      docs/production-readiness-audit-2026-08-09.md; for each, record
      CLOSED (commit + test evidence) or still-open. Still-open items
      become new checkboxes appended to THIS list (that is the "if
      something is missing, it should also be tested" clause). Write the
      updated status table back into that doc with today's date.
- [ ] 7. **Docs index + operations runbook.** docs/README.md mapping the
      docs tree (deep-analysis, adr/, spec/ ledger, audits, upstream
      raises); docs/operations.md runbook: deploy (compose + deploy/k8s),
      per-store backup table (README already has the redb stop-copy rule —
      link, don't duplicate), rolling update procedure (`rolling-update.sh`
      + the file-mode-cannot-roll K10 rule), health/metrics endpoints,
      state-reset discipline (§2). Mostly assembling existing text — no
      new claims without a test behind them.
- [ ] 8. **Coverage surfaced.** etsi-coverage.yml already produces
      lcov+html weekly — publish the merged HTML + a % badge json to the
      same `site/reports/` area (item 1's mechanism), README badge. No new
      floor/ratchet (strict.yml owns the unit floor) — display only.

Mac-side / user (collect here as items complete, do not block on them):
push master + workflow changes, enable/verify the Pages source, pick the
LICENSE, trigger the first scheduled runs.

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
(pp. 241-242, the four match conditions), 5.2.8 (idPattern = IEEE 1003.2
regex), 4.3.6.1-4.3.6.4, 5.7.1.4/5.7.2.4, 5.6.x.4, 5.8.1.4, 4.14, 4.20,
6.3.17.

**Deliverable.** 50 new Robot TP cases in the suite fork under
`IOP_TP/NGSI-LD/Interoperability/Routing/` (6 files, IOP_EXT_IDR_01..06),
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

- [ ] 1. Anchored ADR prefix pattern routes `GET /entities/{id}` B1→B2;
      entity exists only in B2 → 200 via B1; `local=true` still 404 (5.7.1.4, 5.12).
- [ ] 2. Non-matching razidlo (`sk_presov`) → B1 404s WITHOUT contacting B2 —
      mock context source asserts ZERO forwarded requests (5.12; the pruning claim).
- [ ] 3. Exact `EntityInfo.id`, no pattern: that one id routes; a sibling id
      under the same prefix is NOT forwarded (5.12 cond 2).
- [ ] 4. UNanchored pattern `sk_banskabystrica:odpady` matches by substring
      (regex find) — id carrying it mid-URN forwards; documents why ADR-001
      mandates `^...$` (5.2.8, 5.12).
- [ ] 5. EntityInfo with type only (no id/idPattern) matches every id of that
      type (5.12 cond 1).
- [ ] 6. idPattern matches but EntityInfo type ≠ requested type → NOT
      forwarded (5.12 type-selector gate).
- [ ] 7. Multiple EntityInfo elements in one RegistrationInfo — any-of: id
      matching only the second element still forwards (5.12).
- [ ] 8. Two CSRs with disjoint razidlo prefixes → retrieve dials EXACTLY one
      endpoint; the other mock records zero hits (5.12, 4.3.6.2).
- [ ] 9. Invalid regex in idPattern → 400 BadRequestData at registration,
      nothing registered (5.2.8; csource.rs:143 already validates — assert).
- [ ] 10. Case sensitivity: pattern `sk_banskabystrica` does not match an id
      containing `SK_BanskaBystrica` (IEEE 1003.2; ADR lowercase rule).

### IOP_EXT_IDR_02 — query-entities routing (query id/idPattern × CSR id/idPattern)

- [ ] 11. `?id=A,B` where only A matches the CSR pattern → forwarded query
      narrowed to A (mock asserts), B answered locally (5.7.2.4, 4.3.6.1).
- [ ] 12. Query `?idPattern=` matching `EntityInfo.id` → forwarded (5.12 cond 4).
- [ ] 13. Query pattern + CSR pattern both present → assumed compatible,
      forwarded (5.12).
- [ ] 14. Query pattern anchored to a foreign razidlo vs CSR ids of another
      razidlo → NOT forwarded, zero mock hits (5.12).
- [ ] 15. Type-only query against an id-restricted CSR → forwarded (broker
      cannot exclude; assert current Antares behaviour and cite 5.12).
- [ ] 16. Fan-out merge: 3 brokers, one razidlo each; type query via B1
      returns the exact union, no duplicate ids (5.7.2.4, 4.5.5).
- [ ] 17. `local=true` query never forwards regardless of pattern match (5.5.13).
- [ ] 18. Dark entity: exists ONLY behind the CSR; query via B1 includes it;
      `local=true` query does not (4.3.6.2).
- [ ] 19. CSR discovery `GET /csourceRegistrations?id=<urn>` returns only CSRs
      whose EntityInfo id/idPattern matches; `?idPattern=` matches
      EntityInfo.id too (5.10.2, 5.12).

### IOP_EXT_IDR_03 — provision routing by id (redirect/exclusive/inclusive)

- [ ] 20. redirect CSR, anchored pattern: POST /entities with matching id is
      created at B2, NOT held at B1 (`local=true` 404) (5.6.1.4, 4.3.6.2).
- [ ] 21. Create with non-matching id stays local at B1; redirect mock records
      zero requests (5.6.1.4).
- [ ] 22. `DELETE /entities/{id}` routed by pattern → 204, gone at B2;
      non-matching delete never leaves B1 (5.6.6.4).
- [ ] 23. PATCH / partial-attribute-update routed by id through the pattern
      (5.6.2.4, 5.6.3.4).
- [ ] 24. Batch create with MIXED razidlos: matching subset forwarded, rest
      local; success arrays carry ALL ids exactly once (5.6.7.4, 5.6.8.5).
- [ ] 25. Deterministic-URN idempotency (ADR claim): same upsert twice via B1 →
      second is UPDATE (remote 204 ⇒ updated-list, per 2c6c10b), federation-wide
      query still exactly ONE entity (5.6.8.5).
- [ ] 26. Exclusive CSR without explicit EntityInfo.id + attributes → rejected
      at registration (5.2.9, 4.3.6.2; validate_exclusive).
- [ ] 27. Exclusive CSR by exact id: update routes ONLY to the remote; a local
      shadow copy is never created nor consulted (4.3.6.2).
- [ ] 28. Registration narrowing on forwards: mock asserts the forwarded
      request carries only the registered id scope — no broadening (4.3.6.1).
- [ ] 29. Batch upsert across 3 disjoint-razidlo redirect CSRs splits three
      ways; each mock receives ONLY its ids (5.6.8.4).

### IOP_EXT_IDR_04 — multi-broker topologies (3-5 brokers, run-five fleet)

- [ ] 30. Star B1→{B2..B5}, four razidlos: retrieve dials exactly one; unique
      per-broker marker attribute proves the source; others unhit (5.7.1.4).
- [ ] 31. Same star: type query = exact union of four remotes + B1-local
      (5.7.2.4).
- [ ] 32. Cascade B1→B2→B3 (B2 holds a CSR for B3's narrower prefix):
      retrieve via B1 resolves through the chain (4.3.6.4).
- [ ] 33. Loop B1↔B2 with overlapping patterns: Via header terminates the
      cycle, request completes with correct data (4.3.6.3, 6.3.18).
- [ ] 34. `localOnly=true` on B1's CSR → B2 answers from its own data only,
      does NOT cascade to its B3 registration (4.3.6.4, 5.2.34).
- [ ] 35. Overlapping redirect CSRs — two endpoints both match one id: the
      operation is distributed to ALL matching (4.3.6.3).
- [ ] 36. Prefix shadowing: coarse `Typ:Razidlo:.*` CSR→B2 + fine
      `Typ:Razidlo:odpady:.*` CSR→B3 — both match, both consulted, merged
      result correct (5.12, 4.5.5).
- [ ] 37. Matched endpoint down: retrieve returns 404/partial WITH
      `NGSILD-Warning` 199; a non-matching id returns 404 WITHOUT the warning
      (6.3.17; verified live 2026-08-15 — pin it).
- [ ] 38. Registration timeout+cooldown honoured: second request inside the
      cooldown fails fast, mock sees exactly ONE dial (5.2.34).
- [ ] 39. Routing follows the live CSR set: delete the CSR → next retrieve
      404s and the mock records zero new hits (5.9.4, 5.12).

### IOP_EXT_IDR_05 — subscriptions & notifications routed by id

- [ ] 40. Subscription at B1 with `entities:[{type,idPattern}]` overlapping a
      CSR → distributed sub created at B2; create at B2 → notification via B1
      (5.8.1.4).
- [ ] 41. Subscription pattern disjoint from the CSR pattern → NO remote sub
      created at B2 (assert absence via B2's subscription list) (5.8.1.4).
- [ ] 42. Create matching + non-matching ids at B2 → exactly ONE notification;
      the non-matching id absent from every payload (5.8.6, negative).
- [ ] 43. csourceSubscription with EntityInfo idPattern: registering a matching
      CSR notifies, a disjoint CSR does not (5.11.3 — ADR's discovery
      automation).
- [ ] 44. Remote entity deleted → default notificationTrigger excludes
      deletions; with entityDeleted requested the notification fires (5.2.12).
- [ ] 45. Subscription with exact id list: only the listed id notifies across
      the federation; id takes precedence over idPattern (5.2.33, 5.8.1.4).

### IOP_EXT_IDR_06 — ADR-001 URN grammar edge cases on the wire

- [ ] 46. Hierarchy colons in the local segment
      (`...odpady:kontajner:sektor-a:0042`) route through the prefix pattern;
      full ADR validator regex accepted end-to-end (5.12).
- [ ] 47. Crockford-base32 random-suffix ids route identically to natural
      keys (5.12).
- [ ] 48. Unescaped-dot trap: pattern `sk.banskabystrica` (dot = any char)
      ALSO matches `sk_banskabystrica` ids — TP documents why razidlo uses
      `_` and patterns escape metacharacters (5.2.8).
- [ ] 49. Multi-type entity (`["Device","Camera"]`): CSR pattern registered
      with the supertype still matches after the specialization type is
      appended; retrieve via B1 works (4.5.2, 5.12).
- [ ] 50. CSR `tenant` member (4.14): forward to B2 uses the REGISTRATION's
      tenant, the client tenant never propagates; and with
      `operations:["federationOps"]` a write through that CSR must NOT reach
      B2 (4.20 query-op gate, 826afac) — the ADR's read-only agent-tenant
      posture, both halves asserted.

### The /goal prompt — copy-paste to run this campaign

```
/goal Work the "Backlog 2026-08-15 — IOP id/idPattern routing campaign"
checklist in tasks.md top-to-bottom until all 50 boxes are [x] with
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
notes on touched clauses updated. Rules 8 and 9 stay hard; use ponytail;
Mac-side pushes are out of scope — list them at the end. Ask only when
rule 9 leaves you genuinely unsure.
```
