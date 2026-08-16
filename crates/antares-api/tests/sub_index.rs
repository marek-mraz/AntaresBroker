//! Subscription matching is index-shaped. The `SubMirror`
//! candidate lookup must (a) never miss a subscription that could fire —
//! the safety property, checked against a naive reference — and (b) never
//! degrade into a scan: a change only surfaces the subscriptions keyed to
//! its types/attrs plus the broad bucket. The ignored release bench encodes
//! the scaling gate.

#![allow(clippy::unwrap_used)]

use antares_api::notify::SubMirror;
use serde_json::{json, Value};

const T: &str = "t1";

fn type_sub(id: &str, ty: &str) -> Value {
    json!({"id": id, "type": "Subscription",
           "entities": [{"type": ty}],
           "notification": {"endpoint": {"uri": "http://x/"}}})
}

fn watched_sub(id: &str, attr: &str) -> Value {
    json!({"id": id, "type": "Subscription",
           "watchedAttributes": [attr],
           "notification": {"endpoint": {"uri": "http://x/"}}})
}

fn ids(subs: &[Value]) -> Vec<&str> {
    let mut v: Vec<&str> = subs
        .iter()
        .filter_map(|s| s.get("id").and_then(Value::as_str))
        .collect();
    v.sort_unstable();
    v
}

#[test]
fn type_indexed_subs_surface_only_for_their_type() {
    let m = SubMirror::default();
    m.apply(T, "s:a", Some(type_sub("s:a", "urn:T:A")));
    m.apply(T, "s:b", Some(type_sub("s:b", "urn:T:B")));

    assert_eq!(ids(&m.candidates(T, &["urn:T:A"], &[])), vec!["s:a"]);
    assert_eq!(ids(&m.candidates(T, &["urn:T:B"], &[])), vec!["s:b"]);
    assert!(m.candidates(T, &["urn:T:C"], &[]).is_empty());
    // multi-typed entity unions the buckets
    assert_eq!(
        ids(&m.candidates(T, &["urn:T:A", "urn:T:B"], &[])),
        vec!["s:a", "s:b"]
    );
}

#[test]
fn watched_attr_subs_surface_only_when_a_watched_attr_changed() {
    let m = SubMirror::default();
    m.apply(T, "s:w", Some(watched_sub("s:w", "urn:attr:temperature")));

    assert!(m
        .candidates(T, &["urn:T:A"], &["urn:attr:humidity"])
        .is_empty());
    assert_eq!(
        ids(&m.candidates(T, &["urn:T:A"], &["urn:attr:temperature"])),
        vec!["s:w"]
    );
}

#[test]
fn selection_expressions_go_broad_and_always_surface() {
    let m = SubMirror::default();
    // 4.17 expressions the index cannot prove — must be in EVERY candidate set
    m.apply(T, "s:x", Some(type_sub("s:x", "urn:T:A|urn:T:B")));
    m.apply(T, "s:y", Some(type_sub("s:y", "(urn:T:A;urn:T:B)")));

    assert_eq!(
        ids(&m.candidates(T, &["urn:T:Unrelated"], &[])),
        vec!["s:x", "s:y"]
    );
}

#[test]
fn a_sub_with_selector_and_watched_attrs_is_keyed_by_type_only() {
    let m = SubMirror::default();
    let sub = json!({"id": "s:tw", "type": "Subscription",
        "entities": [{"type": "urn:T:A"}],
        "watchedAttributes": ["urn:attr:temperature"],
        "notification": {"endpoint": {"uri": "http://x/"}}});
    m.apply(T, "s:tw", Some(sub));

    // type match surfaces it even when the change touched other attrs —
    // trigger/watched filtering is evaluation's job, not the index's
    assert_eq!(
        ids(&m.candidates(T, &["urn:T:A"], &["urn:attr:humidity"])),
        vec!["s:tw"]
    );
    // wrong type: never a candidate, whatever changed
    assert!(m
        .candidates(T, &["urn:T:B"], &["urn:attr:temperature"])
        .is_empty());
}

#[test]
fn update_rekeys_and_delete_removes() {
    let m = SubMirror::default();
    m.apply(T, "s:m", Some(type_sub("s:m", "urn:T:A")));
    assert_eq!(ids(&m.candidates(T, &["urn:T:A"], &[])), vec!["s:m"]);

    // update A → B: old key gone, new key live
    m.apply(T, "s:m", Some(type_sub("s:m", "urn:T:B")));
    assert!(m.candidates(T, &["urn:T:A"], &[]).is_empty());
    assert_eq!(ids(&m.candidates(T, &["urn:T:B"], &[])), vec!["s:m"]);

    // update to a broad shape, then delete: nothing left anywhere
    m.apply(T, "s:m", Some(type_sub("s:m", "urn:T:A|urn:T:B")));
    m.apply(T, "s:m", None);
    assert!(m.candidates(T, &["urn:T:A", "urn:T:B"], &[]).is_empty());
    assert!(m.docs(T).is_empty());
    assert!(m.tenants().is_empty());
}

