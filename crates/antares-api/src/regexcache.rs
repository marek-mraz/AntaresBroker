//! Process-wide cache of compiled regular expressions.
//!
//! Two NGSI-LD surfaces carry a client-supplied regular expression. The query
//! language, 4.9 **Match pattern** (production rule `patternOp`): "A matching
//! entity shall contain the target element and the target value shall be in
//! the L(R) of the regular pattern specified by the Query Term" — and its
//! `notPatternOp` mirror. And `idPattern`, on an EntitySelector (5.2.33), an
//! EntityInfo (5.2.8) and the query parameters of Table 6.4.3.2-1.
//!
//! Both are evaluated per candidate entity and, for a subscription, per event
//! per subscription — while the pattern text belongs to the query or the
//! subscription, not to the candidate. Compiling at the point of use
//! therefore pays `Regex::new` again for every candidate; compiling through
//! here pays it once per distinct pattern and hands out a shared program.
//!
//! The cache changes no outcome. `compile` accepts and rejects exactly what
//! `regex::Regex::new` accepts and rejects, and returns that call's own
//! `regex::Error`, so an invalid `idPattern` keeps the 400 BadRequestData its
//! call site already returns (Table 6.3.2-1) and an invalid `~=` operand
//! keeps having no L(R), i.e. matching nothing (4.9).
//!
//! Retention is bounded in both dimensions — entries and compiled program
//! size, `bounds::MAX_REGEX_CACHE` and `bounds::MAX_REGEX_PROGRAM_BYTES` —
//! because the key is client input and an unbounded map of it is a memory
//! attack, not a cache.

use crate::bounds::{MAX_REGEX_CACHE, MAX_REGEX_PROGRAM_BYTES};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, RwLock};

static CACHE: LazyLock<RwLock<HashMap<Box<str>, Arc<regex::Regex>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));
static COMPILES: AtomicU64 = AtomicU64::new(0);

/// The compiled program for `pattern`, shared with every other caller that
/// asked for the same pattern text. `Err` is `Regex::new(pattern)`'s error,
/// unchanged, so each call site keeps mapping it to its own spec error.
pub fn compile(pattern: &str) -> Result<Arc<regex::Regex>, regex::Error> {
    if let Some(hit) = cached(pattern) {
        return Ok(hit);
    }
    COMPILES.fetch_add(1, Ordering::Relaxed);
    // Compiled OUTSIDE the lock — a pattern compile must never serialize the
    // matcher — and a poisoned lock degrades to "compile every time", never
    // to a failed request.
    match regex::RegexBuilder::new(pattern)
        .size_limit(MAX_REGEX_PROGRAM_BYTES)
        .build()
    {
        Ok(re) => {
            let re = Arc::new(re);
            if let Ok(mut c) = CACHE.write() {
                // A generational flush, not an LRU: the patterns the next
                // requests use re-enter immediately, and the hit path pays no
                // bookkeeping for the eviction order.
                if c.len() >= MAX_REGEX_CACHE {
                    c.clear();
                }
                c.insert(pattern.into(), Arc::clone(&re));
            }
            Ok(re)
        }
        // Either the pattern is invalid — the error is the same under any
        // size limit — or its program is above the retention ceiling. Compile
        // it with the crate's own default limit so what is accepted stays
        // exactly what `Regex::new` accepts, and leave it out of the cache.
        Err(_) => regex::Regex::new(pattern).map(Arc::new),
    }
}

/// The retained program for `pattern`, without compiling anything.
pub fn cached(pattern: &str) -> Option<Arc<regex::Regex>> {
    CACHE.read().ok().and_then(|c| c.get(pattern).cloned())
}

/// Compilations performed since process start (i.e. cache misses).
pub fn compiles() -> u64 {
    COMPILES.load(Ordering::Relaxed)
}

/// Patterns currently retained — never above `bounds::MAX_REGEX_CACHE`.
pub fn len() -> usize {
    CACHE.read().map(|c| c.len()).unwrap_or(0)
}

static Q_CACHE: LazyLock<RwLock<HashMap<Box<str>, Arc<antares_ql::QNode>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));
static GEO_CACHE: LazyLock<RwLock<HashMap<Box<str>, Arc<crate::geo::GeoQuery>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// The parsed 4.9 query for `q`, shared across every event that evaluates the
/// same subscription. Like the regex cache this changes no outcome: only an
/// `Ok` parse is retained, so an invalid or over-complex `q` keeps exactly
/// the handling its call site has (`parse_q` is re-run and its error stands).
/// Entry size is already capped by the parser's own complexity ceiling
/// (`MAX_Q_NODES`), entry count by the same generational flush as above.
pub fn q_node(q: &str) -> Option<Arc<antares_ql::QNode>> {
    if let Some(hit) = Q_CACHE.read().ok().and_then(|c| c.get(q).cloned()) {
        return Some(hit);
    }
    let node = Arc::new(antares_ql::parse_q(q).ok()?);
    if let Ok(mut c) = Q_CACHE.write() {
        if c.len() >= MAX_REGEX_CACHE {
            c.clear();
        }
        c.insert(q.into(), Arc::clone(&node));
    }
    Some(node)
}

