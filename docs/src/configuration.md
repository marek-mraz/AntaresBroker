# Configuration reference

All configuration is environment variables — no config file. Unknown
`ANTARES_STORE`/`ANTARES_BUS` values are fatal at startup, never silently
defaulted. This table is checked against the source by
`dev/check-env-docs.sh` (CI fails when a variable exists in code but not
here).

## Core

| Variable | Default | Effect |
|---|---|---|
| `ANTARES_STORE` | `memory` | Store mode: `memory`, `file`, `postgres`, `timescale`. Unknown value = fatal. |
| `ANTARES_TEMPORAL` | follows `ANTARES_STORE` | Temporal driver: a store mode, or `none` — history off. Mix freely with `ANTARES_STORE`, e.g. `file` current state with `timescale` history; temporal reads answer `OperationNotSupported` (422, CIM 009 Table 6.3.2-1) and nothing is recorded. A backend different from the store builds a second store instance used only for history. |
| `ANTARES_HTTP_PORT` | `9090` | HTTP listen port. |
| `ANTARES_ROLES` | `all` | Comma list of roles this process runs: `api`, `matcher`, `notifier`, `temporal`, `registry` — the role-split fleet shape. |
| `ANTARES_BUS` | `local` | Change-event bus: `local` (in-process, single node) or `nats` (JetStream, multi-pod). Unknown value = fatal. |
| `ANTARES_HOST_ALIAS` | `antares` | This broker's name in federation `Via` chains (CIM 009 6.3.18) — loop detection identity. Two LB'd replicas of one logical broker share one alias. |
| `ANTARES_PUBLIC_URL` | `http://{host_alias}:{port}` | The URL peers can reach this broker at (distributed-subscription callbacks, 5.8.1.4). Set it whenever the default is not routable from peers. |

## Store backends

| Variable | Default | Effect |
|---|---|---|
| `ANTARES_DATA_DIR` | — (required for `file`) | Directory for the redb file. Must be a mounted volume — data never lives inside the image. |
| `ANTARES_DATABASE_URL` | — (required for `postgres`/`timescale`) | PostgreSQL connection string; PostGIS required, TimescaleDB for `timescale`. Bounded startup retry while the DB boots. |
| `ANTARES_REQUIRE_RLS` | unset | `1`/`true`: refuse to start when the DB role bypasses Row-Level Security (defense-in-depth for shared-schema multi-tenancy). |
| `ANTARES_PG_POOL` | `20` | Connection-pool size for `postgres`/`timescale`. Unparsable value = fatal. Sessions carry `lock_timeout` 5 s. |
| `ANTARES_PG_STATEMENT_TIMEOUT_MS` | `30000` | Per-session `statement_timeout` on every pooled connection: a query past it is cancelled and answered `InternalError` (500, "database statement timeout"; CIM 009 5.5.2 names database timeouts as InternalError). Migrations are exempt. Not a positive integer = fatal. |
| `ANTARES_MIGRATE` | on | `0`/`false` skips running migrations from this process, so serving replicas do not race the DDL — run them once from a job or init container instead. |
| `ANTARES_ALLOW_SHARED_LOCAL` | unset | `1` permits `bus=local` with a `postgres`/`timescale` store — safe ONLY for a strictly single-process deployment; two such processes double-fire notifications. |
| `ANTARES_TEMPORAL_RECORD` | `all` | History gate for the entity endpoints. `all`: every changed attribute instance is recorded. `observed`: only instances carrying `observedAt` enter history — a metadata-only write (no `observedAt`) updates current state and its `modifiedAt` but leaves no history, so `timeproperty=modifiedAt`/`createdAt` temporal queries return nothing for never-observed attributes. `none`: the entity endpoints record nothing; the temporal API still stores and serves what it is given directly (unlike `ANTARES_TEMPORAL=none`, which switches the temporal seam off). The ETSI temporal suites assume `all`, which is why it stays the default. Unknown value = fatal. |
| `ANTARES_TEMPORAL_RETENTION_DAYS` | unset (keep forever) | Temporal history retention; the sweep job prunes older attribute instances. Applies to the temporal half wherever it lives: a `file` store with `postgres` history still runs the job. |
| `ANTARES_SWEEP_SECS` | `900` | Cadence of the background GC sweep (expired entities/registrations, 4.22) — identical across store modes. |

## NATS scale-out

| Variable | Default | Effect |
|---|---|---|
| `ANTARES_NATS_URL` | — (required for `bus=nats`) | NATS server URL; JetStream streams and the subscription KV bucket are asserted at startup. |
| `ANTARES_NATS_REPLICAS` | `1` | JetStream replica count for streams/KV (set 3 on a 3-node NATS cluster). |
| `ANTARES_OUTBOX_DRAIN` | on | `off` disables the notification outbox drainer in this process (crash-drill lever / dedicated-drainer split). |

