# Antares — complete implementation task list

## Goal

**Tick every untagged box in this file, spec-first, with the ETSI pipeline
green in all four store modes as the proof.**

### The loop for one task

1. **Read the clause first** (§0.2). The spec is the requirement; the Robot
   suite is the oracle that confirms it afterwards. Never the reverse.
2. Implement the full normative behaviour with the smallest diff that holds
   it — reuse before writing, stdlib before dependencies.
3. Unit-test that clause's own rules and edge cases.
4. Run `STORE=<affected mode> STOP_ON_ERROR=1 dev/etsi-pipeline.sh`.
5. In ONE commit: the code, its tests, the `docs/ics.yaml` row, the ticked
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
ETSI Robot suite as confirmation → update `docs/ics.yaml` (§14.6).

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
spec-clause tasks) the `docs/ics.yaml` row updated. Checking a box without
its tests is the one way to make this file lie.

---

## A. The store seam (prerequisite for every mode)

- [ ] A1. Extract the store trait from the v0 in-memory store: an
      `EntityStore`-shaped seam in `antares-sql` (`store/` per §9.3);
      `&TenantId` is the FIRST parameter of every public store method
      (§9.3, §16.1.2).
- [ ] A2. `ANTARES_STORE` → enum `StoreMode { Memory, File, Postgres,
      Timescale }`; unknown value fatal at startup (§14.3);
      `ANTARES_DATABASE_URL` required for postgres/timescale.
- [ ] A3. Wiring in `antares-broker::wiring`: mode → store construction; api,
      matcher, notifier, temporal, registry see only the trait; no core crate
      names a backend (§9.2).
