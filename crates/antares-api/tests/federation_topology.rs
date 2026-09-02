// SPDX-License-Identifier: EUPL-1.2
//! Loop-detection topologies (4.3.6.4, 6.3.18) over real sockets: chains and
//! cycles across the tenants of ONE broker, and across TWO brokers.
//!
//! The same-broker cases are the Urbivita federated-twin shape — reader
//! tenants reach other tenants of the same broker through CSRs whose
//! `tenant` member points back at this broker's own endpoint. They only work
//! because the Via pseudonym is tenant-specific (Table 5.2.40-1, ADR-0011):
//! with one alias per process every hop below would read as a loop.

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use std::io::{Read, Write};
use tower::ServiceExt;

/// The programmatic egress override, not `ANTARES_EGRESS_ALLOW_PRIVATE`: a
/// sibling test reading the environment while another rewrote it saw the
/// policy missing and refused the loopback forward. An atomic store carries
/// the same switch with no write for a reader to land in the middle of.
fn allow_private() {
    antares_jsonld::allow_private_egress(true);
}

const ENTITY: &str = "urn:ngsi-ld:Vehicle:topology";

/// Serve a broker's router on a real ephemeral TCP port — forwards from
/// other brokers (and from its own tenants) arrive over the wire, exactly as
/// deployed.
async fn serve(st: &AppState) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let router = antares_api::router(st.clone());
    tokio::spawn(async move {
        axum::serve(listener, router).await.expect("serve");
    });
    port
}

async fn send(st: &AppState, req: Request<Body>) -> axum::http::Response<Body> {
    antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("response")
}

fn with_tenant(
    mut b: axum::http::request::Builder,
    tenant: Option<&str>,
) -> axum::http::request::Builder {
    if let Some(t) = tenant {
        b = b.header("NGSILD-Tenant", t);
    }
    b
}

async fn create_entity(st: &AppState, tenant: Option<&str>) {
    let body = serde_json::json!({
        "id": ENTITY, "type": "Vehicle",
        "brandName": {"type": "Property", "value": "topology"},
    })
    .to_string();
    let req = with_tenant(
        Request::builder()
            .method("POST")
            .uri("/ngsi-ld/v1/entities")
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len()),
        tenant,
    )
    .body(Body::from(body))
    .expect("request");
    assert_eq!(send(st, req).await.status(), StatusCode::CREATED);
}

/// Register `endpoint` as a Context Source for type Vehicle in `tenant`,
/// optionally telling the broker to use `peer_tenant` when contacting it
/// (5.2.9 `tenant` — the cross-tenant hop).
async fn register(
    st: &AppState,
    tenant: Option<&str>,
    mode: &str,
    endpoint: String,
    peer_tenant: Option<&str>,
) {
    let mut doc = serde_json::json!({
        "id": format!("urn:ngsi-ld:ContextSourceRegistration:{mode}-{}-{}",
                      tenant.unwrap_or("default"), peer_tenant.unwrap_or("default")),
        "type": "ContextSourceRegistration",
        "mode": mode,
        // 5.6.1.4/4.20: the default operations set (federationOps) does NOT
        // include createEntity — writes are only forwarded when declared.
        "operations": ["federationOps", "redirectionOps"],
        "information": [{"entities": [{"type": "Vehicle"}]}],
        "endpoint": endpoint,
    });
    if let Some(p) = peer_tenant {
        doc["tenant"] = serde_json::json!(p);
    }
    let body = doc.to_string();
    let req = with_tenant(
        Request::builder()
            .method("POST")
            .uri("/ngsi-ld/v1/csourceRegistrations")
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len()),
        tenant,
    )
    .body(Body::from(body))
    .expect("request");
    assert_eq!(send(st, req).await.status(), StatusCode::CREATED);
}

async fn query_ids(st: &AppState, tenant: Option<&str>) -> Vec<String> {
    let req = with_tenant(
        Request::builder()
            .method("GET")
            .uri("/ngsi-ld/v1/entities?type=Vehicle")
            .header("Accept", "application/json"),
        tenant,
    )
    .body(Body::empty())
    .expect("request");
    let res = send(st, req).await;
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
        .await
        .expect("body");
    let doc: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    doc.as_array()
        .expect("array")
        .iter()
        .filter_map(|e| e["id"].as_str().map(str::to_owned))
        .collect()
}

