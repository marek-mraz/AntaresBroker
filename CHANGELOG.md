# Changelog

Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
versioning per CONTRIBUTING "Versioning & releases". Release images are
published by CI: `:dev` on every green master run, `:dev-<run>` per run,
`:<version>` + `:latest` on `v*` tags. No `v*` tag has been cut yet — the
first tagged release will move the Unreleased section under its heading.

## [Unreleased]

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
  quickstart and an honest broker comparison table.
- Repo hygiene: working notes and analysis downloads moved out of the
  repository; root carries only product files.

### Fixed
- wasm Node tier: 6.3.4 Content-Length enforcement (bare 411 for
  POST/PATCH/PUT without the header) — the browser seam's stamped
  content-length had masked the check in the `wasm-file` cell
  (046_01, the cell's last 3 reds).

### Pre-release milestones

### 2026-08-15
- IOP id/idPattern routing campaign: 58 new Robot TPs (IOP_EXT_IDR_01..07)
  and four routing fixes found red by them — query-side idPattern/attrs/
  datasetId join CSR matching (5.12), forwarded id-lists and batch items
  narrowed to the registration scope (4.3.6.1), inbound remote
  notifications re-filtered by the original subscription selector
  (5.8.6/5.2.33).
- Production-readiness re-audit: every P0/P1 of the 2026-08-09 audit walked;
  the six still open fixed — k8s manifests boot under service links,
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
- The §0.3 conformance audit loop COMPLETE: all 947 ledger sections
  implemented or informative, zero not-implemented; Snapshot API (5.16);
  EntityMaps (5.14); durable HA state (snapshots/entity-maps/dist-subs in
  the store trait); distributed subscriptions consumer half (5.8.1.4).

### 2026-08-10 .. 2026-08-11
- Ledger reset and clause-by-clause audit of chapters 4-6 + annexes;
  federation Via/loop handling (6.3.18), tenant aliasing (4.14), the
  security wall (§16 egress policy, bounds, RLS gate).

### 2026-08-04 .. 2026-08-09
- Store ladder (`memory → file → postgres → timescale`), NATS JetStream
  scale-out (outbox, KV mirrors, durable consumers), MQTT notifications,
  the 4×8 ETSI store × suite CI matrix, K1 drain + K8 roll-under-suite,
  wasm/browser build, production-readiness + security audits.
