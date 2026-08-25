-- Antares schema. Shared schema, tenant_id on every row, RLS as the safety
-- net. Timescale-only statements are guarded so the same migration runs in
-- `plain` mode.

CREATE EXTENSION IF NOT EXISTS postgis;
CREATE EXTENSION IF NOT EXISTS btree_gin;   -- tenant-scoped GIN composites

CREATE TABLE tenants (
  tenant_id  text PRIMARY KEY,
  created_at timestamptz NOT NULL DEFAULT now()
);
INSERT INTO tenants (tenant_id) VALUES ('default') ON CONFLICT DO NOTHING;

-- location_ambiguous: rows CARRYING the default GeoProperty whose geometry
-- could not be extracted (multi-instance, non-GeoJSON, PostGIS-invalid). The
-- geoquery compiles to `<gist pred> OR location_ambiguous`, both arms
-- indexable (BitmapOr), while rows with no geoproperty are excluded in SQL —
-- `location IS NULL OR <pred>` would defeat the GIST index.
CREATE TABLE entities (
  tenant_id          text NOT NULL,
  id                 text NOT NULL,
  entity             jsonb NOT NULL,
  version            bigint NOT NULL DEFAULT 1,
  types              text[] NOT NULL,
  scopes             text[],
  location           geometry(Geometry, 4326),
  location_ambiguous boolean NOT NULL DEFAULT false,
  created_at         timestamptz NOT NULL,
  modified_at        timestamptz NOT NULL,
  expires_at         timestamptz,
  PRIMARY KEY (tenant_id, id)
) WITH (fillfactor = 85);  -- HOT-update headroom on a JSONB-update-heavy table
CREATE INDEX i_entities_location      ON entities USING gist (location);
CREATE INDEX i_entities_jsonb         ON entities USING gin  (entity jsonb_path_ops);  -- serves the q= `@?` form
CREATE INDEX i_entities_types         ON entities USING gin  (tenant_id, types);       -- btree_gin
CREATE INDEX i_entities_loc_ambiguous ON entities (tenant_id) WHERE location_ambiguous;
-- 4.22 garbage collection: the sweep selects on `expires_at < now()`; partial
-- because expires_at is NULL on durable entities, so the index holds exactly
-- the transient set and costs durable writes nothing.
CREATE INDEX i_entities_expires       ON entities (expires_at) WHERE expires_at IS NOT NULL;
-- scopeQ compiles to a regex over unnest(scopes) — no array index can serve
-- it, and nothing orders on modified_at, so neither carries an index.
-- Eager autovacuum — dead-tuple bloat is Scorpio issue #573's suspect class.
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

