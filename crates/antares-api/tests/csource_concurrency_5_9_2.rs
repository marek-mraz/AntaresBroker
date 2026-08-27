// SPDX-License-Identifier: EUPL-1.2
//! 5.9.2 Register Context Source under concurrency: many registrations
//! created at once must all land, and the broker must keep answering
//! while they do. The check-then-write section is serialized (5.9.2.4
//! conflict rules), and that serialization must never park a runtime
//! worker: the Postgres driver runs its queries on the runtime's I/O
//! driver, so a worker blocked on a plain mutex while another worker
//! waits on Postgres under it is a deadlock that takes `/q/health` down
//! with it. Skips without ANTARES_TEST_DATABASE_URL.

use antares_api::AppState;
use antares_model::TenantId;
use antares_sql::store::any::{AnyStore, PgBackend};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use std::sync::Arc;
use std::time::Duration;
use tower::ServiceExt;

const CTX: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld";

fn registration(tenant: &str, k: usize) -> String {
    format!(
        r#"{{"id":"urn:ngsi-ld:ContextSourceRegistration:{tenant}:{k}","type":"ContextSourceRegistration",
            "information":[{{"entities":[{{"type":"Vehicle","idPattern":"urn:ngsi-ld:Vehicle:{k}:.*"}}]}}],
            "endpoint":"http://csr.example.test/{k}","@context":"{CTX}"}}"#
    )
}

/// Twelve workers, 256 concurrent creates: with a worker-parking lock the
/// burst wedges the runtime; with an async lock every create answers 201.
#[tokio::test(flavor = "multi_thread", worker_threads = 12)]
async fn concurrent_registration_creates_all_land_and_health_stays_up() {
    let Ok(url) = std::env::var("ANTARES_TEST_DATABASE_URL") else {
        eprintln!("SKIP: ANTARES_TEST_DATABASE_URL not set");
        return;
    };
    let pool = antares_sql::store::pg::connect(&url, 5)
        .await
        .expect("connect");
    let tenant = format!(
        "csrconc{}",
        &uuid::Uuid::new_v4().simple().to_string()[..10]
    );
    antares_sql::store::pg::ensure_tenant(&pool, &TenantId::new(&tenant).expect("tenant"))
        .await
        .expect("tenant row");
    let st = AppState::with_store(
        "antares1".into(),
        Arc::new(AnyStore::Pg(PgBackend::new(pool))),
        antares_sql::StoreMode::Postgres,
    );

    let mut tasks = Vec::new();
    for k in 0..256 {
        let st = st.clone();
        let tenant = tenant.clone();
        tasks.push(tokio::spawn(async move {
            let body = registration(&tenant, k);
            let req = Request::builder()
                .method("POST")
                .uri("/ngsi-ld/v1/csourceRegistrations")
                .header("Content-Type", "application/ld+json")
                .header("Content-Length", body.len())
                .header("NGSILD-Tenant", &tenant)
                .body(Body::from(body))
                .expect("request");
            antares_api::router(st)
                .oneshot(req)
                .await
                .expect("response")
                .status()
        }));
    }
    let health = {
        let st = st.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let req = Request::builder()
                .uri("/q/health")
                .body(Body::empty())
                .expect("request");
            antares_api::router(st)
                .oneshot(req)
                .await
                .expect("response")
                .status()
        })
    };

    let all = async {
        let mut codes = Vec::new();
        for t in tasks {
            codes.push(t.await.expect("task"));
        }
        (codes, health.await.expect("health task"))
    };
    let (codes, health) = tokio::time::timeout(Duration::from_secs(60), all)
        .await
        .expect(
        "256 concurrent registration creates must finish; a hang here is a worker parked on a lock",
    );
    assert_eq!(
        health,
        StatusCode::OK,
        "health must answer during the burst"
    );
    assert!(
        codes.iter().all(|c| *c == StatusCode::CREATED),
        "every create answers 201, got {codes:?}"
    );
}
