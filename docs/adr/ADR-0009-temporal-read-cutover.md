# ADR-0009 — Temporal read cutover: attr_instances becomes the read path

Date: 2026-08-08. Status: accepted, implemented.

## Context

Since C9/D landed, `attr_instances` carried the full §8.2 machinery
(hypertable + compression in timescale mode, native range partitions +
broker-run maintenance in plain mode) — but every temporal read (5.7.3,
5.7.4, batch query) was served from `temporal_entities.doc`, the migration-
0002 "cutover bridge" JSONB column. The 2026-08-08 store audit named the
consequences: compression and retention acted on a table nothing queried;
the authoritative doc kept 100 % of history forever; every temporal write
did a DELETE + full re-INSERT of the entity's entire instance history
(O(history) write amplification that also fought columnstore compression);
and temporal queries had no LIMIT — every query materialized the tenant's
whole history into broker RAM.

## Decision

1. **Rows are the truth.** Migration 0006 replaces `temporal_entities.doc`
   with a small `meta` jsonb (id/type/scope/@context/createdAt/modifiedAt/
   deletedAt/expiresAt, verbatim strings). Instances live ONLY as
   `attr_instances` rows — which the old write-side sync had kept complete,
   so no instance backfill was needed.
2. **Reads reconstruct.** `get`/`get_range`/`query`/`list` build the
   doc shape the API consumes (`meta || jsonb_object_agg(attr, instances)`),
   with the 4.11 range and the lastN RANK() cap applied over rows — the
   predicates run on the instance JSON with `COLLATE "C"`, byte-exact
   against the in-memory window, never on the partition column. The API
   layer (window(), aggregation, presentation, 206/Content-Range) is
   untouched and remains the arbiter.
3. **Writes are deltas.** `mutate` reconstructs under the meta row lock,
   runs the closure, then diffs rows by (attr, instanceId): only added,
   changed, moved or removed instances touch the table. The auto-recording
   hot path (`mirror_record`, every entity write) uses a dedicated
   `append` seam — a pure multi-row INSERT, no history read at all.
4. **Entity paging pushes down.** `TemporalFilter` gained `page`; when the
   caller has no store-invisible filters (idPattern/q/geo), SQL applies the
   evaluator's entity-qualification rule (≥1 instance, in-window when
   ranged) plus LIMIT/OFFSET and returns the pre-LIMIT total.
5. **Retention is real.** drop_chunks / partition DROP (and the DEFAULT-
   partition DELETE for historic backfill) now shorten what queries return.

## Consequences

- Instance-level ops (5.6.14/5.6.15) still read the entity's history once
  per call (reconstruct-then-diff). Acceptable: they are rare, and the read
  replaced an O(history) *write*. A per-op SQL path is the next lever if a
  benchmark demands it.
- Instance arrays are ordered `(created_at, observed_at, instance_id)` —
  deterministic, but not byte-identical to the memory arm's append order
  for intra-batch ties. The suite validates per-mode; representations that
  window/sort are unaffected.
- The memory/file arm keeps the v0 one-doc-per-entity model (its documented
  dev/ETSI role, ~10k-entity ceiling).
