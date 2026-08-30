// SPDX-License-Identifier: EUPL-1.2
//! The driver contract, as executable code.
//!
//! Every rule here is one a caller in `antares-api` relies on and no
//! backend may decide for itself. A driver that passes both functions can be
//! dropped into the broker; one that does not will break requests in ways
//! its own unit tests are free to miss, because a backend's tests assert
//! what that backend does, not what the seam promises.
//!
//! Both functions PANIC on the first violation, naming the rule. The caller
//! supplies two tenants that already exist (a backend may require a tenant
//! row before it accepts writes) and a prefix that makes the ids of this run
//! unique, so a shared database can host several runs at once.
//!
//! A driver whose calls block on an async runtime (the Postgres arm runs
//! sqlx under `block_in_place`) must be exercised from a multi-threaded
//! runtime context.

use crate::{CurrentStateDriver, CurrentStateDriverExt, Kind, TemporalDriver, TemporalDriverExt};
use antares_model::TenantId;
use serde_json::{json, Value};

fn doc(id: &str) -> Value {
    json!({"id": id, "type": "https://uri.etsi.org/ngsi-ld/default-context/T"})
}

/// Hold a current-state driver to the contract `antares-api` writes against.
///
/// `a` and `b` are two existing tenants; `prefix` namespaces the ids this
/// run creates and deletes. Panics on the first rule broken.
pub fn run_current_state_contract(
    d: &dyn CurrentStateDriver,
    a: &TenantId,
    b: &TenantId,
    prefix: &str,
) {
    let e1 = format!("urn:ngsi-ld:{prefix}:1");
    let e2 = format!("urn:ngsi-ld:{prefix}:2");
    let gone = format!("urn:ngsi-ld:{prefix}:absent");

    // An absent row is absent, not an error and not an empty document.
    assert!(
        d.get(a, Kind::Entity, &gone).expect("get").is_none(),
        "get of an absent row must answer None"
    );
    assert!(
        !d.delete(a, Kind::Entity, &gone).expect("delete"),
        "delete of an absent row must answer false"
    );

    // ADR-0005 / ETSI 047_06: a mutate is a read-modify-write under the row
    // lock, and a missing row is None — NEVER an insert. A get+upsert
    // implementation lets a bookkeeping writeback racing a DELETE resurrect
    // the deleted row.
    let missed = d
        .mutate::<(), ()>(a, Kind::Entity, &gone, |_| Ok(()))
        .expect("mutate");
    assert!(missed.is_none(), "mutate of an absent row must answer None");
    assert!(
        d.get(a, Kind::Entity, &gone).expect("get").is_none(),
        "mutate of an absent row must not insert it (047_06)"
    );

    // create is create-if-absent: the second one is refused and changes
    // nothing.
    assert!(
        d.create(a, Kind::Entity, &e1, doc(&e1)).expect("create"),
        "the first create must report true"
    );
    assert!(
        !d.create(a, Kind::Entity, &e1, doc("urn:overwritten"))
            .expect("create"),
        "a create over an existing id must report false"
    );
    let row = d.get(a, Kind::Entity, &e1).expect("get").expect("row");
    assert_eq!(
        row["id"],
        e1.as_str(),
        "the refused create must not overwrite"
    );

    // A mutate that returns Ok commits; one that returns Err commits nothing.
    let applied = d
        .mutate::<(), ()>(a, Kind::Entity, &e1, |v| {
            v["marker"] = json!(1);
            Ok(())
        })
        .expect("mutate");
    assert_eq!(applied, Some(Ok(())), "a mutate over a present row applies");
    assert_eq!(
        d.get(a, Kind::Entity, &e1).expect("get").expect("row")["marker"],
        json!(1),
        "an accepted mutate must be visible to the next read"
    );
    let rejected = d
        .mutate::<(), &'static str>(a, Kind::Entity, &e1, |v| {
            v["marker"] = json!(2);
            Err("rejected")
        })
        .expect("mutate");
    assert_eq!(
        rejected,
        Some(Err("rejected")),
        "the closure's error crosses back"
    );
    assert_eq!(
        d.get(a, Kind::Entity, &e1).expect("get").expect("row")["marker"],
        json!(1),
        "a rejected mutate must commit nothing"
    );

    // Tenant isolation, on every read and every write path.
    assert!(
        d.get(b, Kind::Entity, &e1).expect("get").is_none(),
        "another tenant must not read the row"
    );
    assert!(
        !d.delete(b, Kind::Entity, &e1).expect("delete"),
        "another tenant must not delete the row"
    );
    assert!(
        d.mutate::<(), ()>(b, Kind::Entity, &e1, |_| Ok(()))
            .expect("mutate")
            .is_none(),
        "another tenant must not mutate the row"
    );
    assert!(
        d.list(b, Kind::Entity)
            .expect("list")
            .iter()
            .all(|r| r["id"] != e1.as_str()),
        "another tenant must not list the row"
    );

    // Kinds are separate namespaces: the same id under two kinds is two rows.
    assert!(
        d.create(a, Kind::Subscription, &e1, doc(&e1))
            .expect("create"),
        "the same id under another kind is a different row"
    );
    assert!(d.delete(a, Kind::Subscription, &e1).expect("delete"));
    assert!(
        d.get(a, Kind::Entity, &e1).expect("get").is_some(),
        "deleting one kind must not touch another"
    );

    // upsert answers whether a document was ALREADY there, and batch_upsert
    // answers the opposite polarity — created-flags, which the batch path
    // needs to split 201 from 204 (5.6.8). Getting one of the two backwards
    // is invisible until a batch reports every create as an update.
    assert!(
        !d.upsert(a, Kind::Entity, &e2, doc(&e2)).expect("upsert"),
        "upsert that created the row reports false"
    );
    assert!(
        d.upsert(a, Kind::Entity, &e2, doc(&e2)).expect("upsert"),
        "upsert over an existing row reports true"
    );
    let e3 = format!("urn:ngsi-ld:{prefix}:3");
    let flags = d
        .batch_upsert(a, vec![(e3.clone(), doc(&e3)), (e2.clone(), doc(&e2))])
        .expect("batch_upsert");
    assert_eq!(
        flags,
        vec![true, false],
        "batch_upsert answers created-flags in input order, the opposite \
         polarity of upsert"
    );
    assert!(d.delete(a, Kind::Entity, &e3).expect("delete"));
    let ids = vec![e2.clone(), gone.clone(), e1.clone()];
    let batch = d
        .batch_mutate::<()>(a, &ids, |_, v| {
            v["batched"] = json!(true);
            Ok(())
        })
        .expect("batch_mutate");
    assert_eq!(batch.len(), ids.len(), "one result per input id");
    assert!(batch[0].is_some(), "results align with ids: {ids:?}");
    assert!(batch[1].is_none(), "an absent id answers None, in place");
    assert!(batch[2].is_some(), "results align with ids: {ids:?}");

    // A query may over-return (the caller re-checks whatever the backend
    // could not decide) but must never drop a matching row, and must never
    // cross a tenant.
    let ids_ref: Vec<&str> = vec![&e1, &e2];
    let mine = d
        .query_entities(
            a,
            &crate::filter::EntityFilter {
                ids: Some(&ids_ref),
                ..Default::default()
            },
        )
        .expect("query_entities");
    for want in [&e1, &e2] {
        assert!(
            mine.rows.iter().any(|r| r["id"] == want.as_str()),
            "query must not drop a matching row: {want}"
        );
    }
    let theirs = d
        .query_entities(
            b,
            &crate::filter::EntityFilter {
                ids: Some(&ids_ref),
                ..Default::default()
            },
        )
        .expect("query_entities");
    assert!(
        theirs.rows.is_empty(),
        "a query must never cross a tenant: {:?}",
        theirs.rows
    );

    // Paging pushdown is optional; claiming it and not doing it is not.
    let paged = d
        .query_entities(
            a,
            &crate::filter::EntityFilter {
                ids: Some(&ids_ref),
                page: Some(crate::filter::Page {
                    offset: 0,
                    limit: 1,
                    count: true,
                }),
                ..Default::default()
            },
        )
        .expect("query_entities");
    if paged.paged {
        assert!(
            paged.rows.len() <= 1,
            "a driver reporting paged=true has applied the LIMIT"
        );
        assert!(
            paged.decided,
            "paged implies decided: a LIMIT over an undecided set pages the wrong rows"
        );
    }

    for id in [&e1, &e2] {
        assert!(
            d.delete(a, Kind::Entity, id).expect("delete"),
            "the contract's own rows must be removable"
        );
    }
}