CREATE TABLE csource_subscriptions (      -- /csourceSubscriptions (same shape)
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

-- Flattened federation match table; Scorpio's ~46 bool columns = one bitmask.
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
  ops               bigint NOT NULL,    -- Operation-enum bitmask (clause 4.20)
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
  id         text PRIMARY KEY,   -- deliberately cross-tenant
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

-- Durable state for HA: snapshots (5.16), EntityMap API docs (5.14) and
-- distributed-subscription mappings (5.8.1.4) live in the store, so a broker
-- restart does not lose them.
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

-- Temporal entity head: only the small `meta` document (id/type/scope/
-- @context/createdAt/modifiedAt/deletedAt/expiresAt, verbatim strings so
-- representations stay byte-faithful). Every attribute instance lives ONLY
-- as attr_instances rows.
CREATE TABLE temporal_entities (
  tenant_id   text NOT NULL,
  id          text NOT NULL,
  types       text[] NOT NULL,
  scopes      text[],
  meta        jsonb NOT NULL,
  created_at  timestamptz NOT NULL,
  modified_at timestamptz NOT NULL,
  deleted_at  timestamptz,
  PRIMARY KEY (tenant_id, id)
);
CREATE INDEX i_temporal_entities_types ON temporal_entities USING gin (tenant_id, types);

-- Maintenance-job claim rows: N broker instances race with
-- SELECT … FOR UPDATE SKIP LOCKED, exactly one wins each run — no
-- coordinator, no leader election. Internal table: no RLS, no tenant column.
CREATE TABLE maintenance_jobs (
  name     text PRIMARY KEY,
  last_run timestamptz
);
INSERT INTO maintenance_jobs (name) VALUES ('temporal_partitions');

-- attr_instances: per-instance temporal rows.
-- ONE table shape for both temporal modes; only the partitioning bootstrap
-- differs (the modes differ in DDL bootstrap + maintenance jobs, never in
-- queries):
--   timescale: hypertable, 7-day chunks, native compression
--   plain:     native PARTITION BY RANGE (observed_at); partitions pre-created
--              by the broker's own scheduled job, catch-all DEFAULT
--              partition so a write never fails before the job first runs
--
-- A shape forced by PostgreSQL itself: a unique index on a partitioned table
-- (and on a hypertable) MUST include the partition column, so the
-- idempotent-upsert key is
-- (tenant_id, entity_id, attr_id, instance_id, observed_at) in BOTH modes.
--
-- geo_value is filled at insert time (5.7.4.4 S3 geo prefilter) via
-- try_geomfromgeojson below; NULL means "reaches the evaluator".
--
-- Retention is DELIBERATELY not set here: a migration that silently drops
-- data by default is the wrong kind of surprise. Retention is a deployment
-- knob wired through the broker's maintenance job.
DO $$
DECLARE cols text := '
  tenant_id   text NOT NULL,
  entity_id   text NOT NULL,
  attr_id     text NOT NULL,
  instance_id text NOT NULL,
  dataset_id  text,
  observed_at timestamptz NOT NULL,
  created_at  timestamptz NOT NULL,
  modified_at timestamptz NOT NULL,
  deleted_at  timestamptz,
  data        jsonb NOT NULL,
  geo_value   geometry(Geometry, 4326)
';
BEGIN
  IF EXISTS (SELECT 1 FROM pg_extension WHERE extname = 'timescaledb') THEN
    EXECUTE 'CREATE TABLE attr_instances (' || cols || ')';
    PERFORM public.create_hypertable('attr_instances', 'observed_at',
                                     chunk_time_interval => INTERVAL '7 days');
  ELSE
    EXECUTE 'CREATE TABLE attr_instances (' || cols || ') PARTITION BY RANGE (observed_at)';
    EXECUTE 'CREATE TABLE attr_instances_default PARTITION OF attr_instances DEFAULT';
  END IF;
END $$;

-- both modes: the read index + the idempotent-upsert key
CREATE INDEX i_attr_instances_lookup
  ON attr_instances (tenant_id, entity_id, attr_id, observed_at DESC);
CREATE UNIQUE INDEX u_attr_instances
  ON attr_instances (tenant_id, entity_id, attr_id, instance_id, observed_at);

-- Row-level security -------------------------------------------------------
--
-- RLS backstop on every tenant-scoped table: ENABLE for app roles, FORCE so
-- the table owner is covered too. Superusers always bypass RLS — the broker
-- must connect as a non-superuser role for the backstop to bite (the RLS
-- denial test in tests/pg.rs proves it with a dedicated role).
--
-- Service-path escape: the outbox drain, the expired-entity sweep (4.22) and
-- the temporal retention job are cross-tenant BY NATURE. Under the intended
-- non-superuser role the plain tenant policy would silence them (the drain
-- peeks zero rows forever and the outbox grows unboundedly). The escape is an
-- explicit, transaction-scoped GUC set ONLY by those internal jobs
-- (store/outbox.rs, maintenance.rs); no request path ever sets it, and the
-- explicit `tenant_id = $1` predicates stay on every request query.
DO $$
DECLARE t text;
BEGIN
  FOREACH t IN ARRAY ARRAY['entities','subscriptions','csource_subscriptions',
                           'csource_registrations','csource_index','entity_maps',
                           'outbox','snapshots','entity_map_docs','dist_subs',
                           'temporal_entities']
  LOOP
    EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY', t);
    EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY', t);
    IF t IN ('entities', 'outbox') THEN
      EXECUTE format(
        'CREATE POLICY tenant_isolation ON %I
           USING (tenant_id = current_setting(''antares.tenant'', true)
                  OR current_setting(''antares.service'', true) = ''on'')
           WITH CHECK (tenant_id = current_setting(''antares.tenant'', true))', t);
    ELSE
      EXECUTE format(
        'CREATE POLICY tenant_isolation ON %I
           USING (tenant_id = current_setting(''antares.tenant'', true))
           WITH CHECK (tenant_id = current_setting(''antares.tenant'', true))', t);
    END IF;
  END LOOP;
END $$;

-- RLS vs compression — a MEASURED collision: TimescaleDB refuses columnstore
-- on a table with row security ("columnstore cannot be used on table with
-- row security"). The memory budget leans on compression, so on THIS ONE
-- TABLE, in timescale mode only, the RLS belt is dropped and tenant isolation
-- rests on the explicit `tenant_id = $1` predicates every store query already
-- carries (store methods take &TenantId as their first parameter by
-- construction). Plain mode keeps the full belt, with the service escape so
-- the DEFAULT-partition retention DELETE works under a non-superuser role.
-- Recorded in ADR-0006.
DO $$
BEGIN
  IF EXISTS (SELECT 1 FROM pg_extension WHERE extname = 'timescaledb') THEN
    -- Compression shape: entity_id in orderby, never segmentby
    -- (1-row segments compress terribly).
    EXECUTE 'ALTER TABLE attr_instances SET (timescaledb.compress,
               timescaledb.compress_segmentby = ''tenant_id, attr_id'',
               timescaledb.compress_orderby   = ''entity_id, observed_at DESC'')';
    PERFORM public.add_compression_policy('attr_instances',
                                          compress_after => INTERVAL '7 days');
  ELSE
    EXECUTE 'ALTER TABLE attr_instances ENABLE ROW LEVEL SECURITY';
    EXECUTE 'ALTER TABLE attr_instances FORCE ROW LEVEL SECURITY';
    EXECUTE 'CREATE POLICY tenant_isolation ON attr_instances
               USING (tenant_id = current_setting(''antares.tenant'', true)
                      OR current_setting(''antares.service'', true) = ''on'')
               WITH CHECK (tenant_id = current_setting(''antares.tenant'', true))';
  END IF;
END $$;

-- Defensive SQL helpers ----------------------------------------------------
--
-- The store is defensive about what it holds: stored data is API-validated
-- (4.7), but a conversion that RAISES would abort a tenant-wide read on one
-- bad row, permanently, including the query needed to find it. Both return
-- NULL instead.

-- 5.7.4.4 S3 geo prefilter: fills attr_instances.geo_value at insert time;
-- anything ST_GeomFromGeoJSON rejects stays NULL and the prefilter treats
-- NULL as "reaches the evaluator" (superset invariant).
CREATE OR REPLACE FUNCTION try_geomfromgeojson(g text) RETURNS geometry
LANGUAGE plpgsql IMMUTABLE AS $$
BEGIN
  RETURN ST_SetSRID(ST_GeomFromGeoJSON(g), 4326);
EXCEPTION WHEN OTHERS THEN
  RETURN NULL;
END $$;

-- 4.22 expiry on the temporal read path: `expiresAt` lives inside
-- `temporal_entities.meta` as text. STABLE, not IMMUTABLE: text ->
-- timestamptz depends on the TimeZone setting.
CREATE OR REPLACE FUNCTION try_timestamptz(t text) RETURNS timestamptz
LANGUAGE plpgsql STABLE AS $$
BEGIN
  RETURN t::timestamptz;
EXCEPTION WHEN OTHERS THEN
  RETURN NULL;
END $$;
