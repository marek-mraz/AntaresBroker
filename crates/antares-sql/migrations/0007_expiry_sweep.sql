-- 4.22 transient storage: the garbage-collection sweep deletes expired
-- entity rows across ALL tenants — the same service-path shape as the outbox
-- drain (0005), so the entities policy gains the same escape. Request paths
-- never set antares.service; their explicit tenant predicates stand.
DROP POLICY tenant_isolation ON entities;
CREATE POLICY tenant_isolation ON entities
  USING (tenant_id = current_setting('antares.tenant', true)
         OR current_setting('antares.service', true) = 'on')
  WITH CHECK (tenant_id = current_setting('antares.tenant', true));
