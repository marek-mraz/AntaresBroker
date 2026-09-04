-- Claim-check retention. An event whose body exceeds the bus message ceiling
-- travels as a reference, and the row that held it is the only place the
-- whole event still exists: the store's current row is the after-image, so a
-- before-image is not recoverable from it and the consumer's diff comes out
-- empty. The drain therefore stamps such a row published instead of deleting
-- it, the consumer reads the event back by `seq`, and the maintenance pass
-- reaps stamped rows once the bus can no longer be carrying them.
ALTER TABLE outbox ADD COLUMN published_at timestamptz;

-- The drain's peek is the hot path and must not walk stamped rows. A partial
-- index over the pending ones keeps it on the same oldest-first scan it had
-- before the column existed.
CREATE INDEX outbox_pending_seq_idx ON outbox (seq) WHERE published_at IS NULL;
CREATE INDEX outbox_published_at_idx ON outbox (published_at) WHERE published_at IS NOT NULL;
