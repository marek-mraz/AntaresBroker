# Changelog

Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
versioning per CONTRIBUTING "Versioning & releases". Release images are
published by CI: `:dev` on every green master run, `:dev-<run>` per run,
`:<version>` + `:latest` on `v*` tags.

## [Unreleased]

### Added
- Notification delivery survives an endpoint that is down:
  `ANTARES_NOTIFY_ATTEMPTS` retries on the backoff of
  `ANTARES_NOTIFY_BACKOFF_MS` until `ANTARES_NOTIFY_MAX_AGE_SECS`, and what
  still fails becomes a dead letter — listed, dropped and replayed under
  `/q/dead-letters` with every credential blanked in the listing.
- `GET /q/tenants` lists the tenants a broker holds, `GET /q/tenants/{tenant}`
  reads what one holds, `DELETE /q/tenants/{tenant}` purges it.
- `POST /ngsi-ld/ex/remote-notify` receives the notifications a distributed
  subscription's context sources send back (5.11).
- Request bounds a deployment sizes for itself: `ANTARES_MAX_BODY_BYTES`,
  `ANTARES_MAX_CONNECTIONS`, `ANTARES_HEADER_READ_TIMEOUT_MS`,
  `ANTARES_DISCOVERY_SCAN_MAX`, `ANTARES_FED_INFLIGHT`.
- The temporal half is configured on its own: `ANTARES_TEMPORAL` picks its
  backend or switches history off, `ANTARES_TEMPORAL_RECORD` decides what the
  entity endpoints record.
- Postgres knobs: `ANTARES_PG_POOL`, `ANTARES_PG_STATEMENT_TIMEOUT_MS`, and
  `ANTARES_MIGRATE=0` so serving replicas never race the DDL.
- `ANTARES_CORS_ORIGINS` serves a browser without a proxy in front,
  `ANTARES_API_SURFACES` chooses which surfaces mount beside the API root, and
  `ANTARES_ALLOW_SHARED_LOCAL` permits `ANTARES_BUS=local` on a shared database
  for a strictly single-process deployment.

### Changed
- **Breaking, store format.** The Postgres and Timescale migrations are
  squashed into one `0001_init.sql`, and `0004` drops the `entity_maps` table
  `0001` created. A database migrated by 0.1.0 carries that release's
  nine-migration history, which this build cannot match: start from an empty
  database, or keep serving the old one with 0.1.0. The broker refuses such a
  database at boot and names the cause, rather than retrying the connection for
  30 s and reporting it as unreachable.
- Clauses 4.3.6.6 and 4.5.19.0 are reclassified `partial`, each with its gap
  named in `docs/spec/`.

### Fixed
- The Registration Subscription the consumer half of a distributed
  subscription owns (5.8.1.4) is no longer served on `/csourceSubscriptions`:
  a subscriber could read it, patch `isActive` to false or delete it —
  disabling the distributed half of a Subscription that still reports status
  `active` — and read every other subscriber's Subscription id out of the
  listing. It is held as internal state (ADR-0012) instead, so 5.11.5 lists
  exactly the subscriptions a client created. A broker upgraded onto a
  persistent store leaves the records it wrote before as inert documents on
  that endpoint; delete them once.
- 89 conformance fixes, each committed under the clause it holds: the temporal
  representation and its aggregations (4.5.9, 4.5.19, 6.3.11), `@context`
  ownership and hosting (5.13, 5.13.1), forwarding and its bounds (4.3.6, 5.7,
  5.10.2), delivery bookkeeping (5.2.14.2, 5.8.6), what a batch answers about an
  expired Entity (5.6.7, 5.6.8, 5.6.9.4, 5.6.10), and the HTTP request wall
  (6.3.4, 6.3.5, 6.3.10).

## [0.1.0] - 2026-08-16

First tagged release: the ETSI-conformant broker with the full store
ladder, NATS scale-out, federation, the wasm build, docs book, examples
and the gated release pipeline.

### Added
- User documentation book (mdBook, rendered to the Pages site under
  `/docs/`): getting started, configuration reference (CI-checked against
  the code's env vars), deployment, federation, wasm, operations,
  conformance.
- `examples/`: quickstart, federation pair, subscriptions (HTTP + MQTT),
  browser shim, smart-city dataset — each executed before commit and run
  by the `examples` workflow on tags and weekly.
- Semver/release policy (CONTRIBUTING) and this changelog format.

### Changed
- **License: BSD-3-Clause → EUPL-1.2.**
- README rewritten around measured differentiators, with a 60-second
  quickstart and a broker comparison table.
- Repo hygiene: working notes moved out of the repository; root carries
  only product files.

### Fixed
- wasm Node tier: 6.3.4 Content-Length enforcement (bare 411 for
  POST/PATCH/PUT without the header) — the browser seam's stamped
  content-length had masked the check in the `wasm-file` cell (TP 046_01).

### Pre-release milestones

### 2026-08-15
- IOP id/idPattern routing campaign: 58 new Robot TPs (IOP_EXT_IDR_01..07)
  and four routing fixes found red by them — query-side idPattern/attrs/
  datasetId join CSR matching (5.12), forwarded id-lists and batch items
  narrowed to the registration scope (4.3.6.1), inbound remote
  notifications re-filtered by the original subscription selector
  (5.8.6/5.2.33).
- Production-readiness fixes: k8s manifests boot under service links,
  `/q/ready` readiness (store ping + bus state), the API surface gated by
  `--roles`, outbox exact-seq ack (no more gap-row loss), 5.2.12 throttling
  actually suppresses, temporal types refresh after first touch.
- `/q/health` reports the bus (`bus: {mode, connected, reconnects}`) and a
  NATS-outage e2e proves the broker serves through an outage and drains the
  backlog on reconnect.
- Weekly rolling-update proof (`roll-weekly`), the clickable ETSI
  conformance report page + shields badges, docs index, operations runbook,
  repo hygiene files (LICENSE, SECURITY, CONTRIBUTING, this file).

### 2026-08-14
- IOP TP campaign: 118 new interoperability TPs (full IOP_TP tree 220/220);
  distributed-operations fixes: 4.20 query-op gating, 6.3.17 peer-warning
  propagation, batch success/error entity-id arrays, 5.2.34
  timeout+cooldown honoured, split-notification merges shaped like local
  ones.
- ETSI-driven coverage (`etsi-coverage.yml`): weekly lcov+html across the
  store matrix.

### 2026-08-13
- Temporal `q=` follow-up: validity-aware scopeQ (4.18), geo prefilter
  migration, exact-q entity paging, `!=`/languageMap/collation query leaves.

### 2026-08-12
- Conformance ledger complete: all 947 clause sections implemented or
  informative, zero not-implemented; Snapshot API (5.16);
  EntityMaps (5.14); durable HA state (snapshots/entity-maps/dist-subs in
  the store trait); distributed subscriptions consumer half (5.8.1.4).

### 2026-08-10 .. 2026-08-11
- Clause-by-clause conformance pass over chapters 4-6 + annexes;
  federation Via/loop handling (6.3.18), tenant aliasing (4.14), the
  security wall (egress policy, bounds, RLS gate).

### 2026-08-04 .. 2026-08-09
- Store ladder (`memory → file → postgres → timescale`), NATS JetStream
  scale-out (outbox, KV mirrors, durable consumers), MQTT notifications,
  the 4×8 ETSI store × suite CI matrix, graceful drain + the
  roll-under-suite drill, wasm/browser build, security hardening.
