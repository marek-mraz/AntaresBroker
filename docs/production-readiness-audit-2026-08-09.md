# Antares — Production-Readiness Audit, 2026-08-09

Independent multi-agent audit of the tree at `ead650c`, run against the design
contract in `claude.md`, the conformance ledger in `docs/ics.yaml`, and the task
ledger in `tasks.md`. Eight agents covered API/HTTP binding, storage+SQL,
subscriptions+notifications, temporal, federation, JSON-LD/QL/model,
ops+security+deployment, and ledger integrity. Every finding below carries a
`file:line`; the ones marked **[verified]** were re-checked by hand against the
source or reproduced by running the binary.

## Baseline

| Signal | Result |
|---|---|
| `cargo clippy --workspace --all-targets` | clean, 0 warnings |
| `cargo test --workspace` | 170 passed, 0 failed, 2 ignored |
| Tests vs code | 172 tests over 29,601 LOC (15.6% test lines) |
| ETSI suite | real and gating on every push — but 3 store modes, not 4 |

**The green test run is weaker than it looks.** 31 of 172 tests `return` early
when `ANTARES_TEST_DATABASE_URL` is unset and still report `ok` — including all
four RLS cross-tenant-denial tests and both outbox tests
(`crates/antares-sql/tests/pg_entity.rs:9-19`). CI does supply the services, so
this is a local-signal hazard rather than a CI hole, but `cargo test` on a bare
box proves nothing about tenant isolation or durability. **[verified]**

Two benchmarks that gate the design's named risks #1 and #2 are `#[ignore]` and
no workflow passes `--ignored`: `crates/antares-api/tests/sub_index.rs:190` and
`crates/antares-jsonld/src/expand.rs:764`.

---

## P0 — must fix before any real traffic

### 1. The shipped Kubernetes manifests cannot boot **[verified — reproduced]**
`crates/antares-broker/src/main.rs:76` treats any unknown `ANTARES_*` key as
fatal, exempting only `ANTARES_TEST_*`. `deploy/k8s/broker-postgres.yaml:13`
creates a Service named `antares`, so kubelet injects `ANTARES_PORT`,
`ANTARES_SERVICE_HOST`, `ANTARES_SERVICE_PORT` into every pod in the namespace.

```
$ ANTARES_PORT=tcp://10.96.0.1:9090 ./target/release/antares
Error: "unknown config key ANTARES_PORT (known: [...])"
```

`kubectl apply -f deploy/k8s/` → 100% CrashLoopBackOff. Fix: `enableServiceLinks:
false` on both pod specs *and* skip the injected shapes in the key check.

### 2. `DELETE /entities?idPattern=.*` wipes the tenant **[verified]**
`crates/antares-api/src/entities.rs:1284` includes `"id"` and `"idPattern"` in
Purge's sufficient-filter set. `entities.rs:719` correctly excludes them for
Query, and CIM 009 5.6.21.4 enumerates exactly type/attrs/q/geoquery/local —
"If none of the above is provided, then an error of type BadRequestData shall be
raised (too wide query)". Since authn is out of core, any caller a PEP permits to
purge at all can purge everything. Fix: drop the two keys from that list.

### 3. Readiness is a liveness check **[verified]**
`crates/antares-api/src/lib.rs:354-366` reads only `state.draining` — no pool
ping, no bus-connected flag. On a Postgres failover every pod stays `Ready`, the
Service keeps routing, every request 500s, and the matcher consumer stops
silently. Fix: split `/q/health` (liveness) from `/q/ready` (pool acquire + bus
connected + mirrors hydrated) and repoint `readinessProbe`.

### 4. `--roles` does not gate the HTTP API **[verified]**
`crates/antares-broker/src/main.rs:404` builds the router unconditionally.
`roles.` appears exactly three times in the whole crate (`main.rs:259`,
`wiring.rs:142`, `wiring.rs:259`); `roles.temporal` and `roles.registry` are
parsed and never read. The shipped `antares-worker` Deployment
(`deploy/k8s/broker-postgres.yaml:132`, commented "health only; no Service
routes here") is a full read/write broker on its pod IP. Worse: with
`roles.api=false` those pods have no KV sync hooks, so a subscription created
against a worker lands in Postgres and no other pod's mirror ever learns of it.

### 5. `q=` silently returns wrong answers for ValueList and Range **[verified]**
`crates/antares-ql/src/lib.rs:37-41` — `QValue` is `Str|Num|Bool` only; no
`List`, no `Range`. `value()` at `:185-216` terminates on `;|()` only, so `,` and
`..` are not grammar. `q=temperature==10..20` parses to `Str("10..20")` and
matches nothing: **200 OK, empty array**, on every store mode. Same for
`q=temperature==10,20,30` (4.9 ValueList). A dashboard filtering by range shows
zero devices with no error anywhere. Fix: implement the 4.9 ABNF literals; until
then *reject* unquoted literals containing `,` or `..` with `BadRequestData` — a
400 is honest, a wrong 200 is not.

### 6. The outbox deletes events that were never published **[verified]**
`crates/antares-sql/src/store/outbox.rs:80` — `DELETE FROM outbox WHERE seq <=
$1`. `bigserial` allocates at INSERT, commits land out of order: tx A takes
seq=100 and is still open, tx B takes 101 and commits, the drain peeks (sees 101
only), publishes, then acks 101 — deleting row 100 the moment A commits. The
notification and the temporal record for A are lost silently, only under write
concurrency. Found independently by two agents. Fix: `DELETE … WHERE seq =
ANY($1)` over the exact published seqs, or claim with `FOR UPDATE SKIP LOCKED`.

### 7. Throttling is a complete no-op
`crates/antares-api/src/notify.rs:997` reads `notification.lastNotification` from
the `SubMirror`, but the bookkeeping writeback at `:1550-1572` goes through
`store.mutate` and never syncs the mirror — so the mirror copy never contains
it. Verified empirically by the agent: `throttling: 15` with three distinct
updates 1 s apart produced **3 notifications**. ETSI TP 046_15 passes vacuously
because it re-PATCHes an identical fragment, so no notification is due anyway.

### 8. A subscription with an offset-bearing past `expiresAt` never expires **[verified]**
`crates/antares-api/src/subscriptions.rs:259` and
`crates/antares-api/src/notify.rs:507-509` both compare ISO-8601 strings
lexicographically against a `…Z` UTC string, while `parse_datetime`
(`crates/antares-jsonld/src/expand.rs:612-634`) accepts `+HH:MM` offsets.
`expiresAt: "2026-08-09T18:00:00+14:00"` (= 04:00Z, in the past) is accepted with
201 instead of 400 and reports `status: "active"` forever. This is the exact
Scorpio L4a defect `claude.md` §4.1 exists to design away.

### 9. One malformed `expiresAt` is a tenant-wide temporal DoS **[verified]**
`crates/antares-sql/src/store/pg_temporal.rs:312,441` inline
`(m.meta->>'expiresAt')::timestamptz`. `parse_datetime` only checks *digit
shape*, so `"2026-13-45T00:00:00Z"` passes validation and is stored uncast (the
entity path binds `$7::timestamptz` and fails at write time; the temporal path
does not). From then on every temporal query in that tenant raises `date/time
field value out of range` → 500. One request, unauthenticated.

### 10. Federated fan-out is serial, unbounded, and has no aggregate deadline **[verified]**
`crates/antares-api/src/federation.rs:480,588,865` iterate matching registrations
with `for … .await`. There are no concurrency primitives in the file at all — no
`FuturesUnordered`, no `Semaphore`, no `join_all`, no `tokio::spawn`. With an 8 s
per-source timeout, 50 blackholed sources = 400 s before the first response byte;
at the §16.7 target of 1,000 CSRs it is hours. `crates/antares-registry/src/lib.rs:7`
writes the contract down — "fan-out is bounded (semaphore + per-source timeout +
aggregate deadline)" — in a 16-line stub that implements none of it.

