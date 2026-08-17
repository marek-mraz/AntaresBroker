//! Transactional outbox atomicity + entity_maps TTL sweep.
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

/// The outbox INSERT lives or dies WITH the surrounding transaction.
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
    // 1000, not 10: sibling tests enqueue concurrently and a small page can
    // miss our row without any bug existing
    let page = outbox::peek(&pool, 1000).expect("peek");
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

    // Drain ack removes EXACTLY the published row: acking one seq must delete
    // one row. `>= 1` would also pass an over-deleting range ack, which is
    // precisely the regression the sibling test above exists to catch.
    let before = outbox::peek(&pool, 1000).expect("peek before ack").len();
    let acked = outbox::ack(&pool, &[seq]).expect("ack");
    assert_eq!(acked, 1, "acking one seq deleted {acked} rows");
    let page = outbox::peek(&pool, 1000).expect("peek3");
    assert!(!page.iter().any(|(s, _, _)| *s == seq));
    // sibling tests enqueue concurrently, so the page can only GROW past the
    // one row we removed — it must never shrink by more than that
    assert!(
        page.len() + 1 >= before,
        "the ack took {} extra rows with it",
        before - page.len() - 1
    );
}

/// entity_maps: per-row registration ids + TTL sweep.
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
    // Regression: each row keeps ITS OWN registration id, never the first's
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

/// 5.5.14: an expired EntityMap "cannot be accessed". The TTL sweep lags by
/// design (it runs on a timer), so the read itself must refuse the rows —
/// paging a map past its expiry until the sweep catches up serves entity
/// positions the broker has already promised to forget.
#[tokio::test(flavor = "multi_thread")]
async fn entity_maps_page_refuses_a_map_past_its_ttl() {
    let url = require_db!();
    let pool = pg::connect(&url, 5).await.expect("pool");
    let t = TenantId::new("pgmapsttl").expect("tenant");
    pg::ensure_tenant(&pool, &t).await.expect("tenant row");
    let maps = EntityMapStore::new(pool.clone());

    maps.put(
        &t,
        "urn:map:ttl",
        "chk",
        "2020-01-01T00:00:00Z",
        &[("urn:e:1".into(), "urn:reg:A".into(), None)],
    )
    .expect("put expired");
    // no sweep in between: the read alone must refuse it
    assert!(
        maps.page(&t, "urn:map:ttl", 0, 10)
            .expect("page")
            .is_empty(),
        "an expired map must not be pageable before the sweep runs"
    );
}

/// Tenant isolation (the explicit predicate plus the RLS policy): a map
/// materialized for one tenant is invisible to every other one, including at
/// an offset past its first page.
#[tokio::test(flavor = "multi_thread")]
async fn entity_maps_never_page_across_tenants() {
    let url = require_db!();
    let pool = pg::connect(&url, 5).await.expect("pool");
    let owner = TenantId::new("pgmapsowner").expect("tenant");
    let other = TenantId::new("pgmapsother").expect("tenant");
    pg::ensure_tenant(&pool, &owner).await.expect("owner row");
    pg::ensure_tenant(&pool, &other).await.expect("other row");
    let maps = EntityMapStore::new(pool.clone());

    maps.put(
        &owner,
        "urn:map:shared-id",
        "chk",
        "2030-01-01T00:00:00Z",
        &[
            ("urn:e:1".into(), "urn:reg:A".into(), None),
            ("urn:e:2".into(), "urn:reg:B".into(), None),
        ],
    )
    .expect("put");
    assert_eq!(
        maps.page(&owner, "urn:map:shared-id", 0, 10)
            .expect("own")
            .len(),
        2
    );
    assert!(
        maps.page(&other, "urn:map:shared-id", 0, 10)
            .expect("cross-tenant page")
            .is_empty(),
        "another tenant's map id must resolve to nothing"
    );
    // and a page BEYOND the map is empty rather than wrapping to position 0
    assert!(maps
        .page(&owner, "urn:map:shared-id", 2, 10)
        .expect("past end")
        .is_empty());
}

/// A re-materialized map replaces its predecessor: `put` clears the old rows
/// first, so a shrinking map cannot page entries that no longer belong to it.
#[tokio::test(flavor = "multi_thread")]
async fn entity_maps_put_replaces_the_previous_materialization() {
    let url = require_db!();
    let pool = pg::connect(&url, 5).await.expect("pool");
    let t = TenantId::new("pgmapsreplace").expect("tenant");
    pg::ensure_tenant(&pool, &t).await.expect("tenant row");
    let maps = EntityMapStore::new(pool.clone());

    maps.put(
        &t,
        "urn:map:r",
        "chk1",
        "2030-01-01T00:00:00Z",
        &[
            ("urn:e:1".into(), "urn:reg:A".into(), None),
            ("urn:e:2".into(), "urn:reg:B".into(), None),
        ],
    )
    .expect("put");
    maps.put(
        &t,
        "urn:map:r",
        "chk2",
        "2030-01-01T00:00:00Z",
        &[("urn:e:9".into(), "urn:reg:Z".into(), None)],
    )
    .expect("re-put");
    let page = maps.page(&t, "urn:map:r", 0, 10).expect("page");
    assert_eq!(page, vec![("urn:e:9".to_owned(), "urn:reg:Z".to_owned())]);
    assert!(
        !page.iter().any(|(e, _)| e == "urn:e:1"),
        "a stale entry from the previous materialization must not survive"
    );
}

/// An empty batch enqueues nothing at all — the guard has to hold inside the
/// caller's transaction, where a zero-row INSERT would still be a round trip.
#[tokio::test(flavor = "multi_thread")]
async fn outbox_enqueue_many_of_nothing_writes_nothing() {
    let url = require_db!();
    let pool = pg::connect(&url, 5).await.expect("pool");
    let t = TenantId::new("pgoutboxempty").expect("tenant");
    pg::ensure_tenant(&pool, &t).await.expect("tenant row");

    let mut tx = pool.begin().await.expect("tx");
    pg::set_tenant(&mut tx, &t).await.expect("set tenant");
    outbox::enqueue_many(&mut tx, &t, &[])
        .await
        .expect("empty batch");
    tx.commit().await.expect("commit");

    let page = outbox::peek(&pool, 1000).expect("peek");
    assert!(
        !page.iter().any(|(_, tn, _)| tn == "pgoutboxempty"),
        "an empty batch must leave no rows behind"
    );
}

/// Ack must delete EXACTLY the
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

    // Ack exactly OUR published seq — not the whole peeked page: sibling
    // tests share this table and acking their in-flight rows steals them
    // (CI #101 flake: the transactional test's own ack then deletes 0 rows).
    // The regression power is unchanged — under the old blanket
    // `DELETE WHERE seq <= max` an ack of just seq_b still kills seq_a.
    outbox::ack(&pool, &[seq_b]).expect("ack");

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
