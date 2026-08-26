# ADR-0002: NATS JetStream as the change bus (with a local mode)

Date: 2026-08-03 · Status: accepted

One `ANTARES_CHANGES` interest stream, durable pull consumers per concern,
JetStream KV for the subscription mirror — this replaces Scorpio's whole
SUB_ALIVE/SUB_SYNC instance-sync protocol. Delivery is at-least-once +
idempotent consumers; entity `version` gives order tolerance. `bus = local`
(in-process) is a first-class single-node mode: no infrastructure beyond
Postgres. Kafka rejected: operational cost without a matching benefit at
our scale.

## Confirmation

`crates/antares-broker/tests/nats_e2e.rs sigkill_between_commit_and_publish_republishes_from_outbox` and `nats_outage_flips_health_and_recovers`; the postgres-nats and timescale-nats cells of the full ETSI matrix.