---

## P1 — silently wrong results

| Finding | Location |
|---|---|
| `orderBy` + projection pushdown: `pick=name&orderBy=speed` strips the sort key before sorting, then returns arbitrary order claiming to be sorted | `entities.rs:749` (`push_proj` omits the `orderBy` guard) |
| `!=` returns *no match* on datatype mismatch; 4.9 says differing datatypes are "considered unequal" so it must match. Both engines are consistently wrong, so a parity test alone would not catch it | `qeval.rs:94-121` |
| DateTime compared lexicographically everywhere: `.500Z` sorts before `Z` (`'.'`=0x2E < `'Z'`=0x5A), so `timerel=after` silently excludes fractional-second instances and `lastN` returns the wrong N | `temporal.rs:222-235`, `compile/temporal.rs:39`, `pg_temporal.rs:173` |
| `POST /temporal/entityOperations/query` drops `entities[].id`, `idPattern`, `geoQ`, aggregation params — an id-scoped body returns every entity of that type | `temporal.rs:1611-1636` |
| `aggregatedValues` handles only numeric `value`: booleans, strings, arrays and Relationships all wrong; empty periods dropped; `Whole` period wrong; timezone not converted before formatting `Z` | `temporal.rs:801-922` vs 4.5.19.1 |
| `aggrPeriodDuration` rejects the spec's own example `P3Y6M4DT12H30M5S` | `temporal.rs:655-660` |
| 5.6.11 temporal upsert *replaces* instances matched on `(datasetId, observedAt)` instead of adding them, and drops new entity types | `temporal.rs:85-128` |
| `temporal_entities` types/scopes/`modifiedAt` frozen at first touch (`ON CONFLICT DO NOTHING`) — an entity that gains a type is invisible to type-filtered temporal queries forever | `pg_temporal.rs:244-258` |
| Deleting an entity destroys its entire temporal evolution; 5.6.16 exists as a separate operation and `deletedAt` is never written | `entities.rs:110-117` |
| `format=simplified` emits bare values for LanguageProperty/JsonProperty/VocabProperty instead of the required `{"languageMap":…}` / `{"json":…}` / `{"vocab":…}` wrapper | `repr.rs:387-407` vs 4.5.4 |
| `expiresAt`/`deletedAt` emitted without `options=sysAttrs`; a client round-tripping a GET into a PUT re-applies an expiry it never set | `repr.rs:111-119,210-215,313-318` vs 6.3.11 |
| Accept precedence follows header order, not the spec's fixed json > ld+json > geo+json list | `negotiate.rs:144-150` vs 6.3.4 |
| `type=*` matches nothing instead of everything, silently | `entities.rs:889-900` |
| Batch write forwarding builds a *union* spec, so a CSR matching one item receives all items; `batch_delete` forwards the entire id array to every matching source | `batch.rs:216-249,654` |
| `entityUpdated` does not imply `attributeDeleted` tombstones in the payload | `notify.rs:1000-1008` vs Table 5.2.12-1 |
| Batch upsert with duplicate ids: memory keeps the last doc and returns `[true,false]`; Postgres keeps the first and returns `[true,true]` | `any.rs:563-566` vs `pg_entity.rs:756-768` |
| Expiry boundary differs by mode: Rust `exp < now` (broker clock) vs SQL `expires_at > now()` (DB clock) | `filter.rs:127` vs `pg_entity.rs:250,317` |
| `/info/sourceIdentity` emits `hostAlias`/`uptime`; the broker's own pinned core context defines `contextSourceAlias`, `contextSourceUptime`, `contextSourceTimeAt`, `contextSourceExtras`. `hostAlias` is not a core term so it does not expand to an NGSI-LD IRI. Zero ETSI TPs, zero Rust tests **[verified]** | `lib.rs:398-403` vs `contexts/core-v1.9.jsonld:74-84` |
| Dead notification sources silently dropped from federated reads — `if !(200..300).contains(&status) { continue }`, client gets 200 with partial data | `federation.rs:547,639` |
| `urn:ngsi-ld:null` accepted as real data on all four provisioning paths 5.5.4 forbids; a later merge-patch then deletes an attribute nobody asked to delete | `expand.rs:344-353,364-387,388-401` |

---

## P2 — will not survive the stated scale

**Unbounded materialization** is the recurring shape; every path below pulls the
whole match set into broker RAM against a 500 MB budget:

- `GET /types`, `/types/{id}`, `/attributes`, `/attributes/{id}` load the tenant's
  entire entity set — `types_attrs.rs:27,67`. One unauthenticated request. **[verified]**
- Any `scopeQ`, any `georel`, any `q=` shape the compiler declines (dotted paths,
  `~=`, string ordering) forces `decided=false` → `SELECT … ORDER BY id` with **no
  LIMIT** — `pg_entity.rs:419-435`, `compile/q.rs:147,162,177`.
- `count=true&limit=0`, `orderBy`, `idPattern`, or *one* matching registration each
  disable pagination pushdown — `entities.rs:744-758`. **[verified]**
- Purge fetches every match then issues one transaction per entity; the `limit`
  param is allowlisted and ignored — `entities.rs:1300-1332`.
- Temporal reads never truncate: `TEMPORAL_INSTANCE_LIMIT = 9` is used only in two
  guards, never to cut the payload, yet the response is labelled **206 Partial
  Content** — `temporal.rs:249,333,401`. **[verified]**
- Forwarded responses are `resp.json::<Value>()` with no size cap, and forwarded
  queries carry no `limit`, `q`, `geoQ` — `federation.rs:376,611-625`.
- `Prefer: ngsi-ld=…` buffers the whole response with `to_bytes(body, usize::MAX)`,
  defeating the streaming path — `conformance.rs:256`.

**Scan-shaped work that should be index-shaped:**

- `csource_index` — the table, its three indexes and its ops bitmask are written
  on every registration upsert and **never read outside a test**. Federation
  matching does `store.list(tenant, Kind::Registration)` and filters in Rust,
  deep-cloning every registration `Value`. **[verified]**
- `interval_tick` runs every 500 ms and does `subscription_tenants()` then a full
  `list(Kind::Subscription)` per tenant, then a full entity-table load per due
  sub — `notify.rs:1073-1121`. At 1,000 tenants that is ~2,000 queries/s at idle
  whether or not any interval subscription exists.
- Nothing is precompiled: regex, `q` AST and geometry are rebuilt per event per
  candidate — `notify.rs:513-585`, `qeval.rs:117`. The `CompiledSubscription` the
  design specifies does not exist. (Candidate *lookup* is genuinely index-shaped
  and well tested — `tests/sub_index.rs` — so this is the next ceiling, not the
  current one.)
- Expired attribute *instances* are pruned only in memory/file; postgres and
  timescale grow forever — `maintenance.rs:105` vs `store.rs:436-443`.

**Runtime and pool:**

- Every Postgres call is sync-over-async via `block_in_place` + `block_on` —
  `pg_entity.rs:47-59`, ADR-0005, which calls it "the compatibility layer, not the
  destination". It is now the throughput ceiling.
- Pool size is the literal `20` at `main.rs:150`; there is no `acquire_timeout`,
  `idle_timeout`, `max_lifetime`, or session `statement_timeout` anywhere. §2.2
  says "deployments size it via config" — there is no config key.
- The accept loop has no connection cap, no `header_read_timeout`, no request
  timeout — `main.rs:457-476`. Slowloris pins tasks and fds until OOM.
