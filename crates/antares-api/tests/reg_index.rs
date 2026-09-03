// SPDX-License-Identifier: EUPL-1.2
//! Registration matching is index-shaped. `DocMirror` is the federation
//! read path's registration source wherever it is installed (`bus=nats`),
//! so it answers the same question the store answers through
//! `matching_registrations`, and it owes the same two properties:
//!
//! (a) it may never drop a registration 5.12 matching would accept — a
//!     prefilter that under-selects loses a Context Source silently, and
//!     the read still answers 200 as if the federation were complete;
//! (b) it may not degrade into a scan. The broker's target is 100 000+
//!     registrations per broker with index-shaped matching, and a mirror
//!     that walked the tenant per distributed request cost 84 ms of copying
//!     plus the 5.12 evaluation of every registration in it.
//!
//! The shapes that cannot be keyed go in the broad sets: a
//! `RegistrationInfo` naming only attributes (5.2.9 allows one with no
//! `entities`), an `EntityInfo` with no `type` (it restricts by id alone),
//! and an `EntityInfo` with an `idPattern` (5.12 condition 5 forwards on a
//! pattern whatever ids were asked for).

#![allow(clippy::unwrap_used)] // an unwrap here is the assertion

use antares_api::mirror::DocMirror;
use serde_json::{json, Value};

const T: &str = "t1";

fn reg(id: &str, information: Value) -> Value {
    json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "mode": "inclusive",
        "information": information,
        "endpoint": "http://csource.example.test",
    })
}

fn typed(id: &str, ty: &str) -> Value {
    reg(id, json!([{"entities": [{"type": ty}]}]))
}

fn ids_of(docs: &[std::sync::Arc<Value>]) -> Vec<String> {
    let mut v: Vec<String> = docs
        .iter()
        .filter_map(|d| d.get("id").and_then(Value::as_str).map(str::to_owned))
        .collect();
    v.sort();
    v
}

fn one(s: &str) -> Vec<String> {
    vec![s.to_owned()]
}

/// The narrowing itself: a type key reaches its own registrations and no
/// others, and asking nothing of a dimension narrows nothing.
#[test]
fn a_type_key_reaches_its_own_registrations() {
    let m = DocMirror::default();
    m.apply(T, "r:a", Some(typed("r:a", "urn:T:A")));
    m.apply(T, "r:b", Some(typed("r:b", "urn:T:B")));

    assert_eq!(ids_of(&m.matching(T, None, Some(&one("urn:T:A")))), ["r:a"]);
    assert_eq!(ids_of(&m.matching(T, None, Some(&one("urn:T:B")))), ["r:b"]);
    assert!(m.matching(T, None, Some(&one("urn:T:C"))).is_empty());
    assert_eq!(ids_of(&m.matching(T, None, None)), ["r:a", "r:b"]);
}

/// Every shape the index cannot decide has to survive every key, or a
/// distributed read stops reaching a Context Source that 5.12 matches.
#[test]
fn a_shape_the_index_cannot_decide_survives_every_key() {
    let m = DocMirror::default();
    // no `entities` at all: 5.2.9 lets a RegistrationInfo name attributes
    m.apply(
        T,
        "r:attrs",
        Some(reg("r:attrs", json!([{"propertyNames": ["urn:a:speed"]}]))),
    );
    // an EntityInfo with no type: restricted by id alone
    m.apply(
        T,
        "r:idonly",
        Some(reg("r:idonly", json!([{"entities": [{"id": "urn:e:1"}]}]))),
    );
    // an EntityInfo with an idPattern: 5.12 condition 5
    m.apply(
        T,
        "r:pattern",
        Some(reg(
            "r:pattern",
            json!([{"entities": [{"type": "urn:T:A", "idPattern": "urn:e:.*"}]}]),
        )),
    );
    // and one the index CAN decide, as the control
    m.apply(T, "r:typed", Some(typed("r:typed", "urn:T:Z")));

    // a type nothing declares: only the undecidable ones come back
    assert_eq!(
        ids_of(&m.matching(T, None, Some(&one("urn:T:Other")))),
        ["r:attrs", "r:idonly"]
    );
    // an id nothing declares: the pattern and the attribute-only entry stay
    assert_eq!(
        ids_of(&m.matching(T, None, Some(&one("urn:T:A")))),
        ["r:attrs", "r:idonly", "r:pattern"]
    );
    assert_eq!(
        ids_of(&m.matching(T, Some(&one("urn:e:999")), None)),
        ["r:attrs", "r:pattern", "r:typed"]
    );
}

