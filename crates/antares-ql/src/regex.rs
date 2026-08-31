// SPDX-License-Identifier: EUPL-1.2
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
//! One compile has to be bounded and a pattern has to be compiled at most
//! once, because both numbers are multiplied by the candidate count.
//! `regex` builds an automaton whose size follows counted repetition of a
//! character class rather than pattern length, so a 21-byte pattern can ask
//! for a 16 MiB program and the tenth of a second that costs;
//! `MAX_REGEX_PROGRAM_BYTES` is the ceiling on that, and a pattern above it
//! is refused with the builder's own error — the error every call site
//! already maps, so an over-large `idPattern` keeps the 400 BadRequestData
//! its call site returns (Table 6.3.2-1) and an over-large `~=` operand
//! keeps having no L(R), i.e. matching nothing (4.9), exactly as a
//! syntactically invalid one does.
//!
//! Every outcome is retained, refusals included, so no pattern is compiled
//! twice. Retention is bounded in entries and in bytes —
//! `MAX_REGEX_CACHE` and `MAX_REGEX_CACHE_BYTES` — because the key is
//! client input and an unbounded map of it is a memory attack, not a cache.

/// Ceiling on the automaton compiled for one pattern, and with it on what
/// one compile costs. The pattern is client input — the
/// `patternOp`/`notPatternOp` operand of a query term (4.9) and the
/// `idPattern` of an EntitySelector (5.2.33), an EntityInfo (5.2.8) or a
/// query parameter (Table 6.4.3.2-1) — and program size does not follow
/// pattern length: `(?:\p{Any}{100}){100}` is 21 bytes and compiles to
/// 16 MiB. Ordinary patterns sit far below the ceiling —
/// `^urn:ngsi-ld:Vehicle:.*$` compiles to 2 KiB,
/// `^urn:ngsi-ld:(Vehicle|Sensor|Building):[A-Za-z0-9_-]{1,64}$` to 16 KiB,
/// a fifty-way alternation of URNs to 128 KiB — and one above it is refused
/// rather than compiled, so no request can buy an unbounded automaton.
pub const MAX_REGEX_PROGRAM_BYTES: usize = 256 * 1024;
/// Distinct patterns retained. A `q` is capped at 4 KiB and so carries at
/// most a low hundreds of distinct patterns, which the cache holds whole:
/// one request never evicts its own working set.
pub const MAX_REGEX_CACHE: usize = 1024;
/// Retained program bytes. Each entry is charged the tier it compiled
/// within, not its true size, so the number is an upper bound on what the
/// map holds.
pub const MAX_REGEX_CACHE_BYTES: usize = 64 * 1024 * 1024;

/// A program that fits this is charged this much against
/// [`MAX_REGEX_CACHE_BYTES`]; anything larger is charged the full
/// [`MAX_REGEX_PROGRAM_BYTES`]. Almost every pattern a deployment writes
/// lands in the first tier, so the byte budget holds thousands of
/// Subscription `idPattern`s and still bounds a mix of the largest programs
/// the ceiling admits.
const PROGRAM_TIER_BYTES: usize = 32 * 1024;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, RwLock};

/// What a pattern compiled to, or the refusal it earned. A refusal is
/// retained as its message so the pattern is not rebuilt per candidate.
type Outcome = Result<Arc<regex::Regex>, Box<str>>;

static CACHE: LazyLock<RwLock<HashMap<Box<str>, Outcome>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));
static COMPILES: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);

fn build(pattern: &str, limit: usize) -> Result<regex::Regex, regex::Error> {
    regex::RegexBuilder::new(pattern).size_limit(limit).build()
}

/// The compiled program for `pattern`, shared with every other caller that
/// asked for the same pattern text. `Err` carries the builder's own message
/// — a syntax error, or the refusal of a program above
/// [`MAX_REGEX_PROGRAM_BYTES`] — and each call site keeps mapping it to its
/// own spec error.
pub fn compile(pattern: &str) -> Result<Arc<regex::Regex>, String> {
    if let Some(hit) = CACHE.read().ok().and_then(|c| c.get(pattern).cloned()) {
        return hit.map_err(String::from);
    }
    COMPILES.fetch_add(1, Ordering::Relaxed);
    // Compiled OUTSIDE the lock — a pattern compile must never serialize the
    // matcher — and a poisoned lock degrades to "compile every time", never
    // to a failed request.
    let (outcome, charge): (Outcome, usize) = match build(pattern, PROGRAM_TIER_BYTES) {
        Ok(re) => (Ok(Arc::new(re)), PROGRAM_TIER_BYTES),
        Err(_) => match build(pattern, MAX_REGEX_PROGRAM_BYTES) {
            Ok(re) => (Ok(Arc::new(re)), MAX_REGEX_PROGRAM_BYTES),
            // A refusal retains a message, not a program.
            Err(e) => (Err(e.to_string().into()), 0),
        },
    };
    if let Ok(mut c) = CACHE.write() {
        // A generational flush, not an LRU: the patterns the next requests
        // use re-enter immediately, and the hit path pays no bookkeeping for
        // the eviction order.
        if c.len() >= MAX_REGEX_CACHE
            || BYTES.load(Ordering::Relaxed).saturating_add(charge) > MAX_REGEX_CACHE_BYTES
        {
            c.clear();
            BYTES.store(0, Ordering::Relaxed);
        }
        BYTES.fetch_add(charge, Ordering::Relaxed);
        c.insert(pattern.into(), outcome.clone());
    }
    outcome.map_err(String::from)
}

