# Antares — Deep Analysis

**An NGSI-LD Context Broker in Rust.**
Design analysis v0.1 — 2026-08-03. Reference implementation studied: Scorpio Broker (`/workspace/ScorpioBroker`), ETSI CIM specs (`/workspace/etsi-cim-specs`), NGSI-LD WebSocket binding draft (`/workspace/websocket.md`).

## 0. Ground rules for agents working in this repo

1. **Never touch the host Mac.** Do not use `/var/run/docker.sock` (or any Docker CLI/API against it), do not start/stop/inspect host containers, do not mount host paths, do not install anything Mac-side. Everything needed for development — build, unit/integration tests, the ETSI suite — runs inside this sandbox. If a task appears to require host Docker (e.g. spinning up Postgres/NATS containers), stop and ask the user instead of doing it.
2. **Spec-first implementation.** Every feature is implemented from its ETSI CIM 009 V1.9.1 clause (the per-clause ledger in `docs/spec/`, §0.3), NOT from ETSI test-suite failures. The Robot suite (§5.3) is the validation oracle run *after* a clause is implemented — it is never the requirements source. Its 686 TPs cover only part of the normative surface; a broker built test-first ships the untested gaps broken. Working order per feature: read the clause text (it is IN the ledger file; the PDF `/workspace/etsi-cim-specs/gs_cim009v010901p.pdf` stays the authority for exact wording) → implement the full normative behaviour → unit-test the clause's own rules and edge cases → only then run the Robot suite as confirmation, and update `docs/spec/<clause>.md`.

## 0.3 GOAL — the conformance audit loop (STRICT, 2026-08-10)

**Goal: every clause file in `docs/spec/` audited — status earned with evidence, code annotated with its CIM 009 clause, Robot TPs (including edge cases) covering its normative surface.** The ledger was deliberately reset to zero (947 sections `not-implemented`, old `docs/ics.yaml` deleted — history in git). Statuses are EARNED by this loop, never assumed from the old ledger or from memory.

Work **file by file, in clause order** (`4.*` → `5.*` → `6.*` → `7.*` → annex `A`, `B`; annexes C–I are informative — mark `status: informative`, nothing to implement). Per file `docs/spec/<ch>/<clause>.md`:

1. **Read the clause text in the file** — the body IS the spec text. For comma-level wording read the PDF pages named in the frontmatter (`mempalace_get_pdf_pages`).
2. **Find the implementation and annotate it.** Every function implementing normative behaviour carries a doc comment citing **the CIM 009 clause number + a one-line summary of the rule**, e.g. `/// 5.6.6 Delete Entity: 204 on success, ResourceNotFound 404 if absent.` **FORBIDDEN as normative citation: internal documents** — never `(see docs/deep-analysis.md §9.3)`, never `claude.md §…`, never tasks.md. Internal docs may be cited for *architecture* decisions only; the requirement's source is always the spec clause. Existing comments citing internal docs as the requirement are fixed on touch.
3. **Verify the FULL normative behaviour** against the clause text: every SHALL, every error case (type + status per Table 6.3.2-1), every output member. A gap → implement it now, or set `status: partial` with the gap NAMED in `notes:`. Never mark `implemented` with a known gap.
4. **Unit tests for the clause's own rules and edge cases** — boundary values, invalid input → the exact spec error type, empty/absent optional members, multi-instance `datasetId` where applicable, tenant isolation. Test names/doc comments cite the clause number.
5. **Robot Framework TPs — check for existing ones FIRST.** The file's `robot:` list + `grep -r "5_6_6" ngsi-ld-test-suite/TP/` (clause tag form). Only write a NEW TP for normative surface no existing TP covers — duplicating an ETSI TP is noise. New TPs go in the suite fork following its conventions: `[Documentation]` quotes the clause requirement briefly, `[Tags]` carries the clause number (`5_6_6` form) — that tag is what feeds `robot:` back into the ledger. **Edge-case TPs are mandatory**, not optional: error paths, boundary inputs, the cases the official 686 TPs skip (that long tail is exactly where §4.1's Scorpio violations lived).
6. **Update the ledger file**: `status:` + `evidence:` (code/test anchors) + `notes:` (dates, named gaps, spec doubts); `python3 dev/spec.py robot` refreshes the TP list. Suspected suite/spec defect → prove it from the clause text first, log in `error.md` + `testsuite-doubts.md`, never hack the broker to a broken test (§ETSI guide).
7. **One clause = one commit**: code, unit tests, Robot TPs, ledger file — message cites the clause number (`5.6.6:` prefix). Validation per run policy: one store mode locally, the CI 4×8 matrix is the authority.
8. **The audit loop starts no brokers and no stacks** (user rule, 2026-08-10): the clause's evidence is its unit tests; Robot TPs are validated by the standard pipeline (CI matrix / an explicitly requested `dev/etsi-pipeline.sh` run), never by ad-hoc local broker instances.

---

## 1. Targets (the contract this design must meet)

| Dimension | Target | Notes |
|---|---|---|
| Entities | 100,000,000 | current-state, one Postgres cluster |
| Tenants | 10,000 | **one shared schema**, `tenant_id` column on every row — no schema-per-tenant, no database-per-tenant |
| Subscriptions | 100,000 per context broker | HTTP callback + MQTT delivery (WebSocket deferred, see below) |
| WebSocket connections | 100,000 per context broker | **DEFERRED — not in v1.** Design stays WS-ready (per the `ngsi-ld-ws` binding draft) so it can land later without redesign |
| CSource registrations | 100,000+ per context broker | broad federation: one tenant may register hundreds–thousands of context sources; matching stays index-shaped and fan-out bounded (§16.7) |
| Broker memory | < 500 MB RSS | per broker process, at full load |
| Postgres memory | < 16 GB | one instance; PostGIS required. **TimescaleDB optional** — the broker runs with or without it (two temporal-store modes, §8.2); the 16 GB target assumes Timescale compression, plain-Postgres mode needs more disk/RAM headroom at the same retention |
| Compliance | full NGSI-LD (ETSI CIM 009) | validated against the ETSI conformance suite Scorpio uses |
| HA | yes | stateless broker pods, NATS JetStream messaging, Postgres primary/replica |

Non-goals for v1: **the WebSocket binding** (decision 2026-08-03 — deferred; the architecture keeps the seams for it: one matcher for all bindings, sink-per-`endpoint.uri`-scheme, the outbox pattern), multi-region federation of Antares clusters (registration-based federation per spec is in scope, cluster-of-clusters is not), CBOR body encoding, per-tenant physical isolation.

### 1.1 Why these numbers are consistent

- 100M entities / 10,000 tenants = 10k entities per tenant average — a mid-size city deployment per tenant, now at national-platform count. Skew is expected (one tenant with 10M entities must not starve the rest); the design handles skew via indexes and fair queueing, not quotas (quotas are a v2 policy knob).
- 100,000 subscriptions over 100M entities makes index-shaped matching (per changed entity, candidate subs via type/attr/tenant lookups in O(log n)) non-negotiable — a naive scan-per-change is ~10⁵ × change-rate evaluations/s.
- 100,000 WS connections ≈ 100,000 subscriptions *(deferred with WS — kept as the sizing assumption the reserved budget is based on)*: some WS clients are query/command clients, some carry several subscriptions; per-connection cost must stay ~15–30 KB idle, and subscription state must live in the shared matcher, not per-connection.

> **Targets raised 10× on 2026-08-10** (user decision, commit `b2700ee`). The §2 budgets below were derived for the ORIGINAL numbers and are now stale in at least two places: the in-memory subscription mirror (100k × ~3 KB ≈ 300 MB — most of the 500 MB RSS by itself; the mirror likely needs per-tenant lazy loading + LRU like the registration mirror) and the registration mirror line. Re-deriving §2 against the new targets is an open task — measured, not guessed; until then §2 documents the old contract.

---

## 2. Capacity budgets (the math behind 500 MB / 16 GB)

### 2.1 Broker process — 500 MB RSS budget