/// One broker, three tenants, a two-hop chain: tenant `a` federates tenant
/// `b`, `b` federates tenant `c`, only `c` holds the entity. Every hop is
/// this broker calling itself — hop 1 stamps `hub~a`, hop 2 arrives in `b`
/// (whose own pseudonym `hub~b` is NOT in the chain) and forwards stamping
/// `hub~b`, hop 3 serves from `c`. A tenant-blind alias would kill the chain
/// at hop 2.
#[tokio::test(flavor = "multi_thread")]
async fn cross_tenant_chain_through_one_broker_completes() {
    allow_private();
    let st = AppState::new("hub".into());
    let port = serve(&st).await;
    let endpoint = format!("http://127.0.0.1:{port}");

    create_entity(&st, Some("tc")).await;
    register(&st, Some("tb"), "inclusive", endpoint.clone(), Some("tc")).await;
    register(&st, Some("ta"), "inclusive", endpoint, Some("tb")).await;

    let ids = query_ids(&st, Some("ta")).await;
    assert!(
        ids.contains(&ENTITY.to_owned()),
        "tenant ta must see tc's entity through the two-hop chain, got {ids:?}"
    );
}

/// One broker, two tenants federating EACH OTHER — a genuine cycle. The
/// data lives in `tb`; a query in `ta` hops to `tb`, whose registration back
/// to `ta` is suppressed by the chain (`hub~ta` is already in it), so the
/// request terminates and still returns the data. The assertion of
/// completion IS the loop-detection assertion — without it this test hangs
/// on requests bouncing between the two tenants.
#[tokio::test(flavor = "multi_thread")]
async fn cross_tenant_cycle_terminates_with_the_data() {
    allow_private();
    let st = AppState::new("hub2".into());
    let port = serve(&st).await;
    let endpoint = format!("http://127.0.0.1:{port}");

    create_entity(&st, Some("tb")).await;
    register(&st, Some("ta"), "inclusive", endpoint.clone(), Some("tb")).await;
    register(&st, Some("tb"), "inclusive", endpoint, Some("ta")).await;

    let ids = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        query_ids(&st, Some("ta")),
    )
    .await
    .expect("a cycle between two tenants must terminate, not bounce forever");
    assert!(
        ids.contains(&ENTITY.to_owned()),
        "the cycle must still deliver tb's entity, got {ids:?}"
    );
}

/// Two brokers, each a redirect proxy for the other — the 6.3.17 p.278 508
/// case ACROSS processes: "if the single registered source and tenant is
/// registered to redirect back on to the Context Broker". broker1 forwards
/// (Via: 1.1 antares1), broker2 forwards back (Via: 1.1 antares1, 1.1
/// antares2), broker1 finds itself in the chain → 508, which propagates
/// verbatim through broker2 (the forward_part 502 mapping exempts 508 — the
/// loop verdict must not be laundered into a generic gateway error).
#[tokio::test(flavor = "multi_thread")]
async fn two_broker_cycle_returns_508() {
    allow_private();
    let b1 = AppState::new("antares1".into());
    let b2 = AppState::new("antares2".into());
    let p1 = serve(&b1).await;
    let p2 = serve(&b2).await;

    register(
        &b1,
        None,
        "redirect",
        format!("http://127.0.0.1:{p2}"),
        None,
    )
    .await;
    register(
        &b2,
        None,
        "redirect",
        format!("http://127.0.0.1:{p1}"),
        None,
    )
    .await;

    let req = Request::builder()
        .method("DELETE")
        .uri(format!("/ngsi-ld/v1/entities/{ENTITY}"))
        .body(Body::empty())
        .expect("request");
    let res = send(&b1, req).await;
    assert_eq!(
        res.status(),
        StatusCode::LOOP_DETECTED,
        "a two-broker redirect cycle must surface as 508, not hang or 502"
    );
}

