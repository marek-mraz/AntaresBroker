-- `jsonld_contexts` joins the tenant-bearing tables (ADR-0021).
--
-- 0001_init left this one table without a `tenant_id` column and therefore
-- without Row-Level Security: a Hosted or ImplicitlyCreated @context records
-- its owning Tenant inside the stored document's "owner" member, and the
-- serve, list and delete paths were the only thing enforcing it. Those term
-- mappings decide what every payload of that Tenant means (5.5.7), so 4.14
-- makes them the Tenant's information as much as its entities are.
--
-- The column is GENERATED from what the row already carries, so no writer
-- changes and the column cannot disagree with the document it is derived
-- from. A 'Cached' row is a copy of a public document the broker downloaded
-- for whoever named its URL (5.13.1); it belongs to no Tenant, and NULL is
-- how the policy says so. Any other kind falls back to the default Tenant,
-- which is how a row written before the "owner" member existed is read.
ALTER TABLE jsonld_contexts
  ADD COLUMN tenant_id text GENERATED ALWAYS AS (
    CASE WHEN kind = 'Cached' THEN NULL
         ELSE COALESCE(body ->> 'owner', 'default') END
  ) STORED;

CREATE INDEX i_jsonld_contexts_tenant ON jsonld_contexts (tenant_id);

-- No `antares.service` escape: the escape exists for the outbox drain and the
-- 4.22 reaps, which are cross-tenant by nature. Nothing about a stored
-- @context is. Split by command for the reason 0005 records — a FOR ALL
-- policy applies USING to the existing row of an UPDATE, so a shared row
-- could change hands.
--
-- `tenant_id IS NULL` is in every clause on purpose: a Cached copy is
-- readable, writable and evictable by every Tenant, because the document is
-- public and one row per URL is the point of a cache.
ALTER TABLE jsonld_contexts ENABLE ROW LEVEL SECURITY;
ALTER TABLE jsonld_contexts FORCE ROW LEVEL SECURITY;

CREATE POLICY tenant_read ON jsonld_contexts FOR SELECT
  USING (tenant_id IS NULL
         OR tenant_id = current_setting('antares.tenant', true));
CREATE POLICY tenant_write ON jsonld_contexts FOR INSERT
  WITH CHECK (tenant_id IS NULL
              OR tenant_id = current_setting('antares.tenant', true));
CREATE POLICY tenant_update ON jsonld_contexts FOR UPDATE
  USING (tenant_id IS NULL
         OR tenant_id = current_setting('antares.tenant', true))
  WITH CHECK (tenant_id IS NULL
              OR tenant_id = current_setting('antares.tenant', true));
CREATE POLICY tenant_reap ON jsonld_contexts FOR DELETE
  USING (tenant_id IS NULL
         OR tenant_id = current_setting('antares.tenant', true));
