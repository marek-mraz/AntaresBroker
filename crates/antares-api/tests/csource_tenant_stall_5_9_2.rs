// SPDX-License-Identifier: EUPL-1.2
//! What one tenant's registration burst costs another tenant.
//!
//! 5.9.2.4 makes a registration create a check-then-write sequence: the
//! conflict rules ("if an exclusive or redirect Context Source Registration
//! already matches ... an error of type Conflict shall be raised") are
//! decided by reading the registration set, and the create that follows
//! writes it. The read and the write are separate store operations, so the
//! sequence is serialized or two conflicting registrations both land.
//!
//! The clause decides conflicts WITHIN a tenant — a registration in tenant A
//! can never conflict with one in tenant B, because the sets are disjoint.
//! So serializing across tenants is a cost the clause does not ask for, and
//! this file measures it before anything is changed: the stall tenant B sees
//! while tenant A creates a burst of idPattern-only registrations, which are
//! the expensive shape (5.9.2.4's overlap check walks undecided rows for a
//! pattern it cannot resolve by id).
//!
//! Skips without ANTARES_TEST_DATABASE_URL.
#![cfg(feature = "postgres")]

use antares_api::AppState;
use antares_model::TenantId;
use antares_sql::store::any::{AnyStore, PgBackend};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tower::ServiceExt;

const CTX: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld";

/// Rounds of the measurement, and the concurrency of one burst. Five rounds
/// is enough for a median to settle without the tenant's registration set
/// growing past what the overlap scan walks in a real deployment.
const ROUNDS: usize = 5;
const BURST: usize = 32;

/// The middle sample, which is what a run this noisy can report honestly.
fn median(mut v: Vec<Duration>) -> Duration {
    v.sort_unstable();
    v[v.len() / 2]
}

/// idPattern-only: no id list for the overlap check to resolve against, so
/// 5.9.2.4 falls back to the scan this box is about.
fn pattern_registration(tenant: &str, k: usize) -> String {
    format!(
        r#"{{"id":"urn:ngsi-ld:ContextSourceRegistration:{tenant}:{k}","type":"ContextSourceRegistration",
            "information":[{{"entities":[{{"type":"Vehicle","idPattern":"urn:ngsi-ld:Vehicle:{k}:.*"}}]}}],
            "endpoint":"http://csr.example.test/{k}","@context":"{CTX}"}}"#
    )
}

/// The control load: the same concurrency, the same tenant, the same store,
/// one write each — but through 5.6.1, which takes no registration lock. It
/// has to be a write of comparable cost, not a query: a query walks a set
/// that grows with every round and would read as a stall the lock never
/// caused. If B is as slow under this as under the create burst, the CPU
/// the burst costs is the cause and the lock is not.
fn control_entity(tenant: &str, k: usize) -> String {
    format!(
        r#"{{"id":"urn:ngsi-ld:Vehicle:{tenant}:{k}","type":"Vehicle",
            "speed":{{"type":"Property","value":{k}}},"@context":"{CTX}"}}"#
    )
}

async fn create_entity(st: &AppState, tenant: &str, body: String) -> StatusCode {
    let req = Request::builder()
        .method("POST")
        .uri("/ngsi-ld/v1/entities")
        .header("Content-Type", "application/ld+json")
        .header("Content-Length", body.len())
        .header("NGSILD-Tenant", tenant)
        .body(Body::from(body))
        .expect("request");
    antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("response")
        .status()
}

async fn create(st: &AppState, tenant: &str, body: String) -> StatusCode {
    let req = Request::builder()
        .method("POST")
        .uri("/ngsi-ld/v1/csourceRegistrations")
        .header("Content-Type", "application/ld+json")
        .header("Content-Length", body.len())
        .header("NGSILD-Tenant", tenant)
        .body(Body::from(body))
        .expect("request");
    antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("response")
        .status()
}

