// SPDX-License-Identifier: EUPL-1.2
//! Transactional outbox atomicity.
//! Skips loudly without ANTARES_TEST_DATABASE_URL.

use antares_model::TenantId;
use antares_sql::store::pg;
use antares_sql::store::pg::outbox;

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
    let page = outbox::peek(&pool, 1000).await.expect("peek");
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
    let page = outbox::peek(&pool, 100).await.expect("peek2");
    assert!(
        !page.iter().any(|(s, _, _)| *s == seq2),
        "a rolled-back write must take its outbox event down with it"
    );

    // Drain ack removes EXACTLY the published row: acking one seq must delete
    // one row. `>= 1` would also pass an over-deleting range ack, which is
    // precisely the regression the sibling test above exists to catch.
    //
    // Counted over THIS tenant's rows, straight from the table. The shared
    // outbox is not a quantity this test can assert: sibling tests both
    // enqueue and ack on it concurrently, so a whole-table page shrinks for
    // reasons that are not this ack, and `peek`'s own LIMIT can truncate the
    // page before our rows are even in it. `pgoutbox` is used by no other
    // test, so its count is exact whatever the siblings are doing, and a
    // difference of one is a stronger statement than the page ever made.
    let mine = || async {
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM outbox WHERE tenant_id = 'pgoutbox'")
            .fetch_one(&pool)
            .await
            .expect("count")
    };
    let before = mine().await;
    let acked = outbox::ack(&pool, &[seq]).await.expect("ack");
    assert_eq!(acked, 1, "acking one seq deleted {acked} rows");
    let page = outbox::peek(&pool, 1000).await.expect("peek3");
    assert!(!page.iter().any(|(s, _, _)| *s == seq));
    assert_eq!(
        mine().await,
        before - 1,
        "the ack took rows of this tenant with it beyond the one seq acked"
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

    let page = outbox::peek(&pool, 1000).await.expect("peek");
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
    let page = outbox::peek(&pool, 1000).await.expect("peek");
    let published: Vec<i64> = page.iter().map(|(s, _, _)| *s).collect();
    assert!(published.contains(&seq_b));
    assert!(
        !published.contains(&seq_a),
        "A is uncommitted, not peekable"
    );

    // A commits between peek and ack — the loss window
    tx_a.commit().await.expect("commit a");

    // Ack exactly OUR published seq — not the whole peeked page: sibling
    // tests share this table, so acking their in-flight rows steals them and
    // the sibling's own ack then deletes nothing. The regression power is
    // unchanged — under a blanket `DELETE WHERE seq <= max` an ack of just
    // seq_b still kills seq_a.
    outbox::ack(&pool, &[seq_b]).await.expect("ack");

    // the gap row MUST survive to be published next round
    let page = outbox::peek(&pool, 1000).await.expect("peek 2");
    assert!(
        page.iter().any(|(s, _, _)| *s == seq_a),
        "a row committing between peek and ack must never be deleted unpublished"
    );
    assert!(
        !page.iter().any(|(s, _, _)| *s == seq_b),
        "the published row is acked"
    );
    // cleanup: drain the survivor
    outbox::ack(&pool, &[seq_a]).await.expect("ack survivor");
}

/// A row the drain kept because the bus could not carry its bodies leaves the
/// drain's page and stays readable by seq — that row is the only remaining
/// copy of the before-image the published message dropped.
#[tokio::test(flavor = "multi_thread")]
async fn a_retained_row_leaves_the_page_and_stays_readable_until_it_is_reaped() {
    let url = require_db!();
    let pool = pg::connect(&url, 5).await.expect("pool");
    let t = TenantId::new("pgretain").expect("tenant");
    let other = TenantId::new("pgretainother").expect("tenant");
    pg::ensure_tenant(&pool, &t).await.expect("tenant row");
    pg::ensure_tenant(&pool, &other).await.expect("tenant row");

    let mut tx = pool.begin().await.expect("tx");
    pg::set_tenant(&mut tx, &t).await.expect("set tenant");
    let seq = outbox::enqueue(
        &mut tx,
        &t,
        &serde_json::json!({"op": "update", "id": "urn:o:big", "prev_payload": {"was": "here"}}),
    )
    .await
    .expect("enqueue");
    tx.commit().await.expect("commit");

    assert_eq!(outbox::retain(&pool, &t, &[seq]).await.expect("retain"), 1);
    let page = outbox::peek(&pool, 1000).await.expect("peek");
    assert!(
        !page.iter().any(|(s, _, _)| *s == seq),
        "a retained row must not be published a second time"
    );
    let kept = outbox::event(&pool, seq, &t)
        .await
        .expect("event")
        .expect("the retained row");
    assert_eq!(kept["prev_payload"]["was"], "here");
    assert!(
        outbox::event(&pool, seq, &other)
            .await
            .expect("event")
            .is_none(),
        "a claim-check reference must not resolve inside another tenant"
    );

    // Window 0 puts every stamped row past the horizon; a pending row carries
    // no stamp, and `NULL < now()` is never true, so the drain's queue is
    // untouched by the reap.
    assert!(outbox::reap_published(&pool, 0).await.expect("reap") >= 1);
    assert!(outbox::event(&pool, seq, &t)
        .await
        .expect("event")
        .is_none());
}

/// The retain runs as the deployed role, not as a superuser. `outbox`
/// carries FORCE ROW LEVEL SECURITY and its UPDATE policy takes no
/// `antares.service` escape (0005 removed it: an escaped UPDATE can move a
/// row into another tenant). A retain that armed the escape instead of the
/// row's tenant updates nothing here — silently, because a no-op UPDATE is
/// not an error — and the drain republishes that row on every page for as
/// long as the broker runs.
#[tokio::test(flavor = "multi_thread")]
async fn retain_and_read_back_hold_under_the_non_superuser_role() {
    let url = require_db!();
    let admin = pg::connect(&url, 5).await.expect("connect");
    for stmt in [
        "DO $$ BEGIN CREATE ROLE antares_app LOGIN PASSWORD 'app';
         EXCEPTION WHEN duplicate_object THEN NULL; END $$",
        "GRANT USAGE ON SCHEMA public TO antares_app",
        "GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO antares_app",
    ] {
        sqlx::query(stmt).execute(&admin).await.expect(stmt);
    }
    let app_url = url.replace("antares:antares@", "antares_app:app@");
    assert_ne!(app_url, url, "test URL must embed antares:antares creds");
    let app = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .connect(&app_url)
        .await
        .expect("app-role connect");

    let t = TenantId::new("pgretainrls").expect("tenant");
    pg::ensure_tenant(&admin, &t).await.expect("tenant row");
    let mut tx = app.begin().await.expect("tx");
    pg::set_tenant(&mut tx, &t).await.expect("set tenant");
    let seq = outbox::enqueue(&mut tx, &t, &serde_json::json!({"id": "urn:o:rls"}))
        .await
        .expect("enqueue");
    tx.commit().await.expect("commit");

    assert_eq!(
        outbox::retain(&app, &t, &[seq]).await.expect("retain"),
        1,
        "the retain updated no row: the drain will republish it forever"
    );
    assert_eq!(
        outbox::event(&app, seq, &t)
            .await
            .expect("event")
            .expect("the retained row")["id"],
        "urn:o:rls"
    );
    outbox::reap_published(&app, 0).await.expect("reap");
}
