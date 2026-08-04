-- Antares initial schema (docs/deep-analysis.md §8).
-- Shared schema, tenant_id on every row, RLS as the safety net (§3).
-- Timescale-only statements are guarded so the same migration runs in
-- `plain` mode (§8.2).

CREATE EXTENSION IF NOT EXISTS postgis;
CREATE EXTENSION IF NOT EXISTS btree_gin;   -- tenant-scoped GIN composites (§8.1)

CREATE TABLE tenants (
  tenant_id  text PRIMARY KEY,
  created_at timestamptz NOT NULL DEFAULT now()
);
INSERT INTO tenants (tenant_id) VALUES ('default') ON CONFLICT DO NOTHING;

CREATE TABLE entities (
  tenant_id   text NOT NULL,
  id          text NOT NULL,
  entity      jsonb NOT NULL,
  version     bigint NOT NULL DEFAULT 1,
  types       text[] NOT NULL,
  scopes      text[],
  location    geometry(Geometry, 4326),
  created_at  timestamptz NOT NULL,
  modified_at timestamptz NOT NULL,
  expires_at  timestamptz,
  PRIMARY KEY (tenant_id, id)
) WITH (fillfactor = 85);  -- HOT-update headroom on a JSONB-update-heavy table (§3.1)
CREATE INDEX i_entities_location ON entities USING gist (location);
CREATE INDEX i_entities_jsonb    ON entities USING gin  (entity jsonb_path_ops);
CREATE INDEX i_entities_modified ON entities (tenant_id, modified_at DESC);
CREATE INDEX i_entities_scopes   ON entities USING gin  (scopes);
CREATE INDEX i_entities_types    ON entities USING gin  (tenant_id, types);  -- btree_gin
-- §3.1.3: eager autovacuum — dead-tuple bloat is Scorpio issue #573's suspect class.
ALTER TABLE entities SET (
  autovacuum_vacuum_scale_factor = 0.05, autovacuum_analyze_scale_factor = 0.02);

CREATE TABLE subscriptions (
  tenant_id         text NOT NULL,
  id                text NOT NULL,
  subscription      jsonb NOT NULL,
  context           jsonb NOT NULL,
  expires_at        timestamptz,
  is_active         bool NOT NULL DEFAULT true,
  times_sent        bigint NOT NULL DEFAULT 0,
  last_notification timestamptz,
  last_success      timestamptz,
  last_failure      timestamptz,
  PRIMARY KEY (tenant_id, id)
);

CREATE TABLE csource_subscriptions (      -- /csourceSubscriptions (§8.3, same shape)
  tenant_id         text NOT NULL,
  id                text NOT NULL,
  subscription      jsonb NOT NULL,
  context           jsonb NOT NULL,
  expires_at        timestamptz,
  is_active         bool NOT NULL DEFAULT true,
  times_sent        bigint NOT NULL DEFAULT 0,
  last_notification timestamptz,
  last_success      timestamptz,
  last_failure      timestamptz,
  PRIMARY KEY (tenant_id, id)
);

CREATE TABLE csource_registrations (
  tenant_id    text NOT NULL,
  id           text NOT NULL,
  registration jsonb NOT NULL,
  PRIMARY KEY (tenant_id, id)
);

-- §8.3 flattened federation match table; Scorpio's ~46 bool columns = one bitmask.
CREATE TABLE csource_index (
  tenant_id         text NOT NULL,
  registration_id   text NOT NULL,
  entity_id         text,
  id_pattern        text,
  entity_type       text,
  property_name     text,
  relationship_name text,
  location          geometry(Geometry, 4326),
  scopes            text[],
  expires_at        timestamptz,
  endpoint          text NOT NULL,
  mode              smallint NOT NULL,  -- 0 auxiliary | 1 inclusive | 2 redirect | 3 exclusive
  ops               bigint NOT NULL,    -- Operation-enum bitmask (§4.20)
  tenant_at_peer    text,
  headers           jsonb,
  host_alias        text,
  FOREIGN KEY (tenant_id, registration_id)
    REFERENCES csource_registrations ON DELETE CASCADE
);
CREATE INDEX i_csource_index_type ON csource_index (tenant_id, entity_type);
CREATE INDEX i_csource_index_id   ON csource_index (tenant_id, entity_id);
CREATE INDEX i_csource_index_geo  ON csource_index USING gist (location);

CREATE TABLE jsonld_contexts (
  id         text PRIMARY KEY,   -- deliberately cross-tenant (§8.3)
  body       jsonb NOT NULL,
  kind       text NOT NULL CHECK (kind IN ('Hosted', 'Cached', 'ImplicitlyCreated')),
  created_at timestamptz NOT NULL DEFAULT now(),
  last_usage timestamptz,
  hits       bigint NOT NULL DEFAULT 0
);

CREATE TABLE entity_maps (               -- 5.5.9.3 distributed pagination
  tenant_id       text NOT NULL,
  map_id          text NOT NULL,
  pos             bigint NOT NULL,
  query_checksum  text NOT NULL,
  entity_id       text NOT NULL,
  remote_query    text,
  registration_id text NOT NULL,
  last_access     timestamptz NOT NULL,
  expires_at      timestamptz NOT NULL,
  PRIMARY KEY (tenant_id, map_id, pos)
);
CREATE INDEX i_entity_maps_expiry ON entity_maps (expires_at);  -- TTL sweep

CREATE TABLE outbox (
  seq        bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  tenant_id  text NOT NULL,
  event      jsonb NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now()
);

-- §3 RLS backstop on every tenant-scoped table: ENABLE for app roles, FORCE
-- so the table owner is covered too. Superusers always bypass RLS — the
-- broker must connect as a non-superuser role for the backstop to bite
-- (the RLS denial test in tests/pg.rs proves it with a dedicated role).
DO $$
DECLARE t text;
BEGIN
  FOREACH t IN ARRAY ARRAY['entities','subscriptions','csource_subscriptions',
                           'csource_registrations','csource_index','entity_maps','outbox']
  LOOP
    EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY', t);
    EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY', t);
    EXECUTE format(
      'CREATE POLICY tenant_isolation ON %I
         USING (tenant_id = current_setting(''antares.tenant'', true))
         WITH CHECK (tenant_id = current_setting(''antares.tenant'', true))', t);
  END LOOP;
END $$;
