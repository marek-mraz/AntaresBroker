// SPDX-License-Identifier: EUPL-1.2
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

use antares_sql::store::pg;

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

/// 5.12 registration matching must be index-shaped: the whole point of
/// narrowing candidates through `csource_index` is to stop reading every
/// registration of the tenant per federated request. The type dimension has to
/// resolve through `i_csource_index_type` on BOTH arms — the named types and
/// the "unconstrained" `entity_type IS NULL` one, which is a second index probe
/// and not a filter over the table. Rows are seeded (and ANALYZEd) because on
/// an empty table the choice between two `(tenant_id, …)` indexes is a coin
/// flip that proves nothing.
#[tokio::test]
async fn registration_matching_uses_the_csource_index() {
    let url = require_db!();
    let pool = pg::connect(&url, 5).await.expect("connect");
    let seed = async {
        let mut tx = pool.begin().await.expect("tx");
        sqlx::query("DELETE FROM csource_registrations WHERE tenant_id = 'pgexplain'")
            .execute(&mut *tx)
            .await
            .expect("clean");
        sqlx::query(
            "INSERT INTO csource_registrations (tenant_id, id, registration)
             SELECT 'pgexplain', 'urn:csr:x'||g, jsonb_build_object('id', 'urn:csr:x'||g)
               FROM generate_series(1, 2000) g",
        )
        .execute(&mut *tx)
        .await
        .expect("seed regs");
        sqlx::query(
            "INSERT INTO csource_index
               (tenant_id, registration_id, entity_type, endpoint, mode, ops)
             SELECT 'pgexplain', 'urn:csr:x'||g, 'T'||(g % 200), 'http://cs:9090', 1, 0
               FROM generate_series(1, 2000) g",
        )
        .execute(&mut *tx)
        .await
        .expect("seed index");
        sqlx::query("ANALYZE csource_index")
            .execute(&mut *tx)
            .await
            .expect("analyze");
        tx.commit().await.expect("commit");
    };
    seed.await;

    let plan = plan_of(
        &pool,
        r#"SELECT DISTINCT r.id, r.registration
             FROM csource_registrations r
             JOIN csource_index x
               ON x.tenant_id = r.tenant_id AND x.registration_id = r.id
            WHERE r.tenant_id = 'pgexplain'
              AND (x.entity_type IS NULL OR x.entity_type = ANY(ARRAY['T7']))
            ORDER BY r.id LIMIT 10000"#,
    )
    .await;
    assert!(
        plan.contains("i_csource_index_type"),
        "type narrowing no longer routes through its index — plan:\n{plan}"
    );
    assert!(
        !plan.contains("Seq Scan on csource_index"),
        "the candidate set must never be a full read of csource_index — plan:\n{plan}"
    );

    // The id dimension can only narrow by tenant: `idPattern` rows must survive
    // every id query (only the matcher owns the regex), so the disjunct stays a
    // recheck on top of the index — but a full read of the table is still out.
    let plan = plan_of(
        &pool,
        r#"SELECT DISTINCT r.id, r.registration
             FROM csource_registrations r
             JOIN csource_index x
               ON x.tenant_id = r.tenant_id AND x.registration_id = r.id
            WHERE r.tenant_id = 'pgexplain'
              AND (x.entity_id IS NULL OR x.id_pattern IS NOT NULL
                   OR x.entity_id = ANY(ARRAY['urn:e:1']))
            ORDER BY r.id LIMIT 10000"#,
    )
    .await;
    assert!(
        plan.contains("i_csource_index"),
        "id narrowing no longer routes through an index — plan:\n{plan}"
    );
    assert!(
        !plan.contains("Seq Scan on csource_index"),
        "id narrowing degenerated into a full read — plan:\n{plan}"
    );

    sqlx::query("DELETE FROM csource_registrations WHERE tenant_id = 'pgexplain'")
        .execute(&pool)
        .await
        .expect("cleanup");
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
