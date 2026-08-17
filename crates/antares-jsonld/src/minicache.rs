//! wasm32 only: a bounded map with moka's call surface. moka's clock
//! calls `std::time::Instant::now()` unconditionally, which panics on
//! wasm32-unknown-unknown — so the browser build swaps in this FIFO-evicting
//! cache behind the same six methods the loader uses. Bounded is the
//! requirement (every cache has a max size); recency-optimal
//! eviction is not — a browser tab's @context set is tiny.
// FIFO eviction is deliberate; port moka's TinyLFU here only if a browser
// workload ever shows cache thrash.

use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
use std::sync::Mutex;

/// Eviction policy, in full: entries leave in the order they were FIRST
/// inserted. `get` is a pure read — it does not refresh recency — and
/// re-inserting a resident key overwrites its value without moving it back in
/// the queue. `entry_count()` never exceeds the capacity the cache was built
/// with, including a capacity of zero.
pub struct Cache<K, V> {
    cap: usize,
    inner: Mutex<(HashMap<K, V>, VecDeque<K>)>,
}

type Inner<K, V> = (HashMap<K, V>, VecDeque<K>);

impl<K: Eq + Hash + Clone, V: Clone> Cache<K, V> {
    pub fn new(cap: u64) -> Self {
        Self {
            cap: cap as usize,
            inner: Mutex::new((HashMap::new(), VecDeque::new())),
        }
    }

