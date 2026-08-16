-- Temporal read cutover: attr_instances becomes the READ path.
-- The 0002 bridge doc dies here — temporal_entities keeps only the small
-- `meta` document (id/type/scope/@context/createdAt/modifiedAt/deletedAt/
-- expiresAt, verbatim strings so representations stay byte-faithful); every
-- attribute instance lives ONLY as attr_instances rows, which the shadow sync
-- has kept complete since 0003 — no instance backfill needed.

-- backfill runs as the table owner; policies must not hide rows from it
SET LOCAL row_security = off;

ALTER TABLE temporal_entities ADD COLUMN meta jsonb;
UPDATE temporal_entities SET meta = (
  SELECT COALESCE(jsonb_object_agg(t.k, t.v), '{}'::jsonb)
  FROM jsonb_each(doc) AS t(k, v)
  WHERE t.k IN ('id','type','scope','@context','createdAt','modifiedAt',
                'deletedAt','expiresAt'));
ALTER TABLE temporal_entities ALTER COLUMN meta SET NOT NULL;
ALTER TABLE temporal_entities DROP COLUMN doc;
