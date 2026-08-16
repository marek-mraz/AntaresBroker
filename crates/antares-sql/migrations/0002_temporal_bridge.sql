-- Temporal current-shape bridge: the temporal_entities table
-- PLUS a `doc` column holding the v0 one-doc-per-entity temporal form.
-- The doc column is the CUTOVER BRIDGE only: when the real
-- attr_instances decomposition lands (per-instance rows, range partitioning), the
-- doc column is dropped and this comment dies with it. Extracted columns are
-- populated from the doc at write time either way, so queries that only need
-- types/timestamps already read real columns.

CREATE TABLE temporal_entities (
  tenant_id   text NOT NULL,
  id          text NOT NULL,
  types       text[] NOT NULL,
  scopes      text[],
  doc         jsonb NOT NULL,             -- v0 bridge form (attr IRI -> instance array)
  created_at  timestamptz NOT NULL,
  modified_at timestamptz NOT NULL,
  deleted_at  timestamptz,
  PRIMARY KEY (tenant_id, id)
);
CREATE INDEX i_temporal_entities_types ON temporal_entities USING gin (tenant_id, types);

ALTER TABLE temporal_entities ENABLE ROW LEVEL SECURITY;
ALTER TABLE temporal_entities FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON temporal_entities
  USING (tenant_id = current_setting('antares.tenant', true))
  WITH CHECK (tenant_id = current_setting('antares.tenant', true));