/// Both dimensions given: a registration has to survive both, and the
/// undecidable ones survive whatever is asked.
#[test]
fn both_dimensions_narrow_together() {
    let m = DocMirror::default();
    m.apply(
        T,
        "r:ab",
        Some(reg(
            "r:ab",
            json!([{"entities": [{"type": "urn:T:A", "id": "urn:e:1"}]}]),
        )),
    );
    m.apply(
        T,
        "r:a2",
        Some(reg(
            "r:a2",
            json!([{"entities": [{"type": "urn:T:A", "id": "urn:e:2"}]}]),
        )),
    );

    assert_eq!(
        ids_of(&m.matching(T, Some(&one("urn:e:1")), Some(&one("urn:T:A")))),
        ["r:ab"]
    );
    assert!(m
        .matching(T, Some(&one("urn:e:1")), Some(&one("urn:T:B")))
        .is_empty());

    // Two RegistrationInfo entries, one carrying the type and the other the
    // id: the per-registration membership keeps it, where a per-row filter
    // would drop it. Wider than the store, which is the safe direction.
    m.apply(
        T,
        "r:split",
        Some(reg(
            "r:split",
            json!([
                {"entities": [{"type": "urn:T:A", "id": "urn:e:9"}]},
                {"entities": [{"type": "urn:T:Q", "id": "urn:e:1"}]},
            ]),
        )),
    );
    assert_eq!(
        ids_of(&m.matching(T, Some(&one("urn:e:1")), Some(&one("urn:T:A")))),
        ["r:ab", "r:split"]
    );
}

/// An update re-keys and a delete unfiles — a stale bucket entry would
/// resurrect a deleted Context Source on the next distributed read.
#[test]
fn update_rekeys_and_delete_unfiles() {
    let m = DocMirror::default();
    m.apply(T, "r:m", Some(typed("r:m", "urn:T:A")));
    assert_eq!(ids_of(&m.matching(T, None, Some(&one("urn:T:A")))), ["r:m"]);

    m.apply(T, "r:m", Some(typed("r:m", "urn:T:B")));
    assert!(m.matching(T, None, Some(&one("urn:T:A"))).is_empty());
    assert_eq!(ids_of(&m.matching(T, None, Some(&one("urn:T:B")))), ["r:m"]);

    // through a broad shape and back out again
    m.apply(
        T,
        "r:m",
        Some(reg("r:m", json!([{"entities": [{"id": "urn:e:1"}]}]))),
    );
    assert_eq!(
        ids_of(&m.matching(T, None, Some(&one("urn:T:Anything")))),
        ["r:m"]
    );
    m.apply(T, "r:m", None);
    assert!(m.matching(T, None, None).is_empty());
    assert!(m.matching(T, None, Some(&one("urn:T:Anything"))).is_empty());
    assert!(m.matching(T, Some(&one("urn:e:1")), None).is_empty());
    assert!(m.docs(T).is_empty());
}

#[test]
fn tenants_are_isolated() {
    let m = DocMirror::default();
    m.apply("t1", "r:1", Some(typed("r:1", "urn:T:A")));
    m.apply("t2", "r:2", Some(typed("r:2", "urn:T:A")));
    assert_eq!(
        ids_of(&m.matching("t1", None, Some(&one("urn:T:A")))),
        ["r:1"]
    );
    assert_eq!(
        ids_of(&m.matching("t2", None, Some(&one("urn:T:A")))),
        ["r:2"]
    );
    assert!(m.matching("t3", None, Some(&one("urn:T:A"))).is_empty());
}

