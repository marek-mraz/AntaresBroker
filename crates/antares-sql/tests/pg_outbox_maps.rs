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

    // drain ack removes everything up to seq
    let acked = outbox::ack(&pool, seq).expect("ack");
    assert!(acked >= 1);
    let page = outbox::peek(&pool, 100).expect("peek3");
    assert!(!page.iter().any(|(s, _, _)| *s <= seq));
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