- `ANTARES_DRAIN_DELAY_MS` defaults to 500 ms while the manifests need ~3–5 s for
  kube-proxy propagation — every rolling restart drops connections. The drain also
  counts *connections*, not requests, so idle keep-alive clients force the full
  20 s deadline — `shutdown.rs:30,76`.
- Two `bus=local` replicas on one Postgres double-fire every notification and
  every interval subscription; the only guard checks `roles.all()` — `main.rs:255-266`.

**Security bounds:**

- Prometheus label cardinality is unbounded on the client-controlled HTTP method —
  `lib.rs:314,328`. Reproduced by the agent: `antares_http_requests_total{method="BAZQUX"}`.
- Raw sqlx error text reaches clients in the 500 body — `any.rs:23-25`.
- The hosted `@context` URL is minted from the client-controlled `Host` header and
  persisted into cross-tenant shared state — `contexts.rs:20-26`.
- Circuit breakers are keyed by host without tenant, so one tenant can trip
  another's peer, and the map is unbounded — `egress.rs:64-76`.
- `usage: RwLock<HashMap<String, CtxUsage>>` is unbounded and keyed by
  client-supplied URLs, directly beneath a comment asserting it is bounded, while
  its three siblings use `BoundedCache` — `loader.rs:399`. **[verified]**
- 15 lock acquisitions use `.expect(…)`; one panic in a critical section poisons
  the lock and every later request panics. `Cargo.toml:124` sets
  `clippy::unwrap_used = "warn"` but **not** `expect_used`, so none of them are
  linted — contradicting §9.5. **[verified]**
- `ip_is_private` misses IPv4-mapped IPv6, so `http://[::ffff:169.254.169.254]`
  bypasses the lockdown switch — `loader.rs:113-119`.
- DSNs are plaintext `env` values in both manifests; no `Secret`, no `secretKeyRef`
  anywhere in `deploy/`.

**Migrations and RLS:** `pg::connect` runs `MIGRATOR.run()` on the serving pool
(`pg.rs:19`) with no `ANTARES_MIGRATE` switch, so the least-privilege split the
manifests document cannot be followed. All tables carry `FORCE ROW LEVEL
SECURITY` (`0001_init.sql:138`), so under a non-superuser owner
`SET LOCAL row_security = off` in `0006_temporal_meta.sql:9` errors rather than
bypasses, and `0005`'s backfill runs with `current_setting('antares.tenant')`
NULL → 0 rows updated, no error. Meanwhile `role_bypasses_rls` (`pg.rs:40`)
checks only `rolsuper OR rolbypassrls` — a non-superuser *table owner* passes the
`ANTARES_REQUIRE_RLS=1` gate while still bypassing every policy that lacks FORCE.
**[verified — FORCE is set on the 0001/0002/0003 tables]**

---

## Ledger integrity

The project's self-reporting is honest where it says *missing* or *partial*, and
overstated where it says *implemented*.

- `docs/ics.yaml:8` defines `implemented` as "full normative behaviour present,
  suite- and/or unit-tested". In practice it means "code exists and the Robot
  suite is green": **5 of 123 rows cite any test**, and 50 of the 101
  `implemented` rows cite only source files containing zero unit tests.
- Rows that should be downgraded: **5.7.11** → `missing` (`types_attrs.rs` contains
  zero occurrences of `federat`/`forward`/`remote`; the cited "CS/DISC suite" tests
  clause 5.10.2, a different resource); **5.15.1** → `partial` (non-conformant
  payload, above); **5.12**'s note ("the mirror serves the hot path") is false;
  **4.9**, **4.6.x**, **5.5.4** overstate given the gaps above. The `4.18–4.19`
  row is duplicated at `:177` and `:187`.
- `ics.yaml:5` says "MQTT TPs excluded" while `:649` says they are included and
  `dev/etsi-pipeline.sh` defaults `MQTT=1`. The header is 4 days stale.
- **The two ledgers disagree wholesale.** `claude.md` §5.4 — designated by §0.2 as
  *the* requirements source — has 1 of 132 boxes ticked. `ics.yaml` claims 101/123.
  One of the two is abandoned and neither says so.
- **CI runs 3 store modes, not 4.** `.github/workflows/etsi-matrix.yml:65` is
  `[file, postgres, timescale]`. `memory` — the default, the `bus=local` mode, and
  the only wasm backend — runs only in `full.yml`, which triggers on `v*` tags and
  manual dispatch. `tasks.md:19-20` and `README.md:153-155` both claim "4 modes,
  32 cells". **[verified]**
- **`full.yml` has no `schedule:` trigger** despite describing a nightly cadence,
  so the serial run (the only thing exercising `reset_state()` between suites),
  the k8s kind smoke, and the wasm tier fire only on release tags.
- **The 350 MiB RSS gate self-disables**: `dev/etsi-report.py:228` —
  `mem_ok = bool(peaks) and (not measurable or all(...))`. Zero samples ⇒ PASS.
  Two artifacts on disk show exactly that.
- **Task N7 is ticked against a red artifact.** `tasks.md:1053` claims
  "1025/1025, gate PASS"; `results/wasm-node/gate-status.txt` reads **FAIL** and
  the summary reads **1018 passed, 7 failed**. **[verified]**
- **Task I4 states the inverse of the shipped default** — it claims private-range
  "deny-by-default"; `loader.rs:98` allows by default per ADR-0010. `main.rs:22-24`
  carries the same wrong comment.
- The suite oracle is a private fork with 18 commits touching 74 `.robot` files
  ("Adjust assertions…", "Update response status codes…", "add sleep statements…"),
  while `error.md` logs 6 entries. The project's own rule is to prove a tool bug,
  log it, and leave the broker correct. Also, `TransientEntities/422_01-07` are 7
  self-authored TPs now counted inside the "ETSI green" total.
- `ExpandedEntity` / `CompactedDoc` — §9.1's load-bearing type-safety claim — have
  **zero occurrences in the tree**. Everything is `serde_json::Value`. **[verified]**

### Structural gap

| §9.3 crate | Planned | Actual |
|---|---|---|
| `antares-api` | thin handlers, "no business logic" | 14,982 LOC = 51% of the workspace |
| `antares-matcher` | 5 modules | 6 lines of comments, 0 tests |
| `antares-registry` | 7 modules | 16 lines (one enum), 0 tests |
| `antares-model` | 10 modules | 286 lines; 8 of 10 absent |
| `antares-ql` | 7 modules incl. `render` | 341 lines, one file; no `render` |
| `antares-temporal`, `antares-ws`, `antares-e2e`, `xtask`, `etsi/` | crates/dirs | absent |

This matters less as layering than as **testability**. The design's pure-function
crates are exactly the shapes that unit-test without a router; folding them into
axum handlers is what made the ETSI suite the only available oracle. Every 400+
LOC file with no `#[cfg(test)]` is in `antares-api` — including `notify.rs` (1,664)
and `temporal.rs` (1,650), the two largest files in the tree.

The document-shaped model itself is a defensible choice (Scorpio is effectively
document-shaped and passes the suite, and `Value` gives the §15.1 tolerant-reader
posture for free). The cheap, worthwhile part to recover is the two newtypes:
`eval_q` requires an expanded document but accepts a compacted one, compiles
cleanly, and silently returns zero matches.

---

## Suggested order of work

**Before any traffic.** The ten P0s. Six are one-line or one-function fixes
(purge filter, `enableServiceLinks`, `q=` reject-what-you-cannot-parse, outbox
`ANY($1)`, `expiresAt` parse-to-instant, readiness split); four are real work
(roles gating, throttle wiring, temporal truncation, bounded fan-out).

**Before the first incident.** A hard server-side row ceiling on every store read
path returning `TooManyResults` 403 — that single change closes most of P2's OOM
surface. Then: pool/timeout configuration, `statement_timeout`, connection caps
and header timeouts, lock-poisoning recovery plus `expect_used` in the lint wall,
redacted 500 bodies, bounded metric labels, drain completion.

