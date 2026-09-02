-- The `antares.service` escape is granted per command.
--
-- 0001_init gave `entities`, `outbox` and (plain mode) `attr_instances` one
-- FOR ALL policy whose USING clause names the escape. USING is what
-- PostgreSQL applies to the EXISTING row of an UPDATE or a DELETE; WITH
-- CHECK applies only to the new row. So a role that armed the escape — a
-- plain custom setting any role can set, including through a SQL-injection
-- bug — could UPDATE another tenant's row and set its tenant_id to its own:
-- USING passed on the escape, WITH CHECK passed because the NEW row is in
-- the caller's tenant, and the row changed hands.
--
-- The escape exists for the outbox drain and the two 4.22 reaps, which read
-- and delete and nothing else. Split by command it reaches exactly those,
-- and an UPDATE stays inside the caller's tenant on both sides.
DO $$
DECLARE t text;
BEGIN
  FOREACH t IN ARRAY ARRAY['entities', 'outbox', 'attr_instances']
  LOOP
    CONTINUE WHEN NOT EXISTS (
      SELECT 1 FROM pg_policies
       WHERE schemaname = 'public' AND tablename = t
         AND policyname = 'tenant_isolation');
    EXECUTE format('DROP POLICY tenant_isolation ON %I', t);
    EXECUTE format(
      'CREATE POLICY tenant_read ON %I FOR SELECT
         USING (tenant_id = current_setting(''antares.tenant'', true)
                OR current_setting(''antares.service'', true) = ''on'')', t);
    EXECUTE format(
      'CREATE POLICY tenant_reap ON %I FOR DELETE
         USING (tenant_id = current_setting(''antares.tenant'', true)
                OR current_setting(''antares.service'', true) = ''on'')', t);
    EXECUTE format(
      'CREATE POLICY tenant_write ON %I FOR INSERT
         WITH CHECK (tenant_id = current_setting(''antares.tenant'', true))', t);
    EXECUTE format(
      'CREATE POLICY tenant_update ON %I FOR UPDATE
         USING (tenant_id = current_setting(''antares.tenant'', true))
         WITH CHECK (tenant_id = current_setting(''antares.tenant'', true))', t);
  END LOOP;
END $$;
