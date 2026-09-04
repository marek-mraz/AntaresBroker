// SPDX-License-Identifier: EUPL-1.2
//! The broker keeps state in tenants it mints for itself: one `snap-<uuid>`
//! per Snapshot holding the frozen copy, `snap-index` mapping a synthetic
//! tenant back to its owner, and `distsub-index` mapping a forwarded
//! Subscription's remote id to the local one (5.8.1.4). None of them is a
//! client tenant, and 6.3.14 says a client-supplied `NGSILD-Tenant` may not
//! name one — a request that did would read and write the keyspace the
//! broker keeps its own bookkeeping in.
//!
//! `tenant_isolation.rs` and `tenants_admin.rs` pin the front door: naming
//! one is refused and none is listed or addressable. This file pins the ways
//! in that do not go through a tenant header at all — the 6.3.22 rewrite that
//! legitimately sets a synthetic tenant, the peer-facing route that carries
//! no tenant header, and the copies a Snapshot leaves behind.
#![allow(clippy::unwrap_used)] // an unwrap here is the assertion

mod common;

use antares_api::AppState;
use antares_store::Kind;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

async fn send_h(
    st: &AppState,
    method: &str,
    path: &str,
    body: Option<String>,
    extra: &[(&str, &str)],
) -> (StatusCode, axum::http::HeaderMap, Value) {
    let mut b = Request::builder().method(method).uri(path);
    for (k, v) in extra {
        b = b.header(*k, *v);
    }
    let req = match body {
        Some(body) => b
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body)),
        None => b.body(Body::empty()),
    }
    .expect("request");
    let res = antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("response");
    let status = res.status();
    let headers = res.headers().clone();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .expect("body");
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, headers, body)
}

async fn state() -> AppState {
    let mut st = AppState::new("antares-leaks".into());
    antares_api::wire(&mut st).await;
    st
}