**Before trusting the ledger.** Correct the rows named above; reconcile or retire
`claude.md` §5.4; fix the "4 modes / 32 cells" claim in `tasks.md` and `README.md`;
make the RSS gate fail closed; add `-- --ignored` to a CI job; untick or re-run N7.

**The tests that would have caught this.** A four-mode parity harness (same
operation sequence through memory/file/postgres/timescale, diff the results) —
that alone catches the batch-upsert divergence, the expiry boundary, the temporal
`get`/`list` split and the `lastN` ordering. A `q=` corpus asserted identical
between `eval_q` and the SQL compiler, plus the proptest round-trips §9.5 mandates
(which needs writing `render` first). Unit tests over the federation pure
functions — 1,800 lines of semantics with zero tests today. Timestamp property
tests across `{Z, offset, fractional}` × `{before, after, between, lastN}`.

---

## Appendix — spec claims re-verified against CIM 009 V1.9.1 text

Every conformance claim below was re-checked by extracting the clause from
`etsi-cim-specs/gs_cim009v010901p.pdf` directly, not taken from an agent's
citation.

**MemPalace could not be used for this at audit time, and that is part of the
story.** Searching the spec by its own path returned `total_before_filter: 0`.
Two causes, both now fixed:

1. The 34 ETSI PDFs were mined on 2026-07-18 into a *different palace*
   (`ngsi-ld`, wing `ngsi_ld`), while this session runs under
   `MEMPALACE_PROJECT=AntaresBroker` — so the MCP server never saw them.
2. The `/workspace` mine that produced the `antaresbroker` wing's 82,292 drawers
   ran in default `projects` mode, which **skips PDFs entirely**. It indexed the
   Rust source, `claude.md`, and — unhelpfully — a tree of Robot Framework
   `log.html` run artefacts whose base64 blobs and `5_6_21`/`since_v1.9.1` tags
   outrank real content on any clause-shaped query.

So the RAG did not fail loudly; it confidently returned the wrong corpus. An
agent obeying `claude.md`'s "MANDATORY: query MemPalace, do not answer from
memory" rule would have received ETSI run logs, found nothing usable, and fallen
back to memory anyway — which is a plausible contributor to the drift catalogued
above.

**Fixed 2026-08-09:** `gs_cim009v010901p.pdf` mined into this palace as wing
`spec` (2,072 drawers, `pdf_page` metadata intact — expand any hit with
`mempalace_get_pdf_pages`), and `/workspace/mempalace.yaml` added to keep run
artefacts out of future mines. The stale `log.html` drawers from the deleted
`ETSI-matrix-results (1)/` tree still need purging via the pgvector recipe.
Verified: the ValueList/Range query that previously returned run logs now
returns p.85 (the ABNF) and p.91 (`temperature!=10..20`).

| Claim | Clause | Verdict |
|---|---|---|
| Purge: id/idPattern are not a sufficient filter | 5.6.21.4, p.194 | **CONFIRMED verbatim.** "At least one of the following input data shall be provided: a) selector of Entity Types; b) list of Attribute names…; c) NGSI-LD Query…; d) NGSI-LD GeoQuery; e) local scope. If none of the above is provided, then an error of type BadRequestData shall be raised (too wide query)." 5.6.21.3 lists id and idPattern as legal *input data* — they filter, they are just never sufficient. Prose confirms: "it is not possible to purge a set of entities by only specifying desired Entity identifiers". **Extra gap found:** (b) and (c) require "at least one **non-system** Attribute" — the code does not check that either. |
| `q=` ValueList and Range are normative | 4.9 ABNF p.85 + behaviour p.91-92 | **CONFIRMED three ways.** ABNF: `ValueList = Value 1*(%x2C Value)`, `Range = ComparableValue dots ComparableValue`, `dots = %x2E %x2E`, `CompEqualityValue = OtherValue / ValueList / Range / URI`. Behaviour bullets define both. The spec's own example is `temperature!=10..20`. |
| `!=` must **match** on datatype mismatch | 4.9, p.92 | **CONFIRMED.** The bullet "If the data type of the target value and the data type of the Query Term value are different, then they shall be considered unequal" is the final sub-bullet of the **Unequal** operator (which opens on p.91). Equal carries the mirror-image rule ("considered as not matching"). The asymmetry is deliberate; Antares returns `false` for both. |
| `!~=` (notPatternOp) exists | 4.9 ABNF, p.85 | **CONFIRMED.** `notPatternOp = %x21 %x7E %x3D`. `antares-ql`'s `CmpOp` has no negated-pattern variant. |
| Subscription with past `expiresAt` → 400 | 5.8.1.4, p.219 | **CONFIRMED verbatim.** "If the expiration timestamp provided represents a moment before the current date and time, then an error of type BadRequestData shall be raised." |
| Subscription `status` enum is active/paused/expired | 5.8.6, p.224 | **CONFIRMED verbatim.** "Notifications shall only be sent if and only if the status … is 'active', i.e. not 'paused' nor 'expired'." Antares emits top-level `"status": "failed"`. |
| `/info/sourceIdentity` payload is non-conformant | 5.2.40, Table 5.2.40-1, p.141 | **CONFIRMED — and worse than first reported.** Three members are mandatory at cardinality **1**: `contextSourceAlias`, `contextSourceUptime`, `contextSourceTimeAt`. Antares emits `hostAlias` and `uptime` (neither is a spec member) and omits `contextSourceTimeAt` entirely. 5.15.1.4 defers to 5.2.40, so that table is the authority. |
| Accept precedence is list order, not header order | 6.3.4, p.270 | **CONFIRMED, narrowly.** "The order of the list above is significant… the first one of the list shall be selected, **unless amended by the HTTP Accept header processing rules, e.g. the presence of a 'q' parameter**." So q-weights legitimately override; the defect is only the equal-q tie-break, which Antares resolves by header order. |
| `LdContextNotAvailable` → 504 | Table 6.3.2-1, p.269 | **CONFIRMED.** Table reads 504. `antares-model/src/error.rs:42` returns 503 (documented as a deliberate suite-compat choice). |
| `format=simplified` must wrap LanguageProperty / JsonProperty / VocabProperty | 4.5.4, p.55 | **CONFIRMED verbatim with the spec's own examples:** `"says": {"languageMap": {…}}`, `"parkingTickets": {"json": {…}}`, `"gender": {"vocab": "Male"}`. Property / GeoProperty / Relationship correctly take the bare value, which is what Antares does for those. |
| GeometryCollection excluded | 4.10, p.93 | **PARTIALLY CONFIRMED.** Verified for geoquery reference geometries ("as defined by the GeoJSON specification …, except GeometryCollection"). The 4.6.3 value-side claim was not separately verified. |
| `type=*` means all types + implicit local | Table 6.4.3.2-1, p.284 | **CONFIRMED verbatim.** "Selection of Entity Types as per clause 4.17. `"*"` is also allowed as a value and `local` is implicitly set to true **and shall not be explicitly set to false**." Three obligations, not one. |
| `sysAttrs` gates `expiresAt` | 6.3.11, Table 6.3.11-1, p.276 | **CONFIRMED verbatim.** "When its value includes the keyword `sysAttrs` … the system generated temporal attributes `createdAt`, `modifiedAt` **and the system temporal attribute `expiresAt`** are included … In the case of temporal representations, also … `deletedAt`." |
| `!=` over an array must require *no* element to match | 4.9 Unequal, p.91 | **CONFIRMED — a bug not previously reported.** "Is not included in the target value, if the latter is an array (e.g. matches `["blue","black","green"]`, **but not** `["blue","red","green"]`)." `qeval.rs:91-93` recurses with `items.iter().any(...)` for *every* operator, so `q=color!="red"` against `["blue","red","green"]` matches on the "blue" element. `.any()` is correct for `==`; `!=` needs `.all()`. |
| simplified `valueList`/`objectList` should be wrapped | 4.5.4, p.57-58 | **CLAIM WITHDRAWN — Antares is correct.** *(This row previously said "the clause is silent" — that was wrong; I had only read to p.56.)* 4.5.4 covers both explicitly: "For each **ListProperty** a member whose key is the Property name and whose value is **an ordered array holding the Property Values**" (EXAMPLE 14), and the same for ListRelationship (EXAMPLE 16). Both are bare arrays with **no** wrapper, unlike languageMap/json/vocab. `repr.rs:387-398` already emits them bare. Do not change. **Note the contrast with 4.5.9** (temporal), which *does* mandate a keyed form — see V-24. |

