# Admin API

This chapter is every route the broker serves OUTSIDE `/ngsi-ld/v1`.
There are three groups and no others: `/q/` is the operator surface,
`/ex/v1/` is the peer-facing wire between brokers, and `/x/` is whatever a
deployment mounted. Everything else the broker answers is CIM 009, and the
[conformance ledger](conformance.md) owns it.

`/q/` carries no NGSI-LD semantics and belongs behind the gateway. A worker
pod (`ANTARES_ROLES` without `api`) serves only these routes. Errors use the
same problem-details shape as the NGSI-LD API.

| Route | Purpose |
|---|---|
| `GET /q/health` | Liveness and the process view |
| `GET /q/ready` | Readiness for the load balancer |
| `GET /q/metrics` | Prometheus text |
| `GET /q/tenants` | Tenant names |
| `GET /q/tenants/{tenant}` | What one tenant holds |
| `DELETE /q/tenants/{tenant}` | Tenant purge |
| `GET /q/dead-letters` | Notifications the delivery policy gave up on |
| `POST /q/dead-letters/{id}/replay` | One more delivery attempt |
| `DELETE /q/dead-letters/{id}` | Drop a dead letter |
| `POST /ex/v1/remote-notify` | The 5.8.1.4 receiver other brokers post to |

## GET /q/health

`200` with the process view, `503` while the instance drains (a roll in
progress). A memory-store broker answers:

```json
{
  "status": "UP",
  "store": "memory",
  "temporal": "memory",
  "version": "0.1.0",
  "commit": "3432674",
  "notificationSchemes": ["http", "https", "mqtt", "mqtts"],
  "deadLetters": 0,
  "changesDropped": 0,
  "taskPanics": 0,
  "temporalDrainErrors": 0,
  "policy": { "engine": "allow-all", "timeoutMs": 250 },
  "surfaces": { "admin": { "prefix": "/q", "routes": 8 } },
  "limits": {
    "maxBatchItems": 1000,
    "maxBodyBytes": 4194304,
    "maxContextFetches": 32,
    "maxFedFanout": 8,
    "maxFedInflight": 256,
    "maxFedResponseBytes": 16777216,
    "maxFoldDocs": 100000,
    "maxGeoVertices": 1024,
    "maxJoinLevel": 10,
    "maxJsonDepth": 64,
    "maxPeerWarnings": 8,
    "maxQLinkLookups": 512,
    "maxQNodes": 512,
    "maxRegexCache": 1024,
    "maxRegexCacheBytes": 67108864,
    "maxRegexProgramBytes": 262144,
    "maxUriBytes": 8192,
    "rejectedBodyTooDeep": 0,
    "rejectedBodyTooLarge": 0,
    "rejectedUriTooLong": 0
  },
  "memory": { "allocatedBytes": 5671592, "residentBytes": 24051712 }
}
```

