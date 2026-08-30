# Admin API

Everything under `/q/` is the operator surface: it sits outside
`/ngsi-ld/v1`, carries no NGSI-LD semantics and belongs behind the
gateway. A worker pod (`ANTARES_ROLES` without `api`) serves only these
routes. Errors use the same problem-details shape as the NGSI-LD API.

| Route | Purpose |
|---|---|
| `GET /q/health` | Liveness and the process view |
| `GET /q/ready` | Readiness for the load balancer |
| `GET /q/metrics` | Prometheus text |
| `GET /q/tenants` | Tenant inventory |
| `DELETE /q/tenants/{tenant}` | Tenant purge |
| `GET /q/dead-letters` | Notifications the delivery policy gave up on |
| `POST /q/dead-letters/{id}/replay` | One more delivery attempt |
| `DELETE /q/dead-letters/{id}` | Drop a dead letter |

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
  "temporalDrainErrors": 0,
  "limits": {
    "maxBatchItems": 1000,
    "maxBodyBytes": 4194304,
    "maxContextFetches": 32,
    "maxFedFanout": 8,
    "maxFedResponseBytes": 16777216,
    "maxGeoVertices": 1024,
    "maxJoinLevel": 10,
    "maxJsonDepth": 64,
    "maxQLinkLookups": 512,
    "maxQNodes": 512,
    "maxRegexCache": 1024,
    "maxRegexProgramBytes": 32768,
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
| `version`, `commit` | Workspace version and the git hash the binary was built from. |
| `notificationSchemes` | The `notification.endpoint.uri` schemes this build can deliver to — the registered bindings (6.3.8, clause 7, and any a deployment added). A subscription naming a scheme absent here is refused at creation with `BadRequestData`. |
| `deadLetters` | Dead letters this process wrote since start ([notification delivery](operations.md#notification-delivery)). |
| `temporalDrainErrors` | Post-response history writes that failed since start; the client's 2xx stands, the counter and a warning record the loss. |
| `limits` | The bounds wall: every `max*` cap in force and the `rejected*` counters of requests refused by it. |
| `memory` | jemalloc live (`allocatedBytes`) and resident (`residentBytes`) bytes. |
| `commitQueueDepth`, `commitQueuePeak` | `file` store only: writers queued behind the single redb committer, now and at peak. |
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

## Tenants

```text
GET /q/tenants
[{"tenant":"default","counts":{"entities":0,"subscriptions":0,"csourceSubscriptions":0,
  "registrations":0,"snapshots":0,"entityMaps":0,"distSubs":0,"attrInstances":0}}]
```

`createdAt` is present on Postgres, where the `tenants` table records it.
`DELETE /q/tenants/{tenant}` purges the tenant: `204` when done, `404` for
an unknown tenant (`{"title":"ResourceNotFound","detail":"tenant nope"}`),
`409` while it still holds distributed subscriptions, `400` for a name
outside the tenant grammar. The default tenant is emptied and keeps
existing. The path names the tenant; an `NGSILD-Tenant` header is ignored.
Background in [operations](operations.md#tenants).

## Dead letters

| Call | Effect |
|---|---|
| `GET /q/dead-letters?tenant=&subscription=&limit=` | Letters of one tenant (the default tenant when `tenant` is absent), newest first, `limit` 100 by default; `400` for a `limit` that is not a positive integer or a tenant outside the grammar. Endpoint userinfo is redacted. |
| `POST /q/dead-letters/{id}/replay?tenant=` | One attempt through the same binding under the egress policy of the moment: `204` and the letter is gone, `502` with the failure text and the letter kept, `404` when the tenant holds no such letter. |
| `DELETE /q/dead-letters/{id}?tenant=` | `204`, or `404` when the tenant holds no such letter. |

A letter carries the subscription id, tenant, endpoint, headers, body,
attempt count, first and last error and timestamps. What produces one and
why an egress refusal never does: [notification delivery](operations.md#notification-delivery).
