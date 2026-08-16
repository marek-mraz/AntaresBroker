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
| `ANTARES_TEMPORAL_RETENTION_DAYS` | unset (keep forever) | Temporal history retention; the sweep job prunes older attribute instances. |
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
| `ANTARES_EGRESS_ALLOW_PRIVATE` | `false` | Allow broker-initiated HTTP to private address ranges (notifications, forwards, `@context` fetches). Default-deny is SSRF protection — enable only in closed networks/local demos. |
| `ANTARES_FED_FANOUT` | `8` | Concurrent forwards per distributed read (4.3.6.1 orders the merge, not the requests). |
| `ANTARES_MAX_FED_RESPONSE_BYTES` | `16777216` (16 MiB) | Ceiling on one forwarded response body — one misbehaving peer cannot balloon broker memory. Over-cap parts fail as warning 111 (Table 6.3.17-1). |
| `ANTARES_MAX_BATCH_ITEMS` | `1000` | Batch entity-count cap (DoS bound; the spec sets none). Raise for trusted bulk producers. |
| `ANTARES_EXTRA_CA_FILE` | unset | PEM bundle of ADDITIONAL trust anchors for egress TLS (private CAs). Verification itself is never disableable. |

## Lifecycle & observability

| Variable | Default | Effect |
|---|---|---|
| `ANTARES_DRAIN_DELAY_MS` | `500` | Rolling update, step 2: keep serving this long after `/q/health` flips to 503 — the load balancer's notice window. |
| `ANTARES_DRAIN_DEADLINE_SECS` | `20` | Bound on waiting for in-flight connections during drain. Container `stop_grace_period` / `terminationGracePeriodSeconds` MUST exceed delay + deadline. |
| `ANTARES_TELEMETRY` | off | `1`/`true`/`on` enables the OTLP span pipeline (needs the endpoint too). |
| `ANTARES_OTLP_ENDPOINT` | unset | OTLP/HTTP traces collector, e.g. `http://collector:4318/v1/traces`. Unset costs nothing. |

Compile-time bounds (not configurable; spec-shaped rejections): body
4 MiB → 413, URI 8 KiB → 414, JSON depth 64 → 400. Current values are
reported live by `GET /q/health` under `limits`.

Node-shim (wasm tier) extras: `ANTARES_FILE` (redb path per shim) — see
the [browser guide](wasm.md).