/// The number the R4 decision rests on: B's create alone, then B's create
/// while A is 32 deep in idPattern-only creates, then B's create under the
/// same concurrency through the lock-free query path. One round is far too
/// noisy to read (a cold pool moves the baseline by tens of milliseconds),
/// so the loop runs five and reports medians. The test asserts only that
/// every create lands and that the stall is bounded well below a timeout —
/// the measurement it prints is what a change to the lock has to beat.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn a_registration_burst_in_one_tenant_stalls_another() {
    let Ok(url) = std::env::var("ANTARES_TEST_DATABASE_URL") else {
        eprintln!("SKIP: ANTARES_TEST_DATABASE_URL not set");
        return;
    };
    let pool = antares_sql::store::pg::connect(&url, 20)
        .await
        .expect("connect");
    // `sqlx` is not a dependency of this crate, so the pool is never named:
    // the two tenants are minted inline, through the store crate's own helper.
    let mut names = Vec::new();
    for prefix in ["stalla", "stallb"] {
        let name = format!(
            "{prefix}{}",
            &uuid::Uuid::new_v4().simple().to_string()[..10]
        );
        antares_sql::store::pg::ensure_tenant(&pool, &TenantId::new(&name).expect("tenant"))
            .await
            .expect("tenant row");
        names.push(name);
    }
    let b = names.pop().expect("tenant b");
    let a = names.pop().expect("tenant a");
    let st = AppState::with_store(
        "antares1".into(),
        Arc::new(AnyStore::Pg(PgBackend::new(pool))),
        "postgres",
    );

    // warm-up: the first create pays for the pool, the context and the
    // prepared statements, and belongs in no measurement
    assert_eq!(
        create(&st, &b, pattern_registration(&b, 0)).await,
        StatusCode::CREATED
    );

    let mut quiet = Vec::new();
    let mut under_writes = Vec::new();
    let mut under_plain = Vec::new();
    for round in 1..=ROUNDS {
        // baseline: B alone, on a quiet broker
        let t0 = Instant::now();
        assert_eq!(
            create(&st, &b, pattern_registration(&b, round * 10)).await,
            StatusCode::CREATED
        );
        quiet.push(t0.elapsed());

        // 32 concurrent idPattern-only creates in A, and B's create into them
        let mut burst = Vec::new();
        for k in 1..=BURST {
            let st = st.clone();
            let a = a.clone();
            burst.push(tokio::spawn(async move {
                create(&st, &a, pattern_registration(&a, round * 100 + k)).await
            }));
        }
        let t1 = Instant::now();
        let b_code = create(&st, &b, pattern_registration(&b, round * 10 + 1)).await;
        under_writes.push(t1.elapsed());
        for t in burst {
            assert_eq!(
                t.await.expect("join"),
                StatusCode::CREATED,
                "a burst create failed"
            );
        }
        assert_eq!(
            b_code,
            StatusCode::CREATED,
            "B's create must land regardless"
        );

        // control: the same 32-way concurrency of writes, no registration
        // lock taken
        let mut plain = Vec::new();
        for k in 1..=BURST {
            let st = st.clone();
            let a = a.clone();
            plain.push(tokio::spawn(async move {
                create_entity(&st, &a, control_entity(&a, round * 100 + k)).await
            }));
        }
        let t2 = Instant::now();
        let b_control = create(&st, &b, pattern_registration(&b, round * 10 + 2)).await;
        under_plain.push(t2.elapsed());
        for t in plain {
            assert_eq!(
                t.await.expect("join"),
                StatusCode::CREATED,
                "a control write failed"
            );
        }
        assert_eq!(b_control, StatusCode::CREATED);
    }

    let (quiet, locked, plain) = (median(quiet), median(under_writes), median(under_plain));
    eprintln!(
        "R4 MEASUREMENT rounds={ROUNDS} burst={BURST} quiet={quiet:?} \
         under_registration_burst={locked:?} under_entity_burst={plain:?} \
         stall={:?} control_stall={:?}",
        locked.saturating_sub(quiet),
        plain.saturating_sub(quiet)
    );
    assert!(
        locked < Duration::from_secs(20),
        "B waited {locked:?} behind another tenant's burst"
    );
}
