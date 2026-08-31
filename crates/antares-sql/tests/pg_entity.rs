// SPDX-License-Identifier: EUPL-1.2
//! PgEntityStore integration, including the concurrency guarantees.
//! Skips loudly without ANTARES_TEST_DATABASE_URL (see tests/pg.rs recipe).

use antares_model::TenantId;
use antares_sql::store::pg;
use antares_sql::store::pg::entity::PgEntityStore;
use serde_json::json;

macro_rules! require_db {
    () => {
        match std::env::var("ANTARES_TEST_DATABASE_URL") {
            Ok(url) => url,
            Err(_) => {
                eprintln!("SKIP: ANTARES_TEST_DATABASE_URL not set");
                return;
            }
        }
    };
}

fn doc(id: &str, n: i64) -> serde_json::Value {
    json!({
        "id": id, "type": "Test",
        "createdAt": "2026-08-04T09:00:00Z", "modifiedAt": "2026-08-04T09:00:00Z",
        "n": {"type": "Property", "value": n}
    })
}

/// 4.6.3: "The Seconds component may optionally contain a decimal fraction.
/// In this case the string shall contain two integer digits, followed by a
/// decimal point and then one or more fractional digits, up to a maximum of
/// six. ... In requests, also a comma instead of a decimal point may be used
/// as separator for compatibility reasons."
///
/// The broker accepts that form deliberately — `parse_datetime` has an
/// explicit branch for it and `filter::expired_at` rewrites the comma before
/// parsing, with a comment saying why. The store then handed the raw text to
/// a bare `::timestamptz` cast, which PostgreSQL refuses: a legal request
/// became a 500 on the postgres and timescale arms while the memory and file
/// arms accepted it.
#[tokio::test(flavor = "multi_thread")]
async fn clause_4_6_3_a_comma_seconds_fraction_is_stored_like_a_point() {
    let url = require_db!();
    let pool = pg::connect(&url, 5).await.expect("connect");
    let s = PgEntityStore::new(pool.clone());
    let t = TenantId::new("pgcomma").expect("tenant");
    pg::ensure_tenant(&pool, &t).await.expect("tenant row");
    let id = "urn:ngsi-ld:Test:comma1";
    let _ = s.delete(&t, id);

    let with_comma = json!({
        "id": id, "type": "Test",
        "createdAt": "2026-08-04T09:00:00,500Z",
        "modifiedAt": "2026-08-04T09:00:00,500Z",
        "expiresAt": "2099-01-01T00:00:00,500Z",
        "n": {"type": "Property", "value": 1}
    });
    assert!(
        s.create(&t, id, &with_comma)
            .expect("4.6.3: the comma form is legal"),
        "created"
    );

    // The extracted columns are the point form of the same instant, so every
    // predicate built on them (4.22 expiry, ordering, the sysAttrs windows)
    // sees the timestamp the client meant.
    // Compared as instants against the point form of the same stamps: the
    // two spellings 4.6.3 allows have to land on the same timestamptz.
    let (created_ok, expires_ok): (bool, Option<bool>) = sqlx::query_as(
        "SELECT created_at = $3::timestamptz, expires_at = $4::timestamptz
           FROM entities WHERE tenant_id = $1 AND id = $2",
    )
    .bind(t.as_str())
    .bind(id)
    .bind("2026-08-04T09:00:00.500Z")
    .bind("2099-01-01T00:00:00.500Z")
    .fetch_one(&pool)
    .await
    .expect("row");
    assert!(created_ok, "the comma and point forms are the same instant");
    assert_eq!(
        expires_ok,
        Some(true),
        "the expiry the client set is the expiry the store holds"
    );

    // A future expiry means present; the entity is readable, not a 4.22 ghost.
    assert!(s.get(&t, id).expect("get").is_some(), "still valid");

    // And the past-comma form actually expires, rather than reading as no
    // expiry at all: the meta-side `try_timestamptz` returns NULL for text it
    // cannot parse, which would silently make the entity immortal.
    let gone = "urn:ngsi-ld:Test:comma2";
    let _ = s.delete(&t, gone);
    let expired = json!({
        "id": gone, "type": "Test",
        "createdAt": "2020-01-01T00:00:00,250Z", "modifiedAt": "2020-01-01T00:00:00,250Z",
        "expiresAt": "2020-01-01T00:00:00,250Z",
        "n": {"type": "Property", "value": 2}
    });
    assert!(s.create(&t, gone, &expired).expect("create expired"));
    assert!(
        s.get(&t, gone).expect("get").is_none(),
        "4.22 + 4.6.3: a comma-stamped expiry in the past is still an expiry"
    );

    let _ = s.delete(&t, id);
    let _ = s.delete(&t, gone);
}

