//! Loop detection over the Via header (6.3.17 / 6.3.18, Tables 5.2.9-1 and
//! 5.2.40-1), end to end through the router against a mock Context Source.
//!
//! The scenario is ETSI `D018_01`: register a source, create through it, read
//! the Via pseudonym the broker sent, then replay it on a DELETE. What the
//! spec makes of that replay depends on the registration:
//!
//! * a single **exclusive/redirect** source looping back is the one case
//!   6.3.17 p.278 answers with **508 Loop Detected**;
//! * an **inclusive** source is dropped from matching instead (Table 6.3.18-2)
//!   and the operation runs locally — 508 would fail an operation the broker
//!   can serve;
//! * the same chain in **another tenant** is not a loop at all: Table 5.2.40-1
//!   makes the alias tenant-specific, so `antares1` and `antares1~zvolen` are
//!   different Context Sources.

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tower::ServiceExt;

const ALIAS: &str = "antares1";
const ENTITY: &str = "urn:ngsi-ld:Vehicle:d018";

/// A Context Source that answers every request `204` and counts the hits.
fn mock_source() -> (u16, Arc<AtomicUsize>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let hits: Arc<AtomicUsize> = Arc::default();
    let seen = hits.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            seen.fetch_add(1, Ordering::SeqCst);
            let mut buf = [0u8; 8192];
            let _ = s.read(&mut buf);
            let _ = s.write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n");
        }
    });
    (port, hits)
}

async fn send(st: &AppState, req: Request<Body>) -> axum::http::Response<Body> {
    antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("response")
}

/// Register one Context Source for `ENTITY` in `tenant`.
async fn register(st: &AppState, tenant: Option<&str>, mode: &str, port: u16, alias: Option<&str>) {
    let mut doc = serde_json::json!({
        "id": format!("urn:ngsi-ld:ContextSourceRegistration:{mode}-{}", alias.unwrap_or("none")),
        "type": "ContextSourceRegistration",
        "mode": mode,
        "operations": ["redirectionOps"],
        "information": [{"entities": [{"type": "Vehicle", "id": ENTITY}]}],
        "endpoint": format!("http://127.0.0.1:{port}"),
    });
    if let Some(a) = alias {
        doc["contextSourceAlias"] = serde_json::json!(a);
    }
    let body = doc.to_string();
    let mut req = Request::builder()
        .method("POST")
        .uri("/ngsi-ld/v1/csourceRegistrations")
        .header("Content-Type", "application/json")
        .header("Content-Length", body.len());
    if let Some(t) = tenant {
        req = req.header("NGSILD-Tenant", t);
    }
    let res = send(st, req.body(Body::from(body)).expect("request")).await;
    assert_eq!(res.status(), StatusCode::CREATED, "registration create");
}

fn delete_with_via(tenant: Option<&str>, via: &str) -> Request<Body> {
    let mut req = Request::builder()
        .method("DELETE")
        .uri(format!("/ngsi-ld/v1/entities/{ENTITY}"))
        .header("Via", via);
    if let Some(t) = tenant {
        req = req.header("NGSILD-Tenant", t);
    }
    req.body(Body::empty()).expect("request")
}

fn state() -> AppState {
    // the mock source is loopback, denied by default (§16.4)
    std::env::set_var("ANTARES_EGRESS_ALLOW_PRIVATE", "true");
    AppState::new(ALIAS.into())
}

/// 6.3.17 p.278: "In the case of an exclusive or redirect registration, where
/// all of the data is held outside of the Context Broker and held in a single
/// registered source … 508 Loop Detected — if the single registered source
/// and tenant is registered to redirect back on to the Context Broker."
#[tokio::test(flavor = "multi_thread")]
async fn single_redirect_source_looping_back_is_508() {
    let st = state();
    let (port, _) = mock_source();
    register(&st, None, "redirect", port, None).await;

    let res = send(&st, delete_with_via(None, "1.1 antares1")).await;
    assert_eq!(res.status(), StatusCode::LOOP_DETECTED);
}

/// The same loop through an **inclusive** registration: Table 6.3.18-2 makes
/// the Via listing amend matching, so the forward is dropped and the
/// operation proceeds locally. (ETSI D018_01 registers `mode=inclusive` and
/// still asserts 508 — logged in error.md as a suite defect.)
#[tokio::test(flavor = "multi_thread")]
async fn inclusive_source_looping_back_runs_locally() {
    let st = state();
    let (port, hits) = mock_source();
    register(&st, None, "inclusive", port, None).await;

    let res = send(&st, delete_with_via(None, "1.1 antares1")).await;
    assert_ne!(
        res.status(),
        StatusCode::LOOP_DETECTED,
        "508 is scoped to a single exclusive/redirect source"
    );
    assert_eq!(
        hits.load(Ordering::SeqCst),
        0,
        "the looping registration must not be forwarded to"
    );
}

/// Table 5.2.40-1: "In the multi-tenancy use case … this id shall be
/// identifying a specific Tenant within a registered Context Source." A chain
/// naming this broker's DEFAULT tenant says nothing about tenant `zvolen`, so
/// federation there proceeds — one alias for the whole process turned every
/// cross-tenant federation inside one broker into a phantom loop.
#[tokio::test(flavor = "multi_thread")]
async fn a_chain_naming_another_tenant_is_not_a_loop() {
    let st = state();
    let (port, hits) = mock_source();
    register(&st, Some("zvolen"), "redirect", port, None).await;

    let res = send(&st, delete_with_via(Some("zvolen"), "1.1 antares1")).await;
    assert_ne!(
        res.status(),
        StatusCode::LOOP_DETECTED,
        "antares1 (default tenant) is not antares1~zvolen"
    );
    assert_eq!(hits.load(Ordering::SeqCst), 1, "the forward must happen");

    // …and the tenant-qualified pseudonym IS detected as the loop it is
    let res = send(&st, delete_with_via(Some("zvolen"), "1.1 antares1~zvolen")).await;
    assert_eq!(res.status(), StatusCode::LOOP_DETECTED);
    assert_eq!(hits.load(Ordering::SeqCst), 1, "no second forward");
}

/// Table 6.3.18-2 + 5.2.9: a registration carrying the `contextSourceAlias`
/// of a source already in the chain is not a matching registration — the
/// request has been there. Nothing is forwarded, and it is not this broker's
/// own loop, so no 508 either.
#[tokio::test(flavor = "multi_thread")]
async fn a_registered_source_already_in_the_chain_is_skipped() {
    let st = state();
    let (port, hits) = mock_source();
    register(&st, None, "redirect", port, Some("peer1")).await;

    let res = send(&st, delete_with_via(None, "1.1 peer1")).await;
    assert_ne!(res.status(), StatusCode::LOOP_DETECTED);
    assert_eq!(
        hits.load(Ordering::SeqCst),
        0,
        "a source already in the Via chain must not be contacted again"
    );
}