- [ ] A4. Startup log + `/q/health` report the active store mode (§15.1
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

- [ ] B1. `redb` **4.x** (4.1.0 current; the API moved since 2.x —
      `set_durability` returns `Result`, `begin_read` needs
      `ReadableDatabase` in scope) in `[workspace.dependencies]`, pinned;
      `cargo deny` license pass.
- [ ] B1b. Commits run on a blocking task (`spawn_blocking`) — redb's API is
      synchronous and an fsync must never stall a tokio worker thread.
- [ ] B2. One redb table per store kind (`entities`, `subscriptions`,
      `csource_registrations`, `csource_subscriptions`, `jsonld_contexts`,
      `temporal_entities`, `attr_instances`, `entity_maps`), key
      `tenant \0 id`, value = expanded JSON bytes; names per §9.1.
- [ ] B3. Write-through with **commit-before-ack**: redb txn commits BEFORE
      the HTTP response; `Durability::Immediate` (fsync per commit). An
      acknowledged write is a durable write.
- [ ] B4. Boot rebuild: scan tables → rebuild in-memory maps; refuse to start
      on checksum/corruption (never silently serve partial data).
- [ ] B5. `ANTARES_DATA_DIR` (KNOWN_KEYS); default never inside the image;
      startup warning when the path is not a mount point.
- [ ] B6. Compose: `STORE=file` → no DB containers, one named volume per
      broker.
- [ ] B7. Pipeline + CI matrix accept `file`.
- [ ] B8. Tests: table round-trip + key-encoding units; e2e SIGKILL-restart
      (full state survives); e2e kill -9 right after 201 (commit-before-ack
      proven); 350 MiB gate unchanged.
- [ ] B9. Docs: README mode table; ADR: mode ladder, redb-as-durability,
      SQLite rejected.
- [ ] B10. **Reset path** (the Scorpio phantom-state trap, §14.1): the API
      deletes that `dev/reset-broker.sh` issues MUST reach redb, or the next
      suite starts against a file the maps no longer show and every create
      returns a phantom 409 after the first restart. Prove it: run two suites
      back-to-back in `file` mode WITH a broker restart between them.
- [ ] B11. On-disk format version in a `meta` table, checked at boot: refuse
      to start with a clear message (or migrate) on mismatch. A value-shape
      change must never be read as valid data by a newer binary.
- [ ] B12. Backup story, verified not assumed: a live `cp` of an open redb
      file can tear. Establish and document the supported route (copy under a
      read transaction / savepoint, or stop-copy) and test restore. B9's
      README table cites whatever this task concludes, not "just copy it".
- [ ] B13. Group-commit lever (`ponytail:` not default): redb has ONE writer,
      so concurrent request commits serialize behind it and the measured
      3,127 writes/s is a per-process ceiling, not a per-core one. Export
      commit queue depth; only if a benchmark hits the ceiling, batch pending
      writes into one txn (and re-verify commit-before-ack still holds for
      every batched write).

## C. `postgres` mode — the phase-1 real store (§3, §6.2, §8)

### C-i. Foundation

- [ ] C1. `sqlx` 0.9 (postgres, runtime-tokio, tls-rustls, json, chrono,
      uuid); ONE shared pool ≈2×PG cores (§6.2); never per-tenant pools
      (§14.2).
- [ ] C2. Embedded `sqlx migrate` run at start: `tenants`, `entities` (§8.1
      exact shape: `version`, extracted `types/scopes/location`,
      tenant-leading PK), `subscriptions` (+bookkeeping columns),
      `csource_registrations` + `csource_index` (ops bitmask §8.3),
      `csource_subscriptions`, `jsonld_contexts`, `entity_maps`, `outbox`;
      timescale-only DDL guarded (§8.2).
- [ ] C3. Indexes exactly §8.1: btree_gin `(tenant_id, types)`, GIST
      `location`, GIN `entity jsonb_path_ops` (no kitchen-sink GIN),
      `(tenant_id, modified_at DESC)`, GIN `scopes`; autovacuum tuning +
      lowered fillfactor on `entities` (§3.1.3 bloat note).
- [ ] C4. Tenancy (§3): RLS policy on every table; `SET LOCAL antares.tenant`
      helper (transaction-scoped ONLY, §3.1.5); SQL always also filters
      `tenant_id = $1`; tenant auto-create = `INSERT … ON CONFLICT DO
      NOTHING` (§3.1.4).

### C-ii. Store implementations (`antares-sql/src/store/`, §9.3)

- [ ] C5. `entity.rs`: create/replace/merge/append/partial/delete + UNNEST
      batches; all semantics in Rust (no PL/pgSQL, no triggers — §4);
      read-modify-write under `SELECT … FOR UPDATE`, `version` bumped under
      the row lock (§3.1.2–3).
- [ ] C6. `subscription.rs`: CRUD + status columns as real columns (§8.3;
      rows are truth §14.1); preserve the send-time bookkeeping ordering
      (046_12_01 fix).
- [ ] C7. `registration.rs`: CSR store + `csource_index` maintenance in Rust;
      expiry filtered at the single mirror yield point (§4.1 L4).
- [ ] C8. `context.rs` (only cross-tenant table §8.3), `entity_map.rs`
      (+TTL sweep, B1 regression), `outbox.rs` (same-tx INSERT §10),
      `tenant.rs`.
- [ ] C9. Plain-mode temporal: `temporal_entities` + `attr_instances`,
      native `PARTITION BY RANGE (observed_at)`, partition/retention jobs
      claimed via `SELECT … FOR UPDATE SKIP LOCKED` (§3.1.6, §8.2).

### C-iii. Query compilation (`antares-sql/src/compile/`)

- [ ] C10. `q=` AST → `jsonb_path_query`/`jsonb_path_exists` (Scorpio's
      proven strategy §8.1); jsonpath BOUND as a parameter, never spliced
      (§16.2).
- [ ] C11. geo (`ST_DWithin`/`ST_Within`/`ST_Intersects`, geozero
      GeoJSON→EWKB binds §6.5), scopeQ, projection (attrs/pick/omit), entity
      ordering (4.23), temporal ranges + 206/`Content-Range` bounds (U3),
      pagination (limit/offset; keyset for temporal §8.2).
- [ ] C12. Guards: CI grep denies `format!` into `sqlx::query` outside the
      compiler allowlist (§16.2); cargo-fuzz targets on q/scopeQ/geoQ
      parsers in scheduled CI.

### C-iv. Cutover & validation

- [ ] C13. All consumers on the trait; memory/file still green after the
      seam refactor (regression gate before SQL lands).
- [ ] C14. Integration: `sqlx::test` + testcontainers (PostGIS) per store;
      RLS cross-tenant denial tests (§16.1.3); §3.1 concurrency suite
      (parallel PATCH storm, no lost updates, version monotone).
- [ ] C15. ETSI: `STORE=postgres` full pipeline green, 350 MiB gate held.

## D. `timescale` mode — TemporalStore second impl (§8.2)

- [ ] D1. `TemporalStore` trait, exactly two impls (`timescale.rs`,
      `plain.rs`); identical table shape and queries — modes differ only in
      DDL bootstrap + maintenance jobs.
- [ ] D2. Timescale DDL: hypertable (7-day chunks), compression
      (`segmentby=tenant_id,attr_id`, `orderby=entity_id,observed_at DESC`),
      retention policy.
- [ ] D3. Detect via `pg_extension`; explicit error when absent — never
      silently fall back (§8.2).
- [ ] D4. Plain-mode parity jobs in CI; `cargo deny`: nothing links TSL
      (§15.4).
- [ ] D5. Store integration tests against BOTH temporal modes (§9.5 matrix).
- [ ] D6. ETSI: `STORE=timescale` green incl. temporal TPs; both modes in CI
      forever after (§5.3).

## E. Pipeline / CI closure for the store ladder

- [ ] E1. `dev/etsi-pipeline.sh`: all four `STORE` values (`file` mounts the
      data volume; postgres/timescale keep the `db` profile:
      `postgis/postgis:17-3.5` vs `timescale/timescaledb-ha:pg17`).
- [ ] E2. CI matrix `[memory, file, postgres, timescale]`; publish requires
      ALL green; per-mode run-summary (suites + CPU/RSS + image size).
      postgres/timescale columns `continue-on-error` until C15/D6, then
      gating.
- [ ] E3. README: store-mode table; `file` RAM ceiling (~100k entities →
      move up a rung).
- [ ] E4. ADRs: mode ladder; redb-as-durability; SQLite rejected.
- [ ] E5. `docs/ics.yaml` per clause as C/D land; `mempalace mine` after
      each phase.
- [ ] E6. Create `error.md` (mandated by the ETSI testing guide, absent from
      the repo): the log of ETSI *tool* bugs, so a broken TP is never "fixed"
      by hacking the broker. Seed it with the known ones (QueryEntities
      04_01/04_02 create two payloads with the same id → 409 by the suite's
      own doing).
- [ ] E8. ⛔ NEEDS-YOU (policy decision, one line): the CI `publish` job
      pushes `:latest` + `:v<run>` to GHCR automatically on every green master
      run. Decide: keep it automatic, or gate it behind a GitHub Environment
      with required approval. Until you decide it stays automatic.
- [ ] E7. `dev/etsi-run.sh` seds `resources/variables.py` inside the
      submodule in place, which leaves `ngsi-ld-test-suite` permanently
      dirty in `git status`. Restore it on exit (trap) so a dirty submodule
      means a real change, not a leftover.

## F. Messaging & scale-out — phase 2 eventing (§6.4, §7, §10)

v0 runs `LocalBus` in one process. Scale-out needs the real spine.

- [ ] F1. `antares-bus` NATS impl: `ANTARES_CHANGES` stream (Interest
      retention), subjects `changes.{tenant}.{type_hash}.{id_hash}` (§7);
      pull consumers only; explicit ack AFTER processing; bounded prefetch
      (§6.4).
- [ ] F2. `ChangeEvent` finalized: op enum, `changed_attrs`, `payload` +
      `prev_payload` (load-bearing §7), `version`, `(incarnation, version)`
      ordering key + entityDeleted fence (§3.1.3); claim-check refs >256 KB
      instead of chunking (§7).
- [ ] F3. Transactional outbox drain (§10): publish from the outbox table
      with `Nats-Msg-Id` = row id dedup; never fire-and-forget after commit.
- [ ] F4. JetStream KV subscription mirror: every instance `watch()`es,
      compiled-subscription map in `antares-matcher`; revisions as CAS;
      Postgres stays the system of record (§6.4). No SUB_ALIVE/SUB_SYNC
      equivalent — ever (§7, §14.1).
- [ ] F5. Registration mirror over `ANTARES_REGISTRY` stream: ONE compiled
      mirror per process, delta-applied, expiry filtered at the single yield
      point (§16.7).
- [ ] F6. `--roles api,matcher,notifier,temporal,registry` actually split
      consumers (§9); `bus=local` asserts single-process-all-roles at
      startup (§9.2).
- [ ] F7. Topology assertions at startup: broadcast (ephemeral per-instance)
      vs balanced (shared durable) explicit per consumer (§6.4, R10 lesson).
- [ ] F8. Temporal auto-recording moves to the durable consumer
      (`antares-temporal::recorder`), idempotent upserts on
      `(tenant, entity, attr, observed_at)` (§6.4).
- [ ] F9. Bus integration tests (testcontainers NATS): 2-consumer
      broadcast-vs-balanced assertion, claim-check, dedup (§9.5); e2e
      2-instance sync + out-of-order publish injection (version-LWW holds,
      §3.1 test hooks).
- [ ] F10. Compose/pipeline: optional NATS profile for multi-role runs;
      2-instance e2e in CI.

## G. MQTT notification binding — clause 7 (feature `mqtt`, §5.4.10)

Currently the suite runs `--exclude '*mqtt*'` — these TPs are open.

- [ ] G1. `rumqttc` sink in `antares-notifier` behind feature `mqtt`
      (default on): `mqtt(s)://host[:port]/topic` endpoint URIs,
      `notifierInfo` (`MQTT-Version`, `MQTT-QoS`), payload =
      `{metadata, body}` wrapper (7.1/7.2).
- [ ] G2. Bounded client pool per endpoint host WITH eviction (audit L5);
      timeouts at construction (U1).
- [ ] G3. `NotificationSink` registry: subscription naming a scheme with no
      registered sink → 422 `OperationNotSupported` (§9.2).
- [ ] G4. emqx (or mosquitto) joins the ETSI compose; drop the `*mqtt*`
      exclusion; MQTT TPs (058_xx) green in all store modes.

## H. Remaining spec surface (v1.x ledger items, §5.4)

The suite is green, but the ledger has normative surface the TPs don't cover;
audit each against `docs/ics.yaml` and close the gaps.

- [ ] H0. Create `docs/ics.yaml` — it does not exist yet, though §14.6
      mandates it and E5/H1 both write to it. CIM 029 shape, one row per
      clause, rendered by CI.
- [ ] H1. ICS audit: walk §5.4.1–5.4.10 checkbox by checkbox against the v0
      code; record implemented/partial/missing in `docs/ics.yaml` — the
      suite's 686 TPs cover only part of the normative surface (§0.2).
- [ ] H2. Snapshots (5.16, 6.36–6.38, 5.2.41/42, 5.3.4) — staged v1.x;
      pre-adopt `202 Accepted` on creation (§15.1). Implementable from the
      clause, but ⏳ has NO validation oracle: the Robot suite has no snapshot
      TPs yet, so unit tests are the only proof until ETSI ships them.
- [ ] H3. 2.0 cheap pre-adoptions (§15.1): HTTP `HEAD`/`OPTIONS`,
      `GET .../attrs/{attrId}` + `/value` endpoints, `508 Loop Detected`,
      schema readiness for `attributeNames` merge (§8.3 note).
- [ ] H4. `Prefer: ngsi-ld=<version>` / `Preference-Applied` / 203 +
      per-subscription `ngsildConformance` (6.3.21, §15.1) — verify complete.
- [ ] H5. Tolerant-reader audit: unknown members stored and echoed on
      Subscription/Registration/EntityMap (§15.1).
- [ ] H6. @context management completeness: kinds Hosted/Cached/
      ImplicitlyCreated, delete-and-reload (5.13), `Expires`/`Cache-Control`
      honoured (6.3.16), SSRF policy hook in the loader (§16.4).

## I. Security hardening (§16) — requirements with tests, not guidelines

- [ ] I1. Authn tower layer: `none | oidc-bearer | mtls`, config-selected
      (§16 posture); authz stays the PEP's job.
- [ ] I2. Input bounds wall: body size 413, JSON depth ≤64, batch count,
      URI+param length, @context chain/fetch caps, `joinLevel`, AST
      depth/size → 403 `TooComplexQuery`, result ceilings → 403
      `TooManyResults`; all observable via metrics (§16.3).
- [ ] I3. Rate limiting layer: global + per-IP v1; per-tenant hooks (§16.3).
- [ ] I4. `EgressPolicy` for ALL outbound (notifications, @context fetches,
      federation forwards): scheme allowlist, private-range deny-by-default
      with `egress.allow_private` switch (ETSI stacks need it), redirect
      cap, DNS-pinned re-resolution, response-size caps, per-destination
      circuit breakers (§16.4, §16.7).
- [ ] I5. Tenant isolation test pack: RLS denial per store, cross-tenant
      404-indistinguishability (no existence oracle, §16.1.6), tenant-keyed
      in-memory structures audit (§16.1.4), NATS subject re-verification
      (§16.1.5).
- [ ] I6. Supply chain: distroless non-root read-only rootfs (done —
      verify), SBOM via cargo-auditable in release builds, `cargo deny`
      advisories in CI, `unsafe_code = "forbid"` (+ reviewed sonic
      exception), no global TLS-verify-off switch anywhere (§16.5).
- [ ] I7. Security regression suite: cache caps asserted, bookkeeping delete
      paths tested (L6), size-check-before-parse on HTTP bodies (WS-44
      class), cross-tenant probes in e2e per release (§16.6).

## J. Performance & the phase-0 spike debts (§12, §13)

- [ ] J1. `json-ld` crate benchmark (risk #1): ≥5k expansions/s/core or
      fork/hand-roll; core-context fast path measured (§6.3).
- [ ] J2. @context parsed-LRU (moka): size cap ~256, core context pinned,
      `Cache-Control`/`Expires` TTLs, Postgres write-through (§6.3, J1b
      lesson).
- [ ] J3. Bounded JSON-LD concurrency semaphore (§6.3.3).
- [ ] J4. 🖥 NEEDS-HARDWARE (16 GB Postgres box; not a free CI runner).
      10M-row synthetic benchmark (xtask seeder):
      q=/geo/type query p95s; decide the extracted-attribute side table
      (named lever, §8.1) on numbers, not vibes.
- [ ] J5. Streaming list endpoints: axum streaming bodies + sqlx `fetch()`
      row streams (J3/J11c lesson).
- [ ] J6. `sonic-rs` feature on the batch-ingest path, serde_json fallback
      compiled always (§6.1).
- [ ] J7. jemalloc + decay tuning + heap-stats metrics export (§6.1);
      RSS ≈ live×1.2 target (§2.1).

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

- [ ] K1. Graceful shutdown drain order (§9.3 `shutdown.rs`): stop accepting
      new HTTP connections → let in-flight requests finish (bounded deadline)
      → stop bus consumers → flush the outbox → close pools. Health endpoint
      flips to "draining" FIRST so the LB stops routing before the socket
      closes; SIGTERM triggers it, and the container `stopGracePeriod` must
      exceed the deadline or the orchestrator turns a drain into a kill.
- [ ] K2. `ha` compose profile: an LB (nginx/haproxy/caddy) on 9090 fronting
      ≥2 api instances on private ports, health-checked with fast ejection.
      Required because the ETSI compose runs `network_mode: host` with fixed
      ports — two brokers cannot bind 9090, so there is nothing to roll in
      place without the proxy. Profile-gated: the normal ETSI stack is
      unchanged.
- [ ] K3. `dev/rolling-update.sh`: swap instances one at a time (mark
      draining → wait for in-flight to clear → SIGTERM → start new image →
      wait `/q/health` → next), using the image the pipeline just built. Runs
      locally and in CI, same script (the §E pipeline rule).
- [ ] K4. N≥2 api pods behind the LB validated in e2e; matcher/notifier as
      shared-durable consumer groups scale and roll independently (§10).
- [ ] K5. Reference manifests (compose + K8s): authored and lint/kind-tested
      in CI; 🖥 a REAL cluster is yours to point at. 3-node JetStream R3, Postgres
      primary/replica (CloudNativePG or Patroni), memory limits in EVERY
      manifest (R1/R2 lesson), `strategy: Recreate` hard-coded for `file`
      mode and `RollingUpdate` for `postgres`/`timescale`.

### K-ii. Prove it (the drills)

- [ ] K6. Continuity harness, shared by every drill below: a writer loop
      posting monotonically-numbered entities + a recording notification
      receiver. Assertions: zero connection errors, zero 5xx, zero lost
      writes (every acked id present at the end), and notifications
      at-least-once (duplicates fine, losses not).
- [ ] K7. **SIGTERM drill (graceful):** roll all instances under K6 load.
      Expected: zero failed requests. This is the only real test of K1 —
      a drain bug shows up here and nowhere else.
- [ ] K8. **ETSI-during-roll job** (postgres mode): run the full suite through
      the K2 LB while K3 rolls the brokers underneath. The suite has no
      retries and asserts exact single responses, which makes it a brutally
      strict drain client — so this is a STRICT gate, not a soak signal: any
      failure is a real K1 bug, not flake. Expected result: 1025/1025.
- [ ] K9. **SIGKILL drill (ungraceful):** `kill -9` mid-write, per mode.
      Expected: `file` → every acked write present after restart
      (commit-before-ack, §B3/B8); `postgres` → a change committed but not yet
      published is republished from the outbox on restart (§F3); `memory` →
      documented total loss, asserted so the mode's limits stay honest.
- [ ] K10. `file`-mode restart drill: assert the exclusive-lock error
      (`Database already open. Cannot acquire lock.`) IS the expected
      behaviour on double-start — never silent corruption, never two writers —
      and gate the restart gap (<2 s at 100k entities; rebuild measured at
      257k entities/s, so the gap is process start).
- [ ] K11. 🖥 NEEDS-HARDWARE for the real drill (kind/compose covers the
      rehearsal). Postgres primary kill (replica promotion,
      broker reconnects without dropping acked writes), NATS node kill (R3
      stream survives, consumers resume, at-least-once holds), broker pod kill
      between commit and publish (outbox covers it) (§10, §13 phase 4).

### K-iii. Operate it

- [ ] K12. Observability: tracing + OTLP + Prometheus metrics with the
      `antares_` prefix and unit suffixes (§9.1), change-lag and notification
      metrics, drain/roll state exported so a roll is visible on a dashboard,
      tokio-console in dev.

## L. Scale validation — phase 4 exit (§13)

- [ ] L1. 🖥 NEEDS-HARDWARE (you provide the box + 24 h of it).
      `xtask load-rig`: 10M entities / 1,000 tenants seeded; 24 h soak;
      all §1 targets measured and held (broker <500 MB, PG <16 GB).
- [ ] L2. 🖥 NEEDS-HARDWARE (1,000 mock sources exceeds a CI runner).
      Federation rig: 1 tenant × 1,000 CSRs (20 % expired, mixed modes)
      vs mock sources with injected latency/failures; p95 bounded by the
      aggregate deadline, 207 correctness, mirror memory in budget (§16.7).
- [ ] L3. 10k active subscriptions matching load: index-shaped candidate
      lookup verified O(log n), never scan-all (§1.1).
- [ ] L4. RLS pen-test with cross-tenant probes as phase-4 exit criterion
      (§13, §16.1).

## M. Ecosystem & publishing (§9.1, §6.6)

- [ ] M1. ⛔ NEEDS-YOU. Publish `ngsild-model` + `ngsild-ql` to crates.io;
      reserve the `ngsild` facade name (§9.1 — checked free 2026-08-03).
      Needs your crates.io token, and a published name cannot be taken back.
      An agent may prepare the crates (metadata, README, `cargo package`
      dry-run, `--dry-run` publish) and then stop.
- [ ] M2. ADR backfill: tenancy, bus choice, WS deferral, store ladder —
      one file per irreversible decision (§9.5 doc rule).
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

- [ ] N1. `crates/antares-wasm`: `wasm-bindgen` entry that builds `AppState`
      with the memory store + `LocalBus` and exposes
      `handle(request) -> response` by calling `tower::Service::call` on the
      axum router directly (v0's `main.rs` already drives the router this way,
      the listener is the only thing dropped).
- [ ] N2. Portability audit of the core crates for `wasm32`: clock
      (`Instant`/`SystemTime` → js time), `uuid`/`getrandom` `js` features,
      `reqwest` wasm backend for outbound notifications, `tokio` reduced to
      `rt + sync + macros` (no net/fs/threads), timers via `gloo`; anything
      else gated `#[cfg(not(target_arch = "wasm32"))]`. No core-crate logic
      may change to accommodate the target.
- [ ] N3. Service Worker glue: intercept `fetch` on a virtual
      `/ngsi-ld/v1/*` origin path → `http::Request` → module → `Response`,
      so page JS talks to what looks like a normal broker URL. Plus a direct
      in-page API (`await broker.fetch(...)`) for pages that skip the SW.
- [ ] N4. Browser persistence. **Verified 2026-08-04:** redb 4.1.0 DOES
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
- [ ] N5. Build + budget: `wasm-pack` + `wasm-opt -Oz`; ≤8 MB raw, ≤3 MB
      compressed; size printed in the CI summary next to the image size.
- [ ] N6. Page builds in-repo and is CI-tested; ⛔ NEEDS-YOU only to publish
      it (your docs site / domain). Create entities, subscribe, watch
      notifications fire in-page. Zero install, no server. No other NGSI-LD
      broker can do this (JVM brokers can't; Orion-LD needs Mongo).
- [ ] N7. ETSI conformance for the WASM artifact, in three tiers:
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
- [ ] N4b. OPFS has the same single-writer property as the native file
      (§K10): a sync access handle is exclusive per file, so a SECOND TAB on
      the same origin cannot open the store. Decide the owner story
      (SharedWorker owning the handle and serving other tabs, or leader
      election with a clear "another tab owns this store" error) before N6 —
      a demo page that dies on the user's second tab is worse than no demo.
- [ ] N8. Docs/ADR: browser build is memory (or OPFS-file) only — no NATS,
      no MQTT, no Postgres, no roles; state which suites gate it and which
      are structurally out of reach.

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