#[tokio::test(flavor = "multi_thread")]
async fn entity_crud_roundtrip_with_extracted_columns() {
    let url = require_db!();
    let pool = pg::connect(&url, 5).await.expect("connect");
    let s = PgEntityStore::new(pool.clone());
    let t = TenantId::new("pgcrud").expect("tenant");
    pg::ensure_tenant(&pool, &t).await.expect("tenant row");
    let id = "urn:ngsi-ld:Test:crud1";
    let _ = s.delete(&t, id);

    assert!(s.create(&t, id, &doc(id, 1)).expect("create"));
    assert!(
        !s.create(&t, id, &doc(id, 1)).expect("dup"),
        "AlreadyExists → false"
    );
    assert_eq!(
        s.get(&t, id).expect("get").expect("present")["n"]["value"],
        1
    );
    assert_eq!(s.version(&t, id).expect("v"), Some(1));

    // extracted columns really extracted (types tenant-scoped index shape)
    let other = TenantId::new("pgcrud_other").expect("tenant");
    assert!(s.get(&other, id).expect("cross-tenant get").is_none());
    assert_eq!(s.list(&t).expect("list").len(), 1);

    let r: Option<Result<(), ()>> = match s.mutate(&t, id, |d| {
        d["n"]["value"] = json!(2);
        d["modifiedAt"] = json!("2026-08-04T09:01:00Z");
        Ok(())
    }) {
        Ok(x) => x,
        Err(e) => panic!("mutate: {e}"),
    };
    assert!(matches!(r, Some(Ok(()))));
    assert_eq!(
        s.get(&t, id).expect("get").expect("present")["n"]["value"],
        2
    );
    assert_eq!(
        s.version(&t, id).expect("v"),
        Some(2),
        "version bumped under the lock"
    );

    // closure error rolls back, version untouched
    let r: Option<Result<(), &str>> = match s.mutate(&t, id, |d| {
        d["n"]["value"] = json!(99);
        Err("nope")
    }) {
        Ok(x) => x,
        Err(e) => panic!("mutate: {e}"),
    };
    assert!(matches!(r, Some(Err("nope"))));
    assert_eq!(
        s.get(&t, id).expect("get").expect("present")["n"]["value"],
        2
    );
    assert_eq!(s.version(&t, id).expect("v"), Some(2));

    assert!(s.delete(&t, id).expect("delete").is_some());
    assert!(s.get(&t, id).expect("get").is_none());
    assert!(s
        .mutate(&t, id, |_| Ok::<(), ()>(()))
        .expect("mutate absent")
        .is_none());
}

