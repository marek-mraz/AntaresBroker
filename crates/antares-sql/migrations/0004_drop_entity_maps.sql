-- The `entity_maps` row store is gone: EntityMaps (5.14) are stored as
-- documents in `entity_map_docs`, which is what every read and write path
-- uses. Nothing ever wrote a row here, so the table drops with its policy
-- and its TTL index.
DROP TABLE IF EXISTS entity_maps;
