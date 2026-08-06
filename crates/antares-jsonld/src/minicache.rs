//! N2 (wasm32 only): a bounded map with moka's call surface. moka's clock
//! calls `std::time::Instant::now()` unconditionally, which panics on
//! wasm32-unknown-unknown — so the browser build swaps in this FIFO-evicting
//! cache behind the same six methods the loader uses. Bounded is the
//! requirement (R4 rule: every cache has a max size); recency-optimal
//! eviction is not — a browser tab's @context set is tiny.
// ponytail: FIFO eviction; port moka's TinyLFU here only if a browser
// workload ever shows cache thrash.

use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
use std::sync::Mutex;

pub struct Cache<K, V> {
    cap: usize,
    inner: Mutex<(HashMap<K, V>, VecDeque<K>)>,
}

impl<K: Eq + Hash + Clone, V: Clone> Cache<K, V> {
    pub fn new(cap: u64) -> Self {
        Self {
            cap: cap as usize,
            inner: Mutex::new((HashMap::new(), VecDeque::new())),
        }
    }

    pub fn get<Q>(&self, key: &Q) -> Option<V>
    where
        K: std::borrow::Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        self.inner.lock().expect("minicache lock").0.get(key).cloned()
    }

    pub fn insert(&self, key: K, value: V) {
        let mut g = self.inner.lock().expect("minicache lock");
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
        let mut g = self.inner.lock().expect("minicache lock");
        let (map, order) = &mut *g;
        map.remove(key);
        order.retain(|k| map.contains_key::<K>(k));
    }

    pub fn invalidate_all(&self) {
        let mut g = self.inner.lock().expect("minicache lock");
        g.0.clear();
        g.1.clear();
    }

    pub fn run_pending_tasks(&self) {}

    pub fn entry_count(&self) -> u64 {
        self.inner.lock().expect("minicache lock").0.len() as u64
    }
}