/// Parallel PATCH storm against ONE entity — no lost updates,
/// version strictly monotone, final state = sum of all increments.
#[tokio::test(flavor = "multi_thread")]
async fn concurrent_mutations_lose_nothing() {
    let url = require_db!();
    let pool = pg::connect(&url, 10).await.expect("connect");
    let t = TenantId::new("pgstorm").expect("tenant");
    pg::ensure_tenant(&pool, &t).await.expect("tenant row");
    let s = std::sync::Arc::new(PgEntityStore::new(pool));
    let id = "urn:ngsi-ld:Test:storm";
    let _ = s.delete(&t, id);
    assert!(s.create(&t, id, &doc(id, 0)).expect("create"));

    const WRITERS: i64 = 8;
    const ROUNDS: i64 = 10;
    let mut tasks = Vec::new();
    for _ in 0..WRITERS {
        let (s, t) = (s.clone(), t.clone());
        tasks.push(tokio::task::spawn_blocking(move || {
            for _ in 0..ROUNDS {
                let r = s
                    .mutate(&t, id, |d| {
                        let n = d["n"]["value"].as_i64().expect("n");
                        d["n"]["value"] = serde_json::json!(n + 1);
                        Ok::<(), ()>(())
                    })
                    .expect("mutate");
                assert!(matches!(r, Some(Ok(()))), "row must exist throughout");
            }
        }));
    }
    for task in tasks {
        task.await.expect("writer");
    }

    let n = s.get(&t, id).expect("get").expect("present")["n"]["value"]
        .as_i64()
        .expect("n");
    assert_eq!(
        n,
        WRITERS * ROUNDS,
        "every increment survived (no lost updates)"
    );
    assert_eq!(
        s.version(&t, id).expect("v"),
        Some(1 + WRITERS * ROUNDS),
        "version = create + one bump per mutate"
    );
}

/// Batch create/delete as single multi-row statements — created flags in
/// input order, duplicate ids deduped (5.5.11.1/.4), delete returns prevs.
#[tokio::test(flavor = "multi_thread")]
async fn batch_create_and_delete_multirow() {
    let url = require_db!();
    let pool = pg::connect(&url, 5).await.expect("pool");
    let t = TenantId::new("pgbatch").expect("tenant");
    pg::ensure_tenant(&pool, &t).await.expect("tenant row");
    let s = PgEntityStore::new(pool);

    // pre-existing entity: its batch item must report false
    assert!(s.create(&t, "urn:b:0", &doc("urn:b:0", 0)).expect("pre"));

    let items = vec![
        ("urn:b:0".to_owned(), doc("urn:b:0", 9)), // exists → false
        ("urn:b:1".to_owned(), doc("urn:b:1", 1)),
        ("urn:b:2".to_owned(), doc("urn:b:2", 2)),
        ("urn:b:1".to_owned(), doc("urn:b:1", 99)), // duplicate → false
    ];
    let flags = s.batch_create(&t, &items).expect("batch create");
    assert_eq!(flags, vec![false, true, true, false]);
    // first instance won (5.5.11.1): value 1, not 99
    let stored = s.get(&t, "urn:b:1").expect("get").expect("present");
    assert_eq!(stored["n"]["value"], 1);

    let deleted = s
        .batch_delete(
            &t,
            &["urn:b:0".into(), "urn:b:1".into(), "urn:b:missing".into()],
        )
        .expect("batch delete");
    let mut ids: Vec<&str> = deleted.iter().map(|(id, _)| id.as_str()).collect();
    ids.sort();
    assert_eq!(ids, vec!["urn:b:0", "urn:b:1"]);
    // prev doc travels with the delete (change-hook before-image)
    let prev1 = &deleted
        .iter()
        .find(|(id, _)| id == "urn:b:1")
        .expect("b1")
        .1;
    assert_eq!(prev1["n"]["value"], 1);
    assert!(s.get(&t, "urn:b:1").expect("get").is_none());

    // cleanup
    let _ = s.batch_delete(&t, &["urn:b:2".into()]);
}

// ---- query pushdown ------------------------------------------------------
// The contract is one-directional and it is the whole reason the pushdown is
// safe: SQL may only NARROW. Every row the in-memory evaluator would accept
// must survive the WHERE clause; extra rows are fine because the caller
// re-filters exactly. So each case below asserts the expected set is present,
// and the refusal cases assert the query widens to everything rather than
// guessing a translation.

const NS: &str = "https://uri.etsi.org/ngsi-ld/default-context/";

fn ex(t: &str) -> String {
    format!("{NS}{t}")
}

