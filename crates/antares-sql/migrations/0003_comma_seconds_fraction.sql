-- 4.6.3: "The Seconds component may optionally contain a decimal fraction.
-- In this case the string shall contain two integer digits, followed by a
-- decimal point and then one or more fractional digits, up to a maximum of
-- six. ... In requests, also a comma instead of a decimal point may be used
-- as separator for compatibility reasons."
--
-- The stamps this function reads live inside jsonb exactly as the client
-- wrote them, so it has to accept that form. PostgreSQL does not, and NULL
-- here does not mean "invalid" — it means "no expiry" to `NOT_EXPIRED` and
-- to the 4.22 instance reap, so a comma-stamped expiry made the document
-- immortal instead of raising. A comma cannot appear anywhere else in a
-- DateTime of that shape, so the FIRST one is the fraction separator
-- (regexp_replace without the g flag rewrites only that one); anything else
-- still fails the cast and still returns NULL, as before.
CREATE OR REPLACE FUNCTION try_timestamptz(t text) RETURNS timestamptz
LANGUAGE plpgsql STABLE AS $$
BEGIN
  RETURN regexp_replace(t, ',', '.')::timestamptz;
EXCEPTION WHEN OTHERS THEN
  RETURN NULL;
END $$;
