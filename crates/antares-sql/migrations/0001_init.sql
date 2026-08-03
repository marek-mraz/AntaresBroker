-- Antares initial schema (docs/deep-analysis.md §8).
-- Shared schema, tenant_id on every row, RLS as the safety net (§3).
-- Timescale-only statements are guarded so the same migration runs in
-- `plain` mode (§8.2).

CREATE EXTENSION IF NOT EXISTS postgis;

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
CREATE INDEX i_entities_types    ON entities USING gin  (types);

ALTER TABLE entities ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON entities
  USING (tenant_id = current_setting('antares.tenant', true));

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
ALTER TABLE subscriptions ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON subscriptions
  USING (tenant_id = current_setting('antares.tenant', true));

CREATE TABLE jsonld_contexts (
  id         text PRIMARY KEY,   -- deliberately cross-tenant (§8.3)
  body       jsonb NOT NULL,
  kind       text NOT NULL CHECK (kind IN ('Hosted', 'Cached', 'ImplicitlyCreated')),
  created_at timestamptz NOT NULL DEFAULT now(),
  last_usage timestamptz,
  hits       bigint NOT NULL DEFAULT 0
);

CREATE TABLE outbox (
  seq        bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  tenant_id  text NOT NULL,
  event      jsonb NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now()
);