/// The shape the broker actually stores: expanded IRI keys, each holding an
/// ARRAY of instances. Testing against the internal shape, not a convenient
/// one, is the point — the jsonpath addresses instances.
fn expanded(id: &str, ty: &str, attrs: serde_json::Value) -> serde_json::Value {
    let mut doc = json!({
        "id": id,
        "type": format!("{NS}{ty}"),
        "createdAt": "2026-08-04T09:00:00Z",
        "modifiedAt": "2026-08-04T09:00:00Z"
    });
    for (k, v) in attrs.as_object().expect("attrs object") {
        doc[ex(k)] = json!([v]);
    }
    doc
}

fn ids_of(rows: &[serde_json::Value]) -> Vec<String> {
    let mut v: Vec<String> = rows
        .iter()
        .map(|r| r["id"].as_str().unwrap_or_default().to_owned())
        .collect();
    v.sort();
    v
}

#[tokio::test(flavor = "multi_thread")]
async fn query_pushdown_narrows_without_dropping_matches() {
    let url = require_db!();
    let pool = pg::connect(&url, 5).await.expect("connect");
    let s = PgEntityStore::new(pool.clone());
    let t = TenantId::new("pgquery").expect("tenant");
    pg::ensure_tenant(&pool, &t).await.expect("tenant row");

    let seed = [
        (
            "urn:ngsi-ld:Room:1",
            "Room",
            json!({"temperature": {"type": "Property", "value": 30},
                   "name": {"type": "Property", "value": "north"}}),
        ),
        (
            "urn:ngsi-ld:Room:2",
            "Room",
            json!({"temperature": {"type": "Property", "value": 10}}),
        ),
        (
            "urn:ngsi-ld:Vehicle:1",
            "Vehicle",
            json!({"speed": {"type": "Property", "value": 60},
                   "name": {"type": "Property", "value": "south"}}),
        ),
        (
            "urn:ngsi-ld:Vehicle:2",
            "Vehicle",
            json!({"brandName": {"type": "Property", "value": "Mercedes"}}),
        ),
    ];
    for (id, ty, attrs) in &seed {
        let _ = s.delete(&t, id);
        assert!(
            s.create(&t, id, &expanded(id, ty, attrs.clone()))
                .expect("create"),
            "seed {id}"
        );
    }
    let all: Vec<String> = seed.iter().map(|(id, ..)| (*id).to_owned()).collect();

    let q = |f: &antares_sql::store::pg::entity::EntityFilter<'_>| {
        ids_of(&s.query(&t, f).expect("query").rows)
    };
    let base = || antares_sql::store::pg::entity::EntityFilter {
        expand: &ex,
        ..Default::default()
    };

    // no filter: everything this tenant holds
    assert_eq!(q(&base()), all);

    // ids
    let want = ["urn:ngsi-ld:Room:2"];
    assert_eq!(
        q(&antares_sql::store::pg::entity::EntityFilter {
            ids: Some(&want),
            ..base()
        }),
        want
    );

    // 5.2.33 idPattern literal: anchored = prefix, unanchored = infix; both
    // only narrow (the regex decides later), so a literal that occurs
    // mid-id keeps the row when unanchored and drops it when anchored
    use antares_sql::store::filter::IdLiteral;
    let lit = |text, anchored| antares_sql::store::pg::entity::EntityFilter {
        id_literal: Some(IdLiteral { text, anchored }),
        ..base()
    };
    assert_eq!(
        q(&lit("urn:ngsi-ld:Room:", true)),
        ["urn:ngsi-ld:Room:1", "urn:ngsi-ld:Room:2"]
    );
    assert_eq!(q(&lit("Room:2", false)), ["urn:ngsi-ld:Room:2"]);
    assert_eq!(q(&lit("Room:2", true)), Vec::<String>::new());
    assert_eq!(
        q(&lit("ngsi-ld:V", false)),
        ["urn:ngsi-ld:Vehicle:1", "urn:ngsi-ld:Vehicle:2"]
    );

    // type selection (OR of AND-groups, expanded IRIs)
    let groups = vec![vec![ex("Vehicle")]];
    assert_eq!(
        q(&antares_sql::store::pg::entity::EntityFilter {
            types: Some(&groups),
            ..base()
        }),
        ["urn:ngsi-ld:Vehicle:1", "urn:ngsi-ld:Vehicle:2"]
    );

    // attrs: carries at least one of them
    let attrs = vec![ex("speed"), ex("brandName")];
    assert_eq!(
        q(&antares_sql::store::pg::entity::EntityFilter {
            attrs: Some(&attrs),
            ..base()
        }),
        ["urn:ngsi-ld:Vehicle:1", "urn:ngsi-ld:Vehicle:2"]
    );

    // q=: numeric comparison over instance values
    let ast = antares_ql::parse_q("temperature>20").expect("parse");
    assert_eq!(
        q(&antares_sql::store::pg::entity::EntityFilter {
            q: Some(&ast),
            ..base()
        }),
        ["urn:ngsi-ld:Room:1"]
    );

    // q=: string equality, and the AND of two predicates
    let ast = antares_ql::parse_q("name==\"south\";speed==60").expect("parse");
    assert_eq!(
        q(&antares_sql::store::pg::entity::EntityFilter {
            q: Some(&ast),
            ..base()
        }),
        ["urn:ngsi-ld:Vehicle:1"]
    );

    // q=: existence, and negated existence (true when the attribute is ABSENT
    // — the case a naive `NOT jsonb_path_exists` on the wrong path gets wrong)
    let ast = antares_ql::parse_q("brandName").expect("parse");
    assert_eq!(
        q(&antares_sql::store::pg::entity::EntityFilter {
            q: Some(&ast),
            ..base()
        }),
        ["urn:ngsi-ld:Vehicle:2"]
    );
    let ast = antares_ql::parse_q("!name").expect("parse");
    assert_eq!(
        q(&antares_sql::store::pg::entity::EntityFilter {
            q: Some(&ast),
            ..base()
        }),
        ["urn:ngsi-ld:Room:2", "urn:ngsi-ld:Vehicle:2"]
    );

    // q= the compiler REFUSES (dotted path, regex, string ordering): the row
    // set must widen to everything, never narrow on a guess.
    for refused in ["address.city==\"Bonn\"", "name~=\"^so\"", "name>\"m\""] {
        let ast = antares_ql::parse_q(refused).expect("parse");
        assert_eq!(
            q(&antares_sql::store::pg::entity::EntityFilter {
                q: Some(&ast),
                ..base()
            }),
            all,
            "{refused} must fall back to the full set, not a guess"
        );
    }

    // and the filters compose: type AND q
    let groups = vec![vec![ex("Room")]];
    let ast = antares_ql::parse_q("temperature<20").expect("parse");
    assert_eq!(
        q(&antares_sql::store::pg::entity::EntityFilter {
            types: Some(&groups),
            q: Some(&ast),
            ..base()
        }),
        ["urn:ngsi-ld:Room:2"]
    );
}