    /// A cache must not turn a panic elsewhere into a permanently poisoned
    /// broker: nothing under this lock can leave the map inconsistent, so a
    /// poisoned guard is taken as-is.
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner<K, V>> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn get<Q>(&self, key: &Q) -> Option<V>
    where
        K: std::borrow::Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        self.lock().0.get(key).cloned()
    }

    pub fn insert(&self, key: K, value: V) {
        if self.cap == 0 {
            return;
        }
        let mut g = self.lock();
        if !g.0.contains_key(&key) {
            while g.0.len() >= self.cap {
                match g.1.pop_front() {
                    Some(old) => {
                        g.0.remove(&old);
                    }
                    None => break,
                }
            }
            g.1.push_back(key.clone());
        }
        g.0.insert(key, value);
    }

    pub fn invalidate<Q>(&self, key: &Q)
    where
        K: std::borrow::Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        let mut g = self.lock();
        let (map, order) = &mut *g;
        map.remove(key);
        order.retain(|k| map.contains_key::<K>(k));
    }

    pub fn invalidate_all(&self) {
        let mut g = self.lock();
        g.0.clear();
        g.1.clear();
    }

    /// Snapshot of the resident entries, in moka's `iter()` item shape — the
    /// lock is released before the caller walks them, so a callback may touch
    /// this cache again.
    pub fn iter(&self) -> std::vec::IntoIter<(std::sync::Arc<K>, V)> {
        let g = self.lock();
        g.0.iter()
            .map(|(k, v)| (std::sync::Arc::new(k.clone()), v.clone()))
            .collect::<Vec<_>>()
            .into_iter()
    }

    pub fn run_pending_tasks(&self) {}

    pub fn entry_count(&self) -> u64 {
        self.lock().0.len() as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filled(cap: u64, n: u32) -> Cache<u32, u32> {
        let c = Cache::new(cap);
        for i in 0..n {
            c.insert(i, i);
        }
        c
    }

    /// The entry count is a hard ceiling: inserting far past the capacity
    /// leaves exactly `cap` entries.
    #[test]
    fn capacity_is_a_hard_bound() {
        for cap in [1u64, 2, 7, 64] {
            let c = filled(cap, 1000);
            assert_eq!(c.entry_count(), cap, "cap {cap}");
        }
    }

    /// A zero capacity means "hold nothing" — it must not degrade into an
    /// unbounded (or one-entry) map.
    #[test]
    fn zero_capacity_holds_nothing() {
        let c = filled(0, 100);
        assert_eq!(c.entry_count(), 0);
        assert_eq!(c.get(&99), None);
    }

    /// Eviction is FIFO by first insertion and therefore deterministic: after
    /// filling past the bound exactly the last `cap` keys survive.
    #[test]
    fn eviction_is_deterministic_fifo() {
        let c = filled(4, 6);
        assert_eq!(c.entry_count(), 4);
        for gone in [0u32, 1] {
            assert_eq!(c.get(&gone), None, "key {gone} should have been evicted");
        }
        for kept in [2u32, 3, 4, 5] {
            assert_eq!(c.get(&kept), Some(kept), "key {kept} should be resident");
        }
    }

    /// Documented policy: `get` is a pure read — it does NOT refresh recency.
    /// A key read on every lookup is still evicted in insertion order.
    #[test]
    fn get_does_not_refresh_recency() {
        let c = filled(4, 4);
        for _ in 0..10 {
            assert_eq!(c.get(&0), Some(0));
        }
        c.insert(4, 4);
        assert_eq!(
            c.get(&0),
            None,
            "get must not protect an entry from FIFO eviction"
        );
        assert_eq!(c.get(&1), Some(1));
    }

    /// Re-inserting an existing key overwrites the value, keeps the count flat
    /// and does NOT move the key to the back of the eviction queue.
    #[test]
    fn reinsert_updates_value_without_reordering() {
        let c = filled(3, 3);
        c.insert(0, 99);
        assert_eq!(c.entry_count(), 3);
        assert_eq!(c.get(&0), Some(99));
        c.insert(3, 3);
        assert_eq!(c.get(&0), None, "re-insert must not refresh eviction order");
        assert_eq!(c.entry_count(), 3);
    }

    /// `invalidate` drops the entry and its queue slot, so the freed room is
    /// reused before anything else is evicted.
    #[test]
    fn invalidate_frees_a_slot() {
        let c = filled(2, 2);
        c.invalidate(&0);
        assert_eq!(c.entry_count(), 1);
        c.insert(2, 2);
        assert_eq!(c.entry_count(), 2);
        assert_eq!(
            c.get(&1),
            Some(1),
            "the surviving entry must not be evicted early"
        );
        c.insert(3, 3);
        assert_eq!(c.entry_count(), 2);
        assert_eq!(c.get(&1), None);
        assert_eq!(c.get(&3), Some(3));
    }

    /// Invalidating an absent key is a no-op, and the bound still holds after.
    #[test]
    fn invalidate_absent_key_is_a_noop() {
        let c = filled(3, 3);
        c.invalidate(&42);
        assert_eq!(c.entry_count(), 3);
        for i in 0..3 {
            assert_eq!(c.get(&i), Some(i));
        }
    }

    #[test]
    fn invalidate_all_clears_both_map_and_queue() {
        let c = filled(4, 4);
        c.invalidate_all();
        assert_eq!(c.entry_count(), 0);
        assert_eq!(c.get(&3), None);
        // the queue was cleared too: the next 4 inserts evict nothing
        for i in 10..14 {
            c.insert(i, i);
        }
        assert_eq!(c.entry_count(), 4);
        for i in 10..14 {
            assert_eq!(c.get(&i), Some(i));
        }
    }

    /// `iter` yields every resident entry exactly once — it is what the
    /// caller filters to invalidate a subset, so a missed entry would leave a
    /// stale one behind. Evicted keys must not appear.
    #[test]
    fn iter_yields_every_resident_entry() {
        let c = filled(4, 6);
        let mut seen: Vec<u32> = c
            .iter()
            .map(|(k, v)| {
                assert_eq!(*k, v, "key and value must belong to the same entry");
                *k
            })
            .collect();
        seen.sort_unstable();
        assert_eq!(
            seen,
            vec![2, 3, 4, 5],
            "the evicted keys must not be listed"
        );
        c.invalidate(&3);
        let mut seen: Vec<u32> = c.iter().map(|(k, _)| *k).collect();
        seen.sort_unstable();
        assert_eq!(seen, vec![2, 4, 5]);
        c.invalidate_all();
        assert_eq!(c.iter().count(), 0);
    }

    /// `run_pending_tasks` exists only to match moka's call surface; it must
    /// be a no-op that leaves the occupancy unchanged.
    #[test]
    fn run_pending_tasks_is_a_noop() {
        let c = filled(3, 5);
        c.run_pending_tasks();
        assert_eq!(c.entry_count(), 3);
        assert_eq!(c.get(&4), Some(4));
    }

    /// Borrowed lookups: a `String`-keyed cache is queried with `&str`, which
    /// is how the loader uses it.
    #[test]
    fn borrowed_key_lookup() {
        let c: Cache<String, u32> = Cache::new(2);
        c.insert("a".to_owned(), 1);
        assert_eq!(c.get("a"), Some(1));
        c.invalidate("a");
        assert_eq!(c.get("a"), None);
        assert_eq!(c.entry_count(), 0);
    }

    /// The bound is enforced under the lock, so it holds no matter how the
    /// inserts interleave. (Native-only: wasm32 has no threads — the browser
    /// build reaches the cache from a single thread.)
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn bound_holds_under_concurrent_inserts() {
        use std::sync::Arc;
        let c: Arc<Cache<u32, u32>> = Arc::new(Cache::new(16));
        let mut handles = Vec::new();
        for t in 0..8u32 {
            let c = Arc::clone(&c);
            handles.push(std::thread::spawn(move || {
                for i in 0..500u32 {
                    let k = t * 500 + i;
                    c.insert(k, k);
                    let _ = c.get(&k);
                    if k % 97 == 0 {
                        c.invalidate(&k);
                    }
                    assert!(c.entry_count() <= 16, "bound breached: {}", c.entry_count());
                }
            }));
        }
        for h in handles {
            h.join().expect("worker thread");
        }
        assert!(c.entry_count() <= 16);
    }
}
