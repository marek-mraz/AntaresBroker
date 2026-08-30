# ADR-0016 — Notification bindings behind the sink registry

Date: 2026-08-30. Status: accepted, implemented.
Supersedes the notification-sink paragraph of ADR-0014.

## Context

`NotificationSink` declared one method, `schemes()`, and `SinkRegistry` was
never constructed outside its own unit test. Delivery did not use either.
`notify.rs` branched on the endpoint URI twice — once to detect the mqtt(s)
schemes, once to build a two-variant `Outbound` enum — `AppState` carried an
`MqttSink` field behind `#[cfg(feature = "mqtt")]`, and the set of
deliverable schemes was written out a third time in `subscriptions.rs` as a
hardcoded array. Adding a binding meant editing antares-api in four places,
and clause 7 leaked into the crate that serves clause 6.

The extending chapter described the registry as the seam. It was not one.

## Decision

1. `NotificationSink` is the whole binding contract:

   - `schemes()` — the URI schemes it serves.
   - `parse_endpoint(uri, notifier_info)` — 5.8.1.4 validation at
     subscription creation, so a malformed endpoint never reaches delivery.
     6.3.8 fixes the scheme set for the HTTP binding, 7.2 for MQTT's.
   - `deliver(uri, out, timeout)` — one attempt on the wire.
   - `network()` — whether an endpoint of this binding names a network
     destination. Defaults to `true`.

2. `SinkRegistry` is the only way a binding is chosen, at creation and at
   delivery, keyed by the lowercased scheme (IETF RFC 3986 §3.1). An
   endpoint whose scheme no sink serves is refused at creation and, for a
   row that was edited around the API, dropped at delivery. There is no
   fall-through to the HTTP binding.

3. That refusal is `BadRequestData` (400), not `OperationNotSupported`
   (422). 5.5.2 leaves the condition for each error type to the operation
   clause; 5.8.1.4 names `BadRequestData` for input that does not meet the
   5.2.12 restrictions, and Table 5.5.2-1 reads `BadRequestData` as "input
   data which does not meet the requirements of the operation" against
   `OperationNotSupported` as "the operation is not supported". Create
   Subscription is supported; the endpoint value is what fails.

4. `Outbound` is one transport-neutral struct — the Notification, its
   `accept`, the `@context` Link value, `receiverInfo`, `notifierInfo` —
   and each sink renders it: HTTP headers per 6.3.8, an MQTT `metadata`
   object per Table 7.2-2. Retries and dead-letter replays render from the
   same parts, so they reproduce the request. Dead letters written before
   this change carry rendered headers or an already-wrapped clause 7
   message; both read back, so an upgrade strands nothing.

5. The egress guard — private-range and metadata-address deny,
   per-destination circuit breaker — runs in the caller before `deliver` and
   never inside a sink, so one guard covers every binding including one
   registered from outside this workspace. It is skipped only for a sink
   that declares `network() == false`, which opens no socket; a unit test
   holds that every binding shipped here is policed. The scheme allowlist
   splits off it: `Egress::check_url` keeps http/https for `@context`
   fetches and federation forwards, while `check_destination` polices the
   host and port of a notification endpoint whatever scheme its sink serves.
   A URI with no host names no destination and is refused.

6. `AppState.mqtt` is gone; `AppState.sinks` holds the registry and
   `with_sink` adds a binding. antares-api names no transport.

Egress key ordering moved to `antares_model::ordered_vec`, since both
bindings and every API response now serialize through it.

## Consequences

A binding is a crate that implements one trait and one registration. The
browser build's page sink became what it always was — how the HTTP binding
delivers when the runtime has no inbound socket — and lives with that
binding. A WebSocket binding (ADR-0003 defers it) needs no change here.

An endpoint scheme this deployment cannot deliver to now answers 400 where
it answered 422. No ETSI test purpose covers that response, and the MQTT
binding already answered 400 for a malformed endpoint URI, so the change
makes one surface consistent rather than splitting it.

## Confirmation

`crates/antares-api/tests/plugin_sink_5_2_15.rs` registers a `memory://`
sink defined in the test crate and asserts the subscription is accepted,
the notification arrives at that sink, an unserved scheme is 400 at
creation, and delivery never falls through to HTTP. In `antares-api` the
only mention of a transport left is the composition root's optional
`MqttSink` registration in `state.rs`; `notify.rs` names none.
