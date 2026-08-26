-- Dead letters: notifications a delivery policy gave up on, one document
-- per letter under the subscription's tenant (replayed or deleted through
-- the admin surface). Same shape and RLS belt as the other doc tables.
CREATE TABLE dead_letters (
  tenant_id text NOT NULL,
  id        text NOT NULL,
  doc       jsonb NOT NULL,
  PRIMARY KEY (tenant_id, id)
);
ALTER TABLE dead_letters ENABLE ROW LEVEL SECURITY;
ALTER TABLE dead_letters FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON dead_letters
  USING (tenant_id = current_setting('antares.tenant', true))
  WITH CHECK (tenant_id = current_setting('antares.tenant', true));
