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
- [B] migrations/0001_init.sql `try_timestamptz`: plpgsql EXCEPTION block = one
  subtransaction per unparsable value. Acceptable: the API validates every
  datetime before it reaches the DB, so the fallback fires only on rows
  written past the API. Note only.
- [x] any.rs batch_upsert: `let _ = id;` leftover replaced by `_` in the
  pattern (fixed in the same commit that added this file).
- [B] 5814_01_02 / 5814_02_02 (local-fork DistributedOperations TPs): one rule-8 run on the memory broker saw "Timeout: request was not received" on the second test of each file at 05:41 UTC 2026-08-25 (no build, pull or sweep running); 4/4 green in isolation on the same broker and 132/132 on the clean DistributedOperations re-run. Suspect: the httpctrl mock restarting on the same port between the two tests. Watch for recurrence in CI before touching either side.
- [B] pg_temporal.rs attr_object_expr ranks lastN on the JSON dt key `(ai.data ->> $tp) COLLATE "C"` for byte-exactness with the API window; the parsed timeproperty column (observed_at / created_at / modified_at, NOT NULL, same instants) gives the same order and measured 1.6x faster (4.1 vs 6.5 ms per 3000-row entity). Small win on a small number; take it when that path is next touched, with the tie/NULLS-LAST cases pinned.
- [B] code radar (dev/code-xray.sh → results/x-ray/code-radar.txt, complexity x churn, coverage join pending a CI coverage.json): the five functions with CCN > 100 are expand.rs expand_instance (131), subscriptions.rs normalize_subscription (132), temporal.rs query_temporal_inner (109), csource.rs normalize_registration (113), batch.rs batch_write (106). Each is one clause's validation matrix written as a single function; splitting by member is mechanical but wide, and every one sits under the 1713-TP matrix. Split on next touch, one function per commit.
- [B] cargo clippy pedantic+nursery sweep: 2147 report lines (results/x-ray/clippy-pedantic.txt); the standing gate lints stay as they are. cargo-machete: no unused dependencies.