/// The safety property against a naive reference: over mixed shapes and
/// keys, the answer is a SUPERSET of every registration whose declared
/// entities could match. The index may over-select; under-selecting loses a
/// Context Source.
#[test]
fn matching_is_a_superset_of_the_naive_reference() {
    let m = DocMirror::default();
    let mut regs = Vec::new();
    for i in 0..200 {
        let id = format!("r:{i}");
        let doc = match i % 5 {
            0 => typed(&id, &format!("urn:T:{}", i % 7)),
            1 => reg(
                &id,
                json!([{"entities": [{"id": format!("urn:e:{}", i % 11)}]}]),
            ),
            2 => reg(
                &id,
                json!([{"entities": [{"type": format!("urn:T:{}", i % 7),
                                      "id": format!("urn:e:{}", i % 11)}]}]),
            ),
            3 => reg(&id, json!([{"entities": [{"idPattern": "urn:e:.*"}]}])),
            _ => reg(&id, json!([{"propertyNames": ["urn:a:speed"]}])),
        };
        m.apply(T, &id, Some(doc.clone()));
        regs.push(doc);
    }
    for case in 0..40 {
        let want_type = format!("urn:T:{}", case % 7);
        let want_id = format!("urn:e:{}", case % 11);
        let got = ids_of(&m.matching(T, Some(&one(&want_id)), Some(&one(&want_type))));
        for doc in &regs {
            let id = doc.get("id").and_then(Value::as_str).unwrap();
            // the naive reference: could ANY declared EntityInfo match?
            let could =
                match doc.get("information").and_then(Value::as_array) {
                    None => true,
                    Some(infos) => infos.iter().any(|info| {
                        match info.get("entities").and_then(Value::as_array) {
                            None => true,
                            Some(es) => es.iter().any(|e| {
                                let ty_ok = match e.get("type").and_then(Value::as_str) {
                                    None => true,
                                    Some(t) => t == want_type,
                                };
                                let id_ok = e.get("idPattern").is_some()
                                    || match e.get("id").and_then(Value::as_str) {
                                        None => true,
                                        Some(i) => i == want_id,
                                    };
                                ty_ok && id_ok
                            }),
                        }
                    }),
                };
            if could {
                assert!(
                    got.iter().any(|g| g == id),
                    "case {case}: registration {id} could match but is not a candidate"
                );
            }
        }
    }
}

/// The scaling gate (run with --release --ignored): a lookup must not scale
/// with the registration count. 100 000 registrations over 50 types is the
/// broker's stated target shape; the 1 000-vs-100 000 ratio is what proves
/// the index is answering instead of a scan wearing an index's name.
#[test]
#[ignore = "release-mode perf gate: cargo test --release -p antares-api --test reg_index -- --ignored"]
fn a_lookup_does_not_scale_with_the_registration_count() {
    let lookup_time = |total: usize| {
        let m = DocMirror::default();
        for i in 0..total {
            let id = format!("r:{i}");
            m.apply(T, &id, Some(typed(&id, &format!("urn:T:{}", i % 50))));
        }
        let want = one("urn:T:absent");
        let start = std::time::Instant::now();
        let mut found = 0usize;
        for _ in 0..1_000 {
            found += m.matching(T, None, Some(&want)).len();
        }
        assert_eq!(found, 0);
        start.elapsed()
    };
    let small = lookup_time(1_000);
    let large = lookup_time(100_000);
    eprintln!("reg index lookup: 1k={small:?} 100k={large:?}");
    assert!(
        large < small * 8,
        "100x the registrations cost {large:?} against {small:?} — the lookup scales with the set"
    );
}
