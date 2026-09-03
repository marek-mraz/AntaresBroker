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
//! Both functions are `async`: a driver is awaited, never blocked on.

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
pub async fn run_current_state_contract(
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
        d.get(a, Kind::Entity, &gone).await.expect("get").is_none(),
        "get of an absent row must answer None"
    );
    assert!(
        !d.delete(a, Kind::Entity, &gone).await.expect("delete"),
        "delete of an absent row must answer false"
    );

    // ADR-0005 / ETSI 047_06: a mutate is a read-modify-write under the row
    // lock, and a missing row is None — NEVER an insert. A get+upsert
    // implementation lets a bookkeeping writeback racing a DELETE resurrect
    // the deleted row.
    let missed = d
        .mutate::<(), ()>(a, Kind::Entity, &gone, |_| Ok(()))
        .await
        .expect("mutate");
    assert!(missed.is_none(), "mutate of an absent row must answer None");
    assert!(
        d.get(a, Kind::Entity, &gone).await.expect("get").is_none(),
        "mutate of an absent row must not insert it (047_06)"
    );

    // create is create-if-absent: the second one is refused and changes
    // nothing.
    assert!(
        d.create(a, Kind::Entity, &e1, doc(&e1))
            .await
            .expect("create"),
        "the first create must report true"
    );
    assert!(
        !d.create(a, Kind::Entity, &e1, doc("urn:overwritten"))
            .await
            .expect("create"),
        "a create over an existing id must report false"
    );
    let row = d
        .get(a, Kind::Entity, &e1)
        .await
        .expect("get")
        .expect("row");
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
        .await
        .expect("mutate");
    assert_eq!(applied, Some(Ok(())), "a mutate over a present row applies");
    assert_eq!(
        d.get(a, Kind::Entity, &e1)
            .await
            .expect("get")
            .expect("row")["marker"],
        json!(1),
        "an accepted mutate must be visible to the next read"
    );
    let rejected = d
        .mutate::<(), &'static str>(a, Kind::Entity, &e1, |v| {
            v["marker"] = json!(2);
            Err("rejected")
        })
        .await
        .expect("mutate");
    assert_eq!(
        rejected,
        Some(Err("rejected")),
        "the closure's error crosses back"
    );
    assert_eq!(
        d.get(a, Kind::Entity, &e1)
            .await
            .expect("get")
            .expect("row")["marker"],
        json!(1),
        "a rejected mutate must commit nothing"
    );

    // Tenant isolation, on every read and every write path.
    assert!(
        d.get(b, Kind::Entity, &e1).await.expect("get").is_none(),
        "another tenant must not read the row"
    );
    assert!(
        !d.delete(b, Kind::Entity, &e1).await.expect("delete"),
        "another tenant must not delete the row"
    );
    assert!(
        d.mutate::<(), ()>(b, Kind::Entity, &e1, |_| Ok(()))
            .await
            .expect("mutate")
            .is_none(),
        "another tenant must not mutate the row"
    );
    assert!(
        d.list(b, Kind::Entity)
            .await
            .expect("list")
            .iter()
            .all(|r| r["id"] != e1.as_str()),
        "another tenant must not list the row"
    );

    // Kinds are separate namespaces: the same id under two kinds is two rows.
    assert!(
        d.create(a, Kind::Subscription, &e1, doc(&e1))
            .await
            .expect("create"),
        "the same id under another kind is a different row"
    );
    assert!(d.delete(a, Kind::Subscription, &e1).await.expect("delete"));
    assert!(
        d.get(a, Kind::Entity, &e1).await.expect("get").is_some(),
        "deleting one kind must not touch another"
    );

    // upsert answers whether a document was ALREADY there, and batch_upsert
    // answers the opposite polarity — created-flags, which the batch path
    // needs to split 201 from 204 (5.6.8). Getting one of the two backwards
    // is invisible until a batch reports every create as an update.
    assert!(
        !d.upsert(a, Kind::Entity, &e2, doc(&e2))
            .await
            .expect("upsert"),
        "upsert that created the row reports false"
    );
    assert!(
        d.upsert(a, Kind::Entity, &e2, doc(&e2))
            .await
            .expect("upsert"),
        "upsert over an existing row reports true"
    );
    let e3 = format!("urn:ngsi-ld:{prefix}:3");
    let flags = d
        .batch_upsert(a, vec![(e3.clone(), doc(&e3)), (e2.clone(), doc(&e2))])
        .await
        .expect("batch_upsert");
    assert_eq!(
        flags,
        vec![true, false],
        "batch_upsert answers created-flags in input order, the opposite \
         polarity of upsert"
    );
    assert!(d.delete(a, Kind::Entity, &e3).await.expect("delete"));
    let ids = vec![e2.clone(), gone.clone(), e1.clone()];
    let batch = d
        .batch_mutate::<()>(a, &ids, |_, v| {
            v["batched"] = json!(true);
            Ok(())
        })
        .await
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
        .await
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
        .await
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
        .await
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

    // `subscription_tenants` is the iteration domain the interval sweep and
    // both mirror hydrations walk. A backend may return MORE tenants than
    // hold a subscription — every caller lists per tenant afterwards and an
    // empty list costs nothing. It may never return FEWER: a tenant missing
    // here is a tenant whose periodic notifications never fire and whose
    // subscriptions never reach the mirror, with no error anywhere.
    let sub = format!("urn:ngsi-ld:Subscription:{prefix}:1");
    let sub_doc = json!({
        "id": &sub,
        "type": "Subscription",
        "status": "active",
        "notification": {"endpoint": {"uri": "http://sink.invalid/n"}},
    });
    d.upsert(a, Kind::Subscription, &sub, sub_doc)
        .await
        .expect("upsert subscription");
    let domain = d
        .subscription_tenants()
        .await
        .expect("subscription_tenants");
    assert!(
        domain.iter().any(|t| t == a.as_str()),
        "a tenant holding a subscription must appear in subscription_tenants: {domain:?}"
    );
    // A REGISTRATION puts a tenant in the domain too. One of the hydrations
    // that walks this domain fills the registration mirror, and the
    // federation path reads that mirror alone once it is installed — so a
    // tenant holding registrations and no subscription is a tenant that
    // silently forwards to no Context Source.
    let reg_only = TenantId::new(&format!("{prefix}regonly")).expect("tenant");
    let reg = format!("urn:ngsi-ld:ContextSourceRegistration:{prefix}:1");
    d.upsert(
        &reg_only,
        Kind::Registration,
        &reg,
        json!({
            "id": &reg,
            "type": "ContextSourceRegistration",
            "endpoint": "http://cs.invalid/ngsi-ld/v1",
            "information": [{"entities": [{"type": "Vehicle"}]}],
        }),
    )
    .await
    .expect("upsert registration");
    let domain = d
        .subscription_tenants()
        .await
        .expect("subscription_tenants");
    assert!(
        domain.iter().any(|t| t == reg_only.as_str()),
        "a tenant holding only a registration must appear in the mirror-hydration \
         domain, or its registration mirror hydrates empty: {domain:?}"
    );

    // Table 5.2.9-2 forward bookkeeping, pinned here for the same reason as
    // 5.2.14.2's: a backend may write it as one statement instead of a
    // read-modify-write, and the result must not depend on which it chose.
    let f1 = "2020-02-01T00:00:00.000Z";
    let f2 = "2020-02-02T00:00:00.000Z";
    let ok1 = d
        .record_forward(&reg_only, &reg, f1, true)
        .await
        .expect("record_forward")
        .expect("the registration is there");
    assert_eq!(
        ok1["timesSent"], 1,
        "the first forward moves timesSent to 1"
    );
    assert_eq!(ok1["lastSuccess"], f1, "lastSuccess takes the stamp");
    assert_eq!(ok1["status"], "ok", "a 2xx leaves the registration ok");
    assert!(
        ok1.get("timesFailed").is_none() && ok1.get("lastFailure").is_none(),
        "a registration that has only succeeded carries no failure members: {ok1}"
    );

    let bad = d
        .record_forward(&reg_only, &reg, f2, false)
        .await
        .expect("record_forward")
        .expect("the registration is still there");
    assert_eq!(
        bad["timesSent"], 2,
        "timesSent counts the failed attempt too (Table 5.2.9-2)"
    );
    assert_eq!(bad["timesFailed"], 1, "the failure moves timesFailed");
    assert_eq!(bad["lastFailure"], f2, "lastFailure takes the stamp");
    assert_eq!(bad["status"], "failed", "status names the LAST attempt");
    assert_eq!(
        bad["lastSuccess"], f1,
        "a failure never rewinds the last success"
    );

    // A registration deleted while its forward was in flight has no row to
    // book against, and the writeback must not resurrect it.
    let vanished = format!("urn:ngsi-ld:ContextSourceRegistration:{prefix}:gone");
    assert!(
        d.record_forward(&reg_only, &vanished, f1, true)
            .await
            .expect("record_forward")
            .is_none(),
        "an absent registration books nothing"
    );
    assert!(
        d.get(&reg_only, Kind::Registration, &vanished)
            .await
            .expect("get")
            .is_none(),
        "a bookkeeping writeback must never insert the row it missed"
    );

    d.delete(&reg_only, Kind::Registration, &reg)
        .await
        .expect("delete");
    // `list_page` is how the readers that must see EVERY document read —
    // the mirror seed above all — so a backend may not refuse it for volume
    // the way `list` may (5.5.6 licenses TooManyResults for a query
    // operation, which an internal bootstrap is not). Every arm must agree
    // on the walk, including the one that takes the trait default: ids
    // strictly greater than `after`, id-ordered, at most `limit`, a short
    // page means the end, and every stored document served exactly once.
    let mut ids: Vec<String> = (0..7)
        .map(|i| format!("urn:ngsi-ld:Subscription:{prefix}:page:{i}"))
        .collect();
    for id in &ids {
        d.upsert(
            a,
            Kind::Subscription,
            id,
            json!({"id": id, "type": "Subscription"}),
        )
        .await
        .expect("upsert page subscription");
    }
    ids.push(sub.clone());
    ids.sort();

    let mut walked: Vec<String> = Vec::new();
    let mut after: Option<String> = None;
    loop {
        let page = d
            .list_page(a, Kind::Subscription, after.as_deref(), 3)
            .await
            .expect("list_page");
        assert!(page.len() <= 3, "a page may not exceed its limit: {page:?}");
        let short = page.len() < 3;
        for doc in &page {
            let id = doc["id"].as_str().expect("a stored doc keeps its id");
            assert!(
                after.as_deref().is_none_or(|prev| id > prev),
                "`after` is exclusive and the walk ascends: {id} after {after:?}"
            );
            after = Some(id.to_owned());
            walked.push(id.to_owned());
        }
        if short {
            break;
        }
    }
    assert_eq!(
        walked, ids,
        "the walk must serve every subscription of the tenant exactly once, in id order"
    );
    // The other tenant's documents are not on this walk. 4.14: operations
    // "only apply to the information of the specified `Tenant` in isolation".
    assert!(
        d.list_page(b, Kind::Subscription, None, 100)
            .await
            .expect("list_page")
            .is_empty(),
        "a tenant with no subscriptions pages empty, whatever another tenant holds"
    );
    // Entities page like every other kind. 5.9.2.4's registration-vs-entity
    // conflict check reads them this way — it must see every Entity of the
    // tenant and has no TooManyResults to raise — so a backend may not refuse
    // this walk for volume either, and may not build the page by
    // materializing the tenant first.
    //
    // 4.22 applies INSIDE the page: an expired Entity is not there to be
    // served, and dropping one after the limit was applied would hand back a
    // short page, which every walker reads as the end of the tenant.
    let stem = format!("urn:ngsi-ld:{prefix}:page:");
    let live: Vec<String> = (0..4).map(|i| format!("{stem}live{i}")).collect();
    // sorts between live1 and live2, so it falls inside the first page below
    let expired = format!("{stem}live1x");
    for id in &live {
        d.upsert(a, Kind::Entity, id, doc(id))
            .await
            .expect("upsert page entity");
    }
    let mut gone_doc = doc(&expired);
    gone_doc["expiresAt"] = json!("2000-01-01T00:00:00.000Z");
    d.upsert(a, Kind::Entity, &expired, gone_doc)
        .await
        .expect("upsert expired entity");

    let page = d
        .list_page(a, Kind::Entity, Some(&stem), 3)
        .await
        .expect("list_page entities");
    let served: Vec<&str> = page.iter().filter_map(|d| d["id"].as_str()).collect();
    assert_eq!(
        served,
        [live[0].as_str(), live[1].as_str(), live[2].as_str()],
        "an expired entity may neither be served nor consume a slot of the page"
    );

    let mut walked: Vec<String> = Vec::new();
    let mut after = stem.clone();
    loop {
        let page = d
            .list_page(a, Kind::Entity, Some(&after), 2)
            .await
            .expect("list_page entities");
        let short = page.len() < 2;
        let mut moved = false;
        for doc in &page {
            let id = doc["id"].as_str().expect("a stored entity keeps its id");
            assert!(
                id > after.as_str(),
                "`after` is exclusive: {id} after {after}"
            );
            after = id.to_owned();
            moved = true;
            if id.starts_with(stem.as_str()) {
                walked.push(id.to_owned());
            }
        }
        if short || !moved || !after.starts_with(stem.as_str()) {
            break;
        }
    }
    assert_eq!(
        walked, live,
        "the walk must serve every live entity of the tenant exactly once, in id \
         order, and never the expired one"
    );
    for id in live.iter().chain(std::iter::once(&expired)) {
        d.delete(a, Kind::Entity, id)
            .await
            .expect("delete page entity");
    }

    // 4.22 at the read boundary, the part a backend must not decide for
    // itself. The stamp marks "a certain Entity, Property or Relationship"
    // invalid; it is not a store-wide rule that hides any document carrying
    // one, and it reaches every Attribute of an Entity, sub-Attributes
    // included.
    let exp_e = format!("urn:ngsi-ld:{prefix}:expiry:entity");
    let mut expired_entity = doc(&exp_e);
    expired_entity["expiresAt"] = json!("2000-01-01T00:00:00.000Z");
    d.upsert(a, Kind::Entity, &exp_e, expired_entity)
        .await
        .expect("upsert expired entity");
    assert!(
        d.get(a, Kind::Entity, &exp_e).await.expect("get").is_none(),
        "an Entity past its expiresAt reads absent, not just out of a page"
    );
    d.delete(a, Kind::Entity, &exp_e).await.expect("delete");

    // An Attribute whose every instance expired leaves; the Entity carrying
    // it does not, and neither does a live sibling.
    let exp_a = format!("urn:ngsi-ld:{prefix}:expiry:attr");
    let iri = |n: &str| format!("https://uri.etsi.org/ngsi-ld/default-context/{n}");
    let mut with_attrs = doc(&exp_a);
    with_attrs[iri("gone")] = json!([{"value": 1, "expiresAt": "2000-01-01T00:00:00.000Z"}]);
    with_attrs[iri("kept")] = json!([{
        "value": 2,
        iri("subgone"): [{"value": 3, "expiresAt": "2000-01-01T00:00:00.000Z"}],
        iri("subkept"): [{"value": 4}],
    }]);
    // 4.6.3 leaves the seconds-fraction separator open, and ',' (0x2C) sorts
    // before both '.' and 'Z', so the two stamps have to be read as instants:
    // a backend that compares them as bytes serves this live Attribute as
    // expired. Every backend answers the same here or it is not the same
    // store.
    with_attrs[iri("commakept")] = json!([{"value": 5, "expiresAt": "2999-01-01T00:00:00,500Z"}]);
    d.upsert(a, Kind::Entity, &exp_a, with_attrs)
        .await
        .expect("upsert entity with an expired attribute");
    let served = d
        .get(a, Kind::Entity, &exp_a)
        .await
        .expect("get")
        .expect("an Entity outlives the expiry of one of its Attributes");
    assert!(
        served.get(iri("gone")).is_none(),
        "an Attribute whose only instance expired is not served"
    );
    assert_eq!(
        served[iri("kept")][0]["value"],
        json!(2),
        "a live Attribute survives its sibling's expiry"
    );
    assert!(
        served[iri("kept")][0].get(iri("subgone")).is_none(),
        "a sub-Attribute is a Property or Relationship too: past its stamp it \
         is not served"
    );
    assert_eq!(
        served[iri("kept")][0][iri("subkept")][0]["value"],
        json!(4),
        "a live sub-Attribute survives its sibling's expiry"
    );
    assert_eq!(
        served[iri("commakept")][0]["value"],
        json!(5),
        "a comma seconds-fraction is an instant, not a byte string"
    );
    d.delete(a, Kind::Entity, &exp_a).await.expect("delete");

    // 5.8.6: an expired SUBSCRIPTION is not deleted and stays retrievable —
    // the API turns the stamp into status "expired" and keeps it updatable.
    // A backend that hid every document with a past expiresAt would lose it.
    let exp_s = format!("urn:ngsi-ld:{prefix}:expiry:sub");
    d.upsert(
        a,
        Kind::Subscription,
        &exp_s,
        json!({
            "id": exp_s,
            "type": "Subscription",
            "expiresAt": "2000-01-01T00:00:00.000Z",
        }),
    )
    .await
    .expect("upsert expired subscription");
    assert!(
        d.get(a, Kind::Subscription, &exp_s)
            .await
            .expect("get")
            .is_some(),
        "an expired Subscription stays retrievable (5.8.6)"
    );
    d.delete(a, Kind::Subscription, &exp_s)
        .await
        .expect("delete");

    // `delete_entity_if` decides and deletes under one lock, and it decides
    // on the STORED document. 5.6.6.4: an Entity the caller's selector
    // excludes "is not known" for the operation, which is the same answer an
    // absent Entity gets — so a refusal and a miss are both `false`, and a
    // refusal must leave the document exactly where it was.
    let cond = format!("urn:ngsi-ld:{prefix}:cond");
    d.upsert(a, Kind::Entity, &cond, doc(&cond))
        .await
        .expect("upsert conditional-delete entity");
    assert!(
        !d.delete_entity_if(a, &cond, &|_| false)
            .await
            .expect("delete_entity_if"),
        "a refused predicate reports no deletion"
    );
    assert!(
        d.get(a, Kind::Entity, &cond).await.expect("get").is_some(),
        "a refused delete leaves the document in place"
    );
    // 4.14: the predicate never even runs for another tenant's document.
    assert!(
        !d.delete_entity_if(b, &cond, &|_| panic!(
            "another tenant's document reached the predicate"
        ))
        .await
        .expect("delete_entity_if"),
        "an Entity of another tenant is absent here"
    );
    assert!(
        d.get(a, Kind::Entity, &cond).await.expect("get").is_some(),
        "a delete addressed to another tenant may not touch this one"
    );
    assert!(
        d.delete_entity_if(a, &cond, &|v| v["id"] == cond.as_str())
            .await
            .expect("delete_entity_if"),
        "the predicate is handed the stored document, not the id"
    );
    assert!(
        d.get(a, Kind::Entity, &cond).await.expect("get").is_none(),
        "an accepted predicate deletes"
    );
    assert!(
        !d.delete_entity_if(a, &cond, &|_| true)
            .await
            .expect("delete_entity_if"),
        "an absent Entity is not deleted, whatever the predicate answers"
    );

    // `list_slice` is the window a 5.5.9.2 limit/offset listing serves from,
    // and it must agree with the walk above on both the order and the set:
    // the same ids, the same order, and a total that counts the whole match
    // set rather than the page. A backend may not refuse it for volume — the
    // window bounds the result by construction.
    let (page, total) = d
        .list_slice(a, Kind::Subscription, 0, 3)
        .await
        .expect("list_slice");
    assert_eq!(total, ids.len(), "the total counts the set, not the page");
    assert_eq!(page.len(), 3, "limit is a maximum, and 3 were available");
    for (i, doc) in page.iter().enumerate() {
        assert_eq!(
            doc["id"].as_str(),
            Some(ids[i].as_str()),
            "the window is the id-ordered prefix: {page:?}"
        );
    }
    // Every single-element window lands on the element the walk has there.
    for (i, want) in ids.iter().enumerate() {
        let (page, _) = d
            .list_slice(a, Kind::Subscription, i, 1)
            .await
            .expect("list_slice");
        assert_eq!(
            page.first().and_then(|d| d["id"].as_str()),
            Some(want.as_str()),
            "offset {i} served the wrong element: {page:?}"
        );
    }
    // limit 0 is legal (6.3.10, with count): the count without the page.
    let (page, total) = d
        .list_slice(a, Kind::Subscription, 0, 0)
        .await
        .expect("list_slice");
    assert!(page.is_empty(), "limit 0 returns no elements: {page:?}");
    assert_eq!(total, ids.len(), "limit 0 still counts the whole set");
    // An offset past the end is an empty page, not an error, and the total
    // is unchanged by where the client asked to start.
    let (page, total) = d
        .list_slice(a, Kind::Subscription, ids.len() + 10, 5)
        .await
        .expect("list_slice");
    assert!(page.is_empty(), "past the end is empty: {page:?}");
    assert_eq!(total, ids.len(), "an offset does not change the match set");
    // 4.14 again: the window is inside one tenant.
    let (page, total) = d
        .list_slice(b, Kind::Subscription, 0, 100)
        .await
        .expect("slice");
    assert!(
        page.is_empty() && total == 0,
        "another tenant's rows reached this window: {page:?} / {total}"
    );

    for id in ids.iter().filter(|i| *i != &sub) {
        d.delete(a, Kind::Subscription, id).await.expect("delete");
    }

    // 5.2.14.2 delivery bookkeeping. A backend may write this as one
    // statement instead of a read-modify-write; the result must not depend on
    // which it chose, so the rule is pinned here rather than in one backend's
    // own tests.
    let t1 = "2020-01-01T00:00:00.000Z";
    let t2 = "2020-01-02T00:00:00.000Z";
    let first = d
        .record_delivery(a, Kind::Subscription, &sub, t1)
        .await
        .expect("record_delivery")
        .expect("the subscription is there");
    let n1 = &first.doc["notification"];
    assert_eq!(n1["timesSent"], 1, "the first attempt moves timesSent to 1");
    assert_eq!(
        n1["lastNotification"], t1,
        "lastNotification takes the stamp"
    );
    assert_eq!(n1["lastSuccess"], t1, "lastSuccess takes the stamp");
    assert_eq!(n1["status"], "ok", "the attempt leaves the notification ok");
    assert!(
        first.doc.get("status").is_none(),
        "the top-level status is a rendered member, not a stored one: {}",
        first.doc
    );
    assert!(
        first.prev_success.is_none(),
        "a subscription that never succeeded has no previous lastSuccess"
    );

    let second = d
        .record_delivery(a, Kind::Subscription, &sub, t2)
        .await
        .expect("record_delivery")
        .expect("the subscription is still there");
    assert_eq!(
        second.doc["notification"]["timesSent"], 2,
        "timesSent counts attempts, it does not reset"
    );
    assert_eq!(
        second.prev_success.as_ref().and_then(Value::as_str),
        Some(t1),
        "the overwritten lastSuccess comes back, or a failed attempt cannot \
         roll it back"
    );

    // 5.8.6: a subscription deleted between matching and delivery has no row
    // to book against, and nothing may be sent.
    assert!(
        d.record_delivery(a, Kind::Subscription, &gone, t1)
            .await
            .expect("record_delivery")
            .is_none(),
        "an absent subscription books nothing"
    );

    assert!(
        d.delete(a, Kind::Subscription, &sub).await.expect("delete"),
        "the contract's own subscription must be removable"
    );

    // ADR-0021: a stored @context is a Tenant's document. "Hosted" and
    // "ImplicitlyCreated" rows hold term mappings authored through one
    // Tenant's requests, and 5.5.7 makes those mappings decide what that
    // Tenant's payloads mean, so a backend that answered every caller would
    // hand one Tenant's meaning of its own data to another. "Cached" is a
    // copy of a public document (5.13.1) and belongs to no Tenant.
    let hosted_id = format!("{prefix}-ctx-hosted");
    let cached_id = format!("{prefix}-ctx-cached");
    let hosted = json!({"localId": hosted_id, "kind": "Hosted", "owner": a.as_str(),
                        "body": {"@context": {"t": "https://example.org/t"}}});
    d.context_put(Some(a), &hosted_id, hosted.clone())
        .await
        .expect("the owning tenant stores its own @context");
    d.context_put(
        None,
        &cached_id,
        json!({"localId": cached_id, "kind": "Cached", "body": {"@context": {}}}),
    )
    .await
    .expect("a Cached copy belongs to no tenant");

    assert!(
        d.context_get(Some(a), &hosted_id)
            .await
            .expect("context_get")
            .is_some(),
        "the owning tenant must read its own @context"
    );
    assert!(
        d.context_get(Some(b), &hosted_id)
            .await
            .expect("context_get")
            .is_none(),
        "another tenant's Hosted @context must be as absent as one never stored"
    );
    assert!(
        d.context_get(None, &hosted_id)
            .await
            .expect("context_get")
            .is_none(),
        "no tenant in scope must reach a tenant's @context"
    );
    assert!(
        d.context_list_meta(Some(b))
            .await
            .expect("context_list_meta")
            .iter()
            .all(|r| r["localId"] != hosted_id.as_str()),
        "another tenant's @context must not be listed"
    );
    assert!(
        !d.context_delete(Some(b), &hosted_id)
            .await
            .expect("context_delete"),
        "another tenant must not delete it"
    );
    assert!(
        d.context_put(Some(b), &hosted_id, hosted).await.is_err(),
        "another tenant must not overwrite it either"
    );
    assert!(
        d.context_get(Some(a), &hosted_id)
            .await
            .expect("context_get")
            .is_some(),
        "the owner's @context must survive every foreign attempt"
    );
    for t in [Some(a), Some(b), None] {
        assert!(
            d.context_get(t, &cached_id)
                .await
                .expect("context_get")
                .is_some(),
            "a Cached copy is a public document every tenant reaches"
        );
    }
    assert!(
        d.context_delete(Some(a), &hosted_id)
            .await
            .expect("context_delete"),
        "the contract's own @context must be removable by its owner"
    );
    assert!(
        d.context_delete(None, &cached_id)
            .await
            .expect("context_delete"),
        "and the Cached copy by anyone"
    );

    for id in [&e1, &e2] {
        assert!(
            d.delete(a, Kind::Entity, id).await.expect("delete"),
            "the contract's own rows must be removable"
        );
    }
}