/// Two brokers chained to a raw-socket tail: the Via a THIRD hop receives
/// carries both brokers' pseudonyms in hop order — the RFC 7230 wire format
/// peers parse for their own loop checks (6.3.18: each broker "shall send an
/// additional field value … using its own unique hostAlias as the
/// pseudonym").
#[tokio::test(flavor = "multi_thread")]
async fn two_broker_chain_stamps_both_pseudonyms_in_order() {
    allow_private();
    let b1 = AppState::new("antares1".into());
    let b2 = AppState::new("antares2".into());
    let p2 = serve(&b2).await;

    // the tail: records the head of the one request it receives
    let tail = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let tail_port = tail.local_addr().expect("addr").port();
    let head: std::sync::Arc<std::sync::Mutex<String>> = std::sync::Arc::default();
    let sink = head.clone();
    std::thread::spawn(move || {
        for stream in tail.incoming() {
            let Ok(mut s) = stream else { continue };
            let mut buf = [0u8; 8192];
            let n = s.read(&mut buf).unwrap_or(0);
            sink.lock()
                .expect("lock")
                .push_str(&String::from_utf8_lossy(&buf[..n]));
            let _ = s.write_all(b"HTTP/1.1 201 Created\r\nContent-Length: 0\r\n\r\n");
        }
    });

    register(
        &b1,
        None,
        "inclusive",
        format!("http://127.0.0.1:{p2}"),
        None,
    )
    .await;
    register(
        &b2,
        None,
        "inclusive",
        format!("http://127.0.0.1:{tail_port}"),
        None,
    )
    .await;

    create_entity(&b1, None).await; // b1 → b2 → tail

    // the create fans out asynchronously through two real sockets
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let seen = head.lock().expect("lock").clone();
        if seen.contains("Via:") {
            assert!(
                seen.contains("Via: 1.1 antares1, 1.1 antares2"),
                "hop order antares1 → antares2, got:\n{seen}"
            );
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the tail never saw the forwarded create"
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

/// 4.14: "If no Tenant information is present in the Context Source
/// Registration, no Tenant information is to be used and thus the default
/// Tenant is targeted on the registered Context Source." Two brokers: the
/// hub's tenant `town-a` federates a peer whose data lives in the peer's
/// DEFAULT tenant, through a CSR WITHOUT a `tenant` member. The forward must
/// carry no NGSILD-Tenant header — flowing `town-a` through would make the
/// peer answer NonexistentTenant (and leak the local tenant name).
#[tokio::test(flavor = "multi_thread")]
async fn tenantless_registration_targets_the_peer_default_tenant() {
    allow_private();
    let hub = AppState::new("hub3".into());
    let peer = AppState::new("peer3".into());
    let peer_port = serve(&peer).await;

    // the peer's data lives in its DEFAULT tenant; tenant `town-a` does not
    // exist on the peer at all
    create_entity(&peer, None).await;
    // hub, tenant town-a: CSR with NO `tenant` member
    register(
        &hub,
        Some("town-a"),
        "inclusive",
        format!("http://127.0.0.1:{peer_port}"),
        None,
    )
    .await;

    let ids = query_ids(&hub, Some("town-a")).await;
    assert!(
        ids.contains(&ENTITY.to_owned()),
        "a tenant-less CSR must reach the peer's default tenant, got {ids:?}"
    );
}

/// 4.3.6.5: "As Tenant information, if applicable, is directly specified in
/// the CSourceRegistration, it shall not be part of contextSourceInfo", and
/// 6.3.19: "Headers derived from other elements of the CSourceRegistration,
/// e.g. NGSILD-Tenant, take precedence and cannot be overridden using
/// contextSourceInfo." The key is a valid RFC 7230 header name, so the
/// registration is accepted at the door and the forward is the only place
/// left to ignore it. A broker that passed it through would hand anyone who
/// may create a registration every tenant of the source: the source answers
/// whichever tenant the header names. The pair is spelled in lower case
/// because a header name is case-insensitive (RFC 7230 clause 3.2) and a
/// skip list that matched only the canonical spelling would be no list.
#[tokio::test(flavor = "multi_thread")]
async fn context_source_info_cannot_name_the_tenant_the_forward_targets() {
    allow_private();
    const HIDDEN: &str = "urn:ngsi-ld:Vehicle:town-b-only";
    let hub = AppState::new("hub4".into());
    let peer = AppState::new("peer4".into());
    let peer_port = serve(&peer).await;

    // the peer holds one Vehicle in its default tenant and a different one in
    // `town-b`, so the answer says which tenant the forward reached
    create_entity(&peer, None).await;
    let body = serde_json::json!({
        "id": HIDDEN, "type": "Vehicle",
        "brandName": {"type": "Property", "value": "hidden"},
    })
    .to_string();
    let req = with_tenant(
        Request::builder()
            .method("POST")
            .uri("/ngsi-ld/v1/entities")
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len()),
        Some("town-b"),
    )
    .body(Body::from(body))
    .expect("request");
    assert_eq!(send(&peer, req).await.status(), StatusCode::CREATED);

    // hub, tenant town-a: no `tenant` member, and a contextSourceInfo pair
    // asking for the peer's town-b
    let doc = serde_json::json!({
        "id": "urn:ngsi-ld:ContextSourceRegistration:csi-tenant",
        "type": "ContextSourceRegistration",
        "mode": "inclusive",
        "operations": ["federationOps"],
        "information": [{"entities": [{"type": "Vehicle"}]}],
        "endpoint": format!("http://127.0.0.1:{peer_port}"),
        "contextSourceInfo": [{"key": "ngsild-tenant", "value": "town-b"}],
    })
    .to_string();
    let req = with_tenant(
        Request::builder()
            .method("POST")
            .uri("/ngsi-ld/v1/csourceRegistrations")
            .header("Content-Type", "application/json")
            .header("Content-Length", doc.len()),
        Some("town-a"),
    )
    .body(Body::from(doc))
    .expect("request");
    assert_eq!(send(&hub, req).await.status(), StatusCode::CREATED);

    let ids = query_ids(&hub, Some("town-a")).await;
    assert!(
        !ids.contains(&HIDDEN.to_owned()),
        "contextSourceInfo re-tenanted the forward and read the peer's town-b, got {ids:?}"
    );
    assert!(
        ids.contains(&ENTITY.to_owned()),
        "the forward must still reach the peer's default tenant, got {ids:?}"
    );
}
