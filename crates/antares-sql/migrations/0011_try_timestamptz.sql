-- 4.22 expiry on the temporal read path: `expiresAt` lives inside the
-- `temporal_entities.meta` jsonb, so the filter has to turn stored TEXT into
-- an instant. A bare `::timestamptz` RAISES on anything it cannot parse, and
-- the read is tenant-wide — one unparseable stamp aborts every temporal query
-- in that tenant, permanently, including the query needed to find it.
-- Same contract as `try_geomfromgeojson` (0009): the store is defensive about
-- what it holds, so an unparseable stamp is simply "no usable expiry" (the
-- row stays visible) instead of an error.
-- STABLE, not IMMUTABLE: text -> timestamptz depends on the TimeZone setting.
CREATE OR REPLACE FUNCTION try_timestamptz(t text) RETURNS timestamptz
LANGUAGE plpgsql STABLE AS $$
BEGIN
  RETURN t::timestamptz;
EXCEPTION WHEN OTHERS THEN
  RETURN NULL;
END $$;
