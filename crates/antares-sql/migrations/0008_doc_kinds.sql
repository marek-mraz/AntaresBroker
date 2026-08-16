-- Durable state for HA: snapshots (5.16), EntityMap API docs (5.14) and
-- distributed-subscription mappings (5.8.1.4) promoted from per-process
-- memory to the store — a broker restart no longer loses them.
CREATE TABLE snapshots (
  tenant_id text NOT NULL,
  id        text NOT NULL,
  doc       jsonb NOT NULL,
  PRIMARY KEY (tenant_id, id)
);

CREATE TABLE entity_map_docs (
  tenant_id text NOT NULL,
  id        text NOT NULL,
  doc       jsonb NOT NULL,
  PRIMARY KEY (tenant_id, id)
);

CREATE TABLE dist_subs (
  tenant_id text NOT NULL,
  id        text NOT NULL,
  doc       jsonb NOT NULL,
  PRIMARY KEY (tenant_id, id)
);

-- same RLS backstop as 0001
DO $$
DECLARE t text;
BEGIN
  FOREACH t IN ARRAY ARRAY['snapshots','entity_map_docs','dist_subs']
  LOOP
    EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY', t);
    EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY', t);
    EXECUTE format(
      'CREATE POLICY tenant_isolation ON %I
         USING (tenant_id = current_setting(''antares.tenant'', true))
         WITH CHECK (tenant_id = current_setting(''antares.tenant'', true))', t);
  END LOOP;
END $$;
