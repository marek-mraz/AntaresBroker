-- attr_instances: per-instance temporal rows.
-- ONE table shape for both temporal modes; only the partitioning bootstrap
-- differs (the modes differ in DDL bootstrap + maintenance jobs,
-- never in queries):
--   timescale: hypertable, 7-day chunks, native compression
--   plain:     native PARTITION BY RANGE (observed_at); partitions pre-created
--              by the broker's own scheduled job, catch-all DEFAULT
--              partition so a write never fails before the job first runs
--
-- A shape forced by PostgreSQL itself: a unique index
-- on a partitioned table (and on a hypertable) MUST include the partition
-- column, so the idempotent-upsert key is
-- (tenant_id, entity_id, attr_id, instance_id, observed_at) in BOTH modes.
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

-- RLS vs compression — a MEASURED collision (discovered 2026-08-04):
-- TimescaleDB refuses columnstore on a table with row security
-- ("columnstore cannot be used on table with row security"). The memory
-- budget leans on compression, so on THIS ONE TABLE, in timescale mode only,
-- the RLS belt is dropped and tenant isolation rests on the explicit
-- `tenant_id = $1` predicates every store query already carries (the
-- suspenders; store methods take &TenantId as their first parameter by
-- construction). Plain mode keeps the full belt. Recorded in ADR-0006.
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
               USING (tenant_id = current_setting(''antares.tenant'', true))
               WITH CHECK (tenant_id = current_setting(''antares.tenant'', true))';
  END IF;
END $$;
