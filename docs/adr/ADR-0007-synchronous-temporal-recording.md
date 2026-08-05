# ADR-0007: temporal auto-recording stays in the write path (F8 reversed)

Date: 2026-08-05 · Status: accepted (supersedes the F8 design in tasks.md §F)

tasks.md F8 moved temporal auto-recording off the request path onto a durable
JetStream consumer (`temporal_writer`, `antares-temporal::recorder`), mirroring
Scorpio's ENTITY-topic recorder. Running the K8 drill (the full ETSI suite
through the LB while brokers roll) surfaced what the design costs.

Two defects, both structural rather than fixable in the consumer:

1. **No read-your-writes.** The ETSI suite reads an entity's temporal
   evolution immediately after writing it, and a great many real clients do
   the same. With recording behind the bus, the history a client just created
   may not exist yet when it looks — a race the client cannot see or wait on.
2. **Late-replay resurrection.** At-least-once delivery means a pre-delete
   change event can arrive *after* a direct `DELETE /temporal/entities/{id}`.
   The consumer re-applies it and the deleted history comes back. Ordering
   tolerance (§3.1) is right for the matcher, which projects no state, but the
   recorder writes state, and there is no fence that closes this window
   without the recorder becoming ordering-dependent — the thing the bus design
   explicitly refuses to be.

The consumer also bought nothing: the store's dual-write already lands
`attr_instances` rows inside the entity write transaction (C9), so the
consumer was a **second** application of the same history, deduplicated only
by the deterministic-instanceId scheme. Work duplicated, races added.

Decision: **auto-recording is synchronous in the write path in every bus
mode.** Every write already goes through an api-role pod holding the shared
store, so recording in-request is a same-transaction write, not an extra hop.

Consequences:

- The `temporal_writer` durable is **deleted**. A durable name is a
  consumer-group contract (§9.1), so this is a breaking change for any
  deployment that ran the F8 build: drop the durable, or JetStream keeps
  accumulating unacked messages for a consumer that no longer exists.
- The `antares-temporal` crate is deleted — the recorder was its last
  resident. Plain-mode partition/retention maintenance lives in
  `antares_sql::maintenance`, invoked from the broker's `temporal` role, so
  the role and the `--roles` vocabulary are unchanged.
- Write latency now includes the temporal decomposition. It is inside the
  transaction the write already holds, so it costs no extra round trip;
  should it ever show up in a benchmark, the lever is batching the
  decomposition, not moving it back off the request path.
- The bus keeps exactly one balanced durable (`matcher`). Fewer moving parts
  in the topology assertion (F7).
- `ANTARES_OUTBOX_DRAIN=off` was added alongside: the drain nudge made the
  commit→publish window ~1 ms wide, so the K9 crash drill can no longer race
  it from outside. The knob makes the crash state deterministic (rows commit,
  this pod never publishes, another pod's drain recovers them) and doubles as
  the dedicated-drainer split if one is ever wanted.
