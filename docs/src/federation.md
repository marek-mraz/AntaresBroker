# Federation

Antares implements the CIM 009 distributed-operations model (4.3.6,
5.9–5.11, 6.3.17, 6.3.18). Context Source Registrations (CSRs) are the
routing table: a broker holding CSRs forwards matching requests to the
registered sources, merges the answers and reports per-source problems
without failing the request. The DistributedOperations and IOP suites
(134 + 286 tests) cover this surface in every CI cell.

Every example below uses two brokers, `broker-a` on 9090 and `broker-b`
on 9091, as in `examples/federation/compose.yml`:

```bash
cd examples/federation && docker compose up -d && ./run.sh
```

`run.sh` creates an entity on B, registers B at A and queries A:

```text
entity created on B
CSR registered on A -> B
federated query via A:
[{"id":"urn:ngsi-ld:ParkingSpot:fed:042","type":"ParkingSpot","status":{"type":"Property","value":"free"}}]
OK: B's entity served by A
```

With two local binaries instead of compose, set `ANTARES_HOST_ALIAS`
per process and run `B_FROM_A=http://localhost:9091 ./run.sh`.

## Registrations

A CSR declares what a source holds (`information[]`: entity types, ids,
`idPattern`, `propertyNames`, `relationshipNames`) and how to treat it
(`mode`):

| mode | meaning | reads | writes |
|---|---|---|---|
| `inclusive` (default) | one of possibly many holders | forwarded and merged (4.5.5) | forwarded, 207 on partial failure |
| `exclusive` | the only holder of the registered entity and attributes | forwarded | forwarded |
| `redirect` | the broker proxies, keeps nothing locally | forwarded | forwarded |
| `auxiliary` | consulted only when nobody else answers | forwarded last | never |

An `exclusive` registration names one entity id and its attributes
(4.3.6.3); a type-only or pattern registration is refused:

```text
{"detail":"an exclusive registration shall name an entity id — an id pattern or Entity type defining a group of entities is not supported (4.3.6.3)","status":400,"title":"BadRequestData", ...}
```

`operations` bounds what may be forwarded (default `federationOps`);
`contextSourceInfo` key/value pairs travel as headers on every forward
to that source (4.3.6.5); `localOnly` adds `local=true` to every
forward (4.3.6.4). `observationInterval` and `managementInterval` gate
temporal forwards to the sources whose window overlaps the query.

A `contextSourceInfo` value of `urn:ngsi-ld:request` is not sent as that
string: it copies the same-named header off the request that triggered the
forward, and sends nothing when the triggering request carried no such
header (4.3.6.5, 6.3.19). `{"key": "Authorization", "value":
"urn:ngsi-ld:request"}` is how a source behind the same identity provider is
given the caller's credential, and it is the reason creating a registration
is a privileged operation: the key names any header and the endpoint is
whatever the registration says. Two groups of keys never reach the source as
a raw header. Headers the registration's other members already decide take
precedence and cannot be overridden (6.3.19), which is `NGSILD-Tenant`, and
so do the ones the binding sets itself: `Host`, `Via`, `Link`, `Connection`,
`Content-Type`, `Content-Length`. The `accept`, `contentType`,
`jsonldContext` and `ngsildConformance` keys are the ones 4.3.6.6 and
4.3.6.8 give their own meaning; the forward acts on them, and passing them
through raw would corrupt the negotiation they steer.

### The registered `@context`

`"jsonldContext"` names a `@context` the Context Source reads its terms in,
and the forward is recompacted into it: the payload, the term-bearing query
parameters (`attrs`, `type`, `geoproperty`) and — for the resources that
name one Attribute in the path, `/entities/{id}/attrs/{name}` and its
temporal and `value` variants — the path segment itself. The segment is not
one of the two things 4.3.6.6 lists, but it carries a term the Context
Source expands with the `@context` the forward advertises, so a request that
switched the context and left the segment alone would write to a different
Attribute.

The whole request travels in one `@context` or none of it does. A payload
the broker cannot express in the registered context — a batch array, a body
that does not re-expand — makes the forward fall back to the request's own
context, logged as a warning, because advertising the registered context
over terms that are not in it is how a peer writes Attributes nobody named.

### Timeout and cooldown

`management.timeout` (5.2.34) bounds one forward in milliseconds; the
broker caps it at 8 seconds, and a registration without it gets the cap.
`management.cooldown` keeps a source that failed out of the fan-out for
that many milliseconds; inside the window the forward is answered as a
timeout without contacting the source. A registration with
`"management": {"timeout": 500, "cooldown": 10000}` pointing at a closed
port answers in 7 ms:

```text
HTTP/1.1 200 OK
Ngsild-Warning: 199 broker-b "no response was received from the registration endpoint within the timeout period"
```