/// 5.5.6: "When a query operation is producing so many results that can
/// potentially exhaust client or server resources, or it can be just
/// impractical to be managed, implementations shall raise an error of type
/// TooManyResults. The threshold conditions used as criteria to raise such
/// error is up to each implementation."
///
/// A `q=` shape the compiler declines leaves the request undecided, so no
/// caller page can be pushed and the statement is bounded by the store's own
/// safety LIMIT. Reaching it means the answer was cut at a bound nobody
/// chose: refused, never served as a silent prefix. The same oversized match
/// set stays perfectly pageable through a decided query — the ceiling bounds
/// what one statement materializes, it does not cap the tenant.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_5_6_undecided_query_past_the_ceiling_is_refused() {
    let url = require_db!();
    let pool = pg::connect(&url, 5).await.expect("connect");
    let t = TenantId::new("pgceiling").expect("tenant");
    pg::ensure_tenant(&pool, &t).await.expect("tenant row");
    let clean = || {
        let pool = pool.clone();
        async move {
            sqlx::query("DELETE FROM entities WHERE tenant_id = 'pgceiling'")
                .execute(&pool)
                .await
                .expect("clean");
        }
    };
    clean().await;
    let ceiling = antares_sql::store::pg::entity::MAX_UNDECIDED_ROWS;
    // One statement, ceiling+1 rows: one more than the safety LIMIT can ever
    // return, so the cut is provable rather than incidental.
    sqlx::query(
        "INSERT INTO entities (tenant_id, id, entity, types, created_at, modified_at)
         SELECT 'pgceiling', 'urn:ngsi-ld:Ceil:' || g,
                jsonb_build_object('id', 'urn:ngsi-ld:Ceil:' || g, 'type', $1::text),
                ARRAY[$1::text], now(), now()
           FROM generate_series(1, $2::bigint) g",
    )
    .bind(ex("Ceil"))
    .bind(ceiling + 1)
    .execute(&pool)
    .await
    .expect("seed");

    let s = PgEntityStore::new(pool.clone());
    // a dotted path is one of the shapes compile_q declines (see the pushdown
    // test above) — the request is undecided and therefore unpaged
    let ast = antares_ql::parse_q("address.city==\"Bonn\"").expect("parse");
    let err = match s.query(
        &t,
        &antares_sql::store::pg::entity::EntityFilter {
            q: Some(&ast),
            expand: &ex,
            ..Default::default()
        },
    ) {
        Ok(out) => panic!(
            "a cut result set must be refused, got {} rows (decided={}, paged={})",
            out.rows.len(),
            out.decided,
            out.paged
        ),
        Err(e) => e,
    };
    let ngsi =
        antares_sql::store::pg::entity::ngsi_error(&err).expect("a spec error, not a driver error");
    assert_eq!(ngsi.kind(), "TooManyResults");
    assert_eq!(
        ngsi.status(),
        403,
        "Table 6.3.2-1 status for TooManyResults"
    );

    // and the negative: the same oversized set answers a DECIDED paged query
    // exactly — the ceiling refuses statements, not tenants
    let groups = vec![vec![ex("Ceil")]];
    let out = s
        .query(
            &t,
            &antares_sql::store::pg::entity::EntityFilter {
                types: Some(&groups),
                page: Some(antares_sql::store::pg::entity::Page {
                    offset: 0,
                    limit: 5,
                    count: true,
                }),
                expand: &ex,
                ..Default::default()
            },
        )
        .expect("a paged query over the same set is answerable");
    assert_eq!(out.rows.len(), 5, "exactly the requested page");
    assert!(out.paged && out.decided);
    assert_eq!(
        out.total,
        Some(ceiling + 1),
        "pre-LIMIT total, past the ceiling"
    );
    clean().await;
}