/// Hold a temporal driver to the same shape. A driver that answers
/// `supported() == false` declares it keeps no history and is held to
/// nothing else.
pub async fn run_temporal_contract(
    d: &dyn TemporalDriver,
    a: &TenantId,
    b: &TenantId,
    prefix: &str,
) {
    if !d.supported() {
        return;
    }
    let e1 = format!("urn:ngsi-ld:{prefix}:t1");
    let gone = format!("urn:ngsi-ld:{prefix}:tabsent");

    assert!(
        d.get(a, &gone).await.expect("get").is_none(),
        "get of an absent temporal document must answer None"
    );
    assert!(
        !d.delete(a, &gone).await.expect("delete"),
        "delete of an absent temporal document must answer false"
    );
    let missed = d
        .mutate::<(), ()>(a, &gone, |_| Ok(()))
        .await
        .expect("mutate");
    assert!(
        missed.is_none(),
        "mutate of an absent document must answer None"
    );
    assert!(
        d.get(a, &gone).await.expect("get").is_none(),
        "mutate of an absent document must not insert it (047_06)"
    );

    assert!(
        d.create(a, &e1, doc(&e1)).await.expect("create"),
        "the first create reports true"
    );
    assert!(
        !d.create(a, &e1, doc(&e1)).await.expect("create"),
        "a create over an existing id reports false"
    );
    let rejected = d
        .mutate::<(), &'static str>(a, &e1, |v| {
            v["marker"] = json!(2);
            Err("rejected")
        })
        .await
        .expect("mutate");
    assert_eq!(rejected, Some(Err("rejected")));
    assert!(
        d.get(a, &e1)
            .await
            .expect("get")
            .expect("row")
            .get("marker")
            .is_none(),
        "a rejected mutate must commit nothing"
    );

    assert!(
        d.get(b, &e1).await.expect("get").is_none(),
        "another tenant must not read the temporal document"
    );
    assert!(
        !d.delete(b, &e1).await.expect("delete"),
        "another tenant must not delete it"
    );
    assert!(
        d.list(b)
            .await
            .expect("list")
            .iter()
            .all(|r| r["id"] != e1.as_str()),
        "another tenant must not list it"
    );

    assert!(
        d.delete(a, &e1).await.expect("delete"),
        "the contract's own rows must be removable"
    );
}
