-- 4.22 garbage collection: the expired-entity sweep selects on
-- `expires_at < now()`, which until now had no index at all — a sequential
-- scan of every entity in the deployment, on every maintenance tick, under a
-- 30 s statement_timeout.
-- Partial by design: expires_at is NULL on durable entities (the vast
-- majority), so the index holds exactly the transient set the sweep deletes
-- and costs the durable writes nothing.
CREATE INDEX i_entities_expires ON entities (expires_at) WHERE expires_at IS NOT NULL;