/// 4.22 through the batch paths.
///
/// 5.6.7.4 (p.170): "For each of the NGSI-LD Entities included in the input
/// Array execute the behaviour defined by clause 5.6.1, but limited to a
/// local operation". 5.6.8.4 (p.172): "Create the Entity locally if it does
/// not exist … executing the behaviour defined by clause 5.6.1". 5.6.10.4
/// (p.176): "For each of the NGSI-LD Entity IDs included in the input Array
/// execute the behaviour defined by clause 5.6.6". Each batch operation is
/// defined as its single-entity clause run per item, so the two paths may
/// not answer differently about the same id — and 4.22 makes an entity past
/// its expiry absent to both, whatever the reaping lag.
#[tokio::test(flavor = "multi_thread")]
async fn the_batch_paths_see_an_expired_entity_the_way_the_single_entity_paths_do() {
    let url = require_db!();
    let pool = pg::connect(&url, 5).await.expect("connect");
    let s = PgEntityStore::new(pool.clone());
    let t = TenantId::new("pgexpiry").expect("tenant");
    pg::ensure_tenant(&pool, &t).await.expect("tenant row");

    fn expired(id: &str) -> serde_json::Value {
        let mut d = doc(id, 1);
        d["expiresAt"] = json!("2020-01-01T00:00:00Z");
        d
    }
    let seed = |id: &str| {
        let _ = s.delete(&t, id);
        assert!(s.create(&t, id, &expired(id)).expect("seed"));
        // The row is there and already invalid: absent to a read, which is
        // what makes every answer below about expiry and not about absence.
        assert!(
            s.get(&t, id).expect("get").is_none(),
            "an expired entity must read as absent"
        );
    };

    // The single-entity answers this batch is measured against.
    let single = "urn:ngsi-ld:Test:exp-single";
    seed(single);
    assert!(
        s.create(&t, single, &doc(single, 2)).expect("create"),
        "5.6.1: creating over an expired entity creates"
    );
    seed(single);
    assert!(
        s.delete(&t, single).expect("delete").is_none(),
        "5.6.6: deleting an expired entity finds nothing"
    );

    let id = "urn:ngsi-ld:Test:exp-batch".to_owned();

    seed(&id);
    assert_eq!(
        s.batch_create(&t, &[(id.clone(), doc(&id, 3))])
            .expect("batch_create"),
        vec![true],
        "5.6.7.4: the batch create of an expired id is the 5.6.1 create, which creates"
    );

    seed(&id);
    let deleted = s
        .batch_delete(&t, std::slice::from_ref(&id))
        .expect("batch_delete");
    assert!(
        deleted.is_empty(),
        "5.6.10.4: the batch delete of an expired id is the 5.6.6 delete, which finds nothing: {deleted:?}"
    );

    seed(&id);
    let upserted = s
        .batch_upsert_replace(&t, &[(id.clone(), doc(&id, 4))])
        .expect("batch_upsert");
    assert_eq!(
        upserted
            .iter()
            .map(|(created, _)| *created)
            .collect::<Vec<_>>(),
        vec![true],
        "5.6.8.4: an expired entity does not exist, so the batch upsert creates it"
    );

    // 5.6.9.4 (p.175): "For each of the NGSI-LD Entities included in the
    // input Array execute the behaviour defined by clause 5.6.3, but limited
    // to a local operation" — and 5.6.3 on an absent entity is
    // ResourceNotFound, which this seam reports as `None`. `batch_mutate`
    // carries `entityOperations/update` and `/merge`; without the guard both
    // reported the id in the SUCCESS array for an entity every read calls
    // absent, wrote to it, and emitted a change notification for it — the
    // merge form able to set a future `expiresAt` and resurrect an entity
    // the single-entity PATCH cannot touch.
    seed(&id);
    let single_patch = s.mutate(&t, single, |_d| Ok::<(), ()>(())).expect("mutate");
    seed(single);
    assert!(
        s.mutate(&t, single, |_d| Ok::<(), ()>(()))
            .expect("mutate")
            .is_none(),
        "5.6.3: an expired entity cannot be updated ({single_patch:?})"
    );
    let mutated = s
        .batch_mutate(&t, std::slice::from_ref(&id), |_id, _d| Ok::<(), ()>(()))
        .expect("batch_mutate");
    assert_eq!(mutated.len(), 1, "one id in, one answer out: {mutated:?}");
    assert!(
        mutated[0].is_none(),
        "5.6.9.4: the batch update of an expired id is the 5.6.3 update, \
         which finds nothing: {mutated:?}"
    );

    let _ = s.delete(&t, &id);
    let _ = s.delete(&t, single);
}