## Federation & egress hardening

| Variable | Default | Effect |
|---|---|---|
| `ANTARES_EGRESS_ALLOW_PRIVATE` | `true` | Broker-initiated HTTP and MQTT (notifications, forwards, `@context` fetches) may reach loopback, link-local, RFC 1918 and cloud-metadata ranges. Set `false` (or `0`) on an internet-exposed deployment to deny them; the scheme allowlist, redirect cap, DNS pinning and response-size caps apply regardless. A refused delivery is booked as a failure (`lastFailure`, `status: failed`) and never retried. |
| `ANTARES_FED_FANOUT` | `8` | Concurrent forwards per distributed read (4.3.6.1 orders the merge, not the requests). |
| `ANTARES_FED_INFLIGHT` | `256` | Forwarded requests in flight for the whole process; callers over the cap wait. Bounds the buffers and connections open federated queries hold (6 000 open queries × 34 sources once reached 7.7 GB). |
| `ANTARES_MAX_FED_RESPONSE_BYTES` | `16777216` (16 MiB) | Ceiling on one forwarded response body — one misbehaving peer cannot balloon broker memory. Over-cap parts fail as warning 111 (Table 6.3.17-1). |
| `ANTARES_MAX_BATCH_ITEMS` | `1000` | Batch entity-count cap (DoS bound; the spec sets none). Raise for trusted bulk producers. |
| `ANTARES_MAX_BODY_BYTES` | `4194304` (4 MiB) | Request body cap, answered with a bare 413 (6.3.4). One number governs the extractor limit and the bounds wall. |
| `ANTARES_CORS_ORIGINS` | unset (no CORS headers) | Browser origins allowed, comma-separated, or `*`. Preflights are answered for every method and header; `Link`, `NGSILD-Tenant` and `NGSILD-Results-Count` are exposed. |
| `ANTARES_API_SURFACES` | `admin` | Comma list of HTTP surfaces mounted beside the NGSI-LD API root, each under its own reserved prefix (`admin` serves `/q`). An unknown name is fatal at startup and names the shelf the binary was built with; a selection that leaves out `admin` serves no `/q` at all, probes included. |
| `ANTARES_EXTRA_CA_FILE` | unset | PEM bundle of ADDITIONAL trust anchors for egress TLS (private CAs). Verification itself is never disableable. |

## Notification delivery

| Variable | Default | Effect |
|---|---|---|
| `ANTARES_NOTIFY_ATTEMPTS` | `1` | Delivery attempts per notification, first one included. `1` is 5.8.6 as written: one send, the outcome booked. Higher values retry on their own task with exponential backoff; the retries never move `timesSent` again. |
| `ANTARES_NOTIFY_BACKOFF_MS` | `1000` | Delay before the first retry; doubles per retry (±20 % jitter, 60 s ceiling). |
| `ANTARES_NOTIFY_MAX_AGE_SECS` | `300` | No retry starts later than this after the first attempt. When the attempts or the age run out the notification becomes a dead letter (`/q/dead-letters`, see [operations](operations.md#notification-delivery)). |

## Lifecycle & observability

| Variable | Default | Effect |
|---|---|---|
| `ANTARES_HEADER_READ_TIMEOUT_MS` | `10000` | A connection that has not finished its request HEAD within this window is closed (slow-loris bound). |
| `ANTARES_MAX_CONNECTIONS` | `10000` | Concurrent-connection ceiling; further accepts are dropped. Counts keep-alive and LB health-check connections too — size accordingly. |
| `ANTARES_DISCOVERY_SCAN_MAX` | `100000` | Entities one `/types`/`/attributes` discovery fold may read; past it the answer is 403 TooManyResults (5.5.6) instead of an unbounded scan. |
| `ANTARES_DRAIN_DELAY_MS` | `500` | Rolling update, step 2: keep serving this long after `/q/health` flips to 503 — the load balancer's notice window. |
| `ANTARES_DRAIN_DEADLINE_SECS` | `20` | Bound on waiting for in-flight connections during drain. Container `stop_grace_period` / `terminationGracePeriodSeconds` MUST exceed delay + deadline. |
| `ANTARES_TELEMETRY` | off | Any value but an off spelling enables the metrics recorder and, with the endpoint, the OTLP span and log pipelines. |
| `ANTARES_OTLP_ENDPOINT` | unset | OTLP/HTTP collector for traces and logs, e.g. `http://collector:4318/v1/traces`; log records go to the `v1/logs` twin of that URL with the same resource attributes. Unset costs nothing. |

Compile-time bounds (not configurable; spec-shaped rejections): body
4 MiB → 413, URI 8 KiB → 414, JSON depth 64 → 400. Current values are
reported live by `GET /q/health` under `limits`.

Node-shim (wasm tier) extras: `ANTARES_FILE` (redb path per shim) — see
the [browser guide](wasm.md).
