#!/usr/bin/env bash
# Bulk entity ingest: the drop-indexes / COPY / rebuild / ANALYZE recipe.
#
# For initial loads and migrations ONLY — run it against a database no broker
# is serving (index drops break live queries; the write path is not consulted,
# so no notifications fire and no history is recorded).
#
# Input: NDJSON — one INTERNAL-form entity document per line (expanded
# attribute IRIs, core members `id`/`type`/`scope`/`createdAt`/`modifiedAt`
# short — the shape `GET /entities/{id}` returns with a core-only context).
# Existing (tenant_id, id) rows are left untouched (ON CONFLICT DO NOTHING).
#
#   DATABASE_URL=postgres://… dev/bulk-load.sh entities.ndjson [tenant]
#
# The derived columns (types, scopes, timestamps, location) are computed
# in SQL from the document, mirroring the store's own extraction
# (compile::geo::extract_location + the write path's try_geomfromgeojson).
set -euo pipefail

FILE=${1:?usage: bulk-load.sh <entities.ndjson> [tenant]}
TENANT=${2:-default}
: "${DATABASE_URL:?set DATABASE_URL}"

LOC='https://uri.etsi.org/ngsi-ld/location'

psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -v tenant="$TENANT" <<SQL
INSERT INTO tenants (tenant_id) VALUES (:'tenant') ON CONFLICT DO NOTHING;
CREATE UNLOGGED TABLE IF NOT EXISTS bulk_stage (doc jsonb);
TRUNCATE bulk_stage;
-- csv with unused quote/delimiter bytes = raw line passthrough, so the
-- jsonb cast is the only parser the payload meets
\copy bulk_stage (doc) FROM '$FILE' WITH (FORMAT csv, QUOTE E'\x01', DELIMITER E'\x02')

-- secondary indexes off during the load; PK stays (ON CONFLICT needs it)
-- exactly the set the migrations leave in place (0005 dropped the scopes
-- and modified_at indexes as dead weight; do not bring them back)
DROP INDEX IF EXISTS i_entities_location, i_entities_jsonb, i_entities_types,
  i_entities_loc_ambiguous, i_entities_expires;

-- the store's extract_location rule: the default GeoProperty, exactly one
-- instance, a GeoJSON geometry (not a collection); anything else with the
-- geoproperty present is location_ambiguous and judged by the evaluator
WITH src AS (
  SELECT doc,
         CASE WHEN jsonb_typeof(doc->'$LOC') = 'array' AND jsonb_array_length(doc->'$LOC') = 1
                THEN COALESCE(doc->'$LOC'->0->'value', doc->'$LOC'->0->'object')
              WHEN jsonb_typeof(doc->'$LOC') = 'object'
                THEN COALESCE(doc->'$LOC'->'value', doc->'$LOC'->'object') END AS g
  FROM bulk_stage
  WHERE doc->>'id' IS NOT NULL
), geom AS (
  SELECT doc,
         CASE WHEN g->>'type' IN ('Point','MultiPoint','LineString','MultiLineString','Polygon','MultiPolygon')
               AND ST_IsValid(try_geomfromgeojson(g::text))
              THEN try_geomfromgeojson(g::text) END AS location
  FROM src
)
INSERT INTO entities
  (tenant_id, id, entity, types, scopes, created_at, modified_at, expires_at,
   location, location_ambiguous)
SELECT :'tenant', doc->>'id', doc,
       CASE WHEN jsonb_typeof(doc->'type') = 'array'
            THEN ARRAY(SELECT jsonb_array_elements_text(doc->'type'))
            ELSE ARRAY[doc->>'type'] END,
       CASE WHEN doc->'scope' IS NULL THEN NULL
            WHEN jsonb_typeof(doc->'scope') = 'array'
            THEN ARRAY(SELECT jsonb_array_elements_text(doc->'scope'))
            ELSE ARRAY[doc->>'scope'] END,
       COALESCE((doc->>'createdAt')::timestamptz, now()),
       COALESCE((doc->>'modifiedAt')::timestamptz, now()),
       (doc->>'expiresAt')::timestamptz,
       location,
       location IS NULL AND doc ? '$LOC'
FROM geom
ON CONFLICT (tenant_id, id) DO NOTHING;

DROP TABLE bulk_stage;

CREATE INDEX i_entities_location ON entities USING gist (location);
CREATE INDEX i_entities_jsonb    ON entities USING gin  (entity jsonb_path_ops);
CREATE INDEX i_entities_types    ON entities USING gin  (tenant_id, types);
CREATE INDEX i_entities_loc_ambiguous ON entities (tenant_id) WHERE location_ambiguous;
CREATE INDEX i_entities_expires ON entities (expires_at) WHERE expires_at IS NOT NULL;

ANALYZE entities;
SQL

echo "bulk load done: $(wc -l < "$FILE") lines offered into tenant '$TENANT'"
