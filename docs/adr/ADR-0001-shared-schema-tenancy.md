# ADR-0001: Shared-schema multi-tenancy (tenant_id + RLS)

Date: 2026-08-03 · Status: accepted, amended (tenant inventory)

One schema; every table carries `tenant_id` (leading PK column); Postgres
Row-Level Security as the enforced backstop (`SET LOCAL antares.tenant`).
NOT database-per-tenant (Scorpio's model: 1,000 DBs, pool explosion,
CREATE-DATABASE races — Scorpio issues #653/#663).

Consequence: tenant create = INSERT, delete = DELETE; one connection pool;
GDPR-grade physical isolation is explicitly out of scope.

Amendment: the `tenants` table is the inventory. Every implicit tenant
creation inserts its row in the same transaction as the document; purge
(`DELETE /q/tenants/{tenant}`) deletes the tenant's rows from every
tenant-bearing table and then the tenant row, in one transaction, and the
temporal tables in a second one on the temporal backend. The default
tenant's row is never deleted.

Confirmation: `antares-sql/tests/pg.rs purge_tenant_empties_every_tenant_table`
(loops over every tenant-bearing table) and
`antares-api/tests/tenants_admin.rs`.

## Confirmation

`crates/antares-sql/tests/pg.rs rls_denies_cross_tenant_reads_and_writes` and `pg_rls_pentest.rs` (every tenant-scoped table under RLS); `crates/antares-api/tests/tenant_isolation.rs`.