### Forward history

Every forward that reaches the wire is booked on the registration it was
made for, in the five read-only members of Table 5.2.9-2:

```json
{
  "id": "urn:ngsi-ld:ContextSourceRegistration:weather",
  "type": "ContextSourceRegistration",
  "endpoint": "http://source-b:8080/ngsi-ld/v1",
  "information": [{"entities": [{"type": "WeatherObserved"}]}],
  "timesSent": 412,
  "timesFailed": 3,
  "lastSuccess": "2026-05-05T11:02:44.118Z",
  "lastFailure": "2026-05-04T22:17:09.640Z",
  "status": "ok"
}
```

`timesSent` counts every attempt, failures included; `timesFailed` counts
the ones the table calls failures, which in the HTTP binding is any response
code other than 2xx, a timeout and a refused connection. `status` names the
last attempt alone, so a source that has recovered reads `"ok"` however
large `timesFailed` is. A member appears when it first has something to say:
a registration that has never failed carries no `timesFailed` and no
`lastFailure`.

Three outcomes are deliberately not counted, because the operation never
left this broker: a destination the egress policy refuses, a source the
circuit breaker is holding open, and a registration inside its own
`management.cooldown` window. The breaker only opens after failures that
were attempted and booked, so a source that has gone away reads `"failed"`
before the first forward is suppressed.

They are read-only. A create or update that carries any of them has that
member dropped, not refused, which is what 5.2.9 asks for.

### Same source, several registrations

Registrations naming the same source (same endpoint, mode, tenant,
`contextSourceAlias`, `contextSourceInfo` and `localOnly`) fold into one
forwarded request whose attribute and entity scopes are the union. A
different `contextSourceAlias` behind the same endpoint is a different
source (5.2.9) and is contacted separately.

## Distributed reads

A read that matches CSRs fans out concurrently, `ANTARES_FED_FANOUT` at
a time (default 8), each forward bounded by the registration timeout and
`ANTARES_MAX_FED_RESPONSE_BYTES`. The forwarded request is narrowed to
what the registration declares (4.3.6.1): a registration with
`propertyNames: ["status"]` receives `attrs=status` even when the client
asked for `attrs=status,owner`, and any `owner` the source returns anyway
is dropped from the merge. The forward carries the `Via` hop and the
client's `Link` context, never the client's `NGSILD-Tenant` (4.14); the
registration's own `tenant` member names the tenant to address at the
source:

```text
GET /ngsi-ld/v1/entities?options=sysAttrs&type=ParkingSpot&attrs=status
Accept: application/json
Via: 1.1 broker-a
Link: <https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json"
```

Entity halves from several sources merge per 4.5.5 before pagination.

### Partial failures

A source that fails does not fail the request. Each problem becomes one
`NGSILD-Warning` header (6.3.17, Table 6.3.17-1) naming the broker that
saw it:

| code | when |
|---|---|
| 199 | no response within the timeout, or the source is in cooldown |
| 299 | the source answered an error status other than 404 |
| 111 | the source answered 2xx with a payload that is not NGSI-LD |

A 404 from a source is a miss, not a warning. Warnings a peer returns
travel back to the client next to the broker's own, up to eight per
source (`maxPeerWarnings` in `/q/health`): the list is written by the
source but sent by this broker, and eight carries a real cascade without
letting one source outgrow the fan-out or bury what the broker has to
say about the others. A source that sends more has the rest dropped and
a line written to the log. Beyond the registration cooldown, a per-host
breaker pauses an endpoint whose forwards keep timing out; a source that
answers, even with an error, is never paused.

## Distributed writes

Writes forward by registration mode and the registered `operations`.
An `exclusive` or `redirect` registration whose `operations` exclude the
write is an error of type Conflict (409), because the data lives only
there; an `inclusive` one that excludes it is skipped. Batch operations
return per-entity success and error arrays with the remote results
folded in; a partial failure across inclusive sources is 207.

## Loop protection

Every forward carries a `Via` hop (6.3.18) whose pseudonym is this
broker's alias for the tenant (ADR-0011): `ANTARES_HOST_ALIAS` for the
default tenant, `{alias}~{tenant}` otherwise. `/info/sourceIdentity`
answers the same value per tenant, and that is what a peer stores as the
registration's `contextSourceAlias`:

```text
GET /ngsi-ld/v1/info/sourceIdentity
{"id":"urn:ngsi-ld:ContextSourceIdentity:broker-a","type":"ContextSourceIdentity","contextSourceAlias":"broker-a", ...}

GET /ngsi-ld/v1/info/sourceIdentity   NGSILD-Tenant: odpady
{"id":"urn:ngsi-ld:ContextSourceIdentity:broker-a~odpady","type":"ContextSourceIdentity","contextSourceAlias":"broker-a~odpady", ...}
```

