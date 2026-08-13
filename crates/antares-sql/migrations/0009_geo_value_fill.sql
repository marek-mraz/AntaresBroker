-- 5.7.4.4 S3 geo prefilter: attr_instances.geo_value (0003, until now
-- unfilled) gets populated at insert time so a windowed EXISTS can narrow
-- the entity set before reconstruction. Extraction must NEVER fail a write:
-- stored data is API-validated (4.7), but the store contract is defensive —
-- anything ST_GeomFromGeoJSON rejects simply stays NULL, and the prefilter
-- treats NULL as "reaches the evaluator" (superset invariant).
-- Existing rows keep NULL geo_value: they always survive the prefilter, so
-- no backfill is required for correctness (backfill only improves speed).
CREATE OR REPLACE FUNCTION try_geomfromgeojson(g text) RETURNS geometry
LANGUAGE plpgsql IMMUTABLE AS $$
BEGIN
  RETURN ST_SetSRID(ST_GeomFromGeoJSON(g), 4326);
EXCEPTION WHEN OTHERS THEN
  RETURN NULL;
END $$;