/// The retained program for `pattern`, without compiling anything. A
/// retained refusal holds no program and reads as `None` here.
pub fn cached(pattern: &str) -> Option<Arc<regex::Regex>> {
    CACHE
        .read()
        .ok()
        .and_then(|c| c.get(pattern).and_then(|o| o.as_ref().ok()).cloned())
}

/// Compilations performed since process start (i.e. cache misses).
pub fn compiles() -> u64 {
    COMPILES.load(Ordering::Relaxed)
}

/// Patterns currently retained — never above `MAX_REGEX_CACHE`.
pub fn len() -> usize {
    CACHE.read().map(|c| c.len()).unwrap_or(0)
}

/// Program bytes currently charged — never above `MAX_REGEX_CACHE_BYTES`.
pub fn retained_bytes() -> usize {
    BYTES.load(Ordering::Relaxed)
}

static Q_CACHE: LazyLock<RwLock<HashMap<Box<str>, Arc<crate::QNode>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));
static GEO_CACHE: LazyLock<RwLock<HashMap<Box<str>, Arc<crate::geo::GeoQuery>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// The parsed 4.9 query for `q`, shared across every event that evaluates the
/// same subscription. Like the regex cache this changes no outcome: only an
/// `Ok` parse is retained, so an invalid or over-complex `q` keeps exactly
/// the handling its call site has (`parse_q` is re-run and its error stands).
/// Entry size is already capped by the parser's own complexity ceiling
/// (`MAX_Q_NODES`), entry count by the same generational flush as above.
pub fn q_node(q: &str) -> Option<Arc<crate::QNode>> {
    if let Some(hit) = Q_CACHE.read().ok().and_then(|c| c.get(q).cloned()) {
        return Some(hit);
    }
    let node = Arc::new(crate::parse_q(q).ok()?);
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

/// Test-only serialization: a test that flushes the shared cache must not
/// race the tests asserting what is retained.
#[cfg(test)]
pub(crate) fn serial_lock() -> std::sync::MutexGuard<'static, ()> {
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
    SERIAL.lock().unwrap_or_else(|poison| poison.into_inner())
}

// regex compiles run for hours under Miri; the fuzz job covers them
#[cfg(all(test, not(miri)))]
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
    /// What is retained for it is the refusal, never a program.
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
            assert!(cached(p).is_none(), "an invalid pattern yields no program");
            assert!(
                compile(p).is_err(),
                "and it stays an error on the second call for {p:?}"
            );
        }
        assert!(compile("^ok$").is_ok(), "a valid pattern is not rejected");
    }

    /// Retention is bounded in both dimensions: distinct patterns are
    /// client input, so many of them must grow the map past neither
    /// `MAX_REGEX_CACHE` nor `MAX_REGEX_CACHE_BYTES`, and the programs
    /// handed out across an eviction stay correct.
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
            assert!(
                retained_bytes() <= MAX_REGEX_CACHE_BYTES,
                "charged {} bytes after {i} distinct patterns",
                retained_bytes()
            );
        }
        assert!(len() > 0, "the cache must still be caching after a flush");
    }

    /// 4.9 evaluates a `patternOp` per candidate Entity and 5.2.33 an
    /// `idPattern` per event per Subscription, so an unbounded compile is
    /// multiplied by the candidate count. Program size follows counted
    /// repetition of a character class, not pattern length, so the bound
    /// has to be on the program: above the ceiling the pattern is refused,
    /// and the refusal is remembered so it costs one build pass, ever.
    #[test]
    fn a_program_above_the_ceiling_is_refused_once_not_rebuilt_per_candidate() {
        let _serial = serial_lock();
        let p = r"(?:\p{Any}{100}){100}";
        assert_eq!(p.len(), 21, "the fixture is what a client can send");
        assert!(
            build(p, MAX_REGEX_PROGRAM_BYTES).is_err(),
            "fixture must ask for a program above the ceiling"
        );
        let before = compiles();
        assert!(
            compile(p).is_err(),
            "a program above the ceiling is refused, not compiled"
        );
        let after = compiles();
        assert_eq!(after, before + 1, "one refusal costs one build pass");
        for _ in 0..64 {
            assert!(compile(p).is_err(), "and it stays refused");
        }
        assert_eq!(
            compiles(),
            after,
            "the refusal is remembered, not rebuilt for the next candidate"
        );
        assert!(cached(p).is_none(), "a refusal retains no program");
    }

    /// The ceiling bounds the automaton, not the expressiveness a
    /// deployment needs: the patterns an `idPattern` or a `~=` operand
    /// actually carries compile well inside it and are retained.
    #[test]
    fn the_ceiling_admits_the_patterns_a_deployment_writes() {
        let _serial = serial_lock();
        for p in [
            r"^urn:ngsi-ld:Vehicle:.*$",
            r"^urn:ngsi-ld:(Vehicle|Sensor|Building):[A-Za-z0-9_-]{1,64}$",
            r"^urn:ngsi-ld:Device:[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$",
            r"(?i)^urn:ngsi-ld:vehicle:.*$",
            r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$",
        ] {
            assert!(compile(p).is_ok(), "the ceiling must not refuse {p:?}");
            assert!(
                cached(p).is_some(),
                "what the ceiling admits, the cache retains: {p:?}"
            );
        }
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
