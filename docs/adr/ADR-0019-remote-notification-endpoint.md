# ADR-0019 — The distributed-subscription notification receiver lives outside the ETSI namespace

Date: 2026-09-01. Status: accepted, implemented.

## Context

The consumer half of distributed subscriptions (5.8.1.4) forwards a reduced
copy of a Subscription to every matching Context Source "where the
notification endpoint is set to that of the local Broker". The source then
POSTs its notifications to that endpoint, and the broker remaps them onto
the original subscriptionId and delivers them to the real subscriber.

CIM 009 defines no path for this. A notification endpoint is any URI
(5.2.15 EndPoint), and clause 6.2 standardizes only what hangs under the
API root `{apiRoot}/ngsi-ld/v1`: "All resource URIs are defined relative to
the above root URI". In the spec's model the receiver of a notification is
a client application, never a server resource, so there is nothing to
inherit and every implementation invents its own URL.

What the other implementations do, read from their sources:

| Broker | Endpoint | Routing key | Mapping lifetime |
|---|---|---|---|
| Orion-LD | `POST /ngsi-ld/ex/v1/notifications/{parentSubId}` | the subscriber's own local Subscription id, in the URL | subscription cache |
| coraine | `POST /ngsi-ld/ex/v1/notifications/{parentSubId}` | the subscriber's own local Subscription id, in the URL | subscription cache |
| Scorpio | `POST /remotenotify/{id}` | a generated callback id, in the URL | in-process map |

Orion-LD serves its whole non-standard surface under `/ngsi-ld/ex/v1/`
(`ping`, `version`, `tenants`, `dbIndexes`, `metrics`, `notify`,
`notifications/*`), and coraine inherits that layout. Antares first followed
the same convention with `/ngsi-ld/ex/remote-notify`.

Three properties of those designs are worth naming, because this broker
takes a different position on each:

- The path prefix `/ngsi-ld/` belongs to ETSI. `ex` is a convention one
  implementation introduced, not a reservation, and a later version of
  CIM 009 is free to define resources anywhere under that prefix.
- A routing key in the URL is written to every access log, proxy log and
  trace along the path, and it is stored by the Context Source as part of
  the subscription it holds. The key is the whole credential for injecting
  a notification into a subscriber, so it belongs in the body, where the
  same infrastructure does not record it.
- Routing on the subscriber's own Subscription id means the id travels to
  every Context Source the copy was forwarded to, and that a caller who
  knows a client-chosen id can post notifications to that subscriber.

## Decision

1. The receiver is `POST /ex/v1/remote-notify`, outside the `/ngsi-ld`
   prefix ETSI owns. `/ex` joins the two roots this broker already reserves
   beside the API: `/q` is its operational ground and `/x` is the
   deployment's (`surface.rs`). It is neither, so it is a core route of its
   own, and `check_prefix` refuses any surface prefix outside `/q` and `/x`,
   so no deployment surface can shadow it. `v1` versions the wire contract
   between brokers independently of the NGSI-LD API version.
2. The routing key stays in the body, as the notification's own
   `subscriptionId` member. It is a broker-generated UUID, never the
   subscriber's Subscription id, so a Context Source learns nothing about
   the subscriber and cannot address any other subscription.
3. The key resolves through the stored mapping (`Kind::DistSub`,
   ADR-0012) to both the tenant and the own Subscription id. The request's
   `NGSILD-Tenant` header is not read on this route at all: the mapping is
   the only thing that decides which tenant's subscriber a notification
   reaches.
4. No alias is kept for the previous `/ngsi-ld/ex/remote-notify`. A Context
   Source holds the callback URL inside the subscription copy it was given,
   so a distributed subscription created by an earlier broker stops
   receiving notifications after the upgrade: delete it and create it again.
   Keeping the old route would keep the ETSI prefix this decision is about.

## Consequences

- The endpoint is peer-facing and must be reachable by every Context Source
  a distributed subscription forwards to: it is the one non-standard route
  that cannot be firewalled off with `/q/`. `ANTARES_PUBLIC_URL` is what
  the forwarded copy advertises.
- Both routes are outside the API nest, so each carries the body limit and
  the bounds wall itself; a peer-facing write path must not be the one
  route where the documented caps do not apply.
- A conformance TP cannot hard-code the receiver path of the broker under
  test. It reads `notification.endpoint.uri` out of the copy the Context
  Source received, which is that broker's own callback URL whatever it is
  (`ngsi-ld-test-suite/TP/NGSI-LD/DistributedOperations/Subscription/5814_01.robot`).
- The path is a wire contract between brokers, so a later change to it is
  the same breaking change: it is versioned for that reason, and a second
  version would be served beside the first rather than in place of it.

## Confirmation

`crates/antares-api/tests/dist_subs_5_8.rs` (the consumer half end to end),
`crates/antares-api/src/distsub.rs` `clause_5_8_1_reduced_copy_carries_only_the_registration_scope`.
