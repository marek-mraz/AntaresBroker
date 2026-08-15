//! C8: transactional outbox atomicity + entity_maps TTL sweep.
//! Skips loudly without ANTARES_TEST_DATABASE_URL.

use antares_model::TenantId;
use antares_sql::pg;
use antares_sql::store::entity_map::EntityMapStore;
use antares_sql::store::outbox;

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

/// §10: the outbox INSERT lives or dies WITH the surrounding transaction.
#[tokio::test(flavor = "multi_thread")]
async fn outbox_enqueue_is_transactional() {
    let url = require_db!();
    let pool = pg::connect(&url, 5).await.expect("pool");
    let t = TenantId::new("pgoutbox").expect("tenant");
    pg::ensure_tenant(&pool, &t).await.expect("tenant row");

    // committed tx → row visible to the drain
    let mut tx = pool.begin().await.expect("tx");
    pg::set_tenant(&mut tx, &t).await.expect("set tenant");
    let seq = outbox::enqueue(
        &mut tx,
        &t,
        &serde_json::json!({"op": "create", "id": "urn:o:1"}),
    )
    .await
    .expect("enqueue");
    tx.commit().await.expect("commit");
    let page = outbox::peek(&pool, 10).expect("peek");
    assert!(page
        .iter()
        .any(|(s, tn, e)| *s == seq && tn == "pgoutbox" && e["id"] == "urn:o:1"));

    // rolled-back tx → the event never existed
    let mut tx = pool.begin().await.expect("tx2");
    pg::set_tenant(&mut tx, &t).await.expect("set tenant");
    let seq2 = outbox::enqueue(
        &mut tx,
        &t,
        &serde_json::json!({"op": "create", "id": "urn:o:2"}),
    )
    .await
    .expect("enqueue2");
    tx.rollback().await.expect("rollback");
    let page = outbox::peek(&pool, 100).expect("peek2");
    assert!(
        !page.iter().any(|(s, _, _)| *s == seq2),
        "a rolled-back write must take its outbox event down with it"
    );

    // drain ack removes exactly the published row
    let acked = outbox::ack(&pool, &[seq]).expect("ack");
    assert!(acked >= 1);
    let page = outbox::peek(&pool, 100).expect("peek3");
    assert!(!page.iter().any(|(s, _, _)| *s == seq));
}

/// §8.3 entity_maps: per-row registration ids (the B1 class) + TTL sweep.
#[tokio::test(flavor = "multi_thread")]
async fn entity_maps_pages_and_ttl_sweep() {
    let url = require_db!();
    let pool = pg::connect(&url, 5).await.expect("pool");
    let t = TenantId::new("pgmaps").expect("tenant");
    pg::ensure_tenant(&pool, &t).await.expect("tenant row");
    let maps = EntityMapStore::new(pool.clone());

    maps.put(
        &t,
        "urn:map:1",
        "chk1",
        "2030-01-01T00:00:00Z",
        &[
            ("urn:e:1".into(), "urn:reg:A".into(), None),
            ("urn:e:2".into(), "urn:reg:B".into(), Some("type=T".into())),
        ],
    )
    .expect("put");
    let page = maps.page(&t, "urn:map:1", 0, 10).expect("page");
    // B1 regression: each row keeps ITS OWN registration id, never the first's
    assert_eq!(
        page,
        vec![
            ("urn:e:1".to_owned(), "urn:reg:A".to_owned()),
            ("urn:e:2".to_owned(), "urn:reg:B".to_owned())
        ]
    );

    // an expired map is swept; the live one survives
    maps.put(
        &t,
        "urn:map:old",
        "chk2",
        "2020-01-01T00:00:00Z",
        &[("urn:e:9".into(), "urn:reg:C".into(), None)],
    )
    .expect("put old");
    let swept = maps.sweep().expect("sweep");
    assert!(swept >= 1, "expired map rows swept");
    assert!(maps.page(&t, "urn:map:old", 0, 10).expect("old").is_empty());
    assert_eq!(maps.page(&t, "urn:map:1", 0, 10).expect("live").len(), 2);
}

/// P0-6 (production-readiness audit 2026-08-09): ack must delete EXACTLY the
/// published seqs. bigserial allocates at INSERT and transactions commit out
/// of order — a lower-seq row whose transaction commits BETWEEN peek and ack
/// must survive the ack and be published next round. The blanket
/// `DELETE WHERE seq <= max` deleted it unpublished: silent event loss under
/// write concurrency.
#[tokio::test(flavor = "multi_thread")]
async fn outbox_ack_never_deletes_an_unpublished_gap_row() {
    let url = require_db!();
    let pool = pg::connect(&url, 5).await.expect("pool");
    let t = TenantId::new("pgoutboxgap").expect("tenant");
    pg::ensure_tenant(&pool, &t).await.expect("tenant row");

    // tx A takes the LOWER seq and stays open across the drain cycle
    let mut tx_a = pool.begin().await.expect("tx a");
    pg::set_tenant(&mut tx_a, &t).await.expect("set tenant");
    let seq_a = outbox::enqueue(&mut tx_a, &t, &serde_json::json!({"id": "urn:gap:a"}))
        .await
        .expect("enqueue a");

    // tx B takes a HIGHER seq and commits first
    let mut tx_b = pool.begin().await.expect("tx b");
    pg::set_tenant(&mut tx_b, &t).await.expect("set tenant");
    let seq_b = outbox::enqueue(&mut tx_b, &t, &serde_json::json!({"id": "urn:gap:b"}))
        .await
        .expect("enqueue b");
    tx_b.commit().await.expect("commit b");
    assert!(seq_b > seq_a, "B allocated after A");

    // the drain peeks: only B is visible
    let page = outbox::peek(&pool, 1000).expect("peek");
    let published: Vec<i64> = page.iter().map(|(s, _, _)| *s).collect();
    assert!(published.contains(&seq_b));
    assert!(
        !published.contains(&seq_a),
        "A is uncommitted, not peekable"
    );

    // A commits between peek and ack — the audited loss window
    tx_a.commit().await.expect("commit a");

    // ack what was actually published
    outbox::ack(&pool, &published).expect("ack");

    // the gap row MUST survive to be published next round
    let page = outbox::peek(&pool, 1000).expect("peek 2");
    assert!(
        page.iter().any(|(s, _, _)| *s == seq_a),
        "a row committing between peek and ack must never be deleted unpublished"
    );
    assert!(
        !page.iter().any(|(s, _, _)| *s == seq_b),
        "the published row is acked"
    );
    // cleanup: drain the survivor
    outbox::ack(&pool, &[seq_a]).expect("ack survivor");
}