## Appendix — second verification round (every remaining spec claim)

With the spec searchable, all remaining spec assertions in this report were
checked clause-by-clause. Verdicts: **CONFIRMED** (code is wrong),
**REFUTED** (code is fine / claim misreads the spec), **SPEC-SILENT** (clause
does not cover it — not provably wrong), **PARTIAL** (right in substance, wrong
in scope).

### Corrections to findings stated earlier in this report

These are the ones that changed. Each was over-stated in the body above and is
corrected here; the body should be read subject to this list.

| Earlier finding | Corrected verdict |
|---|---|
| **Batch upsert duplicate divergence** (memory keeps last, Postgres keeps first) — listed as P1 | **REFUTED at HEAD.** `batch.rs:330-343` splits duplicates into *rounds* — the Nth occurrence of an id goes to round N, rounds run in order, ids within a round are unique — so `AnyStore::batch_upsert` is never handed a duplicate and both backends converge. The first-wins dedup at `pg_entity.rs:757` is **latent, not live**. Separately, 5.5.11.2 (p.155) + 4.6.6 (p.82, "shall come in chronological order") confirm **last** must win for upsert, and `pg_entity.rs:627`'s identical first-wins dedup in `batch_create` is **correct**, because 5.5.11.1 says the first occurrence creates and subsequent ones are errors. |
| **Delete Entity destroys temporal history** — listed as HIGH | **SPEC-SILENT.** 5.6.6.4 (p.168) says only "The input data shall be used to remove the entity locally if it exists" — nothing about the Temporal Evolution. Support is only presuppositional (5.2.4 and 6.3.11 reference `deletedAt` in temporal representations, which is unreachable if history is hard-deleted). Architecturally questionable, **not provably non-conformant**. |
| **`orderBy` on arbitrary attributes is wrong / restricted to `id`** | **Half WITHDRAWN.** The id-only restriction is **5.7.4.4** (Query *Temporal*, p.208), not Query Entities. Table 6.4.3.2-1 positively authorises "an Entity member (`id`, `type`, `scope`) **or an Attribute name**" for `GET /entities`. The *real* violation is different — see V-21. |
| **`local=TRUE`/`1` silently federates** | **REFUTED as a conformance defect.** 6.3.20 (p.279) is a *should*, and scoped to "parameters incompatible with the operation" and unsupported **`options`** values — neither covers a malformed Boolean. Table 6.3.18-1 defines behaviour only for `local=true`. Downgrade to robustness/hardening. |
| **Peer-controlled `modifiedAt` wins the merge** | **SPEC-CONFORMANT.** 4.5.5.3 (p.60) mandates exactly that ordering and offers no defence against a hostile peer. Keep as a **security note**. A real conformance gap does sit in the same function though — see V-19. |
| **`contextSourceInfo` credentials stored/echoed in plaintext** | **SPEC-SILENT.** Table 5.2.9-1 (p.112) defines it as a "Generic {key, value} array" with no output-only marking and no redaction language anywhere. Security note only. |
| **Subscription `lang` not applied to notifications** | **PARTIAL / SPEC-SILENT.** Table 5.2.12-1 scopes `lang` to "the query"; 5.8.6's representation bullets list `format`, `pick`, `omit`, `join`, `attributes`, `sysAttrs`, `showChanges`, `ngsildConformance` — **not** `lang`. The code fact holds but calling it a broken SHALL requires an inference the spec does not license. |
| **All four notification bookkeeping members stamped pre-send** | **PARTIAL.** 5.8.6 (p.226) explicitly permits `timesSent` and `lastNotification` at send time. Only `lastSuccess` and `status:"ok"` are post-response-only. Narrow the finding to those two. |
| **`timesFailed` is a mandated member** | **PARTIAL.** Table 5.2.14.2-1 gives it cardinality **0..1**, so absence is legal. The binding defect is narrower: "implementations shall generate them as part of their representation", and Antares never counts failures at all. |
| **`geometryProperty` must accept a full IRI** | **SPEC-SILENT.** The short-hand mandate in 4.10 is written for `geoproperty` only and is not repeated for `geometryProperty` in 4.5.16.1, 6.3.15 or either table. Downgrade to robustness gap. 4.5.16.1 even sanctions the miss outcome ("the geometry shall be undefined and returned with a value of null"). |
| **Deleted LanguageProperty must always be `{"@none":"urn:ngsi-ld:null"}`** | **PARTIAL / over-scoped.** 5.8.6 (p.225) mandates the map form **only** in the expanded-object case — when a `datasetId` is needed, or `sysAttrs`/`showChanges` is true. In the plain case it is the bare `"<attrName>": "urn:ngsi-ld:null"`. The repo memory note asserts this too broadly. |
| **`508 Loop Detected` is an NGSI-LD 2.0 pre-adoption** (`claude.md` §15.1) | **WRONG — it is in V1.9.1.** 6.3.17 (p.278) mandates it, but *narrowly*: only "In the case of an **exclusive** or **redirect** registration … if the single registered source and tenant is registered to redirect back on to the Context Broker". For inclusive/auxiliary loops the required signal is `NGSILD-Warning: 199` instead. `federation.rs` returns 508 unconditionally whenever `via_loop` fires — **over-application**. |
| **A federation hop limit is spec-required** (implied by §16.7) | **Not a spec requirement.** 4.3.6.4 (p.42) prescribes exactly one remedy for cascades — "a binding-specific mechanism to request operations only on the registered endpoint itself", i.e. the `local` parameter. The hop limit is an Antares design choice; its absence is not a conformance gap. |

### Newly discovered violations (found while verifying)