/// Hold a temporal driver to the same shape. A driver that answers
/// `supported() == false` declares it keeps no history and is held to
/// nothing else.
pub fn run_temporal_contract(d: &dyn TemporalDriver, a: &TenantId, b: &TenantId, prefix: &str) {
    if !d.supported() {
        return;
    }
    let e1 = format!("urn:ngsi-ld:{prefix}:t1");
    let gone = format!("urn:ngsi-ld:{prefix}:tabsent");

    assert!(
        d.get(a, &gone).expect("get").is_none(),
        "get of an absent temporal document must answer None"
    );
    assert!(
        !d.delete(a, &gone).expect("delete"),
        "delete of an absent temporal document must answer false"
    );
    let missed = d.mutate::<(), ()>(a, &gone, |_| Ok(())).expect("mutate");
    assert!(
        missed.is_none(),
        "mutate of an absent document must answer None"
    );
    assert!(
        d.get(a, &gone).expect("get").is_none(),
        "mutate of an absent document must not insert it (047_06)"
    );

    assert!(
        d.create(a, &e1, doc(&e1)).expect("create"),
        "the first create reports true"
    );
    assert!(
        !d.create(a, &e1, doc(&e1)).expect("create"),
        "a create over an existing id reports false"
    );
    let rejected = d
        .mutate::<(), &'static str>(a, &e1, |v| {
            v["marker"] = json!(2);
            Err("rejected")
        })
        .expect("mutate");
    assert_eq!(rejected, Some(Err("rejected")));
    assert!(
        d.get(a, &e1)
            .expect("get")
            .expect("row")
            .get("marker")
            .is_none(),
        "a rejected mutate must commit nothing"
    );

    assert!(
        d.get(b, &e1).expect("get").is_none(),
        "another tenant must not read the temporal document"
    );
    assert!(
        !d.delete(b, &e1).expect("delete"),
        "another tenant must not delete it"
    );
    assert!(
        d.list(b)
            .expect("list")
            .iter()
            .all(|r| r["id"] != e1.as_str()),
        "another tenant must not list it"
    );

    assert!(
        d.delete(a, &e1).expect("delete"),
        "the contract's own rows must be removable"
    );
}