/// 5.5.6 licenses TooManyResults for "a query operation … producing so many
/// results that can potentially exhaust client or server resources", and
/// `list` carries that ceiling. `list_page` exists for the readers that are
/// not query operations and must see every row — 5.9.2.4's
/// registration-vs-entity conflict check among them, which has no
/// TooManyResults to raise and would otherwise refuse every redirect
/// registration on a tenant that outgrew the ceiling. A page bounds the
/// allocation by construction, so it is served whatever the tenant holds,
/// and it is read with a keyset rather than built by materializing the
/// tenant and slicing it.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_5_6_a_page_is_served_where_the_whole_list_is_refused() {
    use antares_sql::store::pg::entity::{ngsi_error, MAX_UNDECIDED_ROWS};

    let url = require_db!();
    let pool = pg::connect(&url, 5).await.expect("connect");
    let s = PgEntityStore::new(pool.clone());
    let t = TenantId::new("pgpagewalk").expect("tenant");
    pg::ensure_tenant(&pool, &t).await.expect("tenant row");

    let over = MAX_UNDECIDED_ROWS + 500;
    let mut tx = pool.begin().await.expect("tx");
    pg::set_tenant(&mut tx, &t).await.expect("set_tenant");
    sqlx::query("DELETE FROM entities WHERE tenant_id = $1")
        .bind(t.as_str())
        .execute(&mut *tx)
        .await
        .expect("clean");
    sqlx::query(
        "INSERT INTO entities (tenant_id, id, entity, types, created_at, modified_at)
         SELECT $1, 'urn:ngsi-ld:Ceiling:' || lpad(i::text, 6, '0'),
                jsonb_build_object('id', 'urn:ngsi-ld:Ceiling:' || lpad(i::text, 6, '0'),
                                   'type', 'Test'),
                '{Test}', now(), now()
           FROM generate_series(1, $2) AS i",
    )
    .bind(t.as_str())
    .bind(over)
    .execute(&mut *tx)
    .await
    .expect("bulk insert");
    tx.commit().await.expect("commit");

    let refused = s
        .list(&t)
        .expect_err("a tenant over the ceiling refuses `list`");
    assert_eq!(
        ngsi_error(&refused).map(antares_model::NgsiError::kind),
        Some("TooManyResults"),
        "the ceiling is the 5.5.6 error, not a driver failure"
    );

    let page = s
        .list_page(&t, None, 1_000)
        .expect("a page is never refused");
    assert_eq!(page.len(), 1_000, "a full page is a full page");
    let first: Vec<&str> = page.iter().filter_map(|d| d["id"].as_str()).collect();
    assert_eq!(
        first[0], "urn:ngsi-ld:Ceiling:000001",
        "id-ordered from the start"
    );
    assert!(first.windows(2).all(|w| w[0] < w[1]), "ascending id order");

    // The cursor is exclusive and the walk keeps moving past the ceiling.
    let next = s
        .list_page(&t, Some(first[999]), 3)
        .expect("a page is never refused");
    let next: Vec<&str> = next.iter().filter_map(|d| d["id"].as_str()).collect();
    assert_eq!(
        next,
        [
            "urn:ngsi-ld:Ceiling:001001",
            "urn:ngsi-ld:Ceiling:001002",
            "urn:ngsi-ld:Ceiling:001003"
        ]
    );

    // The last page is short, which is how a walker learns it is done.
    let tail = s
        .list_page(
            &t,
            Some(&format!("urn:ngsi-ld:Ceiling:{:06}", over - 2)),
            1_000,
        )
        .expect("a page is never refused");
    assert_eq!(tail.len(), 2, "the tail page is short, not padded");

    // The same, through the seam `antares-api` calls: the driver trait, not
    // the store type. This is the read the conflict check makes.
    let store = antares_sql::store::any::AnyStore::Pg(antares_sql::store::any::PgBackend::new(
        pool.clone(),
    ));
    let page = antares_store::CurrentStateDriver::list_page(
        &store,
        &t,
        antares_sql::store::Kind::Entity,
        None,
        1_000,
    )
    .expect("the seam serves a page of a tenant over the ceiling");
    assert_eq!(
        page.len(),
        1_000,
        "a full page is a full page at the seam too"
    );

    let mut tx = pool.begin().await.expect("tx");
    pg::set_tenant(&mut tx, &t).await.expect("set_tenant");
    sqlx::query("DELETE FROM entities WHERE tenant_id = $1")
        .bind(t.as_str())
        .execute(&mut *tx)
        .await
        .expect("clean");
    tx.commit().await.expect("commit");
}
