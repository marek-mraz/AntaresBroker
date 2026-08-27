#!/usr/bin/env bash
# Bulk entity ingest: the drop-indexes / COPY / rebuild / ANALYZE recipe.
#
# For initial loads and migrations ONLY — run it against a database no broker
# is serving (index drops break live queries; the write path is not consulted,
# so no notifications fire and no history is recorded).
#
# Input: NDJSON — one entity document per line in the store's internal form:
# attribute names as expanded IRIs, every attribute an ARRAY of instances,
# core members `id`/`type`/`scope`/`createdAt`/`modifiedAt` short. `type` and
# `scope` may be given as a string or an array; the stored document always
# carries them as arrays, which is what the query evaluator reads.
# Existing (tenant_id, id) rows are left untouched (ON CONFLICT DO NOTHING).
# A line may carry its own tenant as a prefix, `<tenant>\x02<json>`, which
# is what dev/perf/gen.py writes; the file may be a FIFO.
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

# one text column: `tenant\x02json` splits on the byte, a bare json line
# lands whole in the first column (no \x02 in JSON) and takes the argument
psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -v tenant="$TENANT" <<SQL
CREATE UNLOGGED TABLE IF NOT EXISTS bulk_raw (line text);
TRUNCATE bulk_raw;
\copy bulk_raw (line) FROM '$FILE' WITH (FORMAT csv, QUOTE E'\x01', DELIMITER E'\x03')
CREATE UNLOGGED TABLE IF NOT EXISTS bulk_stage (tenant_id text, doc jsonb);
TRUNCATE bulk_stage;
INSERT INTO bulk_stage
SELECT CASE WHEN position(E'\x02' IN line) > 0 THEN split_part(line, E'\x02', 1) ELSE :'tenant' END,
       CASE WHEN position(E'\x02' IN line) > 0 THEN split_part(line, E'\x02', 2) ELSE line END::jsonb
FROM bulk_raw;
DROP TABLE bulk_raw;
INSERT INTO tenants (tenant_id) SELECT DISTINCT tenant_id FROM bulk_stage ON CONFLICT DO NOTHING;

-- secondary indexes off during the load; PK stays (ON CONFLICT needs it)
-- exactly the set the migrations leave in place (0005 dropped the scopes
-- and modified_at indexes as dead weight; do not bring them back)
DROP INDEX IF EXISTS i_entities_location, i_entities_jsonb, i_entities_types,
  i_entities_loc_ambiguous, i_entities_expires;

-- the store's extract_location rule: the default GeoProperty, exactly one
-- instance, a GeoJSON geometry (not a collection); anything else with the
-- geoproperty present is location_ambiguous and judged by the evaluator
WITH src AS (
  SELECT tenant_id, jsonb_set(
           CASE WHEN doc ? 'scope' AND jsonb_typeof(doc->'scope') <> 'array'
                THEN jsonb_set(doc, '{scope}', jsonb_build_array(doc->'scope')) ELSE doc END,
           '{type}',
           CASE WHEN jsonb_typeof(doc->'type') = 'array' THEN doc->'type'
                ELSE jsonb_build_array(doc->'type') END) AS doc,
         CASE WHEN jsonb_typeof(doc->'$LOC') = 'array' AND jsonb_array_length(doc->'$LOC') = 1
                THEN COALESCE(doc->'$LOC'->0->'value', doc->'$LOC'->0->'object')
              WHEN jsonb_typeof(doc->'$LOC') = 'object'
                THEN COALESCE(doc->'$LOC'->'value', doc->'$LOC'->'object') END AS g
  FROM bulk_stage
  WHERE doc->>'id' IS NOT NULL
), geom AS (
  SELECT tenant_id, doc,
         CASE WHEN g->>'type' IN ('Point','MultiPoint','LineString','MultiLineString','Polygon','MultiPolygon')
               AND ST_IsValid(try_geomfromgeojson(g::text))
              THEN try_geomfromgeojson(g::text) END AS location
  FROM src
)
INSERT INTO entities
  (tenant_id, id, entity, types, scopes, created_at, modified_at, expires_at,
   location, location_ambiguous)
SELECT tenant_id, doc->>'id', doc,
       ARRAY(SELECT jsonb_array_elements_text(doc->'type')),
       CASE WHEN doc->'scope' IS NULL THEN NULL
            ELSE ARRAY(SELECT jsonb_array_elements_text(doc->'scope')) END,
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

echo "bulk load done: $(wc -l < "$FILE") lines offered (tenant from the line prefix, else '$TENANT')"
