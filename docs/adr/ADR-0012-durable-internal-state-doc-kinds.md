# ADR-0012 — Internal broker state lives in the store as doc kinds, keyed under reserved tenants

Date: 2026-08-12. Status: accepted, implemented.

## Context

Three subsystems kept their bookkeeping in per-process `AppState` maps:
snapshot metadata (5.16 / 5.2.41 documents), EntityMap API documents
(5.14 / 5.2.39) and the distributed-subscription mappings 5.8.1.4 mandates
("the mapping … shall be stored"). The HA contract says stateless
broker pods; a per-process map is state a pod restart silently loses —
after a restart, a remote broker's notifications 404 forever, snapshots
vanish, and EntityMap references stop resolving.

## Decision

1. The store trait grows three doc kinds: `Kind::Snapshot`,
   `Kind::EntityMap`, `Kind::DistSub`. Every backend serves them through
   the existing `(tenant, kind, id) → JSON doc` surface: the in-memory
   store adds three maps (memory mode keeps today's semantics), file mode
   adds three redb tables (old files load — a missing table is skipped),
   pg/timescale add three ADR-0001-shaped tables (`snapshots`,
   `entity_map_docs`, `dist_subs`; migration 0008; RLS forced like 0001).
2. Internal state that is not owned by a client tenant is keyed under
   **reserved tenant ids inside the shared tables**: the inbound
   distributed-subscription index lives under tenant `distsub-index`
   (id = remote subscriptionId), and snapshot frozen data keeps its
   synthetic `snap-<uuid>` tenants (established with the Snapshot API).
   No HTTP route serves `Kind::DistSub` docs, so the reserved tenant is
   not readable from the API; a real tenant literally named
   `distsub-index` would share the namespace of that one unexposed kind —
   accepted, documented here, not policed.

## Consequences

- pg/timescale (and file) brokers keep snapshots, EntityMaps and
  remote-subscription bookkeeping across restarts
  (`crates/antares-api/tests/durable_state.rs`); the HA contract no
  longer has a named per-process exception.
- Reserved tenant names are now persisted data — renaming the encoding
  later means a data migration, which is why this is an ADR.
- 5.5.15 resource-pressure eviction (`evict_over_cap`) remains the
  sanctioned way to shed snapshots; durability does not repeal it.
- The Registration Subscription the consumer half of a distributed
  subscription owns (5.8.1.4) is internal state of the same class, so it is
  stored under `Kind::DistSub` in the subscriber's own tenant rather than
  under the client-visible `Kind::CSourceSubscription`. The 5.11 endpoints
  then cannot read, patch or delete broker plumbing, and 5.11.5 lists exactly
  the subscriptions a client created; `GET /q/tenants/{tenant}` counts it
  under `distSubs`.
- `tenant_exists` counts the new kinds, so a tenant that only owns an
  EntityMap or snapshot still passes the 5.5.10 gate after a restart.

## Confirmation

`crates/antares-api/tests/durable_state.rs snapshot_and_entity_map_survive_restart`, `dist_sub_mapping_survives_restart`, `dead_letter_survives_restart`.
