# Road to 1.0

What 1.0 means: a broker an operator can run from the book alone, with
every claim in the book backed by a run CI reproduces. The criteria below
are checked off only with evidence (run links or commit hashes) recorded
next to them.

## Landed since 0.x

- Storage as two drivers, current state and history chosen independently
  (`ANTARES_STORE`, `ANTARES_TEMPORAL`, including `none`), with a history
  gate (`ANTARES_TEMPORAL_RECORD`).
- Notification delivery policy: retries with backoff, dead letters with
  an admin replay surface, single attempt by default.
- Tenant inventory and purge through the admin API.
- Logs next to traces over OTLP, one endpoint, one resource.
- Bulk load into Postgres and a documented backup and restore per mode.
- The book: subscriptions, temporal, federation, operations, admin API,
  storage, extension model and conformance chapters written from real
  runs.

## Remaining for 1.0

- [ ] **Three consecutive green full matrices.** Three back-to-back `full`
      runs on master with every one of the seven cells green, including
      `wasm-file`, with nothing but unrelated docs changing between them.
- [ ] **HA soak.** The role-split fleet (10 pods, NATS, Postgres) under
      continuous load for at least 24 hours with a roll every hour: zero
      lost notifications, no 5xx bursts beyond the drain windows, flat
      RSS. Rig and numbers recorded in the operations chapter.
- [ ] **Upgrade-path tests.** Data created on version N-1 (file and
      postgres) served by version N: entities, history, subscriptions
      firing. Runs on every release tag from the first release candidate.
- [ ] **Egress review.** A structured review of the egress wall (SSRF
      guards, breakers, TLS trust, notification, forward and `@context`
      paths); findings fixed or accepted in writing.
- [ ] **Release machinery proven.** One 0.x release shipped end to end
      through the gated pipeline (image, binaries, wasm bundle, SBOM,
      signature, notes) with the examples job green on that tag.

## Waiting on hardware

Performance CI on a dedicated Hetzner runner: the workflow exists
(`perf-weekly.yml`, `perf-janitor.yml`, `dev/perf/`), the first real run
needs the cloud token and runner secrets, and the variance profile from
ten repeated runs comes before any regression gate.

## Out of scope for the broker

Authentication, rate limiting and per-tenant quotas belong to the gateway
in front of the broker; the shared crates give that gateway the broker's
own parser, query engine and matcher. Authorization is out of scope in the
same sense: the broker ships no policy engine and takes no authorization
decision of its own. What it does ship is the seam an engine attaches to
(ADR-0020), because three decisions cannot be made in front of the broker
at all — narrowing the query the store runs, filtering one subscription's
notification, and filtering a federated result before it is rendered. A WebSocket
notification binding waits for the ETSI text. A non-HTTP ingest path is
not planned: an ingester that speaks MQTT or NATS posts to the batch
endpoints, and the in-process router already serves the browser build
without a socket.

When every box holds, tag `v1.0.0-rc1`; `v1.0.0` follows after one more
green full matrix on the candidate with no code changes.