| Consumer | Budget | Sizing rationale |
|---|---|---|
| 10k WS connections | ~150 MB **(reserved — WS deferred)** | ~15 KB/conn: tokio task (~2 KB) + tungstenite read/write buffers tuned to 4 KB each (defaults are 128 KB-class — must be configured down). Until WS lands this is headroom, which is why the v1 gate can stay at Scorpio's stricter 350 MiB |
| HTTP server (hyper/axum) | ~50 MB | request buffers bounded; streaming bodies, no full-buffer-then-parse for large batches |
| @context cache | ~50 MB | LRU, cap ~256 parsed contexts; core context pinned; parsed-context sharing via `Arc` |
| Subscription registry (in-memory mirror) | ~40 MB | 10,000 subs × ~3 KB compiled form (parsed q= AST, geo prepared, type/attr index entries) — mirrored from Postgres, invalidated via NATS JetStream KV watch |
| SQL pool + prepared statements | ~30 MB | one shared pool (shared schema ⇒ no per-tenant pools — a structural win over Scorpio's tenant→pool map) |
| NATS client + JetStream buffers | ~30 MB | bounded consumer prefetch; explicit ack |
| JSON working set | ~80 MB | bounded concurrency on expansion/compaction; per-request allocations dropped at response end |
| Registration mirror | ~20 MB | up to ~10k compiled RegistrationEntries across tenants (~1–2 KB each: parsed info, prepared geometry, op bitmask) — broad-federation sizing (§16.7) |
| Allocator slack / fragmentation | ~50 MB | jemalloc with decay tuned; RSS ≈ live × 1.2 target |

Rules that make the budget hold:
1. **Every buffer is bounded and configured** — WS frame size, HTTP body size, batch op count, JetStream prefetch, notification queue depth. Any unbounded queue is a 3am page.
2. **Backpressure over buffering** — slow WS consumer ⇒ notification coalescing then disconnect with `1013 Try Again Later`, never unbounded queueing (see WS binding close-code registry).
3. **One copy of the truth in memory** — entities live in Postgres, period. The broker holds no entity cache in v1. (`ponytail:` no entity cache; add a per-request read-through cache only if p99 read latency measurably needs it.)

**Measured memory profile (release binary, 2026-08-07)** — the facts future optimization work starts from:
- Idle RSS 16.7 MiB: **resident binary code 10.9 MiB** (largest idle consumer — hence `strip = "symbols"` + `lto = "thin"` in `[profile.release]`; tradeoff: panic backtraces show addresses, not names), live heap ~5.4 MB (core context, runtime, caches), libc ~1.5 MiB.
- **jemalloc purges only on allocation activity**: after a burst + purge, ~48 MiB of dead pages parked forever on an idle broker. Fix shipped in the Dockerfile: `MALLOC_CONF`/`_RJEM_MALLOC_CONF=background_thread:true,dirty_decay_ms:10000,muzzy_decay_ms:10000` (measured: 50 → 33 MiB and decaying). Steady load is unaffected (working set never idles 10 s); the only cost is page re-faulting after >10 s quiet spells.
- **Memory store costs ~9.4 KB/entity** (pointer-heavy expanded `serde_json::Value` trees) — fine for its dev/ETSI role; postgres/timescale hold entities in the DB, not broker RAM. Storing serialized bytes would be ~5× cheaper but taxes every read; don't, unless the store's role changes.
- **Telemetry is a runtime switch**: `ANTARES_TELEMETRY=1` builds the Prometheus recorder + sampler + OTLP at startup; the default (CI/ETSI, dev image) constructs none of it — zero telemetry RAM, `/q/metrics` → 404. Cost when on at idle: <1 MiB. One build for both states.
- tokio-console (`console` feature) only arms under `cfg(tokio_unstable)` — an `--all-features` build without the RUSTFLAGS must boot as a no-op, never panic (this panic was the recurring CI workspace failure).

### 2.2 Postgres — 16 GB memory budget

| Setting | Value | Why |
|---|---|---|
| `shared_buffers` | 4 GB | 25% rule; hot set = indexes + recent entities |
| `work_mem` | 16 MB (per sort) | dynamic-SQL queries with JSONB predicates; keep per-op small, rely on indexes |
| `maintenance_work_mem` | 512 MB | GIN index builds on 10M-row tables |
| `effective_cache_size` | 12 GB | planner hint |
| `max_connections` | 200 | broker uses one pool ≤ 80; headroom for temporal writer + ops |
| TimescaleDB background workers | 8 | compression + retention jobs (timescale mode only — plain mode runs these as broker jobs, §8.2) |

Disk sizing (informative, not a hard target): 10M entities × ~1.5 KB TOASTed (LZ4) ≈ 15 GB heap + ~5 GB indexes (targeted GIN `jsonb_path_ops`, not a kitchen-sink GIN) ; temporal hypertable with Timescale native compression at 10–20× runs a few GB per billion attribute instances retained. Working set that must fit in the 16 GB RAM: indexes + last-days temporal chunks + hot entity pages.

---

## 3. Multi-tenancy model — shared schema, `tenant_id` everywhere

Scorpio isolates tenants physically (separate database per tenant, created on first use, plus a pool per tenant). At 1,000 tenants that is 1,000 databases and a pool explosion — the exact thing Antares must not do.

**Antares: one schema, every table carries `tenant_id`, enforced by Postgres Row-Level Security.**

```sql
-- every table
tenant_id text NOT NULL DEFAULT 'default',  -- from NGSILD-Tenant header, 'default' when absent
PRIMARY KEY (tenant_id, id),                -- tenant-leading composite keys everywhere

-- RLS as the safety net (defense in depth; the query layer always filters too)
ALTER TABLE entities ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON entities
  USING (tenant_id = current_setting('antares.tenant', true));
```

- The broker sets `SET LOCAL antares.tenant = $1` at transaction start (one round trip, pipelined with the query). Application SQL **also** always includes `tenant_id = $1` — RLS is the belt, explicit predicates are the suspenders and what the planner uses.
- Tenant-leading composite indexes make every per-tenant query a tight index range; the 1M-entity tenant and the 100-entity tenant coexist on the same B-tree without interference.
- Tenant provisioning = an INSERT into a `tenants` table (auto-on-first-write, matching NGSI-LD's implicit-tenant behavior). No DDL at runtime — no migrations per tenant, no pool per tenant, backup/restore is one database.
- Per-tenant erasure: `DELETE ... WHERE tenant_id = $1` + Timescale chunk-aware delete. (GDPR-grade hard isolation is explicitly traded away — that is the stated requirement.)

### 3.1 Per-tenant concurrency & thread safety

The design goal is that **no per-tenant lock exists anywhere** — tenants share one pool, one schema, one process, and stay isolated by data, not by synchronization. What makes that safe:

1. **No per-tenant mutable process state.** The broker holds no tenant-owned maps to corrupt (the Scorpio failure class §14.1 — racy tenant→pool HashMaps, check-then-act MQTT client maps). The only shared mutable structures are the matcher/registry mirrors: single-writer (the KV/registry watcher task) publishing immutable snapshots via `ArcSwap`; request tasks only ever read a snapshot. Rust's `Send`/`Sync` bounds make a data race a compile error, not a review item.
2. **Concurrent writes to the same entity serialize in Postgres, not in the broker.** Read-modify-write operations (append, partial update, merge — logic now in Rust, §4) run inside one transaction that takes the entity row lock (`SELECT … FOR UPDATE`) first; two racing PATCHes to one entity execute in some serial order, and neither is lost. The unit of serialization is the *entity row* — a tenant's other 10k entities proceed in parallel, and one tenant's hot entity cannot block another tenant at all.
3. **Every entity write bumps `version`** (bigint, incremented under that row lock). The `ChangeEvent` carries it, which solves the multi-pod ordering race the outbox alone cannot: two pods may publish commits out of order, but consumers are ordering-tolerant by construction. What happens to an old (late/stale) version depends on the consumer:
   - **Matcher: processes it anyway.** Each event is self-contained (its own `payload` + `prev_payload` pair), and the matcher projects no state — a late v4 arriving after v5 still evaluates and still fires the notification for the v4 change (slightly late, carrying v4's state — ordinary at-least-once reality; dropping it would silently lose a watched-attribute change, which is worse).
   - **Temporal recorder: order-free by shape.** History is append-structured; a late event just upserts its attribute instances on the unique key. No last-writer-wins needed.
   - **State-projecting consumers** (WS conflation maps, future caches, LDES bridges): these are the only ones that apply last-writer-wins on `(entity, version)` — a stale version is discarded, which is exactly right for a "current state" projection.
   - **The delete/recreate trap, handled explicitly**: deleting an entity and recreating it restarts `version` at 1, so a naive LWW would drop the new entity's events as "old". The `entityDeleted` event is therefore a **fence** — state-projecting consumers reset their high-water mark for that entity on delete — and the event's ordering key is `(incarnation, version)`, where `incarnation` is the row's `created_at`. Old-vs-new incarnation is never ambiguous.
   - **The old row data itself**: `entities` keeps only the latest state — superseded versions live on as temporal attr instances (if auto-recording is on) and as `prev_payload` in the event, then die. Postgres-side, overwritten rows are dead tuples: the 10M-row table gets explicit autovacuum tuning + a lowered `fillfactor` for HOT updates (a JSONB-update-heavy table left at defaults is how you get bloat-driven "irregular insert times" — Scorpio issue #573's suspect class).
   - The version is internal in v1; exposing it as an HTTP `ETag`/`If-Match` precondition is a cheap future feature the schema already supports (clients doing read-modify-write get optimistic concurrency for free).
4. **Tenant auto-creation is a one-row upsert** — `INSERT … ON CONFLICT DO NOTHING`. Two concurrent first-writes both succeed; compare Scorpio's version of this race: concurrent `CREATE DATABASE` + Flyway deadlocking (issue #653, §14.7).
5. **`SET LOCAL antares.tenant` cannot leak across tenants**: it is transaction-scoped, and a pooled connection is exclusively checked out for the duration of that transaction (sqlx guarantee). There is no code path that issues session-level `SET` (§6.2), so a recycled connection carries no tenant residue.
6. **Scheduled work is single-winner by lock, not by leader election**: interval-subscription firings and plain-mode partition jobs claim rows via `SELECT … FOR UPDATE SKIP LOCKED` — N broker instances race, exactly one wins each job, no coordinator. (`ponytail:` skip-locked job rows; a real scheduler only if job volume ever demands it.)

Test hooks (§9.5): a concurrency suite runs parallel PATCH/append/merge storms against one entity and asserts final-state convergence + no lost updates; the 2-instance e2e includes out-of-order publish injection to prove version-LWW holds.

---

## 4. Reference: Scorpio component map → Antares modules

Scorpio 7.0.0 (Quarkus/Mutiny, Vert.x PgClient, SmallRye messaging) decomposes into 13 Maven modules, 9 of them deployable services plus AllInOneRunner. The remap:

| Scorpio module (port) | Responsibility | Antares home |
|---|---|---|
| EntityManager (10090) | entity CUD + batch + write forwarding | `antares-api` (handlers) + `antares-registry` (forwarding) |
| QueryManager (10091) | query/retrieve, types/attrs discovery, entityMaps, federated merge | `antares-api` + `antares-sql` + `antares-registry` |
| SubscriptionManager (10092) | sub CRUD, matching, HTTP/MQTT delivery, cross-instance sync | `antares-api` + `antares-matcher` + `antares-notifier` |
| RegistryManager (10093) | CSR CRUD, auto-federation bootstrap | `antares-registry` |
| RegistrySubscriptionManager (10094) | csourceSubscriptions (5.11) | `antares-registry` + `antares-matcher` |
| HistoryEntityManager (10095) | temporal writes + auto-recording from ENTITY topic | `antares-temporal` (change-stream consumer) |
| HistoryQueryManager (10096) | temporal queries/aggregation/forwarding | `antares-temporal` + `antares-sql` |
| AtContextServer (10097) | @context cache/registry (5.13) | `antares-jsonld` + `antares-api` |
| InfoManager | `/info/sourceIdentity` | `antares-api` (one handler) |
| Commons | vendored reactive jsonld-java fork (~11k LOC), query AST→SQL terms, HttpUtils (2k LOC incl. six 207 builders), ConnectionManager | dissolved into `antares-jsonld`, `antares-ql`, `antares-sql`, `antares-model` |
| AllInOneRunner | all-in-one JVM, in-memory messaging | `antares-broker` binary with `--roles` (§9) |
| SnsFanoutMessaging | AWS SNS/SQS broadcast shim | dropped — NATS only, one transport (`ponytail:` multi-transport abstraction cut; add a second bus only if a deployment ever demands it) |

Two Scorpio facts that shape the port:

1. **The DB is not a dumb store**: ~38 PL/pgSQL functions + 6 triggers implement merge-patch, append/datasetId semantics, NGSI-LD-null deletion, batch upsert, geo/scope extraction, and the csource→csourceinformation explosion. **Antares decision: business logic moves to Rust with plain parameterized DML** (UNNEST-based multi-row statements for batches); triggers are replaced by computing extracted columns in Rust at write time. Rationale: testable in unit tests, no hidden logic layer, no trigger-order traps — and the Scorpio PL/pgSQL bodies remain available as the semantic oracle when porting.
2. **The JSON-LD layer is a vendored fork**, not a library — Scorpio's `NGSIObject` does NGSI-LD-specific validation *during* expansion. `antares-jsonld` must budget for the same: the `json-ld` crate handles W3C processing, but NGSI-LD structural validation (attribute forms, datasetId rules, null handling) is our own pass.

### 4.1 Lessons ledger — Scorpio's memory analysis (2026-07-25) as Antares design rules

The 17-finding audit of Scorpio (`memory-optimization-analysis.md`) is effectively a list of traps this design must not re-implement. Distilled to rules:

| Scorpio finding | Antares rule |
|---|---|
| R1/R2: no heap/container limits anywhere | ship with memory limits in every compose/K8s manifest; CI memory gate from day 1 |
| R4/L7/J1b: caches with TTL but **no size cap**; no parsed-context cache at all | every cache has a max-size (moka enforces both); the parsed-context LRU is the centerpiece, not an afterthought |
| R10: broken group-id interpolation silently turned broadcast into load-balancing | consumer topology asserted at startup; 2-instance broadcast test in CI (§6.4) |
| L2/L3/L5/L6/L8: maps that only ever grow (tenant pools, sub state, MQTT clients, callback UUIDs) | shared-schema tenancy kills the pool map entirely; all sub/connection state has a delete path exercised by tests; MQTT client pool is bounded with eviction |
| L4a/L4b: expiry enforced in some paths, forgotten in others (expired interval subs notify forever; temporal forwards to expired CSRs) | expiry is checked in ONE place — the compiled registry/subscription mirror refuses to yield expired entries — so no call site can forget it |
| U1: 11 HTTP clients, zero timeouts, unbounded wait queues | one `reqwest` client, connect+request timeouts and bounded pool set at construction; notification sends always have a deadline |
| U2 + PRE_PROCESSING acks: no backpressure, loss on crash | JetStream explicit ack **after** processing; bounded prefetch (§6.4) |
| U3/U4: unbounded temporal aggregation and federated page accumulation | hard server-side bounds: temporal `Content-Range` 206 partials (spec-sanctioned by 6.3.10), federated next-link following capped by the caller's `limit` |
| J3/J11c: nothing streams — full RowSets and pretty-printed buffers in heap | axum streaming bodies + sqlx `fetch()` row streams for list endpoints |
| J4/J5: compaction mutates its input → defensive deep-copy epidemic | `antares-jsonld` compaction takes `&input`, returns a new document — enforced by Rust's borrow checker for free |
| J6/J11d: notification built *then* throttled; seconds-vs-ms bug (window 1000× short) | throttle/conflate decision before serialization; all durations are `std::time::Duration`, never raw ints |
| J7/J9: byte[]→String→byte[] round-trips; one context parse per batch entity | `Bytes` end-to-end on the bus; one context parse per batch request |

Scorpio's spec-violation findings (expired interval subs notifying forever, past-`expiresAt` accepted on create, temporal forwarding to expired CSRs, throttling-unit bug, entity-map csourceid mixup B1) all become explicit test cases in the Antares suite — they're exactly the class of bug the ETSI TPs don't fully catch.

## 5. NGSI-LD compliance surface

Baseline: **CIM 009 V1.9.1 (2025-07)** — `/workspace/etsi-cim-specs/gs_cim009v010901p.pdf` (also vendored in Scorpio's `pdf/`). *Note: NGSI-LD 2.0 was published March 2026 as TS 104 175 (Core API) + TS 104 176 (HTTP Binding) + TS 104 243 (MQTT) under the new ETSI TC DATA — V1.9.1 stays the v1 gate because the conformance suite targets it; the 2.0 migration plan and pre-adopted items are in §15.1.* Conformance instruments: CIM 012 (test-suite structure), CIM 013 (test purposes, **686 leaf TPs**), CIM 014 (Robot Framework suite), CIM 029 (ICS/PICS checklist), CIM 053 (distributed-ops TPs), CIM 054 (IOP) — all V3.1.1/V1.1.1 2025-07, all in `/workspace/etsi-cim-specs/`.

### 5.1 API surface — 38 resource clauses, 62 method+path combinations

Root `{apiRoot}/ngsi-ld/v1/`. Full-compliance surface (CIM 009 Table 6.2-1):

| Group | Endpoints |
|---|---|
| Entities | `POST/GET/DELETE /entities/` (DELETE = **Purge**, new in V1.9.1) ; `GET/PUT/PATCH/DELETE /entities/{id}` (PATCH = **Merge**) ; `POST/PATCH /entities/{id}/attrs/` ; `PATCH/PUT/DELETE /entities/{id}/attrs/{attrId}` |
| Batch | `POST /entityOperations/{create,upsert,update,delete,merge,query}` |
| Temporal | `POST/GET /temporal/entities/` ; `GET/DELETE /temporal/entities/{id}` ; `POST /temporal/entities/{id}/attrs/` ; `DELETE .../attrs/{attrId}` ; `PATCH/DELETE .../attrs/{attrId}/{instanceId}` ; `POST /temporal/entityOperations/query` |
| Subscriptions | `POST/GET /subscriptions/` ; `GET/PATCH/DELETE /subscriptions/{id}` |
| CSource registrations | `POST/GET /csourceRegistrations/` ; `GET/PATCH/DELETE /csourceRegistrations/{id}` |
| CSource subscriptions | `POST/GET /csourceSubscriptions/` ; `GET/PATCH/DELETE /csourceSubscriptions/{id}` |
| Discovery | `GET /types/`, `/types/{type}`, `/attributes/`, `/attributes/{attrId}` |
| @context mgmt | `POST/GET /jsonldContexts` ; `GET/DELETE /jsonldContexts/{id}` |
| EntityMaps (distributed ops) | `GET/POST /entityMaps/` ; `GET/PATCH/DELETE /entityMaps/{id}` ; `GET/POST /temporal/entityMaps/` |
| Info | `GET /info/sourceIdentity` |
| **Snapshots (new V1.9.1)** | `POST/DELETE /snapshots/` ; `GET/PATCH/DELETE /snapshots/{id}` ; `POST /snapshots/{id}/clone` |

Scorpio gap notes that Antares inherits as scope decisions: Scorpio has **no `/snapshots` implementation at all**, and serves `/entityMap` (singular) where V1.9.1 says `/entityMaps` — Antares implements the spec spelling and treats Snapshots as a v1.x milestone, not v1.0 (they're new in V1.9.1 and not yet in the Robot suite Scorpio gates on).

### 5.2 Cross-cutting requirements (where compliance is actually won or lost)

- **@context negotiation (6.3.5)**: `application/json` ⇒ context from `Link` header only; `application/ld+json` ⇒ context from body only; mixing ⇒ `BadRequestData`. Responses in `application/json` MUST carry the Link header. Errors and 207s are always `application/json` with fully-qualified names.
- **Error mapping (Table 6.3.2-1)**: AlreadyExists 409, BadRequestData/InvalidRequest 400, LdContextNotAvailable 504, NoMultiTenantSupport 501, NonexistentTenant 404, OperationNotSupported 422, TooComplexQuery/TooManyResults 403; error type base URI is `https://uri.etsi.org/ngsi-ld/errors/` (https since V1.9.1). Bare 411/415/406 for precondition failures.
- **Representations**: `format` ∈ normalized|concise|simplified(=keyValues); `options` is the deprecated spelling and `format` wins on conflict. Temporal adds temporalValues|aggregatedValues. `options=sysAttrs` adds createdAt/modifiedAt/expiresAt(/deletedAt).
- **GeoJSON (6.3.15)**: `Accept: application/geo+json` valid only on Retrieve/Query Entities (else 406); FeatureCollection output; `geometryProperty` selects the top-level geometry.
- **Projection & filters**: `pick`/`omit` (with `attrs` as deprecated synonym), `q`, `scopeQ`, `datasetId` (incl. `@none`), `lang`, `join`+`joinLevel`, `containedBy`, `expandValues`/`jsonKeys`, **Ordered Entities** (`orderBy`, `orderFrom`, `orderGeometry`, `collation` — new V1.9.1), `splitEntities`, Purge uses `keep`/`drop` instead of pick/omit.
- **Pagination (6.3.10)**: `limit`/`offset` + RFC 8288 `Link rel="next"/"prev"`; `count=true` ⇒ `NGSILD-Results-Count`; `limit=0` without count ⇒ 400. Temporal pagination is different: **206 Partial Content + `Content-Range` in DateTime units**, direction driven by `lastN`.
- **Tenancy (6.3.14)**: `NGSILD-Tenant` echoes in responses and in notifications of tenant-scoped subscriptions; `NonexistentTenant` 404 (read paths) vs implicit creation (write paths).
- **Version negotiation (new V1.9.1)**: `Prefer: ngsi-ld=<version>` ⇒ `Preference-Applied` + **203 Non-Authoritative**; a Subscription's `ngsildConformance` pins notification format.
- **Distributed ops**: `NGSILD-EntityMap` header, RFC 7230 `Via` chains with `hostAlias` pseudonyms, `contextSourceInfo`, registration-scope narrowing on forwards (spec-mandated per 4.3.6.1 — a lesson already fought over in Scorpio).
- **@context caching (6.3.16)**: honour `Expires`/`Cache-Control` on downloaded contexts. No compression requirement exists in CIM 009 — compression is a binding concern (WS binding uses permessage-deflate).

### 5.3 Conformance validation plan

**The suite validates; it is not the requirements source.** Requirements come from the clause ledger in §5.4 (rule §0.2) — the Robot suite runs after a clause is implemented, to confirm it. Antares gates on the same instrument as Scorpio: the ETSI Robot Framework suite (`forge.etsi.org/rep/cim/ngsi-ld-test-suite`, run via the fork + `dev/etsi-serial.sh` recipe — 8 suites: CommonBehaviours, CI/Consumption, CI/Provision, CI/Subscription, ContextSource, jsonldContext, DistributedOperations, IOP against a 5-broker stack). Leaf-TP distribution to plan effort by: CI/Prov 235, CI/Cons 166, CI/SUB 87, CS/REGSUB 54, CTX 61, CS/DISC 30, CB/HTTP 25, CS/REG+CSR 28. Scorpio's CI additionally gates on **peak broker RSS ≤ 350 MiB** during the serial run — Antares adopts the same gate (it's stricter than the 500 MB target and free to reuse). Secondary harness: Scorpio's Postman collections (`api-test.json`, 1,584 requests) for fast smoke coverage.

### 5.4 Spec-first implementation ledger — CIM 009 V1.9.1, clause by clause

This is the requirements list (rule §0.2): implement each item from its clause text, check it off only when the full normative behaviour (all SHALLs, error cases, and output data of that clause) is implemented and unit-tested — then validate with the Robot suite. Each checkbox maps to a file in `docs/spec/` (the per-clause full-text ledger, §0.3 — replaced `docs/ics.yaml` 2026-08-10). Items marked **v1.x** are deliberately staged after v1.0 (§5.1 scope decisions); everything else is v1.0 scope.

#### 5.4.1 Clause 4 — framework, representations, languages (`antares-model`, `antares-jsonld`, `antares-ql`)

Foundations (read before writing `antares-model`):

- [ ] 4.2 Information model: meta-model (4.2.2), cross-domain ontology (4.2.3), domain models & instantiation (4.2.4)
- [ ] 4.3.5 API structure & implementation options — the broker roles Antares claims (Context Broker + Registry + Discovery + Subscription + Temporal)
- [ ] 4.3.6 Distributed-operation semantics: additive registrations (4.3.6.2), proxied registrations (4.3.6.3), limiting cascading ops (4.3.6.4), extra info when contacting a Context Source (4.3.6.5), pre/post-processing of that info (4.3.6.6), unitary distributed query/retrieve (4.3.6.7), Context-Source payload backwards compatibility (4.3.6.8) → `antares-registry`
- [ ] 4.3.7 Snapshots concept — **v1.x**
- [ ] 4.4 Core and user @context, precedence rules → `antares-jsonld`
- [ ] Annex A (normative) identifier considerations; Annex B (normative) core @context — pinned at build time

Representations (4.5) — each a serialize/deserialize pair in `antares-model` + `antares-jsonld`, round-trip tested:

- [ ] 4.5.1 Entity representation
- [ ] 4.5.2 Property: normalized (4.5.2.2), concise (4.5.2.3)
- [ ] 4.5.3 Relationship: normalized (4.5.3.2), concise (4.5.3.3)
- [ ] 4.5.4 Simplified representation (keyValues)
- [ ] 4.5.5 Multi-attribute support: datasetId instance sets (4.5.5.1), conflicting transient entities (4.5.5.2), conflicting attributes (4.5.5.3)
- [ ] 4.5.6–4.5.8 Temporal representation of Entity / Property / Relationship
- [ ] 4.5.9 Simplified temporal representation (temporalValues)
- [ ] 4.5.10–4.5.12 Entity type list / detailed type list / type information
- [ ] 4.5.13–4.5.15 Attribute list / detailed attribute list / attribute information
- [ ] 4.5.16 GeoJSON representation: top-level geometry selection algorithm (4.5.16.1), single entity (4.5.16.2), multiple entities (4.5.16.3)
- [ ] 4.5.17 Simplified GeoJSON representation (single + multiple)
- [ ] 4.5.18 LanguageProperty: normalized (4.5.18.2), concise (4.5.18.3) — incl. the deleted-LP `{"@none":"urn:ngsi-ld:null"}` map form (§14 lesson)
- [ ] 4.5.19 Aggregated temporal representation + aggregation-function behaviours (4.5.19.1)
- [ ] 4.5.20 VocabProperty: normalized/concise
- [ ] 4.5.21 ListProperty: normalized/concise
- [ ] 4.5.22 ListRelationship: normalized/concise
- [ ] 4.5.23 Linked Entity Retrieval: inline (4.5.23.2), flattened (4.5.23.3) — `join`, `joinLevel`, `containedBy`
- [ ] 4.5.24 JsonProperty: normalized/concise
- [ ] 4.5.25 EntityMap representation

Restrictions & value spaces (4.6–4.8) — the `antares-jsonld::validate` pass:

- [ ] 4.6.1 supported text encodings; 4.6.2 supported names; 4.6.3 supported datatypes for Values; 4.6.4 supported content; 4.6.5 LanguageMap datatypes; 4.6.6 ordering of duplicate Entities in arrays
- [ ] 4.7 Geospatial: GeoJSON geometries (4.7.1), their JSON-LD representation (4.7.2), concise GeoProperty (4.7.3)
- [ ] 4.8 Temporal properties (`observedAt`, system timestamps — value rules)

Languages — each a parser + in-memory evaluator (matcher) + SQL compiler (`antares-ql` → `antares-sql`), proptest round-trips per §9.5:

- [ ] 4.9 NGSI-LD Query Language (`q=`)
- [ ] 4.10 Geoquery Language (`georel`, `geometry`, `coordinates`, `geoproperty`)
- [ ] 4.11 Temporal Query Language (`timerel`, `timeAt`, `endTimeAt`, `timeproperty`)
- [ ] 4.12 Pagination; 4.13 counting results
- [ ] 4.14 Multiple tenants
- [ ] 4.15 Language Filter (`lang=`)
- [ ] 4.16 Multiple entity types; 4.17 Entity Type Selection Language
- [ ] 4.18 Scopes; 4.19 Scope Query Language (`scopeQ`)
- [ ] 4.20 Distributed Operation names (the Operation enum + operation groups → the `ops` bitmask, §8.3)
- [ ] 4.21 Attribute Projection Language (`pick`/`omit`, dotted paths)
- [x] 4.22 Transient storage of Entities and Attributes (`expiresAt`) — read-boundary invalidity + per-backend GC sweep (2026-08-08)
- [ ] 4.23 Entity Ordering: datatype comparison order (4.23.2), ordering language (`orderBy`, `orderFrom`, `orderGeometry`, `collation`) (4.23.3)

#### 5.4.2 Clause 5.2–5.4 — data types (`antares-model`, one Rust type per clause, §9.1 naming)

- [ ] 5.2.2 common members; 5.2.3 @context; 5.2.4 Entity; 5.2.5 Property; 5.2.6 Relationship; 5.2.7 GeoProperty
- [ ] 5.2.8 EntityInfo; 5.2.9 CSourceRegistration; 5.2.10 RegistrationInfo; 5.2.11 TimeInterval
- [ ] 5.2.12 Subscription; 5.2.13 GeoQuery; 5.2.14 NotificationParams incl. output-only members (5.2.14.2); 5.2.15 Endpoint
- [ ] 5.2.16 BatchOperationResult; 5.2.17 BatchEntityError; 5.2.18 UpdateResult; 5.2.19 NotUpdatedDetails
- [ ] 5.2.20 EntityTemporal; 5.2.21 TemporalQuery; 5.2.22 KeyValuePair; 5.2.23 Query (the POST-query body)
- [ ] 5.2.24 EntityTypeList; 5.2.25 EntityType; 5.2.26 EntityTypeInfo; 5.2.27 AttributeList; 5.2.28 Attribute
- [ ] 5.2.29 Feature; 5.2.30 FeatureCollection; 5.2.31 FeatureProperties (GeoJSON output types)
- [ ] 5.2.32 LanguageProperty; 5.2.33 EntitySelector; 5.2.34 RegistrationManagementInfo
- [ ] 5.2.35 VocabProperty; 5.2.36 ListProperty; 5.2.37 ListRelationship; 5.2.38 JsonProperty
- [ ] 5.2.39 EntityMap; 5.2.40 Context Source Identity; 5.2.43 OrderingParams; 5.2.44 AggregationParams
- [ ] 5.2.41 Snapshot; 5.2.42 ExecutionResultDetails — **v1.x**
- [ ] 5.3.1 Notification; 5.3.2 CSourceNotification; 5.3.3 TriggerReasonEnumeration; 5.3.4 SnapshotNotification (**v1.x**)
- [ ] 5.4 NGSI-LD Fragments (the partial-update / merge input shapes)

#### 5.4.3 Clause 5.5 — common behaviours (cross-cutting; `antares-api` + `antares-jsonld` + `antares-sql`)

- [ ] 5.5.2 error types; 5.5.3 error response payload (ProblemDetails)
- [ ] 5.5.4 general NGSI-LD validation
- [ ] 5.5.5 default @context assignment
- [ ] 5.5.6 operation execution and generic error handling
- [ ] 5.5.7 term↔URI expansion and compaction
- [ ] 5.5.8 Partial Update Patch behaviour
- [ ] 5.5.9 Pagination behaviour: general (5.5.9.1), limit/offset (5.5.9.2), with Entity maps (5.5.9.3)
- [ ] 5.5.10 Multi-Tenant behaviour
- [ ] 5.5.11 duplicate Entity instances in one array — per-batch-op rules (5.5.11.1 create, .2 upsert, .3 update, .4 delete, .5 merge)
- [ ] 5.5.12 Merge Patch behaviour
- [ ] 5.5.13 limiting operations to local scope (`local=true`)
- [ ] 5.5.14 distributed transactional behaviour
- [ ] 5.5.15 Snapshot behaviour — **v1.x**

#### 5.4.4 Clause 5.6 — Context Information Provision (one public fn per clause, §9.1)

- [ ] 5.6.1 Create Entity → `create_entity`
- [ ] 5.6.2 Update Attributes → `update_attributes`
- [ ] 5.6.3 Append Attributes → `append_attributes`
- [ ] 5.6.4 Partial Attribute Update → `partial_attribute_update`
- [ ] 5.6.5 Delete Attribute → `delete_attribute`
- [ ] 5.6.6 Delete Entity → `delete_entity`
- [ ] 5.6.7 Batch Entity Creation → `batch_create`
- [ ] 5.6.8 Batch Entity Creation or Update → `batch_upsert`
- [ ] 5.6.9 Batch Entity Update → `batch_update`
- [ ] 5.6.10 Batch Entity Delete → `batch_delete`
- [ ] 5.6.11 Upsert Temporal Evolution → `upsert_temporal_entity`
- [ ] 5.6.12 Add Attributes to Temporal Evolution → `add_temporal_attributes`
- [ ] 5.6.13 Delete Attribute from Temporal Evolution → `delete_temporal_attribute`
- [ ] 5.6.14 Modify Attribute instance in Temporal Evolution → `modify_temporal_instance`
- [ ] 5.6.15 Delete Attribute instance from Temporal Evolution → `delete_temporal_instance`
- [ ] 5.6.16 Delete Temporal Evolution → `delete_temporal_entity`
- [ ] 5.6.17 Merge Entity → `merge_entity`
- [ ] 5.6.18 Replace Entity → `replace_entity`
- [ ] 5.6.19 Replace Attribute → `replace_attribute`
- [ ] 5.6.20 Batch Entity Merge → `batch_merge`
- [ ] 5.6.21 Purge Entities → `purge_entities` (`keep`/`drop` params, not pick/omit)

#### 5.4.5 Clause 5.7 — Context Information Consumption

- [ ] 5.7.1 Retrieve Entity → `retrieve_entity`
- [ ] 5.7.2 Query Entities → `query_entities`
- [ ] 5.7.3 Retrieve Temporal Evolution → `retrieve_temporal_entity`
- [ ] 5.7.4 Query Temporal Evolution → `query_temporal_entities`
- [ ] 5.7.5 Retrieve Available Entity Types → `retrieve_entity_types`
- [ ] 5.7.6 Retrieve Details of Available Entity Types → `retrieve_entity_type_details`
- [ ] 5.7.7 Retrieve Available Entity Type Information → `retrieve_entity_type_info`
- [ ] 5.7.8 Retrieve Available Attributes → `retrieve_attributes`
- [ ] 5.7.9 Retrieve Details of Available Attributes → `retrieve_attribute_details`
- [ ] 5.7.10 Retrieve Available Attribute Information → `retrieve_attribute_info`
- [ ] 5.7.11 architecture-related aspects of types/attributes retrieval (distributed discovery)

#### 5.4.6 Clause 5.8 — Context Information Subscription (`antares-matcher` + `antares-notifier`)

- [ ] 5.8.1 Create Subscription (incl. rejection of past `expiresAt` — a named Scorpio violation, §4.1)
- [ ] 5.8.2 Update Subscription
- [ ] 5.8.3 Retrieve Subscription
- [ ] 5.8.4 Query Subscriptions
- [ ] 5.8.5 Delete Subscription
- [ ] 5.8.6 Notification behaviour: notificationTrigger semantics incl. deletion payload forms, `showChanges` (prev-payload), `sysAttrs`, `timeInterval` subscriptions, throttling (Duration-typed, §4.1), status/`timesSent`/`lastNotification`/`lastSuccess`/`lastFailure` bookkeeping, expiry enforcement at the single mirror yield point

#### 5.4.7 Clauses 5.9–5.12 — Context Source Registration, Discovery, Registration Subscriptions, Matching (`antares-registry`)

- [ ] 5.9.1 registration semantics; 5.9.2 Register Context Source → `register_context_source`; 5.9.3 Update → `update_csource_registration`; 5.9.4 Delete → `delete_csource_registration`
- [ ] 5.10.1 Retrieve Context Source Registration; 5.10.2 Query Context Source Registrations
- [ ] 5.11.2–5.11.6 Create / Update / Retrieve / Query / Delete Context Source Registration Subscription
- [ ] 5.11.7 CSource notification behaviour
- [ ] 5.12 Matching Context Source Registrations (the algorithm behind `csource_index`, §8.3)

#### 5.4.8 Clauses 5.13–5.16 — @contexts, EntityMaps, Identity, Snapshots

- [ ] 5.13.2 Add @context; 5.13.3 List @contexts; 5.13.4 Serve @context; 5.13.5 Delete and Reload @context (kinds `Hosted`/`Cached`/`ImplicitlyCreated` per 5.13.1)
- [ ] 5.14.1 Retrieve EntityMap; 5.14.2 Update EntityMap; 5.14.3 Delete EntityMap
- [ ] 5.14.4 Create EntityMap for Query Entities; 5.14.5 Create EntityMap for Query Temporal Evolution
- [ ] 5.15.1 Retrieve Context Source Identity Information (`/info/sourceIdentity`)
- [ ] 5.16.1–5.16.7 Snapshots: Create, Clone, Retrieve Status, Update Status, Delete, status notifications, Purge — **v1.x**

#### 5.4.9 Clause 6 — HTTP binding (`antares-api`; TS 104 176 successor layer)

- [ ] 6.2 global definitions and resource structure (`{apiRoot}/ngsi-ld/v1/`, Table 6.2-1)
- [ ] 6.3.2 error types; 6.3.3 reporting errors
- [ ] 6.3.4 HTTP request preconditions (411/415/406 bare status codes)
- [ ] 6.3.5 JSON-LD @context resolution (Link-header vs body rules per Content-Type; mixing ⇒ BadRequestData)
- [ ] 6.3.6 HTTP response common requirements (Link header on `application/json` responses, Content-Type negotiation)
- [ ] 6.3.7 representation of Entities (`format`/`options` params, precedence)
- [ ] 6.3.8 notification behaviour over HTTP; 6.3.9 csource notification behaviour
- [ ] 6.3.10 pagination (RFC 8288 next/prev; temporal: 206 + `Content-Range` in DateTime units, `lastN` direction)
- [ ] 6.3.11 sysAttrs; 6.3.12 simplified/aggregated temporal representation; 6.3.13 count (`NGSILD-Results-Count`)
- [ ] 6.3.14 tenant specification (`NGSILD-Tenant` header, echo rules)
- [ ] 6.3.15 GeoJSON representation (`Accept: application/geo+json` validity, `geometryProperty`)
- [ ] 6.3.16 expiration for cached @contexts (`Expires`/`Cache-Control`)
- [ ] 6.3.17 distributed-ops caching and timeout; 6.3.18 limiting distributed ops; 6.3.19 extra info to Context Sources (`Via`, `NGSILD-EntityMap`, `contextSourceInfo`)
- [ ] 6.3.20 invalid parameters
- [ ] 6.3.21 `Prefer: ngsi-ld=<version>` / `Preference-Applied` / 203 profile negotiation
- [ ] 6.3.22 snapshot specification — **v1.x**
- [ ] Resources, each method implemented per its clause: 6.4 `entities/` POST/GET/DELETE(Purge) · 6.5 `entities/{id}` GET/DELETE/PUT/PATCH(Merge) · 6.6 `entities/{id}/attrs/` POST/PATCH · 6.7 `entities/{id}/attrs/{attrId}` PATCH/DELETE/PUT · 6.8 `csourceRegistrations/` POST/GET · 6.9 `csourceRegistrations/{id}` GET/PATCH/DELETE · 6.10 `subscriptions/` POST/GET · 6.11 `subscriptions/{id}` GET/PATCH/DELETE · 6.12 `csourceSubscriptions/` POST/GET · 6.13 `csourceSubscriptions/{id}` GET/PATCH/DELETE · 6.14–6.17 `entityOperations/{create,upsert,update,delete}` POST · 6.18 `temporal/entities/` POST/GET · 6.19 `temporal/entities/{id}` GET/DELETE · 6.20 `temporal/entities/{id}/attrs/` POST · 6.21 `…/attrs/{attrId}` DELETE · 6.22 `…/attrs/{attrId}/{instanceId}` PATCH/DELETE · 6.23 `entityOperations/query` POST · 6.24 `temporal/entityOperations/query` POST · 6.25 `types/` GET · 6.26 `types/{type}` GET · 6.27 `attributes/` GET · 6.28 `attributes/{attrId}` GET · 6.29 `jsonldContexts/` POST/GET · 6.30 `jsonldContexts/{id}` GET/DELETE · 6.31 `entityOperations/merge` POST · 6.32 `entityMaps/{id}` GET/PATCH/DELETE · 6.33 `info/sourceIdentity` GET · 6.34 `entityMaps` GET/POST · 6.35 `temporal/entityMaps` GET/POST · 6.36–6.38 `snapshots` POST/DELETE, `snapshots/{id}` GET/PATCH/DELETE, `snapshots/{id}/clone` POST (**v1.x**)

#### 5.4.10 Clause 7 — MQTT notification binding (`antares-notifier`, feature `mqtt`)

- [ ] 7.1/7.2 MQTT notification behaviour: `mqtt(s)://host[:port]/topic` endpoint URIs, `notifierInfo` (`MQTT-Version`, `MQTT-QoS`), payload = `{metadata, body}` wrapper, connection handling per endpoint

#### 5.4.11 Ledger → roadmap phase mapping

| Roadmap phase (§13) | Ledger sections |
|---|---|
| 1 — single-node core | 5.4.1 (all), 5.4.2 (minus Snapshot types), 5.4.3 (minus 5.5.14/15), 5.4.4 (5.6.1–5.6.10, 5.6.17–5.6.21), 5.4.5 (5.7.1/2/5–10), 5.4.8 @contexts, 5.4.9 core |
| 2 — eventing | 5.4.4 temporal ops (5.6.11–16), 5.4.5 temporal queries (5.7.3/4), 5.4.6, 5.4.10 |
| 3 — federation | 5.4.7, 5.4.8 EntityMaps + Identity, 5.4.5 (5.7.11), 4.3.6 semantics, 6.3.17–6.3.19 |
| 4 — scale & hardening | re-validate everything at load; Snapshots (**v1.x**) start here at the earliest |

## 6. Rust software stack

Crate versions verified against crates.io 2026-08-03. Every pick below is production-maturity except `json-ld` (single maintainer — wrap it, budget for a fork) and `sonic-rs` (young, ByteDance-backed — keep the serde_json fallback).

### 6.1 The stack at a glance

| Layer | Crate | Version | Why this one |
|---|---|---|---|
| Async runtime | `tokio` | 1.x | the ecosystem |
| HTTP server | `axum` (on `hyper` 1.11, `tower` 0.5) | 0.8.9 | lowest per-conn memory, tower middleware (timeouts, body limits, tracing, compression); actix-web is ~10–15 % faster at saturation but loses on footprint + ecosystem |
| WebSocket *(feature `ws` — deferred, §9.2)* | `tokio-tungstenite` via `axum::extract::ws` | 0.30.0 | standard; **must** tune `WebSocketConfig` buffers down to 4–16 KB (defaults are 128 KB-class → would blow the RSS budget alone). Escape hatch if framing gets hot: `fastwebsockets` / `tokio-websockets` |
| MQTT notifications *(feature `mqtt`, default on)* | `rumqttc` | 0.24.x | maintained async MQTT 3.1.1/5 client; one shared client pool per endpoint host, bounded, with eviction (Scorpio audit L5) |
| Postgres | `sqlx` (postgres, runtime-tokio, tls-rustls, json, chrono, uuid) | 0.9.0 | `QueryBuilder` fits dynamically-compiled NGSI-LD `q=`/`geoQ=` SQL; compile-time macros are useless here (all SQL is dynamic) and that's fine. Escape hatch for batch-op latency: `tokio-postgres` + `deadpool` (pipelining, ~20 % on hot endpoints) |
| Messaging | `async-nats` | 0.50.0 | official client; JetStream + KV in core crate; pre-1.0 API churn is the only cost |
| JSON-LD 1.1 | `json-ld` | 0.21.4 | the only maintained full W3C JSON-LD 1.1 processor in Rust (expansion/compaction/flattening, runs W3C test suite). Single-author — isolate behind our own `antares-jsonld` wrapper crate |
| JSON | `serde_json` baseline; `sonic-rs` 0.5.8 behind a feature flag | 1.x | sonic-rs: 3–4× faster deserialization, direct-to-struct, x86_64/aarch64 only — batch-ingest hot path only |
| Geo | `geo` 0.33 + `geozero` 0.15 (`with-postgis-sqlx`) + `geojson` 0.24 | — | geozero encodes GeoJSON→PostGIS EWKB directly in query binds, zero intermediate copies |
| Allocator | `tikv-jemallocator` | 0.7.0 | decay-based purging returns pages when idle (mimalloc <3.x plateaus); `MALLOC_CONF` decay tuning + heap profiling. glibc malloc is the one wrong answer (arena fragmentation) |
| Cache | `moka` | 0.12.x | @context cache (see §6.3) |
| Observability | `tracing` + `opentelemetry` + `metrics` | — | plus `tokio-console` in dev; jemalloc stats exported as metrics |
| TLS | `rustls` | 0.23.x | no OpenSSL linkage anywhere |

### 6.2 Postgres access decisions

- **All query SQL is dynamic** — `q=`, `geoQ=`, `scopeQ=`, `attrs`, `pick/omit`, temporal ranges compile to SQL text + bind list via a query-builder module. `sqlx::query(&built)` with manual binds; no ORM (diesel's typed DSL fights JSONB predicate trees).
- **JSONB indexing**: GIN `jsonb_path_ops` only where generated SQL uses `@>` containment (3–4× smaller/faster than `jsonb_ops`, but blind to `?` and `->>`); hot scalar paths get expression B-trees, with the extracted-attribute side table (§8.1) as the measured escalation — Scorpio passes the full ETSI suite on pure-JSONB current state, so the side table is a benchmark-gated lever, not a default. Never a kitchen-sink GIN on the whole entity document. GIN `fastupdate`/pending-list settings set explicitly on write-heavy tables.
- **RLS cost is real but small**: measured +2.3–5.9 % (PG16, 10M rows, 500 tenants), near-negligible with tenant-leading composite indexes. Policy stays a bare `current_setting()` comparison (no SECURITY DEFINER wrapper — that can force per-row evaluation). `SET LOCAL` (transaction-scoped) keeps it safe under transaction pooling; plain `SET` would leak tenants across pooled connections.
- **One shared pool**, sized ≈ 2×cores of the PG box (~30–50 app-side connections total for all 1,000 tenants); each idle PG backend costs 5–10 MB of the 16 GB budget, so small pools are a memory decision, not just a contention one.

### 6.3 JSON-LD strategy (the #1 CPU risk)

JSON-LD expansion is the top CPU cost in every existing broker (Orion-LD's speed comes from a custom C JSON-LD layer; Scorpio's context-cache bug cost us 61 ETSI tests once — same lesson). Antares:

1. **Cache parsed contexts**, not fetched bytes: a `moka` LRU keyed by URL+content-hash storing the post-context-processing term map, fronted by our own `Loader` impl (TTL + `Cache-Control` respected, core context pinned forever).
2. **Fast-path the core context**: when the request's @context is exactly the NGSI-LD core context (the overwhelmingly common case), skip the generic processor and use a precompiled term-substitution table; full `json-ld` processing only for user contexts.
3. **Bounded concurrency** on expansion/compaction (semaphore) so a burst of exotic-context requests can't blow the JSON working-set budget.

### 6.4 NATS JetStream usage pattern

- **Pull consumers only** (push consumers are legacy). One `ANTARES_CHANGES` stream, **Interest retention**, subjects `changes.{tenant}.{type_hash}.{id_hash}` (hashes because entity types/ids are IRIs/URNs containing `.` and `:` — illegal or ambiguous as NATS subject tokens; tenant names are validated token-safe at creation); each concern (subscription matcher, temporal writer, registration forwarder) is its own **durable** consumer group — multiple broker instances sharing a durable get work-queue load-balancing per concern, and each concern sees every message. (WorkQueue retention would forbid multiple consumer groups on one stream.)
- **Delivery is at-least-once, engineered to idempotent**: publish-side dedup via `Nats-Msg-Id` within the stream `duplicate_window`, explicit double-ack, and every consumer idempotent (temporal writes upsert on `(tenant, entity, attr, observed_at)`; notification send is naturally at-least-once per NGSI-LD). No exactly-once design anywhere.
- **JetStream KV** holds the subscription registry cache: each broker instance `watch()`es the bucket and maintains the in-memory compiled-subscription map; revisions give CAS. The matcher hot path never touches Postgres (Postgres remains the durable system of record for subscriptions).
- Scorpio's Kafka lesson baked in: its `$[quarkus.uuid}` typo made all instances share one consumer group and silently load-balance what should have been broadcast — in Antares the broadcast-vs-balanced distinction is explicit in consumer types (ephemeral per-instance consumer = broadcast, shared durable = balanced), asserted at startup, and integration-tested with two instances.

### 6.5 Geo decisions

All query-time geo runs DB-side (`ST_DWithin` / `ST_Within` / `ST_Intersects` on `geometry(Geometry,4326)` + GIST — nothing server-side competes at 10M rows). Server-side `geo` is for: validating/normalizing incoming GeoJSON, and the subscription matcher's geo-conditions — one changed entity against ≤10k in-memory prepared geometries per event, where a DB round-trip per change would be absurd.

### 6.6 Prior art (what Antares learns from each)

- **No NGSI-LD broker exists in Rust** — Antares is first; the NGSI-LD API model crate (`antares-model`) should be written as a publishable standalone crate.
- **Orion-LD (C)**: monolith, Mongo for current state + Postgres/Timescale for temporal, custom JSON-LD + arena allocation. Lesson: owning the JSON/context layer is where speed lives. Anti-lesson: split persistence (Mongo+PG) is operational pain — Antares is Postgres-only.
- **Stellio (Kotlin)**: the closest architectural cousin — Postgres+Timescale+PostGIS single store, Kafka-decoupled search/subscription services. Anti-lesson: JVM footprint, hundreds of MB per service — exactly what the 500 MB target attacks.
- **Scorpio (Java)**: the compliance reference; its schema, ETSI-suite coverage, and its memory-analysis findings (bounded caches, serde pitfalls, broadcast-vs-balanced messaging) transfer directly (§4).

## 7. Messaging topology — Scorpio's topics remapped to JetStream

Scorpio's wire spine (kafka profile) and what happens to each topic:

| Scorpio topic | Producers → consumers | Semantics | Antares equivalent |
|---|---|---|---|
| `ENTITY` | EntityManager, HistoryEntityManager → SubscriptionManager, HistoryEntityManager | broadcast; `BaseRequest` subclasses (requestType int, payload + **prevPayload**), chunked at 1 MB | `ANTARES_CHANGES` stream, subjects `changes.{tenant}.{type_hash}.{id_hash}`; one `ChangeEvent` per operation |
| `REGISTRY` | RegistryManager → 6 modules (each holds an in-VM registration mirror — 7 copies in Scorpio) | broadcast; CSource CUD | `ANTARES_REGISTRY` stream + **one** compiled mirror per process (in `antares-registry`), refreshed by ephemeral (per-instance broadcast) consumers |
| `SUB_ALIVE` / `SUB_SYNC` | Subscription + RegistrySubscription managers, both ways | instance-liveness + which-instance-owns-which-sub protocol | **eliminated** — the reason these exist is per-instance in-memory sub state with no shared registry; Antares replaces the whole protocol with the JetStream **KV bucket** every instance watches (§6.4). No alive-pings, no ownership dance: matching is a shared-durable work queue |
| `HIST_SUB_SYNC` | HistoryEntityManager ↔ itself | temporal-instance sync announcements | **eliminated** — same reasoning; the temporal writer is a durable consumer group over `ANTARES_CHANGES` |
| `TEMPORAL` | nobody (dead config in Scorpio) | — | not carried over |

`ChangeEvent` design (the one message type that matters):

- Carries: `tenant`, `entity_id`, `types`, `op: ChangeOp` (create/update/append/merge/delete/batch…, mirroring Scorpio's requestType int registry as a Rust enum — the name `request_type` is a Scorpio-ism deliberately not carried over), `changed_attrs`, `payload` (expanded), `prev_payload`, `version` (the entity's row version bumped under the write lock — consumers order by it, §3.1). **`prev_payload` is load-bearing** — Scorpio's wire carries it because `showChanges`, `attributeDeleted` and `entityDeleted` notifications (5.8.6) need the before-image; dropping it would force the matcher back to the DB on every change.
- **Claim-check instead of chunking**: Scorpio splits messages at `scorpio.messaging.maxSize` with a custom chunker (and a 2 GB default outside the kafka profile — one of the audit findings). NATS caps payloads at ~1 MB. Antares never chunks: events over a threshold (256 KB) carry `payload_ref`/`prev_payload_ref` (entity id + version) instead of inline bodies, and consumers fetch from Postgres. Oversized entities are rare; the common path stays zero-round-trip.
- Serialization: serde into JSON bytes (`Bytes`, no String round-trips — Scorpio audit J7). Not the single-char-key compaction Scorpio uses; subject + explicit field names, compression left to NATS.
- Publish reliability: transactional-outbox drain (§10) with `Nats-Msg-Id` = outbox row id for dedup; consumers ack after processing (never Scorpio's PRE_PROCESSING commit-before-work).

Single-node/dev mode: the `ChangeBus` trait's second implementation (`bus = local`, §9.2) — an in-process `tokio::sync::broadcast` ring, Scorpio's in-memory profile equivalent, minus the per-receiver deep copies (`ChangeEvent` is immutable, shared by `Arc`). A single-node Antares therefore needs no infrastructure beyond Postgres.

## 8. Database schema — Scorpio's schema remapped to shared-tenant tables

Scorpio's final schema (64 Flyway migrations, PostGIS only — **no Timescale**) is the semantic baseline; every table gains `tenant_id` and loses its per-tenant-database context. Migrations via `sqlx migrate` (embedded, run-at-start like Scorpio's Flyway).

### 8.1 Current state

```sql
CREATE TABLE entities (
  tenant_id   text NOT NULL,
  id          text NOT NULL,
  entity      jsonb NOT NULL,              -- expanded JSON-LD (Scorpio: entity)
  version     bigint NOT NULL DEFAULT 1,   -- bumped under the row lock; event ordering + optimistic concurrency (§3.1)
  types       text[] NOT NULL,             -- extracted in Rust at write time (Scorpio: e_types via trigger)
  scopes      text[],
  location    geometry(Geometry, 4326),    -- extracted default GeoProperty
  created_at  timestamptz NOT NULL,
  modified_at timestamptz NOT NULL,
  expires_at  timestamptz,                 -- transient entities (V1.9.1)
  PRIMARY KEY (tenant_id, id)
);
CREATE INDEX ON entities USING gin  (tenant_id, types);         -- btree_gin: tenant-scoped type match
CREATE INDEX ON entities USING gist (location);                 -- geoQ
CREATE INDEX ON entities USING gin  (entity jsonb_path_ops);    -- q= containment
CREATE INDEX ON entities (tenant_id, modified_at DESC);         -- pagination/ordering
CREATE INDEX ON entities USING gin  (scopes);                   -- scopeQ
```

Scorpio's `q=` engine (QQueryTerm, 4k LOC) compiles to `jsonb_path_query`/`jsonb_path_exists` + correlated `JSONB_ARRAY_ELEMENTS` subqueries over the expanded document, with LEFT JOIN traversal for linked-entity queries — that compilation strategy is proven against the ETSI suite and ports directly to `antares-ql`→`antares-sql`. An extracted-attribute side table is **not** in v1 (Scorpio passes the suite without one); it's the named perf lever if the 10M-row benchmark says the JSONB path is too slow. (`ponytail:` JSONB-only querying; add the attribute side table when the phase-0 benchmark demands it.)

### 8.2 Temporal — two modes: with TimescaleDB and without

**Hard requirement: Antares runs with OR without TimescaleDB.** Not a fallback — both are supported, CI-tested modes selected by `ANTARES_STORE=timescale|postgres`. *(As built — deviation recorded in tasks.md D1: there is no Rust `TemporalStore` trait; because table shape and every query are IDENTICAL across modes, the two "implementations" are the two DDL branches of migration 0003 plus the maintenance-job branch, which is PINNED at startup from the actual `attr_instances` relkind — `detect_temporal_backend` — never re-probed per tick. `ANTARES_STORE=timescale` against a database migrated without the extension is a fatal, named error. Since ADR-0009 (2026-08-08) the rows are the READ path in both modes: reads reconstruct the doc shape from `attr_instances`, writes are per-instance deltas, and retention/compression act on data queries actually consume.)* The modes differ only in DDL bootstrap and maintenance jobs:

| Concern | `timescale` mode (default when extension present) | `plain` mode (vanilla PostgreSQL) |
|---|---|---|
| Partitioning | `create_hypertable` (auto chunks, 7-day) | native `PARTITION BY RANGE (observed_at)`; partitions pre-created by the broker's own scheduled job (or pg_partman if the operator prefers) |
| Compression | columnar, 90 %+ (the thing the 16 GB budget leans on) | none built-in — plan ~5–10× more temporal disk at equal retention, or shorter retention |
| Retention | `add_retention_policy` (chunk drops) | broker-scheduled `DROP TABLE` of expired partitions |
| Maintenance workers | Timescale background workers | the broker's `temporal` role runs the partition/retention job (no pg_cron dependency — one less extension to demand) |

Motivation: managed-Postgres targets without the TSL extension (plus the TigerData licensing exposure, §15.4). The ETSI suite runs against **both** modes in CI — temporal compliance must never silently depend on the extension.

```sql
CREATE TABLE temporal_entities (
  tenant_id text NOT NULL, id text NOT NULL,
  types text[] NOT NULL, scopes text[],
  created_at timestamptz NOT NULL, modified_at timestamptz NOT NULL, deleted_at timestamptz,
  PRIMARY KEY (tenant_id, id)
);

CREATE TABLE attr_instances (
  tenant_id   text NOT NULL,
  entity_id   text NOT NULL,
  attr_id     text NOT NULL,               -- expanded attribute IRI
  instance_id text NOT NULL,
  dataset_id  text,
  observed_at timestamptz NOT NULL,        -- hypertable time dimension (falls back to modified_at)
  created_at  timestamptz NOT NULL, modified_at timestamptz NOT NULL, deleted_at timestamptz,
  data        jsonb NOT NULL,              -- full expanded attribute instance
  geo_value   geometry(Geometry, 4326),    -- for geo-typed attributes (Scorpio: geovalue)
  UNIQUE (tenant_id, entity_id, attr_id, instance_id)
);
-- timescale mode only (plain mode: PARTITION BY RANGE (observed_at) + broker-managed partitions instead)
SELECT create_hypertable('attr_instances', by_range('observed_at', INTERVAL '7 days'));
ALTER TABLE attr_instances SET (timescaledb.compress,
  timescaledb.compress_segmentby = 'tenant_id, attr_id',       -- entity_id in orderby: 1-row segments compress terribly
  timescaledb.compress_orderby   = 'entity_id, observed_at DESC');
SELECT add_compression_policy('attr_instances', compress_after => INTERVAL '7 days');

-- both modes
CREATE INDEX ON attr_instances (tenant_id, entity_id, attr_id, observed_at DESC);
```

Scorpio's temporal read patterns to port deliberately, not accidentally: per-attribute `array_agg(... ORDER BY observed_at)` with `lastN` as an array slice (audit U3: **bound it** — aggregate within the 206/`Content-Range` window, never all instances); keyset pagination (`created_at, id` cursor) instead of OFFSET for the recursive page builder. Auto-recording = the `antares-temporal` durable consumer on `ANTARES_CHANGES` (Scorpio: same idea via the ENTITY topic + a 0.5 s-flush unbounded buffer — ours is the JetStream prefetch window, inherently bounded). Retention: `add_retention_policy` per deployment; per-tenant horizons via a nightly delete job (chunk-drop retention is global — Timescale limitation, documented).

### 8.3 Registrations, subscriptions, contexts, entity maps

Table-naming rule: **a table is named after the spec resource it stores, snake_cased** (`/csourceRegistrations` → `csource_registrations`, `/jsonldContexts` → `jsonld_contexts`) — anyone holding CIM 009 can find the table without a glossary. Scorpio-isms (`csource`, `c_id`, `e_types`, `i_location`) are deliberately not carried over; column names use the spec's own member names (`propertyName` per RegistrationInfo, not `e_prop`).

```sql
CREATE TABLE csource_registrations (
  tenant_id text NOT NULL, id text NOT NULL,     -- registration @id (Scorpio: csource.c_id)
  registration jsonb NOT NULL,                   -- full registration document
  PRIMARY KEY (tenant_id, id)
);

-- Scorpio's csourceinformation: the flattened federation match table, kept —
-- but its ~46 boolean operation columns collapse into one bitmask.
CREATE TABLE csource_index (
  tenant_id text NOT NULL,
  registration_id text NOT NULL,
  entity_id text, id_pattern text, entity_type text,     -- RegistrationInfo.entities members
  property_name text, relationship_name text,            -- 1.9.1 propertyNames/relationshipNames; NGSI-LD 2.0 (#31) merges them into attributeNames — migration = coalesce into one attribute_name column
  location geometry(Geometry, 4326),
  scopes text[],
  expires_at timestamptz,
  endpoint text NOT NULL, mode smallint NOT NULL,      -- 0 auxiliary | 1 inclusive | 2 redirect | 3 exclusive (4.20)
  ops bigint NOT NULL,                                 -- bitmask over the Operation enum (Scorpio: 46 bool columns)
  tenant_at_peer text, headers jsonb, host_alias text, -- the peer's contextSourceAlias (Table 5.2.9-1); consumed in matching per Table 6.3.18-2 (ADR-0011)
  FOREIGN KEY (tenant_id, registration_id) REFERENCES csource_registrations ON DELETE CASCADE
);
CREATE INDEX ON csource_index (tenant_id, entity_type);
CREATE INDEX ON csource_index (tenant_id, entity_id);
CREATE INDEX ON csource_index USING gist (location);

CREATE TABLE subscriptions (          -- also csource_subscriptions (/csourceSubscriptions), same shape
  tenant_id text NOT NULL, id text NOT NULL,
  subscription jsonb NOT NULL, context jsonb NOT NULL,
  expires_at timestamptz, is_active bool NOT NULL DEFAULT true,
  times_sent bigint NOT NULL DEFAULT 0, last_notification timestamptz,
  last_success timestamptz, last_failure timestamptz,
  PRIMARY KEY (tenant_id, id)
);

CREATE TABLE jsonld_contexts (        -- Scorpio's contexts table, kept nearly as-is
  id text PRIMARY KEY,                -- shared across tenants BY DESIGN (only cross-tenant table; WS-47 sanctions a shared @context cache)
  body jsonb NOT NULL,
  kind text NOT NULL,                 -- spec values verbatim (5.13): 'Hosted' | 'Cached' | 'ImplicitlyCreated'
  created_at timestamptz NOT NULL DEFAULT now(),
  last_usage timestamptz, hits bigint NOT NULL DEFAULT 0
);

CREATE TABLE entity_maps (            -- 5.5.9.3 distributed pagination (Scorpio's 3rd rewrite, V20250731)
  tenant_id text NOT NULL, map_id text NOT NULL, pos bigint NOT NULL,
  query_checksum text NOT NULL, entity_id text NOT NULL,
  remote_query text, registration_id text NOT NULL,
  last_access timestamptz NOT NULL, expires_at timestamptz NOT NULL,
  PRIMARY KEY (tenant_id, map_id, pos)
);
CREATE INDEX ON entity_maps (expires_at);   -- TTL sweep (Scorpio default: 90 s TTL, 30 s sweep)
```

Plus `ws_notification_buffer` (§11) and `tenants` + `outbox` (§10). Differences from Scorpio worth naming: subscription bookkeeping columns (`times_sent`, `last_*`, `is_active`) are real columns, not buried in the JSONB — the audit found Scorpio serving stale status precisely because state lived in in-VM maps; here the row is the truth and instances only cache. Scorpio's entity-map insert-without-LIMIT and the `first`-row csourceid bug (B1) are both named test cases. **Storage-estimate note:** the audit's per-request registration mirror (7 Guava tables) becomes one compiled mirror per process, keyed `(tenant, registration_id)`, expiry-filtered at the single yield point (§4.1).

## 9. Project structure — one binary, decoupled by crates + subjects

Scorpio decouples with microservices (9 deployables + AllInOneRunner). Antares inverts that: **one binary, many roles** — the decoupling lives in the crate boundaries and NATS subjects, not in deployment units. Every process runs the same image; `--roles api,matcher,notifier,temporal,registry` (default: all) selects which consumers start. That gives Scorpio's scale-out story (run 3 pods with `--roles api` + 2 with `--roles matcher,notifier`) without the 10-JVM overhead that motivated AllInOneRunner in the first place.

```
antares/
├── Cargo.toml                  # workspace: [workspace.dependencies] single-source versions,
│                               #   [workspace.lints] (clippy + rustc) inherited by every crate
├── rust-toolchain.toml         # pinned toolchain; MSRV policy = latest-stable minus 2
├── deny.toml                   # cargo-deny: license allowlist (TSL never linked!), advisories
├── .github/workflows/
│   ├── ci.yml                  # ONE pipeline: fmt+clippy+workspace tests × feature
│   │                           #   matrix → image → 4×8 ETSI matrix + 350 MiB RSS
│   │                           #   gate → publish dev; +serial +release on a v* tag
│   └── fuzz.yml                # weekly cargo-fuzz over the q/geo/scope parsers
├── crates/
│   ├── antares-model/          # publishable as `ngsild-model`
│   ├── antares-jsonld/
│   ├── antares-ql/             # publishable as `ngsild-ql`
│   ├── antares-sql/
│   ├── antares-bus/
│   ├── antares-api/
│   ├── antares-matcher/
│   ├── antares-notifier/
│   ├── antares-ws/             # [feature `ws`, OFF in v1]
│   ├── antares-temporal/
│   ├── antares-registry/
│   ├── antares-broker/         # binary crate; [[bin]] name = "antares"
│   └── antares-e2e/            # unpublished test-only crate: testcontainers harness
├── etsi/                       # suite-fork pin + runner scripts (port of dev/etsi-serial.sh)
├── docs/
│   ├── deep-analysis.md        # this document
│   ├── adr/                    # one file per irreversible decision (ADR-0001 tenancy, …)
│   └── spec/                   # the conformance ledger (§0.3): one file per CIM 009
│                               #   clause, full spec text + status/evidence/robot
│                               #   frontmatter; tooling dev/spec.py (replaced ics.yaml)
└── xtask/                      # cargo-xtask: load-rig, ICS render, synthetic 10M-row seeder
```

Dependency rule: `model ← {jsonld, ql} ← sql ← {api, matcher, temporal, registry}`; `bus` is leaf-shared; optional crates (`ws`, the `mqtt` half of notifier) depend on core crates, **never the reverse** — only `broker` (the composition root) knows they exist; nothing depends on `broker`. `antares-model` + `antares-ql` are the crates worth publishing to crates.io (nothing NGSI-LD exists in Rust — free ecosystem win).

### 9.2 Pluggability matrix — what can be removed or added, and what it costs

Validation rule: **an optional capability is one crate (or feature) + a registration in `broker`; removing it must not touch a core crate.** The two seams that make this true: (1) `NotificationSink` — a trait keyed by `endpoint.uri` scheme (`http/https` → reqwest sink, `mqtt(s)` → rumqttc sink, `ws(s)` → ws sink); a subscription naming a scheme with no registered sink is rejected at creation with `OperationNotSupported` (422); (2) `Router::merge` — each optional API surface is an axum sub-router merged in by `broker`.

| Capability | Lives in | Toggle | Cost to remove / add |
|---|---|---|---|
| WebSocket binding (§11) | `antares-ws` (sink + upgrade route + outbox + its own migration) | cargo feature `ws` — **off in v1** | one feature flag + one `broker` registration; core crates unchanged. This is the §11 deferral made structural: implementing later = turning the feature on and writing one crate |
| MQTT notifications | `antares-notifier` mqtt module (rumqttc) | cargo feature `mqtt` — on by default (ETSI MQTT TPs 058_xx need it) | feature flag; without it those TPs stay tag-excluded, everything else green |
| TimescaleDB | `TemporalStore` impl in `antares-sql` | runtime `temporal.store = timescale\|plain`, auto-detected | zero — both modes always compiled, CI runs both (§8.2) |
| NATS | `ChangeBus` impl in `antares-bus` | runtime `bus = nats\|local` | zero — `local` (in-process broadcast) serves single-node deployments with **no infrastructure beyond Postgres**; startup asserts `local` ⇒ single process running all roles. NATS only becomes mandatory when you scale out |
| sonic-rs hot path | `antares-api` ingest path (batch-body deserialization) | cargo feature `sonic` — off by default | feature flag; serde_json is always the compiled fallback |
| Federation / distributed ops | `antares-registry` | none — spec-core, always in | not removable: `/csourceRegistrations` is part of the compliance surface; the forwarding machinery is simply idle when no registrations exist |
| Snapshots (V1.9.1) | future `antares-api` module | staged v1.x (§5.1) | additive only |
| SensorThings API façade (read-only) | future `antares-sta` crate | **not implemented — documented only (§15.6)** | one crate over the query + temporal APIs; core untouched |
| WFS / OGC API Features (read-only) | future `antares-wfs` crate | **not implemented — documented only (§15.6)** | one crate over the query API's geo+json path; core untouched |

Everything not in this table (entities/query/temporal APIs, subscriptions with HTTP delivery, @context management, tenancy) is core and deliberately **not** pluggable — optionality there would be abstraction without a customer.

### 9.3 Crate internals — module map and the traits that hold the seams

**`antares-model`** *(publishable as `ngsild-model`; deps: serde, iref only — no async, no DB, no HTTP)*
```
src/
├── id.rs            # newtypes: EntityId(Urn), TenantId (validated token-safe, §7), AttrIri
├── entity.rs        # Entity; ExpandedEntity / CompactedDoc newtype pair (§9.1)
├── attribute.rs     # enum Attribute { Property, Relationship, GeoProperty, LanguageProperty,
│                    #   JsonProperty, VocabProperty, ListProperty } + datasetId multi-instance sets
├── subscription.rs  # Subscription (5.2.12), NotificationParams, Endpoint, notificationTrigger
├── registration.rs  # CSourceRegistration (5.2.9), RegistrationInfo, Operation bitflags (§8.3 ops)
├── notification.rs  # Notification (5.3.1), CSourceNotification (5.3.2)
├── temporal.rs      # AttrInstance, temporal representations (temporalValues/aggregatedValues)
├── entity_map.rs    # EntityMap (5.2.x, V1.9.1)
├── error.rs         # NgsiError enum = Table 6.3.2-1 verbatim; → ProblemDetails + spec URI
└── fragment.rs      # entity fragments / merge-patch inputs (5.4 shapes)
```
Rule: this crate defines *shapes and invariants only*. Any function that needs I/O, a clock, or a config value belongs one layer up. Tolerant-reader posture lives here: every struct keeps unknown members in an `extra: Map` field that serializes back out untouched (§15.1).

**`antares-jsonld`** *(deps: model, json-ld, moka, reqwest)*
```
src/
├── loader.rs        # CachingLoader: moka LRU of PARSED contexts, Cache-Control/Expires TTLs,
│                    #   Postgres jsonld_contexts write-through (kind='Cached'), SSRF policy hook
├── core.rs          # core context pinned at build; precompiled term-substitution table
├── fastpath.rs      # request-context == core-context detector → table-driven expand/compact
├── expand.rs        # full-processor path; &input → ExpandedEntity (never mutates, §14.4)
├── compact.rs       # &ExpandedEntity + context → CompactedDoc; lang filtering applied here
├── validate.rs      # the NGSIObject equivalent: NGSI-LD structural validation during expansion
└── limits.rs        # Semaphore (bounded concurrency §6.3), depth/size caps
```

**`antares-ql`** *(publishable as `ngsild-ql`; deps: model only — a pure function library)*
```
src/
├── q/               # lexer.rs, parser.rs, ast.rs  — q= grammar (4.9), dotted paths, coercions
├── scope.rs         # scopeQ AST (4.19)
├── geo.rs           # geoQ params → GeoQuery (georel/geometry/coordinates validation)
├── temporal.rs      # timerel/timeAt/endTimeAt/lastN/aggr params
├── params.rs        # QueryParams: the one typed struct every binding parses into
└── render.rs        # AST pretty-print (for forwarding: re-render narrowed queries, §14.8)
```
Every parser here is `&str → Result<Ast, NgsiError>` with **proptest round-trip properties** (`parse(render(ast)) == ast`) — the q= grammar is risk #2 and gets property-based coverage, not just examples.

**`antares-sql`** *(deps: model, ql, sqlx, geozero)*
```
src/
├── compile/         # AST → (SQL string, Vec<Bind>): q.rs, scope.rs, geo.rs, projection.rs,
│                    #   order.rs, temporal.rs — mirrors Scorpio's proven jsonb_path strategy
├── store/
│   ├── entity.rs        # EntityStore: create/replace/merge/append/partial/delete + batch (UNNEST)
│   ├── temporal.rs      # trait TemporalStore { record(); query(); … } + timescale.rs + plain.rs (§8.2)
│   ├── subscription.rs  # SubscriptionStore incl. status columns writeback
│   ├── registration.rs  # RegistrationStore + csource_index maintenance (the trigger logic, in Rust)
│   ├── context.rs       # jsonld_contexts CRUD
│   ├── entity_map.rs    # EntityMapStore + TTL sweep
│   ├── outbox.rs        # transactional outbox: same-tx INSERT + drain loop (§10)
│   └── tenant.rs        # SET LOCAL antares.tenant helper; every store method takes &TenantId first
└── migrations/      # sqlx migrate; timescale-only statements guarded (§8.2)
```
Rule: **`tenant: &TenantId` is the first parameter of every public store method** — the compiler makes tenant-scoping unforgettable, RLS makes it unforgeable.

**`antares-bus`** *(deps: model)*
```
src/
├── event.rs         # ChangeEvent { tenant, entity_id, types, op: ChangeOp, changed_attrs,
│                    #   payload, prev_payload, seq } + claim-check refs (§7)
├── bus.rs           # trait ChangeBus { publish(); consume(durable) -> Stream; watch_kv() }
├── nats.rs          # JetStream impl: streams, pull durables, KV mirror, Nats-Msg-Id dedup
├── local.rs         # in-process broadcast impl (single-node mode)
├── subjects.rs      # changes.{tenant}.{type_hash}.{id_hash} hashing (§9.1)
└── topology.rs      # startup assertions: broadcast-vs-balanced consumer audit (§6.4, R10 lesson)
```

**`antares-api`** *(deps: model, jsonld, ql, sql, bus)* — the TS 104 176 HTTP-binding layer; handlers stay thin: parse → call domain → respond.
```
src/
├── routes/          # one file per §5.1 resource: entities.rs, batch.rs, temporal.rs,
│                    #   subscriptions.rs, csource_registrations.rs, csource_subscriptions.rs,
│                    #   types_attrs.rs, jsonld_contexts.rs, entity_maps.rs, info.rs
├── extract.rs       # axum extractors: Tenant (header), ContextLink (6.3.5 rules), Pagination
├── negotiate.rs     # Accept precedence json > ld+json > geo+json (6.3.4); Prefer: ngsi-ld
├── respond.rs       # Link headers, NGSILD-Results-Count, 206/Content-Range, the six 207 shapes
└── state.rs         # AppState { stores, jsonld, bus, registry_mirror, sink_registry }
```
Handler naming = spec operation (§9.1); a handler contains **no business logic** — the same `create_entity` domain call must serve a future binding (that is what TS 104 175 vs 176 now formalizes).

**`antares-matcher`** *(deps: model, ql, bus, geo)*
```
src/
├── compiled.rs      # CompiledSubscription: parsed q AST, prepared geometry, trigger set, ~3 KB
├── index.rs         # candidate lookup: (tenant, type) and (tenant, watched-attr) maps — the
│                    #   O(log n) structure §1.1 demands; never a full scan
├── mirror.rs        # KV-watched mirror; THE single yield point that filters expired subs (§4.1)
├── evaluate.rs      # in-memory q/scope/geo evaluation against one ChangeEvent (+prev for triggers)
└── triggers.rs      # notificationTrigger semantics incl. deletion payload forms (4.5.18 lesson)
```

**`antares-notifier`** *(deps: model, jsonld, sql)*
```
src/
├── sink.rs          # trait NotificationSink { schemes(); deliver(&Notification) } + SinkRegistry
├── http.rs          # reqwest sink: shared client, timeouts at construction (U1 lesson)
├── mqtt.rs          # [feature mqtt] rumqttc sink: bounded client pool + eviction (L5 lesson)
├── throttle.rs      # throttling/coalescing decided BEFORE payload build (J6 lesson); Duration-typed
└── status.rs        # times_sent / last_success / last_failure writeback (rows are truth, §8.3)
```

**`antares-temporal`** *(deps: model, sql, bus)*: `recorder.rs` (durable consumer, idempotent upserts), `queries.rs` (TRoE queries, aggregation, 206/Content-Range bounds — U3 lesson), `partitions.rs` (plain-mode partition/retention job, §8.2).

**`antares-registry`** *(deps: model, jsonld, ql, sql, bus)*: `store.rs`, `mirror.rs` (the ONE compiled registration mirror), `matching.rs` (4.3.6 + reg-mode semantics + scope narrowing — spec-mandated, §14.8), `forward.rs` (write/read forwarding, `Via` chains, hop limit, 508 pre-adopt), `merge.rs` (multi-source merge + 207 classification), `entity_map.rs` (distributed pagination incl. the B1 regression test), `bootstrap.rs` (auto-federation peers).

**`antares-broker`** *(the composition root — the only crate that sees everything)*
```
src/
├── main.rs          # parse config → build wiring → run roles → graceful shutdown
├── config.rs        # figment: defaults < antares.toml < ANTARES_* env; unknown key = fatal (§14.3)
├── roles.rs         # --roles parsing; role → set of tasks/consumers to start
├── wiring.rs        # constructs stores, sink registry (http [+mqtt] [+ws]), bus (nats|local),
│                    #   merges routers (api [+ws]); asserts topology; local-bus ⇒ all-roles check
├── telemetry.rs     # tracing + OTLP + metrics + jemalloc stats export
└── shutdown.rs      # drain order: stop HTTP accept → stop consumers → flush outbox → close pools
```

### 9.4 Request lifecycles (which module touches what, in order)

**Write** (`POST /entities`): `api::routes::entities` → `extract::{Tenant, ContextLink}` → `jsonld::expand` (fastpath or full) + `jsonld::validate` → `model` invariants → one transaction in `sql::store::entity`: `tenant.rs` SET LOCAL → DML → extracted columns → `outbox.rs` INSERT (same tx) → 201 via `respond`. Outbox drain (broker task) publishes `ChangeEvent` to `bus` → consumed independently by `matcher` (→ `notifier` sinks → status writeback) and `temporal::recorder` (→ `TemporalStore`). Registry forwarding, when registrations match, happens in the request path via `registry::forward` and folds into the 201/207 decision.

**Query** (`GET /entities?type=…&q=…&geoQ…`): `api` → `ql::params` parse → local: `sql::compile` → one SQL round-trip (streamed rows) — distributed: `registry::matching` selects CSRs, `ql::render` re-renders the narrowed query per host, `registry::merge` folds local + remote + reg-mode precedence → `jsonld::compact` per the request context → `respond` with pagination Links.

**Notification**: `matcher::index` candidates → `evaluate` (incl. prev-payload triggers) → `notifier::throttle` gate **first** → `jsonld::compact` once per subscription context → `Bytes` fan-out to the sink for that endpoint scheme → `status` writeback. No step may buffer unboundedly (§2.1 rules).

### 9.5 Testing layout & workspace hygiene

| Layer | Where | What |
|---|---|---|
| Unit + property | each crate `src/…` + `proptest` in `antares-ql`/`antares-jsonld` | parser round-trips, compaction immutability (`assert_eq!(input_before, input_after)` — J5 as a test), trigger semantics, §14 regression cases (B1, throttling units, expired-sub yield) |
| Store integration | `antares-sql/tests/` via `sqlx::test` + testcontainers Postgres (PostGIS, ±Timescale) | every store × **both** temporal modes; RLS cross-tenant denial tests |
| Bus integration | `antares-bus/tests/` (testcontainers NATS) | broadcast-vs-balanced 2-consumer assertion (R10), claim-check, dedup |
| End-to-end | `antares-e2e` crate | full binary against docker Postgres+NATS: 2-instance sync, failover drill, notification delivery, `bus=local` single-node smoke |
| Conformance | `etsi/` in `ci.yml` CI | the 8 Robot suites + 350 MiB RSS gate; per-suite pass counts are the progress metric (§13) |
| Load | `xtask load-rig` | phase-4 10M/1000-tenant soak (§13) |

Workspace hygiene, enforced not aspirational: all dependency versions live in `[workspace.dependencies]` only; `[workspace.lints]` sets `unsafe_code = "forbid"` (exception: the `sonic` feature module, locally allowed and reviewed), `unwrap_used`/`expect_used` denied outside tests; `cargo deny` gates licenses (nothing links TSL — Timescale is talked to over SQL, never linked) and advisories; CI builds the feature matrix `--no-default-features`, default, `--all-features`, and runs stores against both temporal modes and both buses. Doc rule: every `docs/adr/` entry records one irreversible decision (tenancy model, bus choice, WS deferral) — this analysis stays the map, ADRs are the ledger.

### 9.1 Naming conventions (validated 2026-08-03)

**Crates & binary.** Package prefix `antares-`; the binary crate is `antares-broker` but its `[[bin]] name = "antares"` — operators type `antares --roles api,matcher`. crates.io check (2026-08-03): bare `antares` is squatted by a dead 2020 crate, so the *package* can't have that name; `antares-*` is all free. The two publishable spec crates should ship under **spec-discoverable names** — `ngsild-model` and `ngsild-ql` (both free; `ngsild` itself is free and worth reserving as a facade re-export) — because someone searching crates.io for NGSI-LD types will type "ngsild", not "antares". Workspace path stays `crates/antares-model` with `package.name` set at publish time.

**Roles = crate suffixes.** `--roles api,matcher,notifier,temporal,registry` — a role name is always the suffix of the crate that implements it. No new vocabulary between deployment and code.

**Rust types.** PascalCase names taken verbatim from CIM 009 §5.2: `Entity`, `Property`, `Relationship`, `GeoProperty`, `LanguageProperty`, `JsonProperty`, `VocabProperty`, `ListProperty`, `Subscription`, `CSourceRegistration`, `Notification`, `EntityMap`, `ProblemDetails`. Expanded vs compacted documents are **distinct newtypes** (`ExpandedEntity` / `CompactedDoc`) so the compiler forbids the mix-ups Scorpio managed at runtime. Error enum `NgsiError` with variants named exactly as Table 6.3.2-1 (`AlreadyExists`, `BadRequestData`, `LdContextNotAvailable`, …) — variant name → spec error URI is a derive, not a lookup table.

**Functions.** One public function per spec operation, snake_cased from the spec's own operation name (clause 5.6/5.7/5.8): `create_entity` (5.6.1), `query_entities` (5.7.2), `purge_entities` (5.6.21), `merge_entity` (5.6.17), `partial_attribute_update` (5.6.4), `batch_upsert` (5.6.8)… — so an ETSI TP id maps to a function without archaeology. Internal helpers are verb-first (`compile_subscription`, `narrow_to_registration`, `drain_outbox`). Banned suffixes: `Manager`, `Service`, `Util`, `Helper`, `Tools` (the Scorpio pattern where `HttpUtils` grew to 2,033 lines) — modules are domain nouns, functions are verbs, and anything called "util" gets renamed or dissolved at review.

**Database.** Table = spec resource, snake_cased (`entities`, `temporal_entities`, `attr_instances`, `subscriptions`, `csource_registrations`, `csource_subscriptions`, `csource_index`, `jsonld_contexts`, `entity_maps`, `tenants`, `outbox`, `ws_notification_buffer`); columns use spec member names snake_cased; `tenant_id` is always the first PK column; timestamps end `_at`; geometry columns are `location` (default GeoProperty) or `geo_value` (attribute values). No `e_`/`i_` prefixes.

**NATS.** Streams `ANTARES_*` SCREAMING_SNAKE (`ANTARES_CHANGES`, `ANTARES_REGISTRY`); subjects lowercase dot-tokens with hashed IRI segments (`changes.{tenant}.{type_hash}.{id_hash}`); durable consumer names snake_case after their role (`matcher`, `temporal_writer`, `registry_forwarder` — a durable name IS a consumer-group contract, so renaming one is a migration and gets a doc note); KV bucket `antares_subscriptions`.

**Config & metrics.** Env prefix `ANTARES_` mapping to TOML sections named after crates (`ANTARES_BUS__PREFETCH` ↔ `[bus] prefetch`); unknown keys are a startup error (§14.3). Metrics follow Prometheus conventions with the `antares_` prefix and unit suffixes (`antares_notifications_sent_total`, `antares_context_cache_entries`, `antares_change_lag_seconds`).

## 10. High availability

- **Broker**: stateless pods (all durable state in Postgres + JetStream). N≥2 `api`-role pods behind any L4/L7 LB with WS-aware (long-lived connection) handling; matcher/notifier roles scale independently as shared-durable consumer groups — JetStream load-balances within a durable, broadcasts across durables (§6.4). Connection-scoped WS subscriptions die with their pod by design (WS-09); durable ones resume via `streamSeq` cursor + replay from the Postgres outbox on whichever pod the client reconnects to.
- **NATS**: 3-node JetStream cluster, R3 replication on the `ANTARES_CHANGES` stream and the KV bucket. Memory-light (Go, ~100 MB/node at this scale).
- **Postgres**: primary + streaming replica, Patroni or CloudNativePG for failover; synchronous_commit=on for the entity write path (correctness), temporal writer may batch with `synchronous_commit=off` per-transaction (replayable from JetStream on loss).
- **Delivery semantics under failover**: everything downstream of the change stream is at-least-once + idempotent (§6.4); the entity write itself is the only strictly-once point, and it's a Postgres transaction. A broker pod crash between commit and publish is covered by the transactional outbox pattern on the change stream (publish from a Postgres outbox table drained by the bus crate, not fire-and-forget after commit).

## 11. WebSocket binding (`ngsi-ld-ws`) — DEFERRED (design retained)

**Scope decision 2026-08-03: WS is not implemented in v1.** This section is kept because the binding shaped load-bearing architecture choices that stay in v1 regardless (one matcher for all bindings, notifier sinks selected by `endpoint.uri` scheme, Postgres outbox for durable delivery, tenant-from-connection-not-frame) — and because when WS lands, it must slot in without redesign. The deferral also aligns with the standards timeline: **a WebSocket Notification Binding is now an official NGSI-LD 2.1 work item (TC DATA Issue #8)** — implementing ahead of it risks divergence, and `/workspace/websocket.md` is better spent as input to that work item (it would arrive as its own TS beside TS 104 176 HTTP and TS 104 243 MQTT).

The deferral is also structural, not just scheduling: the entire binding is scoped to one crate, `antares-ws`, behind the cargo feature `ws` (§9.2) — it plugs into the `NotificationSink` scheme registry and an axum `Router::merge`, so implementing OR removing it never touches a core crate.

When implemented, Antares follows `/workspace/websocket.md` (ADR draft, 47 requirements WS-01…WS-47) as a first-class notification binding next to HTTP callback and MQTT — the first broker designed around it rather than retrofitting. The draft's §11 annex maps the binding onto Kafka+Postgres for Scorpio; Antares substitutes NATS but keeps every architectural rule:

| Binding concern | websocket.md annex says | Antares implementation |
|---|---|---|
| Change detection & fan-out | Kafka entity channel (internal only) | `ANTARES_CHANGES` JetStream stream (internal only — **never exposed to clients/peers**, same hard rule) |
| Matching | one matcher for all bindings | `antares-matcher`, one code path for HTTP/MQTT/WS |
| Durable sub registry | Postgres `subscriptions` table; connection-scoped subs memory-only | identical (connection-scoped subs never get a row — auto-cleanup on disconnect, WS-09) |
| Replay/ack buffer (WS-17/35) | Postgres outbox, explicitly NOT the message bus | identical: `ws_notification_buffer(tenant_id, subscription_id, stream_seq GENERATED ALWAYS AS IDENTITY, payload jsonb CHECK ≤1 MiB, created_at)` PK `(tenant_id, subscription_id, stream_seq)`; ack ⇒ `DELETE ≤ cursor`, replay ⇒ `SELECT > cursor ORDER BY stream_seq`; RLS applies |
| Conflation `latest` (WS-18) | in-memory entityId→pending map per connection | identical, bounded per §2.1 budget |

Implementation checkpoints from the requirement set: subprotocol `ngsi-ld-ws.v1` + 426 fallback (WS-02); tenant fixed at handshake, never switchable in-band (WS-05/WS-47); protocol pings not JSON heartbeats (WS-06); capabilities frame with limits after handshake (WS-16); overflow policies latest|drop-oldest|disconnect + close 4408 (WS-18); size-check-before-parse, inflate cap, depth cap 64 (WS-44); the close-code registry 4400–4500 (WS-39). Conformance staging when the feature lands: MUST tier first (WS-01…15, 17, 18, 31, 32, 37, 39), then durable+replay SHOULD tier, federation-over-WS (WS-21…26) last and only after plain CSR federation is green.

## 12. Risk register

| # | Risk | Severity | Mitigation |
|---|---|---|---|
| 1 | `json-ld` crate: single maintainer, verbose generic API, unknown throughput at our load | **High** | isolate behind `antares-jsonld`; core-context fast path bypasses it for ~95 % of traffic; benchmark in week 1 — if it can't do ~5k expansions/s/core, fork or hand-roll the NGSI-LD-subset processor (Orion-LD precedent says owning this layer is where speed lives) |
| 2 | `q=` semantics depth (dotted attribute paths, datasetId, temporal q, value-type coercion) | High | port semantics from Scorpio's query builder test-first; the 166 CI/Cons TPs are the acceptance oracle |
| 3 | ETSI-suite long-tail (the last 5 % of 686 TPs is where months go) | High | run the Robot suite from month 1 in CI (Scorpio's serial-run recipe is already proven in this workspace); track per-suite pass count as the only progress metric |
| 4 | 16 GB Postgres vs 10M entities with GIN bloat | Medium | extracted-attribute side table instead of kitchen-sink GIN; `jsonb_path_ops` only; Timescale compression ≥90 % on temporal; measure `pg_stat` weekly against a synthetic 10M/1000-tenant dataset |
| 5 | RSS creep (default buffers, unbounded queues; 10k WS conns when the `ws` feature lands) | Medium | §2.1 budget enforced by the 350 MiB CI memory gate from day 1 (inherited from Scorpio's pipeline) |
| 6 | JetStream semantics misuse (broadcast vs balanced — Scorpio's `$[quarkus.uuid}` class of bug) | Medium | consumer topology asserted at startup + 2-instance integration test in CI |
| 7 | Snapshots/EntityMaps (V1.9.1 additions) scope creep | Low | explicitly staged: EntityMaps v1.0 (tests exist), Snapshots v1.x (no tests yet) |
| 8 | async-nats / sqlx pre-1.0 API churn | Low | version-pinned workspace; upgrade in dedicated PRs |

## 13. Roadmap (spec-driven, suite-validated)

Each phase's work items are the §5.4 ledger sections mapped in §5.4.11 — implement from the clauses, then the suite named in the exit criterion confirms the phase.

| Phase | Deliverable | Exit criterion |
|---|---|---|
| 0 — spike (2-3 wk) | `json-ld` benchmark + core-context fast path; q= parser skeleton; 10M-row synthetic schema benchmark on 16 GB Postgres | go/no-go on crate choices (risk #1, #4) |
| 1 — single-node core | entities CRUD + batch + query (q/geoQ/scopeQ/pagination), shared-schema tenancy, @context mgmt | CommonBehaviours + CI/Prov + CI/Cons suites green, single node, no NATS |
| 2 — eventing | NATS change stream, subscriptions, HTTP+MQTT notifications, temporal writer + TRoE queries | CI/SUB + temporal TPs green; 2-instance broadcast/balanced test green |
| 3 — federation | CSR store, forwarding, Via/hostAlias, EntityMaps, csourceSubscriptions | ContextSource + DistributedOperations + IOP (5-broker) suites green |
| 4 — scale & hardening | 10M-entity/1000-tenant load rig, RLS pen-test, failover drills, Snapshots | all §1 targets (minus WS) measured and held for a 24 h soak |
| 5 — WS binding *(deferred, unscheduled)* | in-band + out-of-band WS, capabilities, overflow, durable+replay outbox per §11 | WS MUST-tier self-conformance checklist (WS-30/42); 10k-conn soak under 500 MB |

## 14. What Scorpio got wrong — the improvement catalogue

Every item below is evidenced either by the 2026-07-25 memory audit, by a bug fixed in this workspace's ETSI campaign (with the fix recorded), or by the architecture survey. Grouped by root cause, each with the Antares counter-design.

### 14.1 State that lives in the wrong place

The single biggest defect class. Scorpio keeps subscriptions, registrations, MQTT clients, and callback bookkeeping in **per-instance in-VM maps**, then bolts on a Kafka sync protocol (SUB_ALIVE/SUB_SYNC/HIST_SUB_SYNC heartbeats every ~15 s) to reconcile instances. Consequences, all observed:

- The sync protocol itself broke twice independently: Kafka serde auto-detection resolved **non-deterministically across rebuilds** (String consumers receiving `byte[]` → `ClassCastException` → depending on the build, *all* notification delivery silently dead, not just replication); and the `$[quarkus.uuid}` interpolation typo collapsed all instances into one consumer group, load-balancing what must broadcast.
- In-VM subscription state leaks on remote delete (audit L3), serves **stale status** after expiry (L4a — expired interval subs notify forever, violating 5.8.6), and survives DB truncation — the ETSI campaign needed a broker **restart** before every measured subscription run because `clean_db.sh` couldn't reach the maps.
- Registration mirrors exist in **seven** copies per JVM (L4c), each with its own expiry-checking code path — two of which forgot the check (L4b), one of which over-evicts (inner-iterator bug).
- Remote-subscription bookkeeping used wrong-typed Table keys → guaranteed NPE on the unsubscribe path, orphaned callback UUIDs forever (L6).

**Antares**: durable state has exactly one home (Postgres), the shared mirror has exactly one implementation (KV-watched, expiry-filtered at its single yield point), and there is no reconciliation protocol to break — the entire SUB_ALIVE/SUB_SYNC/HIST_SUB_SYNC machinery has no equivalent. Reset-for-tests is `TRUNCATE`; correctness does not depend on process lifetime.

### 14.2 Tenancy that multiplies infrastructure

Database-per-tenant (`CREATE DATABASE "ngb"+tenant.hashCode()` on first write, Flyway per database, one pool per tenant in a map that started as a racy HashMap) — with **no tenant deletion path anywhere in the codebase**. At 1,000 tenants: 1,000 databases × 64 migrations, 1,000 pools, unbounded backend count. **Antares**: §3 — shared schema, RLS, one pool; tenant create is an INSERT, tenant delete is a DELETE.

### 14.3 A codebase fighting its own structure

- 9 deployable services whose overhead forced AllInOneRunner back into existence — the microservice split paid its costs without delivering its benefit. The 64 Flyway migrations are **copy-pasted into all 10 modules**; InfoManager's config is a paste of AtContextServer's *including its application name*. Dead config abounds (a TEMPORAL topic nobody binds, 7 declared-but-unbound channels, inert BUSHOST env vars, 5 Kafka containers running unconnected in the IOP compose ≈ 5 GB RSS).
- Business logic split across Java and ~38 PL/pgSQL functions + 6 triggers — merge-patch semantics live in `merge_json()`, entity extraction in triggers. Two languages, two test harnesses, invisible-to-the-debugger writes.
- The refactor-regression pattern: a "refactor + SpotBugs" commit deleted the empty-local-entity guard in `splitEntity` → 500s (`Uni set is empty`) on minimal entity creates, −30 ETSI tests. Nothing in the type system made the guard load-bearing.

**Antares**: one binary/roles (§9), migrations exist once, config is typed + validated at startup (unknown keys fatal), all semantics in Rust where invariants are encodable (e.g. the local-part-may-be-empty case is a variant of an enum the compiler forces every caller to handle).

### 14.4 JSON-LD as an afterthought retrofitted into the hot path

A vendored fork of jsonld-java (~11 kLOC, upstream package names shadowed) whose compaction **mutates its input** (J5) — which forced the defensive deep-copy epidemic (J4, up to 1,000 discarded entity copies per interval-sub firing), which is where much of the CPU went. No in-VM parsed-context cache existed at all (J1b: every parse = HTTP fetch + full term-definition rebuild); the Postgres-backed cache had a URL-rewrite branch that silently bypassed caching whenever gateway ≠ atcontext URL — one conditional fixed 25/61 → 61/61 on the jsonldContext suite. Per-string-GeoProperty the code allocated a fresh ObjectMapper and cloned the entire core context (J8b).

**Antares**: §6.3 — immutable compaction (borrow-checker-enforced), the parsed-context LRU as a first-class component, one context parse per batch request. This is also the top performance risk (§12 #1) and gets the phase-0 benchmark.

### 14.5 Nothing bounded, nothing streaming

11 WebClients with no timeouts and unbounded wait queues (one dead notification endpoint parks all later notifications forever); caches with TTL but no size cap, keyed by *client-supplied URLs*; unbounded temporal aggregation with `lastN` applied after aggregating everything; federated next-link accumulation without a ceiling; zero streaming (`RowStream`/`StreamingOutput` unused repo-wide); notifications fully built and *then* throttle-checked; a throttling window computed in the wrong unit (1000× too short). **Antares**: §2.1 rules — every buffer bounded at construction, throttle before build, `Duration` types, streaming list endpoints.

### 14.6 Spec drift without a tracking instrument

README claims CIM 009 V1.2.2 while the tree targets V1.9.1; `/entityMap` served in the singular where the spec says `/entityMaps`; Snapshots absent; expired-sub behaviour split between the two subscription services (one implements 5.8.1/5.8.2.4, the other forgot). There is no artifact that says "which clause, which version, implemented y/n". **Antares**: `docs/spec/` — the whole of CIM 009 in-repo, one file per clause with full spec text and status/evidence/robot frontmatter (§0.3), updated per PR — the compliance ledger is code-reviewed like code, and a CIM 029-shape ICS export is a `dev/spec.py` render away when a release needs it.

### 14.7 External evidence (the same defect classes, seen from outside)

The public issue tracker independently confirms every §14 class, across seven years:

- **State/subscriptions** — the most recurrent theme from 2019 to 2026: [#26](https://github.com/ScorpioBroker/ScorpioBroker/issues/26) (SubscriptionManager >70 % CPU / >4 GB RAM under minimal load, only a restart recovered), [#648](https://github.com/ScorpioBroker/ScorpioBroker/issues/648)/[#621](https://github.com/ScorpioBroker/ScorpioBroker/issues/621) (subscriptions silently not firing), [#388](https://github.com/ScorpioBroker/ScorpioBroker/issues/388) (duplicate notifications), [#30](https://github.com/ScorpioBroker/ScorpioBroker/issues/30) (false-positive notifications, open since 2019), [#662](https://github.com/ScorpioBroker/ScorpioBroker/pull/662)/[#663](https://github.com/ScorpioBroker/ScorpioBroker/issues/663) (per-tenant subscription loading at boot blocks the event loop / NPEs on restart).
- **Tenancy-by-DDL** — [#653](https://github.com/ScorpioBroker/ScorpioBroker/issues/653) (deadlock: concurrent upserts racing on-the-fly tenant-DB creation + Flyway), [#617](https://github.com/ScorpioBroker/ScorpioBroker/issues/617) (tenant contexts lost across restart). Exactly the race §3 designs away (tenant create = one INSERT).
- **Kafka as operational liability** — [#579](https://github.com/ScorpioBroker/ScorpioBroker/issues/579) (11th topic blew Azure EventHub tier limits — "massive cost increase"), [#154](https://github.com/ScorpioBroker/ScorpioBroker/issues/154)/[#385](https://github.com/ScorpioBroker/ScorpioBroker/issues/385) (no SASL/OAuth path, open for years), [#657](https://github.com/ScorpioBroker/ScorpioBroker/issues/657)/[#551](https://github.com/ScorpioBroker/ScorpioBroker/issues/551) (shipped compose fails on the same missing `value.serializer` we hit locally). The sizing doc even couples broker RAM to Kafka client counts (+64 MB/producer, +16 MB/consumer).
- **Performance long-tail** — [#573](https://github.com/ScorpioBroker/ScorpioBroker/issues/573) (irregular insert times, never root-caused), [#366](https://github.com/ScorpioBroker/ScorpioBroker/issues/366) (type queries ranging instant → 30 min), [#615](https://github.com/ScorpioBroker/ScorpioBroker/issues/615)/[#659](https://github.com/ScorpioBroker/ScorpioBroker/issues/659) (temporal aggregation seconds-slow and degrading with resolution), [#446](https://github.com/ScorpioBroker/ScorpioBroker/issues/446) (JSON-LD compaction blocking event-loop threads >2 s — §14.4 from the outside).

**Benchmark evidence** (Ntallaris/Bouloukakis/Magoutis, ACM IoT 2024 — real bus-fleet + traffic data, [paper](https://dl.acm.org/doi/10.1145/3703790.3703802), [code](https://github.com/satrai-lab/scbenchmark)): general queries are fine on all brokers (~47–75 ms); **temporal queries are 10–100× worse everywhere** (~48–110 s across Orion-LD/Scorpio/Stellio); Scorpio is ~2.5× slower than Orion-LD on batch ingestion (16k × 1 KB: ~45 s vs ~18 s, ≈355 entities/s) but degrades gracefully to 80 qps where Orion-LD collapses past 10–20 qps. The paper explicitly attributes part of Scorpio/Stellio latency to "Apache Kafka processing delays." Three Antares readings: (1) temporal is the industry-wide weak spot — the TimescaleDB + bounded-aggregation design (§8.2) is aimed at exactly the published failure mode; (2) the bus must stay off the request path (in Antares it is: NATS fan-out is post-commit, reads never touch it); (3) "graceful at 80 qps" is the Scorpio property to keep, "18 s batch ingest" is the Orion-LD number to beat.

### 14.8 What Scorpio got *right* (kept deliberately)

Fairness matters in a reference doc: the expanded-JSONB + extracted-columns storage model passes the full ETSI suite and ports directly; the `q=`→`jsonb_path` compilation strategy is proven; `csourceinformation` as a flattened federation match table is the right index; the probe-row pagination fix, the 207 forwarded-response classification, registration-scope narrowing per 4.3.6.1 (spec-mandated — was nearly "fixed" away once and reverted), and the entityMap TTL design are all battle-tested semantics Antares copies rather than reinvents.

## 15. Future-proofing

The design must survive three kinds of change: spec evolution, protocol evolution, and dependency evolution.

### 15.1 Spec evolution — NGSI-LD 2.0 is here, under a new ETSI home

CIM 009 went V1.1.1 (2019) → V1.9.1 (2025) with breaking-ish additions each cycle. **The landscape shifted in 2026** (per the ETSI TC DATA status presentation, 2026-01-27):

- ETSI created **TC DATA**, which absorbed ISG CIM; NGSI-LD is now a TC DATA deliverable.
- **NGSI-LD 2.0 published March 2026**, restructured from one monolithic GS into: **TS 104 175 NGSI-LD Core API** (abstract, transport-independent), **TS 104 176 NGSI-LD HTTP Binding**, **TS 104 243 NGSI-LD MQTT Notification Binding** — plus TS 104 178 (Information Model), TS 104 179 (Provenance/Integrity). A bidirectional GS 009↔TS mapping is documented, so nothing was semantically lost.
- The whole test-suite family transferred (TTF046): TS 104 190 (test suite), 104 192 (TPs), 104 193 (TSS), 104 191 (DO TPs), 104 188 (IOP), 104 187 (ICS), TR 104 186 (EU Interoperable Test Bed study).
- **2.1 is already in progress**, with the outlook list including **Issue #8: WebSocket Notification Binding** (§11 — the deferral is now also a standards-alignment play, and `/workspace/websocket.md` is a candidate contribution), Issue #4 request/response compression, #2 partial-success harmonization, #16 service execution, #21 temporal point-in-time queries, #35 except/omit registrations.

**Antares posture:** the v1 compliance baseline stays **CIM 009 V1.9.1**, because the Robot conformance suite that gates CI still targets it. But the 2.0 delta is tracked as a named migration with cheap items pre-adopted where 2.0 only *adds*: HTTP `HEAD`/`OPTIONS` support (2.0 issues #58/#59 — trivial in axum), `GET .../attrs/{attrId}` and the `/attrs/{attrId}/value` endpoint (#14/#15), `508 Loop Detected` for federation cycles (#25), `202 Accepted` on snapshot creation (#27), and schema readiness for #31's `propertyNames`/`relationshipNames` → `attributeNames` merge (§8.3 note). The Core-vs-Binding split also **independently validates the crate architecture**: `antares-model`/`antares-ql` implement TS 104 175 (Core), `antares-api` implements TS 104 176 (HTTP Binding) — the spec now cuts where the workspace already cuts.

NGSI-LD's anchoring is global (DSBA recommends it as *the* data-space exchange API; India IS 18003/IUDX; Korea CityHub TTA-certifies against the ETSI suite; TR 104 204 maps SAREF) — betting a broker on this spec is safe; betting on any frozen version of it is not. Antares bakes in the spec's own evolution mechanisms rather than chasing releases:

- **`Prefer: ngsi-ld=<version>` / `Preference-Applied` / 203** and per-subscription **`ngsildConformance`** are wired from v1.0 — the spec-native downgrade path means old clients keep working when Antares moves to V1.10/2.0, and notifications pin to what the consumer declared.
- **Tolerant-reader posture everywhere** (the websocket.md rule generalized): unknown members of Subscription/Registration/EntityMap documents are stored and echoed, never rejected or stripped — new spec members (as `notificationTrigger` once was) flow through a broker that predates them.
- **Feature registry**: each spec feature (Snapshots, ordered entities, aggregation, langprops…) is a named capability in code, reported by `/info` and the WS `capabilities` frame, and individually testable — so a new CIM 013 TP maps to a named unit, not to archaeology.
- **Schema headroom**: `entities.entity` stores the full expanded document — additive spec members cost zero migrations. The extracted columns are an index layer, not the storage format.

### 15.2 Protocol evolution

- **HTTP/3 & WebTransport**: `hyperium/h3` is still self-described "very experimental" (only the quinn backend production-supported; `h3-webtransport` more so) — so HTTP/3 is an *optional listener behind a trait*, never the foundation; axum-on-hyper stays the base. The WS binding already reserves new transports as **new subprotocol tokens/endpoints** (WS-28) — WebTransport arrives as `ngsi-ld-wt.v1` beside, not inside, `ngsi-ld-ws.v1`.
- **CBOR / binary bodies**: the W3C JSON-LD WG rechartered in 2025 with **CBOR-LD 1.0** as a deliverable (FPWD April 2026, claiming >60 % better compression than generic compressors via semantic compression) — so binary NGSI-LD payloads are a *when*, not an *if*. All Antares API types are serde types; CBOR-LD is a codec swap behind content negotiation when CIM adopts it. Explicitly out of v1.
- **gRPC for internal APIs**: skipped — tonic is feature-frozen mid-migration into CNCF `grpc-rust`; NATS + HTTP cover the internal surface, and adopting a bridge-state dependency for no current need is exactly the kind of bet this section exists to avoid.
- **Version negotiation as a pattern**: subprotocol token + capabilities frame + additive-only registries (envelope types, metadata keys, close codes) — the WS binding's §10 checklist is adopted broker-wide as the extension policy.

### 15.3 Scale beyond the targets

The §1 numbers are the v1 contract, not the ceiling. The pre-planned levers, in order: extracted-attribute side table (§8.1, named lever); read replicas for the query role (roles already split — point `--roles api` pods at the replica); hash-partition `entities` by tenant bucket (PK is already tenant-leading — partitioning is a migration, not a redesign); JetStream subject-sharding of `ANTARES_CHANGES`. Past one cluster, scale-out is **spec-native federation**: registrations + distributed operations across Antares instances — the mechanism is a compliance feature, so the scale-out path is tested by the ETSI DO suite itself.

### 15.4 Dependency evolution

- **TimescaleDB is TSL-licensed** — Timescale Inc. became **TigerData** (June 2025); the Apache-2.0 core excludes exactly the features the 16 GB budget leans on (columnar compression, continuous aggregates), which live in the TSL "Community" edition (free self-hosted, but a hard constraint on anyone offering Antares as DBaaS). The temporal store sits behind a Rust trait with exactly two implementations — and per §8.2 this is a **product requirement, not a hedge**: `timescale` and `plain` (native partitioning, broker-managed jobs) are both first-class, CI-tested modes, so no deployment is ever blocked on the TSL extension (Tembo's `pg_timeseries`, the would-be third option, is effectively stalled). Worse compression in plain mode, same SQL surface; migration between modes is a data move, not an API change. Worth tracking: **PostgreSQL 18** (async I/O 2–3× on seq/bitmap scans, B-tree skip scan, native `uuidv7()` — timestamp-ordered ids are directly interesting for temporal-instance PK locality) narrows the gap the fallback pays.
- **Single-maintainer `json-ld` crate**: wrapped (§6.3), forkable, and the core-context fast path means even a full replacement touches one crate.
- **Pre-1.0 crates** (async-nats, sqlx): workspace-pinned, upgraded in dedicated PRs, integration-suite-gated.
- **NATS itself**: `antares-bus` is the only crate that knows the bus exists; the in-process broadcast implementation used for dev/single-node is the proof the trait boundary holds.

### 15.5 Ecosystem adjacencies (kept out of core, fed by the change stream)

Data-space connectors, LDES publishers, OGC SensorThings bridges, and digital-twin tooling all want the same thing: the entity change feed. They attach as consumers of `ANTARES_CHANGES` (own durable each) or as WS clients — never as in-core modules. That keeps the core's compliance surface pure while making Antares a good citizen in the stacks it will actually be deployed in (the workspace's own data-space-connector / tmforum-api / FROST deployments are the template).

### 15.6 Future façade modules: SensorThings API and WFS — documented, deliberately NOT implemented

Two read-only façades are recorded here as future `antares-sta` / `antares-wfs` crates (§9.2) so the analysis exists when someone needs it. **Neither is scheduled; do not implement in v1.x.**

**SensorThings API (OGC STA), read-only.** The conceptual mapping is mostly manageable and prior art exists (FIWARE STA-interop work, academic mappings):

| STA | NGSI-LD |
|---|---|
| `Thing` | entity (e.g. `Device`) |
| `Datastream` | attribute of a device entity, or a Datastream-like entity (SAREF / Smart Data Models style) |
| `Observation` | attribute value + `observedAt` — i.e. a temporal attribute instance |
| `ObservedProperty` | the attribute name / property definition |
| `Sensor` | `Device`/`Sensor` entity (Smart Data Models) |
| `Location` / `FeatureOfInterest` | GeoProperty (`location`) |

Where it gets hard (the reasons this is a crate, not a weekend):
1. **Historical data is STA's bread and butter** — `Observations?$filter=phenomenonTime ge …` must translate to NGSI-LD temporal queries. Antares is well-positioned here *because* the Temporal API is built-in and first-class (§8.2) — the façade is only viable at all because of it.
2. **OData semantics** — `$filter/$orderby/$top` map tolerably onto `q=`/`orderBy`/`limit`, but `$expand` (nested relationship expansion) doesn't line up with NGSI-LD's limited traversal; the façade ends up doing multiple queries and stitching. Full OData conformance is the long tail — scope any first cut to Things/Datastreams/Observations with basic `$filter/$top/$orderby`.
3. **Model mismatch** — STA's strict 8-entity model vs loose NGSI-LD data in the wild: the façade must *synthesize* virtual Datastreams/Sensors per attribute per device by convention, which makes the mapping deployment-specific unless Smart Data Models are followed strictly. The mapping convention must be config, not code.
4. **Write path explicitly excluded** — read-only façade only (feeding STA clients: Grafana, QGIS). STA deep-insert + its MQTT extension would be a full project.

**The credible alternative to a façade, already proven in this workspace**: subscribe to NGSI-LD notifications and push Observations into a real FROST-Server — a fully conformant STA endpoint for free, at the cost of duplicated time-series storage and consistency management. That is exactly the CIVITAS pipeline pattern (MQTT→FROST/STA already deployed there), and per §15.5 it needs **zero Antares code**: a bridge consumes `ANTARES_CHANGES` or subscriptions. Decision rule when the need arrives: fully conformant STA required → FROST-sync bridge (no new crate); lightweight read view over well-modeled data → `antares-sta` façade.

**WFS / OGC API Features, read-only.** The smaller sibling: NGSI-LD already emits GeoJSON `FeatureCollection`s on Query/Retrieve Entities (`Accept: application/geo+json`, §5.2) — an `antares-wfs` crate is mostly an OGC API Features resource layout (landing page, collections = entity types, `items` = geo+json query with bbox↔`georel` translation) over the existing query path. Classic WFS 2.0 XML is *not* worth implementing; OGC API Features is the modern target, and for full GIS-server semantics (styling, tiles, WFS-T) the workspace answer is again a bridge: GeoServer/pygeoapi in front, as CIVITAS already runs.

## 16. Security requirements

Posture: Antares commonly runs behind a data-space PEP (ODRL/OPA policy front-ends, §15.5) — but it must be **safe when exposed directly**. **Authentication and authorization are both out of core (decision 2026-08-04)** — neither is NGSI-LD, both are generic HTTP middleware with no clause behind them, and the PEP / reverse proxy Antares already sits behind is where they belong. The earlier plan for a `none | oidc-bearer | mtls` tower layer is dropped, not deferred. What is never delegated: tenant isolation, injection safety, and resource bounds. These are requirements with tests, not guidelines.

"Safe when exposed directly" is therefore scoped precisely: an unauthenticated request cannot cross a tenant boundary (§16.1), inject SQL (§16.2), exhaust the process (§16.3), or make the broker attack someone else's network (§16.4). It CAN read and write, because deciding who may do that is the PEP's job.

### 16.1 Tenant isolation — zero slippage, enforced at seven seams

Adapted from the WS binding's WS-47 analysis and applied broker-wide:

1. **One source of tenant truth**: the `NGSILD-Tenant` header (or connection handshake, later WS), parsed once into the validated `TenantId` newtype — charset-restricted, token-safe (§7). Tenant is **never** read from a body, a query parameter, a forwarded frame, or a CSR payload (federation peer tenants come from the registration's own `tenant` member, mapped at registration time).
2. **Type-system threading**: `&TenantId` is the first parameter of every public store method (§9.3) — omitting a tenant filter is a compile error, not a review catch.
3. **RLS backstop always on** (§3): even a bug that builds tenant-less SQL returns zero foreign rows. `SET LOCAL` only, pool-safe. RLS denial tests run per store in CI (§9.5); the phase-4 pen-test (§13) includes cross-tenant probes as an exit criterion.
4. **Tenant-keyed everything in memory**: matcher indexes, registration mirror, KV entries, WS conflation maps — all keyed `(tenant, …)`. The single sanctioned cross-tenant structure is the `jsonld_contexts` cache (§8.3, WS-47 precedent).
5. **Tenant-segmented transport**: NATS subjects carry the tenant segment; consumers re-verify the event tenant against the row they touch (defense against subject-mapping bugs).
6. **Indistinguishable errors**: a cross-tenant entity/subscription/registration id returns the same `ResourceNotFound` 404 as a nonexistent one — no existence oracle. Timing: lookups go through the same code path either way.
7. **No side-channel leaks**: logs/metrics/traces never mix tenant data; per-tenant metric labels are bounded (label cardinality is also a DoS surface); notification bodies are built only from rows already tenant-filtered.

### 16.2 SQL injection — impossible by construction, then verified anyway

- User input **never** reaches SQL as a string fragment. `q=`/`scopeQ`/`geoQ`/params parse into the `antares-ql` AST (reject on error), and `antares-sql::compile` emits SQL whose *structure* comes only from the compiler and whose *values* travel exclusively as binds (`$n`). Attribute IRIs inside `jsonpath` expressions are the classic escape hatch — Antares binds jsonpath as parameters (`@? $n::jsonpath`) built from AST tokens that passed IRI validation; no user token is ever spliced into SQL or jsonpath text.
- Identifiers (tables, columns) are compiler constants; there is no dynamic identifier path.
- **Enforcement, not intention**: CI greps deny `format!`/string-concat feeding `sqlx::query` (reviewed allowlist for the compiler module only); clippy lint wall (§9.5); `cargo-fuzz` targets on the q/scopeQ/geoQ parsers and the JSON-LD expansion input path run in scheduled CI; the ETSI error-handling TPs (400-class) act as an external oracle.
- Same discipline for the *other* interpreters: GeoJSON goes through `geozero` typed encoding (never string-built WKT), NATS subjects through the hash encoder (never raw ids), shell-outs don't exist.

### 16.3 Input hardening & resource bounds

Every request-shaped resource has a configured cap, rejected with the spec error (§2.1 rule made security-normative): body size (413), JSON depth ≤ 64, batch entity count, URI+params length, @context chain length and fetch count per request, `joinLevel`, query AST depth/size → `TooComplexQuery` 403, result ceilings → `TooManyResults` 403. **Rate limiting is out of core (decision 2026-08-04)** — same reasoning as authn above: no clause, generic middleware, and per-IP counting is meaningless once traffic arrives through the load balancer or PEP that fronts the broker. Per-request *cost* is bounded here instead, which is the part that is genuinely the broker's own (a single request can no longer be made expensive enough to matter). Per-tenant quotas remain the named v2 policy knob (§1.1) if a deployment ever wants them enforced broker-side. All limits observable via metrics before users hit them.

### 16.4 Outbound safety (SSRF) — three egress classes, one policy

The broker makes outbound requests for: notification endpoints, @context fetches, and federation forwards. One `EgressPolicy` governs all three: scheme allowlist (http/https/mqtt(s)), a private-range guard for loopback/link-local/RFC 1918/metadata ranges — **allow-by-default since 2026-08-08** (notifications must reach private nets out of the box: dev boxes, compose stacks, ETSI/IOP mocks), with `ANTARES_EGRESS_ALLOW_PRIVATE=false` as the deliberate lockdown switch for internet-exposed deployments, redirect cap, DNS-pinned re-resolution (resolve once, connect to the resolved IP), response-size caps on @context fetches (LdContextNotAvailable 504 on breach), and per-destination circuit breakers (§16.7).

### 16.5 Supply chain & platform

`cargo-deny` advisories + license gate in CI (§9.5); `unsafe_code = "forbid"` outside the reviewed sonic module; rustls only (no OpenSSL CVE surface); distroless container, non-root, read-only rootfs; secrets only via env/file mounts, never in config files that reach logs; SBOM (cargo-auditable) in release builds. Notification/federation TLS verification is never disableable globally — per-registration `insecureSkipVerify` does not exist.

### 16.6 Security regression suite

The §14 Scorpio findings with security character become permanent tests: unbounded client-keyed caches (R4-class) — cache caps asserted; callback-UUID orphaning (L6) — bookkeeping delete paths tested; the WS-44-style parse-order tests (size check before parse) apply to HTTP bodies today; cross-tenant probes (§16.1) run in e2e per release.

### 16.7 Broad federation — thousands of registrations from one tenant

A single tenant may legitimately hold **1,000+ Context Source Registrations** (a national platform federating every municipality; a data space federating every participant). Design consequences, each load-bearing:

- **Matching is SQL, not iteration**: candidate CSRs for an operation come from indexed `csource_index` lookups (`(tenant_id, entity_type)` / `(tenant_id, entity_id)` + GIST on location + op-bitmask filter) — never a scan over all of a tenant's registrations. The in-memory mirror exists for the *matcher's* hot path and per-request narrow sets, sized ~1–2 KB/entry (§2.1 line: ~10k entries ≈ 20 MB); it is loaded lazily per tenant and evicted LRU — a 1,000-CSR tenant costs ~2 MB only while active.
- **Fan-out is bounded even when the match set is huge**: a distributed query matching hundreds of sources runs under a per-request forward semaphore (default ~16 concurrent), a per-source timeout (default 2 s, Scorpio's `federation.timeout` heritage), and an **aggregate request deadline** — sources that miss it are reported in the 207 as failures rather than stalling the response. Per-endpoint circuit breakers stop a dead peer from consuming its timeout on every request (the U1 lesson at federation scale).
- **Loop and depth control**: `Via` chains with **tenant-qualified pseudonyms** (`{alias}~{tenant}`, Table 5.2.40-1 — ADR-0011: one alias per process turned cross-tenant federation inside one broker into phantom loops), loop handling per 6.3.17/6.3.18 (508 only for a single exclusive/redirect source looping back; other loops drop out of registration matching, incl. peers' registered `contextSourceAlias` values already in the chain), and registration-scope narrowing (4.3.6.1) keeping forwarded queries minimal. A hop limit is NOT implemented — the 2026-08-09 audit established 4.3.6.4 prescribes the `local` param, not a hop count; self/peer-alias detection terminates every cycle through this broker.
- **EntityMaps make broad federation pageable**: distributed pagination over hundreds of sources materializes the id→source map once (§8.3) instead of re-fanning per page; the B1-class correctness tests guard it.
- **Churn at scale**: registration create/update/delete flows through `ANTARES_REGISTRY` as deltas; mirrors apply increments (never full reloads), and expiry filtering stays at the single yield point (§4.1) — 1,000 expiring registrations must not produce 1,000 forgotten-expiry code paths.
- **Discovery stays push-based**: csourceSubscriptions notify peers of registration changes — at 1,000+ CSRs, polling `/csourceRegistrations` is the anti-pattern the WS binding's WS-41 was written to kill.

Test rig: the phase-4 load rig (§13) gains a federation scenario — 1 tenant × 1,000 CSRs (mix of inclusive/exclusive/auxiliary, 20 % expired) against mock sources with injected latency/failures; exit criteria: query p95 bounded by the aggregate deadline, memory within the §2.1 mirror budget, 207 correctness under partial failure.

---

# ETSI NGSI-LD Testing & Runtime Guide (inherited from the Scorpio campaign)

*(Broker-agnostic knowledge about validating an NGSI-LD Context Broker against the ETSI suite — the environment, scripts and traps proven during the Scorpio work in `/workspace/ScorpioBroker`. Applies to Antares the moment it has an HTTP endpoint to point the suite at.)*

## Authoritative Spec Lookups (MemPalace) - MANDATORY

**Whenever you need NGSI-LD / ETSI spec information, query MemPalace — DO NOT answer from memory.**
*   **Tool:** `mempalace_search` — semantic search over the palace, including the indexed ETSI spec PDFs (`/workspace/etsi-cim-specs`, e.g. `gs_cim009v010901p.pdf`).
*   **Tool:** `mempalace_get_pdf_pages` — pull the exact PDF pages a search hit points at (via its `pdf_page` metadata) to read the full clause, not just the chunk.
*   **When to use:** Any question about payload shapes, API endpoint contracts, attribute semantics (`datasetId`, `observedAt`), or temporal behavior.
*   **How to answer:** Retrieve first, then ground the answer in the returned chunks. Always cite the clause number and location.
*   Before changing broker behavior to satisfy a failing test, also read the **precise robot file** in the ETSI suite to see what the test actually asserts.

## Fixing ETSI Test-Suite Failures — Broker Bug vs ETSI Tool Bug

*   **Look at the spec first:** Confirm what the spec actually requires via `mempalace_search` (+ `mempalace_get_pdf_pages` for the full clause) before changing broker code.
*   **Be 100% sure — never guess:** Only flag the test suite as wrong if you can definitively prove it contradicts or invents spec behavior.
*   **Broker Bug:** Fix the broker. Prefer native broker features over app-level hacks; keep DB changes version-controlled migrations.
*   **ETSI Tool Bug:** Log it in `error.md`. Do NOT hack the broker to pass a broken test.
*   **Prefer `https` URLs; use `http` only when necessary.** Default to `https` for @context URLs, endpoints and registrations (e.g. the forge `ngsi-ld-test-suite` context); use `http` only where actually required (local notification-receiver / context-server mocks, local broker endpoints).

## State Reset Between Suites (the phantom-state trap)

*   **Reset = API-level delete PAIRED with DB truncate:** `dev/reset-broker.sh <base_url>` (deletes all subscriptions/registrations/entities via the **NGSI-LD API**, which evicts broker-side caches — no restart needed) paired with `ngsi-ld-test-suite/clean_db.sh` (truncates data tables incl. temporal, preserving schema/migrations). `etsi-serial.sh` already does this pairing in `reset_state()`. `reset-broker.sh --temporal` also wipes temporal history via the API.
*   **Never** drop/recreate the Postgres database, truncate tables by hand, or restart the postgres container to clear state — raw SQL truncate leaves broker-side state behind (phantom 409s on create, phantom subscription matches, federated csource leakage).
*   **Cross-suite pollution is real:** a full serial run inflates ContextSource/Subscription failures (CommonBehaviours `045_01_03` registers a dead csource that leaks). Re-measure those two suites individually on a **fresh** broker before concluding a failure is real.
*   **Measure from a torn-down stack, not just `clean_db`:** federation/temporal state leaks across runs and clean_db (even a broker restart) does NOT clear it — only `./dev/run-iop.sh down` (volume wipe) does. A polluted stack produces phantom temporal-query results (e.g. `/temporal/entities?type=...` returning entities that exist in NO DB table). See `error.md`.
*   *(Antares note: the design makes rows the single source of truth precisely so `TRUNCATE` alone is a valid reset — but keep the API-level reset pairing in the harness anyway, so the suite setup stays broker-agnostic and the assumption is tested, not trusted.)*

## ONE environment for everything (dev AND CI) — the 5-broker stack

> **Antares run policy (supersedes the Scorpio recipe below for this repo).**
> Locally run **one store mode — the one your change touches**:
> `STORE=<memory|file|postgres|timescale> dev/etsi-local.sh` (workspace tests
> + that mode's 8 suite cells), or `STORE=<mode> STOP_ON_ERROR=1
> dev/etsi-pipeline.sh` for the tight debug loop. Local cells run serially,
> so running all four costs ~4× wall-clock for a signal CI already produces.
> **CI runs all four modes in parallel** — ONE pipeline,
> `.github/workflows/ci.yml`, fans out a 4 × 8 store × suite matrix
> (`fail-fast: false`, one image build feeds all 32 cells) and is the
> authority. Per commit: workspace tests → build → matrix → matrix summary →
> publish `:dev` + `:<sha>`. On a `v*` tag it additionally runs the serial
> all-suites job (E9d) and publishes `:<version>` + `:latest` — so `:latest`
> is a released version, never an untagged master build, and the serial run
> never lengthens a commit. `STORE=all dev/etsi-local.sh` reproduces the
> matrix locally on the rare occasion that is worth the wall-clock.
> Never build (cargo or docker) while a measured ETSI run is in flight — CPU
> contention manufactures phantom mock-502 and notification-timeout failures.

**Always use the single stack `compose-files/docker-compose-iop.yml`** — 5 self-contained brokers, each with its own Postgres + Kafka + MQTT (emqx), built from the working tree. Do NOT maintain a separate single-broker compose: single-broker suites just run against **broker1 (`scorpio1`)**, the federation suites (DistributedOperations, IOP) use all five, and MQTT suites use the per-broker emqx. This keeps local and CI on the same config so results match.

```bash
./dev/install-tools.sh        # 1. toolchain — once per session (state is wiped between sessions)
./dev/etsi-serial.sh          # 2. build image + bring up all 5 brokers + run EVERY suite serially
# (dev/etsi-serial.sh is the SAME entrypoint CI uses — .github/workflows/etsi-serial-test.yml.
#  It points the suite at broker1, resets state between suites with clean_db + reset-broker, keeps
#  https @contexts. Env knobs B1..B5 / CALLBACK_HOST select reachability; defaults = this dev box,
#  CI overrides them.)
```

To bring the stack up / down without running the suite: `./dev/run-iop.sh` and `./dev/run-iop.sh down` (tears down volumes too). `--no-mvn` reuses the existing locally-built broker image instead of rebuilding.

**While DEBUGGING, run with `STOP_ON_ERROR=1` so the suite halts at the FIRST failing test** (adds robot's `--exitonfailure` to every invocation, stops the serial run right there, writes `etsi-failures.md` for that suite + points at its `log.html`). Fast fix-loop: break, read the one error, fix, repeat — instead of waiting ~30 min for the full run. Default (flag unset) runs every suite to completion for the authoritative/CI result.
```bash
STOP_ON_ERROR=1 ./dev/etsi-serial.sh    # debug: stop & report at first failing test
./dev/etsi-serial.sh                    # full run (CI-identical), all suites to completion
```

## Federation testing — IOP & DistributedOperations (same single stack)

Both the ETSI **IOP** suite (`IOP_TP`) and the **DistributedOperations** suite (`TP/NGSI-LD/DistributedOperations`) run against the SAME stack. The broker image is **always built from the local working tree** (`dev/build-image.sh`, invoked by `dev/run-iop.sh`), so the stack always exercises the current code.

- **Reachability:** this is a docker-out-of-docker box, so published ports `9081..9085` land on the VM host, **not** this container's localhost. From the Robot suite running *here*, and between the brokers, reach them by **hostname**: `http://scorpio1:9090 .. http://scorpio5:9090` on the stack's own `iop_scorpio-net` network; `dev/run-iop.sh` attaches this dev container to it (and bridges onto `compose-files_default` only for the hardcoded MQTT mock hostname). For a CI runner use `http://host.docker.internal:9081..9085` (works as both client and as the federation endpoint).
- **Run DistributedOperations** (broker1 is the SUT; its HttpCtrl mock context sources run in this container): point the suite `url` at `http://scorpio1:9090/ngsi-ld/v1` and run `robot --exclude mqtt TP/NGSI-LD/DistributedOperations`.
- **Run IOP:** `robot --variable b1_url:http://host.docker.internal:9081/ngsi-ld/v1 ... --variable b5_url:http://host.docker.internal:9085/ngsi-ld/v1 IOP_TP`.
- **Isolation is built INTO the suite (no external scripts, no DB access):** `IOP_TP/__init__.robot` runs a Suite Setup/Teardown that resets every broker (b1..b5) via `libraries/FederationReset.py` — standard NGSI-LD API only (delete subscriptions, delete Context Source Registrations, query+batch-delete entities). Just run `IOP_TP` with the bN_url variables — the reset is automatic. Per-test cleanup stays in each test's own `Test Teardown`.
- **Known ETSI-tool setup bugs:** e.g. QueryEntities 04_01/04_02 create two payloads with the same id on one broker → 409. Log such cases in `error.md`, never hack the broker around them. See memory `multi-broker-fed-stack.md`.

## Environment gotchas (this dev box)

- `pgrep`/`pkill`/`free` are not installed — scan `/proc/[0-9]*/cmdline` instead.
- A backgrounded command that PREFIXES a kill-loop may never run its real payload or write its log — run the kill in a separate foreground call first, then launch the bare build/run command with `> log 2>&1`.
- Background long-running commands need an explicit `cd /workspace &&` — the cwd is not inherited reliably.
