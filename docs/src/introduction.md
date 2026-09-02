# Antares

Antares is an NGSI-LD Context Broker written in Rust: one native binary
that stores entities, answers queries, keeps attribute history, delivers
notifications over HTTP and MQTT, and federates with other brokers
through Context Source Registrations. The same crates compile to
WebAssembly and run the whole broker inside a web page.

## What it implements

ETSI GS CIM 009 V1.9.1, the NGSI-LD API, clause by clause: the
information model (4.x), the operations (5.x), the HTTP binding (6.x)
and the MQTT notification binding (7). The [Conformance](conformance.md)
chapter carries the ledger, one file per clause, and the ETSI test suite
runs against every storage backend in CI. NGSI-LD 2.0 splits the same
material into a core specification and an HTTP binding; Antares keeps
the operation semantics and the binding in separate modules for that
reason, and the [Extending Antares](extending.md) chapter says what a
new binding attaches to.

## Three deployment shapes

| shape | store | bus | when |
|---|---|---|---|
| single binary | `memory` or `file` (redb on a volume) | in-process | development, edge devices, one node, the browser build |
| postgres | `postgres` (PostGIS) | in-process or NATS | production on one broker |
| scaled | `postgres` or `timescale` | NATS JetStream | several stateless broker pods, role split, rolling updates |

Current state and history are chosen independently: `ANTARES_STORE`
picks where entities live, `ANTARES_TEMPORAL` where their history goes,
including `none`. [Storage drivers](storage.md) has the ladder and the
measured costs.

## What the broker does not do

Authentication, rate limiting and quotas belong to the policy enforcement
point in front of the broker (an API gateway), and so does every
authorization *decision*: the broker ships no policy engine, only the seam
one attaches to (ADR-0020). The broker validates, stores, serves, notifies
and federates; it trusts the `NGSILD-Tenant` header it receives and
enforces tenant isolation in the store. The [Shared crates](shared-crates.md) chapter shows how a gateway
uses the broker's own parser, query engine and matcher to make its
decisions with identical semantics.

## Which chapter

| you want to | read |
|---|---|
| run a broker in five minutes | [Getting started](getting-started.md) |
| set every knob | [Configuration](configuration.md) |
| run it in production, back it up, roll it | [Deployment](deployment.md), [Operations](operations.md) |
| receive notifications | [Subscriptions and notifications](subscriptions.md) |
| query history | [Temporal API](temporal.md) |
| connect brokers | [Federation](federation.md) |
| run it in a browser | [Browser & WebAssembly](wasm.md) |
| see the health, metrics and admin routes | [Admin API](admin-api.md) |
| know what is conformant | [Conformance](conformance.md) |
| add a backend or a hook | [Extending Antares](extending.md) |
| compare with other brokers | [Ecosystem & positioning](ecosystem.md) |

Links: [live conformance report](https://antares-ngsi-ld-demo.marek-mraz.com/reports/latest/),
[browser playground](https://antares-ngsi-ld-demo.marek-mraz.com/),
[source](https://github.com/marek-mraz/AntaresBroker),
[ADRs](https://github.com/marek-mraz/AntaresBroker/tree/master/docs/adr).
