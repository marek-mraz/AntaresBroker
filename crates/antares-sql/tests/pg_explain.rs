//! EXPLAIN-based index-usability regression tests: a review once
//! found three of five entity indexes dead — the compiled q= used the
//! function form `jsonb_path_exists()` the GIN index can never match, and the
//! geo predicate's `location IS NULL OR …` guard defeated the GIST index.
//! These tests pin the repaired shapes: with `enable_seqscan = off` the
//! planner MUST be able to route each predicate through its index — a
//! sequential scan in the plan means the index went dead again.
//!
//! Literal values are inlined (EXPLAIN cannot carry extended-protocol binds);
//! this is test-only SQL, never a pattern for request paths.

use antares_sql::pg;

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

async fn plan_of(pool: &sqlx::PgPool, sql: &str) -> String {
    let mut tx = pool.begin().await.expect("tx");
    sqlx::query("SET LOCAL enable_seqscan = off")
        .execute(&mut *tx)
        .await
        .expect("seqscan off");
    // the pentest role is not in play here; run as-is
    let rows: Vec<String> =
        sqlx::query_scalar(sqlx::AssertSqlSafe(format!("EXPLAIN (FORMAT TEXT) {sql}")))
            .fetch_all(&mut *tx)
            .await
            .expect("explain");
    rows.join("\n")
}

#[tokio::test]
async fn q_predicate_uses_the_gin_jsonb_path_ops_index() {
    let url = require_db!();
    let pool = pg::connect(&url, 5).await.expect("connect");
    let plan = plan_of(
        &pool,
        // no tenant predicate: it would hand the planner the PK as an easy
        // index path and hide whether the GIN can serve the @? at all
        r#"SELECT id FROM entities
           WHERE entity @? '$."https://uri.etsi.org/ngsi-ld/default-context/speed"[*]."value" ? (@ > 20)'::jsonpath"#,
    )
    .await;
    assert!(
        plan.contains("i_entities_jsonb"),
        "q= predicate no longer routes through the GIN index — plan:\n{plan}"
    );
}

#[tokio::test]
async fn geo_predicate_bitmap_ors_gist_and_ambiguous_indexes() {
    let url = require_db!();
    let pool = pg::connect(&url, 5).await.expect("connect");
    let plan = plan_of(
        &pool,
        r#"SELECT id FROM entities
           WHERE ((ST_Within(location, ST_SetSRID(ST_GeomFromGeoJSON(
                     '{"type":"Polygon","coordinates":[[[0,0],[0,1],[1,1],[1,0],[0,0]]]}'), 4326)))
                  OR location_ambiguous)"#,
    )
    .await;
    assert!(
        plan.contains("i_entities_location"),
        "geo arm no longer routes through GIST — plan:\n{plan}"
    );
    assert!(
        plan.contains("i_entities_loc_ambiguous") || plan.contains("BitmapOr"),
        "ambiguous arm no longer index-shaped — plan:\n{plan}"
    );
}