#[test]
fn tenants_are_isolated() {
    let m = SubMirror::default();
    m.apply("t1", "s:1", Some(type_sub("s:1", "urn:T:A")));
    m.apply("t2", "s:2", Some(type_sub("s:2", "urn:T:A")));
    assert_eq!(ids(&m.candidates("t1", &["urn:T:A"], &[])), vec!["s:1"]);
    assert_eq!(ids(&m.candidates("t2", &["urn:T:A"], &[])), vec!["s:2"]);
}

/// Safety property against a naive reference: for randomized subscription
/// shapes and changes, the candidate set is a SUPERSET of every sub whose
/// structural pre-condition could hold. The index may over-select (broad
/// bucket), never under-select.
#[test]
fn candidates_are_a_superset_of_the_naive_reference() {
    let m = SubMirror::default();
    let mut subs = Vec::new();
    for i in 0..200 {
        let sub = match i % 4 {
            0 => type_sub(&format!("s:{i}"), &format!("urn:T:{}", i % 7)),
            1 => watched_sub(&format!("s:{i}"), &format!("urn:a:{}", i % 5)),
            2 => type_sub(
                &format!("s:{i}"),
                &format!("urn:T:{}|urn:T:{}", i % 7, (i + 1) % 7),
            ),
            _ => json!({"id": format!("s:{i}"), "type": "Subscription",
                        "notification": {"endpoint": {"uri": "http://x/"}}}),
        };
        m.apply(T, &format!("s:{i}"), Some(sub.clone()));
        subs.push(sub);
    }
    for case in 0..50 {
        let types = [format!("urn:T:{}", case % 7)];
        let changed = [format!("urn:a:{}", case % 5)];
        let tref: Vec<&str> = types.iter().map(String::as_str).collect();
        let cref: Vec<&str> = changed.iter().map(String::as_str).collect();
        let cands = m.candidates(T, &tref, &cref);
        let got = ids(&cands);
        for sub in &subs {
            let id = sub.get("id").and_then(Value::as_str).unwrap();
            let must_surface = match (
                sub.get("entities").and_then(Value::as_array),
                sub.get("watchedAttributes").and_then(Value::as_array),
            ) {
                (Some(sel), _) => sel.iter().any(|e| {
                    let t = e.get("type").and_then(Value::as_str).unwrap();
                    // expressions must always surface; plain types on match
                    t.contains(['|', ',', ';', '(']) || types.iter().any(|ty| ty == t)
                }),
                (None, Some(w)) => w
                    .iter()
                    .filter_map(Value::as_str)
                    .any(|a| changed.iter().any(|c| c == a)),
                (None, None) => true,
            };
            if must_surface {
                assert!(
                    got.binary_search(&id).is_ok(),
                    "case {case}: sub {id} could fire but is not a candidate"
                );
            }
        }
    }
}

/// The scaling gate (run with --release --ignored):
/// candidate lookup must not scale with the total subscription count. With
/// 10k type-keyed subscriptions across 1k types, one lookup touches ~10
/// docs; 100k lookups in well under a second proves the index is doing the
/// work, and the 1k-vs-10k timing ratio proves it is not a hidden scan.
#[test]
#[ignore = "release-mode perf gate: cargo test --release -p antares-api --test sub_index -- --ignored"]
fn ten_thousand_subs_lookup_does_not_scale_with_sub_count() {
    let lookup_time = |total: usize| {
        let m = SubMirror::default();
        for i in 0..total {
            let ty = format!("urn:T:{}", i % (total / 10)); // ~10 subs per type
            m.apply(T, &format!("s:{i}"), Some(type_sub(&format!("s:{i}"), &ty)));
        }
        let start = std::time::Instant::now();
        let mut found = 0usize;
        for i in 0..100_000usize {
            let ty = format!("urn:T:{}", i % (total / 10));
            found += m.candidates(T, &[ty.as_str()], &[]).len();
        }
        // sanity: every lookup returned exactly its type's ~10-sub bucket
        assert_eq!(found, 100_000 * 10);
        start.elapsed()
    };
    let t1k = lookup_time(1_000);
    let t10k = lookup_time(10_000);
    eprintln!("100k lookups: 1k subs = {t1k:?}, 10k subs = {t10k:?}");
    assert!(
        t10k < std::time::Duration::from_secs(2),
        "100k candidate lookups at 10k subs took {t10k:?} — the index is not index-shaped"
    );
    // 10× the subscriptions must not cost anywhere near 10× the lookup —
    // 3× headroom absorbs allocator noise while still damning a scan.
    assert!(
        t10k < t1k * 3,
        "lookup scales with sub count (1k: {t1k:?} → 10k: {t10k:?}) — scan smell"
    );
}