Two rules follow from the inbound chain:

- A registration whose `contextSourceAlias` already appears in the chain
  is not a matching registration (Table 6.3.18-2); the request has been
  there.
- A request whose chain names this broker itself runs locally without
  re-forwarding. When the only matching registration for a write is a
  single `exclusive` or `redirect` source, the loop closes on data that
  lives nowhere else and the answer is 508 (6.3.17):

```text
POST /ngsi-ld/v1/entities/urn:ngsi-ld:Loop:1/attrs   Via: 1.1 broker-b
HTTP/1.1 508 Loop Detected
{"detail":"the Via chain already contains this broker","status":508,"title":"Loop Detected", ...}
```

A chain longer than 32 hops is treated as a loop whatever it names.
Replicas of one logical broker behind a load balancer share one alias on
purpose; they are one hop. Changing an alias breaks every peer's loop
detection, so treat it as a published identifier.

## Context source subscriptions

`POST /csourceSubscriptions` (5.11) watches the registrations instead of
the entities. A subscription on `ParkingSpot` receives the matching
registrations at creation and each later change with a `triggerReason`:

```json
{"id": "urn:ngsi-ld:ContextSourceNotification:bafc4692-…", "type": "ContextSourceNotification",
 "subscriptionId": "urn:ngsi-ld:Subscription:csr-watch", "notifiedAt": "2026-08-26T16:02:43.945Z",
 "triggerReason": "newlyMatching",
 "data": [{"id": "urn:ngsi-ld:ContextSourceRegistration:broker-c", "type": "ContextSourceRegistration",
           "endpoint": "http://localhost:9092", "information": [{"entities": [{"type": ["ParkingSpot"]}]}]}]}
```

`csf` filters registrations by their Context Source Properties (4.9).

## Distributed subscriptions

An entity subscription whose scope matches CSRs is reduced per source and
created at the remote broker (5.8); the remote notifies back to
`POST {ANTARES_PUBLIC_URL}/ex/v1/remote-notify`, so set that variable
whenever the default `http://{host_alias}:{port}` is not routable from
peers. That endpoint is the one non-standard route a federated deployment
must leave reachable from its context sources: CIM 009 defines no path for
it (5.8.1.4 says only that the copy carries "the notification endpoint of
the local Broker", and 5.2.15 makes a notification endpoint any URI), so it
lives outside the `/ngsi-ld` prefix ETSI owns rather than inside it, and it
is versioned on its own (ADR-0019). Other brokers place it elsewhere:
Orion-LD and coraine serve `POST /ngsi-ld/ex/v1/notifications/{subId}`,
Scorpio `POST /remotenotify/{id}`.

What arrives there is routed by the mapping alone. The forwarded copy
carries a broker-generated subscription id, never the subscriber's own, so
a context source learns nothing about the subscriber and cannot address any
other subscription; the tenant comes from the stored mapping and not from
the request, and the notified entities are re-filtered against the local
subscription's own selector before delivery. Reduced copies
follow the local subscription's lifecycle (update, delete); the
registration's `csf` gates which sources take part. Inbound
notifications from peers are matched against local subscriptions like
local changes.

## Pagination without amplification

The first distributed query can build an EntityMap (5.14): entity id to
contributing registrations. Later pages contact only the sources that
hold the page's entities instead of re-broadcasting the query. Maps
expire (`expiresAt`, one hour by default) and are honoured on retrieve
and temporal paths.

A federated map is merged from the maps the Context Sources return, so
what a source sends is held to Table 5.2.39-2 before it becomes part of
a document this broker stores under its own id and serves. A key that is
not an Entity id is dropped, per key rather than per source, and so is
the `@none` a source uses for what it holds locally — that marker is
about the source, and this broker has no Entity id to record it under.
A source's own map id reaches `linkedMaps` only if it is a valid URI,
because it travels back out as the `NGSILD-EntityMap` header of every
later forwarded page; a source without a usable one simply re-runs its
query when the page arrives. One source contributes at most as many
entries as the broker's own page ceiling (5.5.9, 1 000 by default),
which is the ceiling the local half of the map already carries.

## Tenancy across the federation

The client's `NGSILD-Tenant` never propagates to forwards (4.14); a CSR
addresses a specific tenant of a remote source through its `tenant`
member, and the `~`-suffixed alias keeps each (source, tenant) pair
distinct in loop detection. A registration pointing back at the same
broker for another tenant is a legitimate federation shape, not a loop.

## The five-broker stack

The IOP worked example, five brokers and no Docker:

```bash
dev/run-five.sh    # ports 9090..9094, aliases antares1..antares5
```

Each broker gets `ANTARES_PUBLIC_URL=http://localhost:PORT`; this is the
stack the 286-test IOP tree runs against in CI.