| Field | Meaning |
|---|---|
| `status` | `UP`, or `DRAINING` with status 503 once shutdown began. |
| `store` | The current-state backend: `memory`, `file`, `postgres`, `timescale`. |
| `temporal` | The history backend: one of the four, or `none` when history is off (`ANTARES_TEMPORAL`). |
| `storeInfo`, `temporalInfo` | What each driver actually runs on: `{engine}` for the built-in stores (`memory` or `redb`), and on Postgres `{engine, poolSize, poolAcquireTimeoutSeconds, server, postgis?, timescaledb?}` — the pool's own shape always, the server probe read once at startup and omitted if it failed. Absent when a driver has nothing to add to its name (`none` history). Two deployments answering `postgres` are told apart here. |
| `version`, `commit` | Workspace version and the git hash the binary was built from. |
| `notificationSchemes` | The `notification.endpoint.uri` schemes this build can deliver to — the registered bindings (6.3.8, clause 7, and any a deployment added). A subscription naming a scheme absent here is refused at creation with `BadRequestData`. |
| `deadLetters` | Dead letters this process wrote since start ([notification delivery](operations.md#notification-delivery)). |
| `changesDropped` | Changes the bounded matcher queue refused since start, each one a notification never matched: delivery is slower than the write rate. |
| `taskPanics` | Panics absorbed at the notification-task boundary since start, each one a lost notification. Reported here because the Prometheus recorder is off unless `ANTARES_TELEMETRY` is set. |
| `temporalDrainErrors` | Post-response history writes that failed since start; the client's 2xx stands, the counter and a warning record the loss. |
| `policy` | The policy engine this binary was started with (`ANTARES_POLICY`, `allow-all` unless a deployment registered another) and the deadline one decision gets (`ANTARES_POLICY_TIMEOUT_MS`). An engine that overruns it is a refusal, not a delay. |
| `surfaces` | The mounted admin surfaces by name, each with its prefix and route count. |
| `limits` | The bounds wall: every `max*` cap in force and the `rejected*` counters of requests refused by it. |
| `memory` | jemalloc live (`allocatedBytes`) and resident (`residentBytes`) bytes. |
| `commitQueueDepth`, `commitQueuePeak` | Present only for a store with a single committer to queue behind (`file`, and the browser build over OPFS): writers queued now and at peak. |
| `bus` | `ANTARES_BUS=nats` only: `{mode, connected, reconnects}`. |

## GET /q/ready

`200 {"status":"READY","store":true}` when the instance is not draining,
the store answers (`SELECT 1` on Postgres) and, under `bus=nats`, the bus
is connected; otherwise `503 {"status":"NOT_READY", …}` with the failing
member `false`. Point the readiness probe here and the liveness probe at
`/q/health`: a restart does not cure a lost database.

## GET /q/metrics

Prometheus text with the `antares_` prefix:

| Metric | Meaning |
|---|---|
| `antares_http_requests_total` | Requests served, by method and status class. |
| `antares_http_request_duration_seconds` | Request service time histogram. |
| `antares_limit_rejections_total` | Bounds-wall rejections, by limit. |
| `antares_commit_queue_depth` | `file` store: writers queued behind the redb committer. |
| `antares_memory_allocated_bytes`, `antares_memory_resident_bytes` | jemalloc live and resident bytes. |
| `antares_uptime_seconds` | Seconds since process start. |
| `antares_draining` | 1 while this instance drains. |
| `antares_temporal_drain_errors_total` | Failed post-response history writes. |
| `antares_notifications_sent_total`, `antares_notifications_retried_total`, `antares_notifications_failed_total` | Notification deliveries by outcome. |
| `antares_notification_changes_dropped_total` | Change events the notifier dropped under back-pressure. |
| `antares_notification_task_panics_total` | Delivery tasks that panicked (a bug, never expected). |
| `antares_change_lag_seconds` | Age of the change a notifier is handling. |
| `antares_policy_failures_total` | Decisions the policy seam had to make itself because the engine did not, by reason (`panic`, `timeout`). Always zero under the built-in `allow-all` engine. |
| `antares_pg_transaction_begin_seconds` | `postgres`/`timescale`: time to obtain a pooled connection and open a transaction — the pool wait plus one BEGIN round trip. |
| `antares_pg_pool_timeouts_total` | `postgres`/`timescale`: acquire timeouts, each one a request answered 503 with `Retry-After`. |

Every `_seconds` metric is a true histogram, bucketed at 5 ms, 10 ms, 25 ms,
50 ms, 100 ms, 250 ms, 500 ms, 1 s, 2.5 s, 5 s, 10 s, 30 s and 60 s. The
bounds are what `histogram_quantile()` can resolve, and the top ones exist
because service time reaches tens of seconds once the accept path saturates.

## Tenants

### GET /q/tenants

Answers the names, sorted, and nothing else:

```text
GET /q/tenants
["default","acme","zoo"]
```

Names only on purpose. A deployment runs up to 10 000 tenants (ADR-0001), and
a list carrying per-kind counts would cost a count per kind per tenant on one
request. `200` always; the route takes no parameters. Pick a name, then read
its detail:

### GET /q/tenants/{tenant}

```text
GET /q/tenants/acme
{"tenant":"acme","counts":{"entities":0,"subscriptions":0,"csourceSubscriptions":0,
  "registrations":0,"snapshots":0,"entityMaps":0,"distSubs":0,"attrInstances":0}}
```

`200`, `404` for a tenant that does not exist, `400` for a name outside the
tenant grammar or one of the broker's internal names. `createdAt` is present
on Postgres, where the `tenants` table records it. The default tenant always
exists (5.5.10) and is always readable, even when empty.

### DELETE /q/tenants/{tenant}

Purges the tenant: `204` when done, `404` for
an unknown tenant (`{"title":"ResourceNotFound","detail":"tenant nope"}`),
`409` while a distributed subscription of it still holds a copy at a
Context Source, `400` for a name outside the tenant grammar. The default
tenant is emptied and keeps existing. The path names the tenant; an
`NGSILD-Tenant` header is ignored. Background in
[operations](operations.md#tenants).

## Dead letters

A letter carries the subscription id, tenant, endpoint, headers, body,
attempt count, first and last error and timestamps. What produces one and
why an egress refusal never does:
[notification delivery](operations.md#notification-delivery).

### GET /q/dead-letters

`?tenant=&subscription=&limit=` — letters of one tenant (the default tenant
when `tenant` is absent), newest first, `limit` 100 by default; `400` for a
`limit` that is not a positive integer or a tenant outside the grammar.
Endpoint userinfo, `receiverInfo`, `notifierInfo` and the rendered `headers`
of an older letter are shown blanked; the stored letter keeps them so a
replay still authenticates.

### POST /q/dead-letters/{id}/replay

`?tenant=` — one attempt through the same binding under the egress policy of
the moment: `204` and the letter is gone, `502` with the failure text and the
letter kept, `404` when the tenant holds no such letter.

### DELETE /q/dead-letters/{id}

`?tenant=` — `204`, or `404` when the tenant holds no such letter.

## The peer-facing wire

### POST /ex/v1/remote-notify

Where a Context Source posts a notification for a distributed subscription
this broker created (5.8.1.4). It is the one non-standard route that cannot
be firewalled off with `/q/`: every Context Source a subscription copy was
forwarded to has to reach it, and `ANTARES_PUBLIC_URL` is what that copy
advertises. It sits outside the `/ngsi-ld` prefix ETSI owns, and `v1`
versions the broker-to-broker wire independently of the NGSI-LD API version
(ADR-0019).

The body is an NGSI-LD Notification. Its `subscriptionId` is the routing
key, and it is a broker-generated UUID rather than the subscriber's own
Subscription id, so a Context Source learns nothing about the subscriber and
cannot address any other subscription. That key resolves through the stored
mapping to the tenant and the local subscription; the request's
`NGSILD-Tenant` header is not read on this route at all.

`200` when the notification was accepted — including when the mapping
resolves but nothing was left to deliver. `400` for a body that is not JSON,
a body without `subscriptionId`, or a `data` array carrying more Entities
than `ANTARES_MAX_BATCH_ITEMS` (the cap is applied before any store touch:
one notification drives one local retrieve and one federated fan-out per
Entity, so an uncapped array is an amplification lever). `404` when no
distributed subscription maps the key. Like `/q/`, this route carries the
body limit and the bounds wall itself — a peer-facing write path must not be
the one route where the documented caps do not apply.

## Deployment surfaces under /x

`/x`, and any path below it, belongs to the deployment. A surface registered
there ([extending](extending.md#api-surfaces)) mounts its own routes and
documents them itself; the broker only guarantees the ground rules. A prefix
outside `/q`, `/x` and below-`/x` is refused at startup, as is one that
overlaps a surface already mounted, so a deployment route can never shadow a
spec resource or race another surface for a path. `ANTARES_API_SURFACES`
names which surfaces are mounted, and `/q/health` reports each one's prefix
and route count under `surfaces`.

The shipped binary mounts `admin` and nothing else. The reference plugin
(`examples/plugin-example`, off by default) mounts `/x/example` and is the
worked example of both a surface and a façade.
