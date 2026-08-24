# Antares audit findings

Rebuilt 2026-08-24 (the previous file was untracked and lost with the
container). Rule: claude.md §0.5.2 — every questionable implementation found
while auditing gets a `[B]` line here; small + safe is fixed in the same
commit, large stays on the list until measured.

## 2.4 Findings

- [B] egress.rs `evict_oldest`: O(N) `min_by_key` sweep over up to 4096
  tracked hosts, under the breaker lock, on every insert once full. Cost is
  microseconds per request at that size; an LRU (moka is already a dep) makes
  it O(1). Swap only on a measured stall — bench first.
- [B] migrations/0011 `try_timestamptz`: plpgsql EXCEPTION block = one
  subtransaction per unparsable value. Acceptable: the API validates every
  datetime before it reaches the DB, so the fallback fires only on rows
  written past the API. Note only.
- [x] any.rs batch_upsert: `let _ = id;` leftover replaced by `_` in the
  pattern (fixed in the same commit that added this file).
