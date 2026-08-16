# ADR-0001: Shared-schema multi-tenancy (tenant_id + RLS)

Date: 2026-08-03 · Status: accepted

One schema; every table carries `tenant_id` (leading PK column); Postgres
Row-Level Security as the enforced backstop (`SET LOCAL antares.tenant`).
NOT database-per-tenant (Scorpio's model: 1,000 DBs, pool explosion,
CREATE-DATABASE races — Scorpio issues #653/#663).

Consequence: tenant create = INSERT, delete = DELETE; one connection pool;
GDPR-grade physical isolation is explicitly out of scope.
