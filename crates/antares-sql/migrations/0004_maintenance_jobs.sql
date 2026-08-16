-- Maintenance-job claim rows: N broker instances race
-- with SELECT … FOR UPDATE SKIP LOCKED, exactly one wins each run — no
-- coordinator, no leader election. Internal table: no RLS, no tenant column.
CREATE TABLE maintenance_jobs (
  name     text PRIMARY KEY,
  last_run timestamptz
);
INSERT INTO maintenance_jobs (name) VALUES ('temporal_partitions');
