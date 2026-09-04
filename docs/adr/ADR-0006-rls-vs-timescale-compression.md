# ADR-0006: RLS and Timescale compression collide on attr_instances

Date: 2026-08-04 · Status: accepted

Discovered while landing the attr_instances decomposition: TimescaleDB
refuses to enable columnstore compression on a table with row security
("columnstore cannot be used on table with row security"), and conversely
refuses RLS/index ALTERs once compression is enabled. Two standing rules —
RLS on every tenant-scoped table, and a 16 GB Postgres budget that leans on
~90 % temporal compression — cannot both hold on this one table.

Decision: **compression wins on `attr_instances` in timescale mode.** That
table keeps only the explicit-predicate isolation — every store query already
carries `tenant_id = $1`, enforced structurally by `&TenantId` being the
first parameter of every store method. Plain mode keeps the full
RLS belt on attr_instances; every other table keeps RLS in both modes.

Consequences:
- The "even tenant-less SQL returns zero foreign rows" guarantee has
  one named exception: attr_instances under timescale. The isolation test
  pack asserts the explicit-predicate discipline for this table instead:
  `tests/attr_instances_tenant_predicate.rs` reads every SQL literal in the
  Postgres store that selects, inserts into, updates or deletes from the
  table, and requires each to compare `tenant_id` or to appear in a list of
  cross-tenant statements with the reason it runs under the service role.
  It needs no database, so it gates every cell and not only the timescale
  one — the discipline it guards is what a statement written in plain mode
  carries into timescale.
- Migration 0003 orders DDL as: table → hypertable/partitions → indexes →
  (RLS | compression), because Timescale also rejects post-compression
  ALTERs — compression must be the LAST thing that touches the table.
- If Timescale ever lifts the restriction, adding the policy back is a
  single forward migration.

## Confirmation

`crates/antares-sql/tests/pg_rls_pentest.rs rls_tables` names the RLS table set with `attr_instances` handled by its own rule; the timescale cell of the full ETSI matrix runs with compression enabled.
