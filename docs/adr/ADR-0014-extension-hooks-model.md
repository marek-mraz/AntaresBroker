# ADR-0014: extension hooks — fixed phases, batch granularity

Date: 2026-08-24 · Status: accepted; the notification-sink paragraph superseded by ADR-0016

## Context

Beyond the storage drivers (ADR-0013), the broker has a small set of
lifecycle seams where deployment-specific behaviour can attach: the
change hook after commit, the temporal-event drain after the response,
the notification sink before send, and the tower layer stack around the
HTTP surface. The temptation is a generic plugin chain; the evidence
says a generic chain's cost is marshalling values across the seam per
event, not the dispatch itself — Tremor's plugin conversion cost 30-36%
throughput for exactly that reason, while a dynamic call is ~3 ns
against sub-millisecond request floors. Seam GRANULARITY decides cost.

## Decision

Extension points are a FIXED, named set of phases — extensions never
define phases, and each phase has one declared position in the request
lifecycle:

| phase | fires |
|---|---|
| `on_request` | after parse+validate, before the operation |
| `on_change` | after commit, with the before/after diff |
| `temporal_event` | in the post-response drain, with the request's events |
| `pre_notify` | notification built, before send |
| `on_response` | render/annotate, as a tower layer |

Rules, load-bearing:

1. **Hooks fire per request or per drained batch, never per attribute or
   per matched subscription.** A hook needing per-item work takes the
   batch and iterates inside its own boundary — the marshalling cost is
   paid once per request, not once per item.
2. **Failure policy is declared per hook**: fail-open for observers
   (metrics, audit — a broken observer loses its own data), fail-closed
   for gates (a broken gate refuses, never waves through). A broken
   extension degrades its own concern, never the broker.
3. **Configuration is data, code is compile-time.** Which hooks are
   active, and their per-scope settings, may be reloaded at runtime from
   the stores the broker already has (Postgres, NATS KV); extension CODE
   stays a cargo feature until a dynamic tier is actually demanded
   (ADR-0013's deferred loader).
4. **HTTP-level middleware is the tower `Layer` stack** — Rust already
   has the phase chain, and gateway concerns (authn, authz, rate
   limiting, request transforms) stay in the gateway in front of the
   broker, outside the conformance surface.

Not adopted: a generic ordered plugin chain around the typed NGSI-LD hot
path (conformance would become a function of deployment configuration),
and in-process scripting for trusted logic (a cargo feature costs zero
at runtime and stays type-checked).

## Confirmation

The phase table above must stay in sync with the seams in code:
`ChangeHook` (antares-store), the temporal drain, the notification sink
boundary, and the axum/tower router. A new extension point = a new row
here first, with its granularity and failure policy named.
