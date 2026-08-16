-- Service-path RLS escape: the outbox drain and the
-- temporal retention job are cross-tenant BY NATURE (one drain serves every
-- tenant's events; retention reclaims every tenant's expired rows). Under the
-- intended non-superuser role the plain tenant policy silences both — the
-- drain peeks zero rows forever and the outbox grows unboundedly (the exact
-- failure it exists to prevent). The escape is an explicit, transaction-
-- scoped GUC set ONLY by the two internal jobs (store/outbox.rs,
-- maintenance.rs); no request path ever sets it, and the explicit
-- `tenant_id = $1` predicates (the suspenders) stay on every request query.

DROP POLICY tenant_isolation ON outbox;
CREATE POLICY tenant_isolation ON outbox
  USING (tenant_id = current_setting('antares.tenant', true)
         OR current_setting('antares.service', true) = 'on')
  WITH CHECK (tenant_id = current_setting('antares.tenant', true));

-- ---- index repair ----------------------------------------------------------

-- Geo decidability: `location IS NULL OR <pred>` defeated the GIST index on
-- every geoquery (GIST cannot serve the IS NULL arm of an OR). Split the two
-- meanings of NULL: rows CARRYING the default GeoProperty whose geometry
-- could not be extracted (multi-instance, non-GeoJSON, PostGIS-invalid) are
-- flagged here and OR'd into the compiled predicate; rows with no geoproperty
-- can never match and are now excluded in SQL. Both OR arms are indexable →
-- BitmapOr(GIST, partial index) instead of a sequential scan.
ALTER TABLE entities ADD COLUMN location_ambiguous boolean NOT NULL DEFAULT false;
UPDATE entities
   SET location_ambiguous = true
 WHERE location IS NULL
   AND entity ? 'https://uri.etsi.org/ngsi-ld/location';
CREATE INDEX i_entities_loc_ambiguous ON entities (tenant_id) WHERE location_ambiguous;

-- Dead weight (proven unused by any compiled statement):
--  * i_entities_scopes: scopeQ compiles to a regex over unnest(scopes) — a
--    GIN array index cannot serve it.
--  * i_entities_modified: no query in the crate orders or filters on it.
-- The GIN jsonb_path_ops index STAYS: the q= compiler now emits the `@?`
-- operator form, which is exactly what jsonb_path_ops serves.
DROP INDEX IF EXISTS i_entities_scopes;
DROP INDEX IF EXISTS i_entities_modified;

-- attr_instances carries RLS in plain mode only (ADR-0006: timescale
-- columnstore refuses row security) — extend that policy the same way so the
-- DEFAULT-partition retention DELETE works under a non-superuser role.
DO $$
BEGIN
  IF EXISTS (SELECT 1 FROM pg_policies
             WHERE tablename = 'attr_instances' AND policyname = 'tenant_isolation') THEN
    EXECUTE 'DROP POLICY tenant_isolation ON attr_instances';
    EXECUTE 'CREATE POLICY tenant_isolation ON attr_instances
               USING (tenant_id = current_setting(''antares.tenant'', true)
                      OR current_setting(''antares.service'', true) = ''on'')
               WITH CHECK (tenant_id = current_setting(''antares.tenant'', true))';
  END IF;
END $$;