/// Create one Vehicle and a Snapshot over it, and wait for the fill.
/// Returns the snapshot id.
async fn snapshot_over_a_vehicle(st: &AppState, tenant: &[(&str, &str)], id: &str) -> String {
    let body = json!({"id": id, "type": "Vehicle",
        "speed": {"type": "Property", "value": 80}})
    .to_string();
    let (s, _, b) = send_h(st, "POST", "/ngsi-ld/v1/entities", Some(body), tenant).await;
    assert_eq!(s, StatusCode::CREATED, "{b}");

    let snap = json!({"type": "Snapshot",
        "snapshotQueries": [{"type": "Query", "entities": [{"type": "Vehicle"}]}]})
    .to_string();
    let (s, h, b) = send_h(st, "POST", "/ngsi-ld/v1/snapshots", Some(snap), tenant).await;
    assert_eq!(s, StatusCode::CREATED, "{b}");
    let loc = h.get("Location").unwrap().to_str().unwrap().to_owned();
    for _ in 0..100 * antares_api::state::slow_factor() {
        let (s, _, body) = send_h(st, "GET", &loc, None, tenant).await;
        assert_eq!(s, StatusCode::OK, "{body}");
        if body["snapshotStatus"] != "preparing" {
            assert_eq!(body["snapshotStatus"], "success", "{body}");
            return body["id"].as_str().expect("id").to_owned();
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("snapshot never left preparing");
}

/// 6.3.22: the scoping rewrites the request's Tenant to the snapshot's
/// synthetic one so the ordinary handlers serve the frozen copy. The name is
/// the broker's own and must not come back out: a client that learned it
/// still could not send it (the wall refuses it), but a response that carried
/// it would put internal bookkeeping in a client's hands and in its logs.
///
/// Both directions are asserted — the response's own `NGSILD-Tenant` is the
/// caller's, and no rendered document anywhere in the exchange spells a
/// `snap-` name.
#[tokio::test(flavor = "multi_thread")]
async fn a_snapshot_scoped_response_never_names_the_synthetic_tenant() {
    let st = state().await;
    let tenant = &[("NGSILD-Tenant", "acme")];
    let sid = snapshot_over_a_vehicle(&st, tenant, "urn:ngsi-ld:Vehicle:leak1").await;

    let scoped = &[("NGSILD-Tenant", "acme"), ("NGSILD-Snapshot", &sid)];
    for path in [
        "/ngsi-ld/v1/entities?type=Vehicle",
        "/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:leak1",
    ] {
        let (s, h, body) = send_h(&st, "GET", path, None, scoped).await;
        assert_eq!(s, StatusCode::OK, "{body}");
        assert_eq!(
            h.get("NGSILD-Tenant").map(|v| v.to_str().unwrap()),
            Some("acme"),
            "{path}: the response named a tenant other than the caller's"
        );
        assert!(
            !body.to_string().contains("snap-"),
            "{path}: a synthetic tenant name reached the client: {body}"
        );
        // 6.3.22: the snapshot header is echoed on every response
        assert_eq!(
            h.get("NGSILD-Snapshot").map(|v| v.to_str().unwrap()),
            Some(sid.as_str()),
            "{path}: the snapshot header was not echoed"
        );
    }

    // the default tenant carries no header out either, rather than the
    // synthetic one the layer put in
    let sid2 = snapshot_over_a_vehicle(&st, &[], "urn:ngsi-ld:Vehicle:leak2").await;
    let (s, h, body) = send_h(
        &st,
        "GET",
        "/ngsi-ld/v1/entities?type=Vehicle",
        None,
        &[("NGSILD-Snapshot", &sid2)],
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{body}");
    assert_eq!(
        h.get("NGSILD-Tenant"),
        None,
        "the default tenant's response named a tenant"
    );
    assert!(
        !body.to_string().contains("snap-"),
        "a synthetic tenant name reached the client: {body}"
    );
}

/// The refusal of a reserved `NGSILD-Tenant` and the 6.3.22 rewrite that sets
/// one are two layers, and only their ORDER keeps both true: the wall wraps
/// the snapshot layer, so it always judges the name the caller sent and never
/// the one the broker substituted.
///
/// Order is not visible from either layer's own tests, which is why it is
/// asserted here: a request carrying BOTH a reserved tenant and a valid
/// snapshot is refused, not served. Swapping the two `.layer` calls in
/// `router` makes this the test that fails.
#[tokio::test(flavor = "multi_thread")]
async fn the_reserved_tenant_wall_runs_before_the_snapshot_rewrite() {
    let st = state().await;
    let tenant = &[("NGSILD-Tenant", "acme")];
    let sid = snapshot_over_a_vehicle(&st, tenant, "urn:ngsi-ld:Vehicle:order1").await;

    for reserved in [
        "snap-index",
        "snap-0123456789abcdef0123456789abcdef",
        "distsub-index",
    ] {
        let (s, _, body) = send_h(
            &st,
            "GET",
            "/ngsi-ld/v1/entities?type=Vehicle",
            None,
            &[("NGSILD-Tenant", reserved), ("NGSILD-Snapshot", &sid)],
        )
        .await;
        assert_eq!(
            s,
            StatusCode::BAD_REQUEST,
            "{reserved} reached the snapshot layer: {body}"
        );
        assert!(
            body.to_string().contains("BadRequestData"),
            "{reserved}: expected BadRequestData, got {body}"
        );
    }
}

/// 5.5.10: `/q/tenants` is the list of customer accounts. A Snapshot's fill
/// writes real Entities under its synthetic tenant, and on Postgres that
/// write claims a row in `tenants` like any other — the row is deliberate,
/// because it is also the enumeration the notification paths walk. What must
/// never happen is the inventory serving it.
///
/// Asserted after a REAL fill rather than a hand-seeded tenant, so the path
/// under test is the one a deployment takes.
#[tokio::test(flavor = "multi_thread")]
async fn a_filled_snapshots_tenant_never_reaches_the_account_inventory() {
    let st = state().await;
    let tenant = &[("NGSILD-Tenant", "acme")];
    let sid = snapshot_over_a_vehicle(&st, tenant, "urn:ngsi-ld:Vehicle:inv1").await;

    // the copy really is there — otherwise the assertion below passes vacuously
    let (s, _, copied) = send_h(
        &st,
        "GET",
        "/ngsi-ld/v1/entities?type=Vehicle",
        None,
        &[("NGSILD-Tenant", "acme"), ("NGSILD-Snapshot", &sid)],
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{copied}");
    assert_eq!(copied.as_array().map(Vec::len), Some(1), "{copied}");

    let (s, _, list) = send_h(&st, "GET", "/q/tenants", None, &[]).await;
    assert_eq!(s, StatusCode::OK, "{list}");
    assert!(
        list.to_string().contains("acme"),
        "the owning account is missing from the inventory: {list}"
    );
    assert!(
        !list.to_string().contains("snap-"),
        "a snapshot's own tenant is listed as an account: {list}"
    );
}

/// 5.8.1.4 / ADR-0019: `POST /ex/v1/remote-notify` is the peer-facing wire,
/// outside the tenant wall, and it takes its tenant from the broker's own
/// inbound index — never from the request. A peer that could steer the route
/// with a header would deliver its Entities into a tenant it was never
/// registered with.
///
/// Both tenants hold a Subscription under the SAME id, so a lookup in the
/// wrong one resolves rather than 404s: only the sink that receives tells the
/// two apart.
#[tokio::test(flavor = "multi_thread")]
async fn the_peer_route_takes_its_tenant_from_the_mapping_not_the_header() {
    async fn sink() -> (String, tokio::sync::mpsc::Receiver<Value>) {
        let (tx, rx) = tokio::sync::mpsc::channel::<Value>(8);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let app = axum::Router::new().route(
            "/notify",
            axum::routing::post(move |body: axum::body::Bytes| {
                let tx = tx.clone();
                async move {
                    let _ = tx
                        .send(serde_json::from_slice(&body).unwrap_or(Value::Null))
                        .await;
                    StatusCode::OK
                }
            }),
        );
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });
        (format!("http://{addr}/notify"), rx)
    }

    antares_jsonld::allow_private_egress(true);
    let st = state().await;
    const OWNER: &str = "dsroute-owner";
    const OTHER: &str = "dsroute-other";
    const SUB: &str = "urn:ngsi-ld:Subscription:dsroute";
    let remote_id = "urn:ngsi-ld:Subscription:distsub:11111111222233334444555555555555";

    let (uri_owner, mut rx_owner) = sink().await;
    let (uri_other, mut rx_other) = sink().await;
    for (tenant, uri) in [(OWNER, &uri_owner), (OTHER, &uri_other)] {
        let doc = json!({"id": SUB, "type": "Subscription",
            "entities": [{"type": "Vehicle"}],
            "notification": {"endpoint": {"uri": uri}}})
        .to_string();
        let (s, _, b) = send_h(
            &st,
            "POST",
            "/ngsi-ld/v1/subscriptions",
            Some(doc),
            &[("NGSILD-Tenant", tenant)],
        )
        .await;
        assert_eq!(s, StatusCode::CREATED, "{tenant}: {b}");
    }

    // the mapping the broker writes when it forwards a copy (inbound_put)
    let idx = antares_model::TenantId::new_internal("distsub-index").expect("index tenant");
    st.store
        .create(
            &idx,
            Kind::DistSub,
            remote_id,
            json!({"tenant": OWNER, "own": SUB}),
        )
        .await
        .expect("seed the inbound mapping");

    // the peer names the OTHER tenant; the mapping says OWNER
    let inbound = json!({"id": "urn:ngsi-ld:Notification:dsroute", "type": "Notification",
        "subscriptionId": remote_id,
        "notifiedAt": "2026-08-12T12:00:00Z",
        "data": [{"id": "urn:ngsi-ld:Vehicle:dsroute", "type": "Vehicle",
                  "speed": {"type": "Property", "value": 99}}]})
    .to_string();
    let (s, _, b) = send_h(
        &st,
        "POST",
        "/ex/v1/remote-notify",
        Some(inbound),
        &[("NGSILD-Tenant", OTHER)],
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b}");

    let got = tokio::time::timeout(
        std::time::Duration::from_secs(5 * antares_api::state::slow_factor() as u64),
        rx_owner.recv(),
    )
    .await
    .expect("the owning tenant's subscriber was never notified")
    .expect("sink closed");
    assert!(
        got.to_string().contains("urn:ngsi-ld:Vehicle:dsroute"),
        "the owner got a notification without the peer's Entity: {got}"
    );
    assert!(
        rx_other.try_recv().is_err(),
        "the header steered the notification into a tenant the mapping never named"
    );
}