/// The parsed geoquery (4.10) for a subscription's `geoQ`, keyed by the
/// caller's serialization of that member. `build` runs on a miss and only a
/// `Some` is retained — a `geoQ` that does not parse keeps failing exactly as
/// before, per call. Geometry size is already capped at the parse
/// (`MAX_GEO_VERTICES`), entry count by the generational flush.
pub fn geo_query(
    key: &str,
    build: impl FnOnce() -> Option<crate::geo::GeoQuery>,
) -> Option<Arc<crate::geo::GeoQuery>> {
    if let Some(hit) = GEO_CACHE.read().ok().and_then(|c| c.get(key).cloned()) {
        return Some(hit);
    }
    let gq = Arc::new(build()?);
    if let Ok(mut c) = GEO_CACHE.write() {
        if c.len() >= MAX_REGEX_CACHE {
            c.clear();
        }
        c.insert(key.into(), Arc::clone(&gq));
    }
    Some(gq)
}

/// Test-only serialization. The bounded-retention test deliberately flushes
/// the shared cache, which would otherwise race every test that asserts what
/// is retained — including the query-term test in `qeval`.
#[cfg(test)]
pub(crate) fn serial_lock() -> std::sync::MutexGuard<'static, ()> {
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
    SERIAL.lock().unwrap_or_else(|poison| poison.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The same pattern text is compiled once: the second call gets the very
    /// program the first one built (4.9 patternOp / 5.2.33 idPattern are
    /// evaluated per candidate, so this identity is the whole point).
    #[test]
    fn same_pattern_is_compiled_once() {
        let _serial = serial_lock();
        let p = "^urn:ngsi-ld:Vehicle:compiled-once-[0-9]+$";
        assert!(cached(p).is_none(), "cold pattern must not start retained");
        let before = compiles();
        let first = compile(p).expect("valid pattern");
        assert!(compiles() > before, "a cold pattern is compiled");
        let second = compile(p).expect("valid pattern");
        assert!(
            Arc::ptr_eq(&first, &second),
            "the second call must reuse the compiled program, not rebuild it"
        );
        assert!(
            cached(p).is_some_and(|c| Arc::ptr_eq(&c, &first)),
            "the cache holds that same program"
        );
        assert!(first.is_match("urn:ngsi-ld:Vehicle:compiled-once-7"));
        assert!(
            !first.is_match("urn:ngsi-ld:Vehicle:compiled-once-x"),
            "a shared program still rejects what the pattern excludes"
        );
    }

    /// An invalid pattern yields `Regex::new`'s own error, verbatim, so the
    /// spec error each call site maps it to is unchanged — an `idPattern`
    /// stays 400 BadRequestData (Table 6.3.2-1) and never becomes a 500.
    /// Nothing invalid is retained.
    #[test]
    fn invalid_pattern_keeps_the_error_regex_new_returns() {
        let _serial = serial_lock();
        for p in ["[", "(", "a{2,1}", "(?P<", "*", "\\", "(?"] {
            let want = regex::Regex::new(p).map_err(|e| e.to_string());
            let got = compile(p).map(|_| ()).map_err(|e| e.to_string());
            assert!(want.is_err(), "fixture must be an invalid pattern: {p:?}");
            assert_eq!(
                got.map(|_| ()),
                want.map(|_| ()),
                "the cache must return the same error text for {p:?}"
            );
            assert!(cached(p).is_none(), "an invalid pattern is never retained");
            assert!(
                compile(p).is_err(),
                "and it stays an error on the second call for {p:?}"
            );
        }
        assert!(compile("^ok$").is_ok(), "a valid pattern is not rejected");
    }

    /// Retention is bounded: distinct patterns are client input, so many of
    /// them must not grow the map past `bounds::MAX_REGEX_CACHE`, and the
    /// programs handed out across an eviction stay correct.
    #[test]
    fn cache_stays_bounded_under_many_distinct_patterns() {
        let _serial = serial_lock();
        for i in 0..MAX_REGEX_CACHE * 3 {
            let p = format!("^bounded-{i}-[a-z]+$");
            let re = compile(&p).expect("valid pattern");
            assert!(re.is_match(&format!("bounded-{i}-abc")));
            assert!(!re.is_match(&format!("bounded-{i}-123")));
            assert!(
                len() <= MAX_REGEX_CACHE,
                "retained {} patterns after {i} distinct ones",
                len()
            );
        }
        assert!(len() > 0, "the cache must still be caching after a flush");
    }

    /// A program above the retention ceiling is still compiled — acceptance
    /// is `Regex::new`'s, not the cache's — but it is not retained, so the
    /// 32 MiB worst case documented in `bounds` holds.
    #[test]
    fn oversized_program_compiles_but_is_not_retained() {
        let _serial = serial_lock();
        let p = format!("^(?:{})$", "a{255}".repeat(64));
        assert!(
            regex::RegexBuilder::new(&p)
                .size_limit(MAX_REGEX_PROGRAM_BYTES)
                .build()
                .is_err(),
            "fixture must exceed the retention ceiling"
        );
        assert!(regex::Regex::new(&p).is_ok(), "…while still being valid");
        let first = compile(&p).expect("valid pattern");
        let second = compile(&p).expect("valid pattern");
        assert!(cached(&p).is_none(), "an oversized program is not retained");
        assert!(
            !Arc::ptr_eq(&first, &second),
            "each call compiles its own — nothing is shared, nothing is held"
        );
        assert!(first.is_match(&"a".repeat(255 * 64)));
        assert!(!first.is_match("a"), "and it is the pattern that was asked");
    }

    /// The same q text parses once and every caller shares the tree; an
    /// invalid or over-complex q is parsed as before and never retained, so
    /// its call-site handling (no-match / 400) is unchanged.
    #[test]
    fn q_text_is_parsed_once_and_bad_q_is_not_retained() {
        let _serial = serial_lock();
        let q = r#"speed>20;brandName=="cache-q-probe""#;
        let first = q_node(q).expect("valid q");
        let second = q_node(q).expect("valid q");
        assert!(
            Arc::ptr_eq(&first, &second),
            "the second evaluation must reuse the parsed tree"
        );
        for bad in ["", "==5", "a==\"unterminated", "a==1)"] {
            assert!(q_node(bad).is_none(), "invalid q {bad:?} must not parse");
            assert!(
                Q_CACHE.read().is_ok_and(|c| !c.contains_key(bad)),
                "invalid q {bad:?} must not be retained"
            );
        }
        // boundedness under distinct client q texts
        for i in 0..MAX_REGEX_CACHE + 64 {
            q_node(&format!("qbound{i}>0")).expect("valid q");
            assert!(
                Q_CACHE.read().map(|c| c.len()).unwrap_or(usize::MAX) <= MAX_REGEX_CACHE,
                "q cache must stay bounded"
            );
        }
    }

    /// The geo cache shares one parsed geometry per distinct geoQ key, and a
    /// geoQ whose build fails is rebuilt (and re-fails) per call — never a
    /// cached wrong answer.
    #[test]
    fn geo_query_is_shared_per_key_and_failures_are_not_retained() {
        let _serial = serial_lock();
        let build_calls = std::sync::atomic::AtomicUsize::new(0);
        let params = || {
            let mut m = HashMap::new();
            m.insert("georel".to_owned(), "near;maxDistance==2000".to_owned());
            m.insert("geometry".to_owned(), "Point".to_owned());
            m.insert("coordinates".to_owned(), "[13.38,52.52]".to_owned());
            m
        };
        let build = || {
            build_calls.fetch_add(1, Ordering::Relaxed);
            crate::geo::GeoQuery::from_params(&params()).ok().flatten()
        };
        let first = geo_query("geo-probe-1", build).expect("valid geoQ");
        let second = geo_query("geo-probe-1", build).expect("valid geoQ");
        assert!(Arc::ptr_eq(&first, &second), "one parse per key");
        assert_eq!(
            build_calls.load(Ordering::Relaxed),
            1,
            "the second call must not rebuild"
        );
        assert!(
            geo_query("geo-probe-bad", || None).is_none(),
            "a failing build yields None"
        );
        assert!(
            geo_query("geo-probe-bad", build).is_some(),
            "…and is not retained as a failure: the next build runs"
        );
    }

    /// Concurrent use: the same and different patterns compiled from many
    /// threads at once must stay correct and bounded, never deadlock, and
    /// never hand out a program built for another pattern.
    #[test]
    fn concurrent_compiles_are_safe() {
        let _serial = serial_lock();
        let pats = [
            ("^shared-a-[0-9]+$", "shared-a-1", "shared-a-x"),
            ("^shared-b-[a-z]+$", "shared-b-z", "shared-b-9"),
            ("shared-c", "xx-shared-c-xx", "shared-d"),
        ];
        let handles: Vec<_> = (0..8)
            .map(|t| {
                std::thread::spawn(move || {
                    for round in 0..200 {
                        let (p, hit, miss) = pats[round % pats.len()];
                        let re = compile(p).expect("valid pattern");
                        assert!(re.is_match(hit), "thread {t} pattern {p}");
                        assert!(!re.is_match(miss), "thread {t} pattern {p}");
                        // a per-thread pattern exercises concurrent inserts
                        let own = format!("^t{t}-r{round}-[0-9]+$");
                        assert!(compile(&own)
                            .expect("valid")
                            .is_match(&format!("t{t}-r{round}-5")));
                        assert!(compile("[").is_err(), "invalid stays invalid");
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().expect("no thread panicked");
        }
        assert!(len() <= MAX_REGEX_CACHE, "still bounded after the race");
        assert!(
            cached("shared-c").is_some(),
            "a hot pattern survives concurrent use"
        );
    }
}