| ID | Clause + page | Defect |
|---|---|---|
| V-14 **FIXED 2026-08-09** (NGSILD-Warning 199/299/111 on GET /entities(/{id}); 404 stays normal) | 6.3.17, p.278 | **`NGSILD-Warning` is never emitted** — `grep -rni "ngsild-warning" crates/` → zero hits. The header is how failed/timed-out read sources must be surfaced (`199` no-response-or-loop, `299` error response, `111` unparseable payload). This *replaces* my earlier "should be a 207" framing, which was wrong. A `404` from a source "should not be considered as abnormal behaviour". |
| V-15 **FIXED 2026-08-09** (combine_attr_parts: UpdateResult on all five /attrs methods, registrationId per 5.2.19; note 6.6.3.2-2's remark contradicts its own Data Type column — logged in error.md) | 5.2.18 / 5.2.19, Tables 6.6.3.1-1 p.296 & 6.7.3.1-1 p.299 | **207 on `/attrs` operations returns the wrong data type.** `federation::combine` (batch-shaped `{success, errors}`) is called from `attrs.rs:211,476,608,740,870`, but those clauses mandate an **`UpdateResult`** — `updated: String[]` (card. 1) + `notUpdated: NotUpdatedDetails[]` (card. 1), each carrying mandatory `attributeName` and `reason`. |
| V-16 **FIXED 2026-08-09** (cooldown gate before any bookkeeping; drops, never queues) | 5.2.15, Table 5.2.15-1, p.121 | **`endpoint.cooldown` unimplemented** — "minimum period of time in milliseconds which shall elapse before attempting a subsequent notification to the same endpoint after failure". The only cooldown in-tree is the generic egress breaker, not the per-subscription member. |
| V-17 **FIXED 2026-08-09** (per-endpoint HTTP deadline, clamped 100ms–30s under the clause's system-override licence) | 5.2.15, Table 5.2.15-1, p.121 | **`endpoint.timeout` unimplemented** — no `"timeout"` key is read anywhere; delivery uses the shared client's global timeout. |
| V-18 **FIXED 2026-08-09** | Table 5.2.14.1-1, p.119 + 5.8.1.4 | **Empty-array restriction unenforced** on `notification.attributes` / `pick` / `omit` ("Empty array (0 length) is not allowed") → must be BadRequestData. `watchedAttributes` and `entities` *are* correctly guarded. |
| V-19 **FIXED 2026-08-09** (merge_docs discards past-expiresAt instances before recency; expired base instances lose to live remote ones) | 4.5.5.3, p.60 | **`recency()` skips the mandatory first step** — "if an `expiresAt` DateTime is present on the Attribute and the date lies in the past, **it shall be discarded**", *then* order by `observedAt`/`modifiedAt`. `federation.rs:419-424` only compares timestamps. 4.5.5.2 (conflicting transient entities) is likewise unimplemented in the merge path. |
| V-20 **FIXED 2026-08-09** (also rejects the simplified synonym) | Table 5.2.14.1-1, p.120 | **`showChanges: true` with `format: keyValues` is not rejected** — "showChanges cannot be true in case format is keyValues", enforced via 5.8.1.4 → BadRequestData. Also covers `format: "simplified"` (declared synonym). |
| V-21 **FIXED 2026-08-09** | 5.7.2.4 p.201 + 4.23.1 p.102 | **`orderBy` has no local-scope precondition.** "If the ordering parameter is present and the execution of the operation is not limited to the local scope … BadRequestData" — appearing verbatim in *both* 5.7.2.4 and 5.7.4.4 — reinforced by 4.23.1: "**Sort ordering is never applied to distributed operations**". `entities.rs:760` calls `order_entities` unconditionally. Also unimplemented: the `orderFrom`-missing guard and the ICU-collation validity guard. |
| V-22 **FIXED 2026-08-09** | 5.7.2.4, p.201 | **`geometryProperty` without `Accept: application/geo+json` must 400.** Antares reads the param only inside the GeoJson branches (`entities.rs:448,804`) and silently ignores it otherwise. |
| V-23 **FIXED 2026-08-09** (type emitted for json and geo+json too) | 6.3.10, p.275 | **Pagination `Link` omits the mandatory `type` attribute** for `application/json` — "At least, the `type` Link Target Attribute **shall** be included … and its value shall be exactly equal to the media type resulting from the original request". `entities.rs:1152-1156` emits it only for `Accept::LdJson`, i.e. never for the common case. |
| V-24 **FIXED 2026-08-09** (bare arrays per EXAMPLE 3/7; router test pins both) | 4.5.9, p.63 (EX. 3) + p.65 | **Temporal ListProperty/ListRelationship use the wrong form** — 4.5.9 mandates a `valueLists`/`objectLists` key whose first element is a **bare ordered array**; `temporal.rs:531-534` emits `{"valueList": …}` / `{"objectList": …}`. (Distinct from 4.5.4 simplified, where bare *is* correct — see the withdrawn row above.) |
| V-25 | 4.5.9, p.62 | **Extra member in simplified temporal representation** — each attribute object "shall **only** contain a member whose key shall be `values`"; `temporal.rs:554-556` also inserts `datasetId`. Flagged, not prescribed: 4.5.9 provides no slot for multi-dataset attributes, so this may be a spec gap. |
| V-26 **FIXED 2026-08-09** (Z-only + comma separator + 1-6 digit fraction + real calendar via chrono; test rewritten — it had pinned the offset bug as "valid") | 4.6.3, p.80-81 | **`parse_datetime` accepts what the spec forbids and rejects what it allows** — `expand.rs:629-633` accepts a trailing `+`/`-` offset ("shall not contain expressions of the difference between local time and UTC"; "shall always be equal to the character `Z`") and rejects the **comma** decimal separator the spec explicitly permits in requests. No cap on the six-digit fractional maximum either. |
| V-27 **FIXED 2026-08-09** (per-datatype dispatch incl. lexicographic string min/max, Time avg, List sizes, Relationship distinct targets; InvalidRequest for ineligible; no Infinity-as-null) | 5.7.4, p.211 | **No `InvalidRequest` for ineligible aggregation** — "if any of the requested Attributes is not eligible for at least one of the aggregation methods … an error of type InvalidRequest shall be raised". Worse, `min`/`max` over non-numerics produce `f64::INFINITY`, which is not representable in JSON, so `temporal.rs:895-900` silently emits **`null`**. |
| V-28 **FIXED 2026-08-09** (bounds.rs, HTTP/1.x-scoped, no invented chunked exemption; Robot 046_01 4/4) | 6.3.4 p.270 + 6.3.2 p.269 | **411 Length Required is genuinely mandated** for POST/PATCH/PUT with no `Content-Length` — bare status, empty body. No such check exists in `antares-api`, and hyper serves `Transfer-Encoding: chunked` bodies instead of 411-ing them. |
| V-29 **FIXED 2026-08-09** (accept/contentType/jsonldContext/ngsildConformance processed at forward per 4.3.6.6; values validated at registration; jsonldContext recompaction limited to entity-shaped bodies + attrs/type/geoproperty params) | 4.3.6.5 p.42 + 6.3.19 p.279 | **`contextSourceInfo` reserved keys unhandled** — `contentType`, `jsonldContext` (broker "shall apply a compaction operation over **both payload and query parameters**" and "shall remove any `@context` members from the payload") and `ngsildConformance` carry normative meaning on forward; none is validated at registration or applied at forward. Matches `ics.yaml`'s own 4.3.6 note. |
| V-30 **FIXED 2026-08-09** (received-by token equality; 508 scoped to single exclusive/redirect source, other loops clear the forward set + warn 199) | 6.3.18, p.279 | **`via_loop` compares Via tokens by suffix.** RFC 7230's `received-by` pseudonym is a token and must be compared as one; `ends_with(alias)` false-positives (alias `b1` matches peer `sub-b1` → spurious 508) and misses the converse. Compare the last whitespace-separated token for equality. |

### Confirmed without change

`q=` ValueList/Range and `!=` datatype/array semantics; subscription past-`expiresAt`; subscription `status` enum; `/info/sourceIdentity` members; Accept precedence; `LdContextNotAvailable` 504; simplified LanguageProperty/JsonProperty/VocabProperty wrappers; `entityUpdated` ⇒ attribute-trigger equivalence (Table 5.2.12-1, p.116); output-only subscription members "shall ignore them" (5.2.14.2, p.120); `endpoint.accept` geo+json delivery (5.8.6, p.226); Table 6.4.3.2-1's four missing params and `dist-asc`/`dist-desc`; concise `type`-bearing value must not collapse (4.5.2.3, p.50); 406 must carry a body (6.3.4); 405 for wrong verb (6.3.2); dereferenceable Link targets (6.3.10); `pick`-empty entities must be **reduced, not dropped** (5.7.2.5, p.203 — "reduced down to", with no removal rule anywhere); temporal 206-without-truncation and the `DateTime` unit (6.3.10); aggregation type-dispatch, empty periods and attribute `type` labelling (4.5.19.1); `aggrPeriodDuration` (`P3Y6M4DT12H30M5S` is the spec's own example); 5.6.11 upsert must **add**, not replace, and must merge new entity types; `lastN` positive integer; `ts_float` numeric coercion vs 4.6.4; batch per-registration narrowing (5.6.7.4/5.6.10.4/5.6.20.4, "Remove from IN all Entities not matched by CSR"); `BatchOperationResult.success` cardinality 1 and `BatchEntityError.entityId` mandatory; EntityMaps are **not** optional (no conditional language, unlike Snapshots and tenancy); Purge `local=true`-alone **is** sufficient (5.6.21.4(e)) and must not be "fixed".

### Verdicts that closed claims in Antares' favour

`lang` q-weight authority is **RFC 5646 + RFC 3282** (4.15), not RFC 7231 §5.3.5 — the defect stands, the citation was wrong. `stddev` population-vs-sample is **SPEC-SILENT**. 4.6.2 name validation is a **SHOULD**, not a SHALL. GeoJSON null-geometry fallback matches 4.5.16.1. Bare `valueList`/`objectList` in *simplified* output matches 4.5.4.

## Appendix — remediation for the confirmed conformance defects

Each fix below was written against both the clause text and the code as it
actually stands at `ead650c`. Where the spec is silent or a change has a cost,
that is stated rather than papered over.

### C1 · Purge accepts an insufficient filter — `entities.rs:1284`

Clause 5.6.21.4 permits exactly: (a) Entity Type selector, (b) Attribute-name
list *including at least one non-system Attribute*, (c) NGSI-LD Query *including
at least one non-system Attribute*, (d) GeoQuery, (e) local scope.

```rust
// now — id/idPattern wrongly count, and the non-system qualifier is unchecked
let has_filter = ["id", "idPattern", "type", "attrs", "q", "georel"]
```

Change to `["type", "attrs", "q", "georel"]`. Keep `id`/`idPattern` in the
`check_params` allowlist — 5.6.21.3 lists both as legal input data and they must
still filter; they are simply never *sufficient*. Then add the non-system test:
`attrs` must contain at least one name that is not in `ENTITY_META`
(`createdAt`, `modifiedAt`, `expiresAt`, `deletedAt`, `scope`), and the parsed
`q` AST must reference at least one non-system attribute path. Query Entities at
`entities.rs:719` is already correct and needs no change — Table 6.4.3.2-1 p.284
states the same "at least one among type, attrs, q, or georel" rule for it.

### C2 · `q=` ValueList and Range — `antares-ql/src/lib.rs:37-41,185-216` — **FIXED 2026-08-09**

> Fixed as specified below: `QValue::List`/`QValue::Range` + `CmpOp::NotPattern`
> in the parser (lists/ranges/bools with ordering ops rejected 400 per p.84);
> full p.90-92 semantics in `qeval` (incl. array targets and the `!=`
> type-mismatch rule); SQL pushdown compiles `==`+List/numeric-Range exactly and
> declines the rest to the evaluator. Tests: antares-ql (5 new), qeval (3 new,
> spec examples verbatim), compile/q (1 new), Robot extension TP
> `QueryEntities/019_28` — 6/6 green against the memory-store broker.

ABNF (p.85): `CompEqualityValue = OtherValue / ValueList / Range / URI`,
`ValueList = Value 1*(%x2C Value)`, `Range = ComparableValue dots ComparableValue`.

Correct fix: add `QValue::List(Vec<QValue>)` and `QValue::Range(Box<QValue>,
Box<QValue>)`; in `value()`, after reading a `ComparableValue`, look ahead for
`..` (range, both ends inclusive per p.91) or `,` (list). Then in `qeval::compare`
implement `==` as "identical to, or included in, any list member / within the
closed interval" and `!=` as the negation, per p.91-92.

If that is more than you want to land now, the *safe* interim is to make
`value()` **reject** an unquoted literal containing `,` or `..` with
`BadRequestData`. A 400 is spec-defensible under 6.3.20 (invalid parameter); the
current `200 []` is not. Do not ship the current behaviour either way.

Note `notPatternOp` (`!~=`, ABNF p.85) is also absent from `CmpOp` and parses as
an error today — same clause, same fix window.

### C3 · `!=` semantics — `qeval.rs:89-121` — **FIXED 2026-08-09**

> Both defects fixed exactly as below; the SQL compiler now declines `!=`
> entirely (jsonpath cannot reproduce the type-mismatch match nor the
> universal array quantification), so the evaluator is the arbiter. Unit tests
> pin the p.91-92 examples verbatim.

Two distinct defects, both provable from p.91-92.

**(a) Datatype mismatch must match.** The Unequal operator's final sub-bullet:
"If the data type of the target value and the data type of the Query Term value
are different, then they shall be considered unequal." Equal carries the
mirror-image rule ("considered as not matching"), so the asymmetry is deliberate.
Today all three arms `return false` on a failed cast. Add, immediately after the
array recursion:

```rust
// 4.9 p.92: differing datatypes are "considered unequal"
if op == CmpOp::Ne && !same_datatype(target, want) { return true; }
```

with `same_datatype` testing `is_number` / `is_string` / `is_boolean` against the
`QValue` variant. The element must still be *present* — 4.9 requires "a matching
entity shall contain the target element" — which the caller already guarantees.

**(b) Arrays.** `qeval.rs:91-93` recurses with `.any()` for every operator, but
p.91 requires that `!=` match only when the value is *not included* in the array.
Special-case `CmpOp::Ne` to `.all()`; `.any()` stays correct for the others.

Both engines are consistently wrong here, so the parity harness alone would not
have caught it — fix `qeval` and the SQL compiler together.

### C4 · Subscription `expiresAt` — `subscriptions.rs:259`, `notify.rs:507-509`, `subscriptions.rs:399-402`

5.8.1.4 p.219: "If the expiration timestamp provided represents a moment before
the current date and time, then an error of type BadRequestData shall be raised."
The rule is correct in the code; the *comparison* is not — `s < now_iso().as_str()`
byte-compares an ISO-8601 string that `parse_datetime` allows to carry a `+HH:MM`
offset.

**Corrected after verifying 4.6.3 (p.80-81) — the earlier prescription here was
incomplete.** The spec does not merely require instant comparison; it forbids
the offending input outright: "The trailing timestamp component … **shall always
be equal to the character `Z`**", and "All the referred components shall appear
in the string; reduced representations are not permitted." So `parse_datetime`
accepting `+HH:MM` and a bare 19-char form is itself a **4.6.3 violation**
(V-26), not just a comparison hazard.

The fix therefore has three parts, and part 3 is the one that is easy to miss:
1. **Reject** any timestamp not ending in `Z`, and any reduced form.
2. **Accept** the comma decimal separator — 4.6.3: "In requests, also a comma
   instead of a decimal point may be used as separator for compatibility
   reasons" — and cap the fraction at six digits.
3. **Still normalise the fraction** before any byte comparison. Rejecting
   offsets is *not* sufficient: `2020-01-01T00:00:00.500Z` byte-sorts *before*
   `2020-01-01T00:00:00Z`, because `.`(0x2E) < `Z`(0x5A). Either pad to a fixed
   fractional width at storage or compare as instants.

Do it once in `antares-jsonld` and let all sites call it — that covers the
temporal `timerel`/`lastN` defect (V-1/P1), `notify.rs:507`, and the `expired`
computation at `subscriptions.rs:399`.

### C5 · Subscription `status` — `subscriptions.rs:406-411`

5.8.6 p.224: "Notifications shall only be sent if and only if the status … is
'active', i.e. not 'paused' nor 'expired'." The enum is exactly
`active | paused | expired`. The code computes those three correctly, then adds a
fourth:

```rust
} else if obj.get("status").and_then(Value::as_str) == Some("failed") {
    "failed" // 5.8.6 / 5.11.7 delivery-failure status
```

**Softened after verification — the spec contradicts itself here, so this is a
judgement call, not a clean fix.** Table 5.2.12-2 (p.118) restricts subscription
`status` to `active|paused|expired`, and `ok|failed` belong to
`notification.status` (Table 5.2.14.2-1, p.120). But **5.11.7 (p.241) says
"Update the *subscription* `status` to `\"failed\"`"** while 5.8.6 (p.226) says
"Update `notification.status` to `\"failed\"`" for the same condition.

**Likeliest reconciliation, found on re-reading p.241 in context:** 5.11.7 is
*"CSource notification behaviour"* — it governs **csourceSubscriptions**, while
5.8.6 governs ordinary **subscriptions**. So the two may not contradict at all;
they may simply address different resources, in which case top-level
`status: "failed"` is right for `/csourceSubscriptions` and wrong for
`/subscriptions`. That reading is not airtight either, because Table 5.2.12-2
restricts subscription `status` to `active|paused|expired` and the repo stores
both resources with the same shape.

Recommended: follow the data-type tables for `/subscriptions` — drop the
top-level `"failed"` branch there and keep the failure signal in
`notification.status`, which the code already sets correctly — and leave the
csourceSubscription path alone pending a decision. Record the ambiguity in
`error.md`, and check what the Robot suite asserts before changing behaviour.
Do **not** cite this as an unambiguous violation.

### C6 · `/info/sourceIdentity` — `lib.rs:398-403`

5.15.1.4 defers to clause 5.2.40, whose Table 5.2.40-1 makes three members
mandatory at cardinality 1: `contextSourceAlias`, `contextSourceUptime` (ISO 8601
duration) and `contextSourceTimeAt` (DateTime per 4.6.3). `contextSourceExtras`
is optional (0..1). Today the payload emits `hostAlias` and `uptime` — neither is
a spec member, so neither expands to an NGSI-LD IRI — and omits
`contextSourceTimeAt` entirely.

```rust
"contextSourceAlias": state.host_alias,
"contextSourceUptime": format!("PT{uptime}S"),   // already a valid ISO 8601 duration
"contextSourceTimeAt": now_iso(),
```

Keep `id` and `type` as they are; both are correct.

### C7 · Accept precedence — `negotiate.rs:144-150`

6.3.4 p.270: "The order of the list above is significant. If the Accept header
can be expanded to more than one of the options of the list, the first one of the
list shall be selected, **unless amended by the HTTP Accept header processing
rules, e.g. the presence of a `q` parameter**."

So q-weighting legitimately overrides list order — the code is right to honour
`q`, and only the equal-q tie-break is wrong. Carry a list-rank alongside the
existing `(q, specificity)` tuple — `application/json`=0, `application/ld+json`=1,
`application/geo+json`=2 — and extend the comparison:

```rust
q > *bq || (q == *bq && (spec > *bspec || (spec == *bspec && rank < *brank)))
```

The `application/*` → Json and `*/*` → Json mappings are already correct, since
`application/json` heads the list.

### C8 · `sysAttrs` must gate `expiresAt` — `repr.rs:210-215`

Table 6.3.11-1 p.276 names `createdAt`, `modifiedAt` **and `expiresAt`** as the
members `options=sysAttrs` controls, plus `deletedAt` for temporal
representations. The gate currently covers only two:

```rust
"createdAt" | "modifiedAt" if !r.sys_attrs => continue,
```

Extend to `"createdAt" | "modifiedAt" | "expiresAt" | "deletedAt"`.

**Verified safe:** `repr::apply` is called only from `entities.rs`, `batch.rs`
and `notify.rs` — Entity and notification payloads. Subscriptions and
registrations render through `present_subscription` and
`present_registration(doc, ctx, sys_attrs)`, which are untouched by this change.
That matters because `expiresAt` on a Subscription (5.2.12) or a
CSourceRegistration (5.2.9) is an ordinary client-supplied member that must
always be returned — it is *not* a system attribute there.

### C9 · `type=*` — `entities.rs:888-900`

Table 6.4.3.2-1 p.284 imposes three obligations, and the code meets none: `"*"`
selects all types; `local` is implicitly true; and `local` "shall not be
explicitly set to false". Today `"*"` is run through `ctx.expand_key` like any
term, yielding an IRI that matches nothing — a silent `200 []`.

Before the split at `:889`, special-case `params.get("type") == Some("*")` to
produce `type_sel = None` (no type predicate) and force the local-only path;
and reject `type=*` combined with `local=false` as `BadRequestData`. Note the
`has_filter` gate still passes on the presence of `type`, so no change is needed
there.

### C10 · `LdContextNotAvailable` — `antares-model/src/error.rs:42` — decide, don't drift

Table 6.3.2-1 p.269 maps this error to **504**. The code returns 503 with the
comment "503 per the conformance suite's V1.8-era expectation (043_01); V1.9.1
moved this to 504 — flip when the suite catches up."

I checked the suite:
`TP/NGSI-LD/CommonBehaviours/CommonResponses/VerifyLdContextNotAvailable/043_01.robot:14`
hardcodes `${expected_status_code}= 503`. So this is a genuine suite-versus-spec
conflict, and it is provable in exactly the way the repo's testing guide demands.

By this repo's own stated policy — *"Prove it is a tool bug, log it in
`error.md`, leave the broker correct"* — the correct action is to return **504**
and add an `error.md` entry. Be aware of the cost, which is why this is a
decision rather than a mechanical fix: flipping turns all eight 043_01 cases red,
so the CommonBehaviours count drops until the suite is updated or the TP is
excluded with a recorded reason. Choosing to stay on 503 is defensible too — but
then `ics.yaml`'s 6.3.2 row should say `partial` and name the deviation, because
right now the ledger claims conformance the code deliberately does not have.

### Not to be "fixed"

- **Simplified `valueList` / `objectList`.** 4.5.4 covers Property, GeoProperty,
  LanguageProperty, JsonProperty, VocabProperty and Relationship only. ListProperty
  and ListRelationship are not in the clause, so the current bare-value output is
  not provably wrong. Leave it and record the ambiguity.
- **Registration-scope narrowing on forwards.** Spec-mandated by 4.3.6.1; it was
  nearly "fixed" away once already (`claude.md` §14.8).
- **Simplified Property / GeoProperty / Relationship.** The bare-value form at
  `repr.rs:387-407` is correct per Examples 1, 3, 7 and 8; only the LanguageProperty,
  JsonProperty and VocabProperty wrappers are missing. The `if k == "json"` branch
  inside that loop is dead code — `"json"` is not in the iterated list — and should
  be removed as part of the same change.

## What is genuinely solid

Worth stating plainly, because the list above is one-sided. SQL injection is
closed by construction — every value is a bind including jsonpath, GeoJSON and
scope regexes, and `format!` only ever assembles compiler constants plus `$n`.
Tenant predicates are on every tenant-scoped statement, and `&TenantId` really is
the first parameter of every store method. `SET LOCAL` is used correctly and no
session-level `SET` exists. The egress policy, redirect cap and DNS pinning are
real and tested. Candidate subscription lookup is index-shaped with a 10k-sub
scaling test. `cargo auditable` is wired so the shipped binary carries an SBOM,
the base is distroless-nonroot, and the postgres manifest sets `runAsNonRoot`,
`readOnlyRootFilesystem`, `drop: [ALL]`, `seccompProfile`. The ETSI pipeline is
identical locally and in CI, gates image publication, and `check_suites_complete`
makes an unrun suite a hard error — a genuinely good control. Clippy is clean at
`--all-targets`. And the graceful-shutdown steps that *are* implemented were
verified working end to end.
