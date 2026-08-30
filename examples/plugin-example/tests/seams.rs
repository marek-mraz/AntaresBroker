// SPDX-License-Identifier: EUPL-1.2
//! The three seams the driver contract does not cover: the HTTP surface,
//! the notification binding, and the store standing behind a live
//! `AppState`. Everything here is exercised through the broker's own
//! router, so a seam that changes shape breaks this file rather than
//! silently leaving the plugin behind.

use antares_api::AppState;
use antares_model::TenantId;
use antares_notifier::NotificationSink;
use antares_plugin_example::{ExampleStore, ExampleSurface, MemorySink};
use antares_store::{CurrentStateDriver, Kind};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;

fn state() -> (AppState, Arc<ExampleStore>) {
    let store = Arc::new(ExampleStore::new());
    let st = AppState::with_drivers(
        "plugin".into(),
        store.clone(),
        store.clone(),
        antares_plugin_example::NAME,
    )
    .with_surface(Box::new(ExampleSurface))
    .expect("/x/example is a reserved prefix");
    (st, store)
}

async fn get_as(st: &AppState, path: &str, tenant: Option<&str>) -> (StatusCode, String) {
    let mut req = Request::get(path);
    if let Some(t) = tenant {
        req = req.header("NGSILD-Tenant", t);
    }
    let resp = antares_api::router(st.clone())
        .oneshot(req.body(Body::empty()).expect("req"))
        .await
        .expect("resp");
    let status = resp.status();
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

/// A driver from outside the workspace backs a live `AppState`, and
/// `/q/health` names it on both seams — the state carries what its store is
/// called, never an enumeration of the built-in ones.
#[tokio::test(flavor = "multi_thread")]
async fn the_plugin_driver_backs_a_live_state() {
    let (st, _) = state();
    let (code, body) = get_as(&st, "/q/health", None).await;
    assert_eq!(code, StatusCode::OK, "{body}");
    let v: Value = serde_json::from_str(&body).expect("health json");
    assert_eq!(v["store"], "example", "{v}");
    assert_eq!(v["temporal"], "example", "one instance serves both: {v}");
    assert_eq!(
        v["surfaces"]["example"]["prefix"], "/x/example",
        "the surface reports itself: {v}"
    );
}

/// 6.3.14: the surface answers for the tenant that asked, and for no other.
/// A surface reading a default tenant instead of the request's would report
/// one deployment's data to another.
#[tokio::test(flavor = "multi_thread")]
async fn the_surface_answers_only_for_the_requesting_tenant() {
    let (st, store) = state();
    let (a, b) = (
        TenantId::new("plugintenanta").expect("tenant"),
        TenantId::new("plugintenantb").expect("tenant"),
    );
    for i in 0..3 {
        store
            .create(
                &a,
                Kind::Entity,
                &format!("urn:ngsi-ld:Seam:a{i}"),
                json!({"id": format!("urn:ngsi-ld:Seam:a{i}")}),
            )
            .expect("create");
    }
    store
        .create(
            &b,
            Kind::Entity,
            "urn:ngsi-ld:Seam:b0",
            json!({"id": "urn:ngsi-ld:Seam:b0"}),
        )
        .expect("create");

    let count = |body: &str| -> Value { serde_json::from_str(body).expect("json") };
    let (code, body) = get_as(&st, "/x/example/entities/count", Some(a.as_str())).await;
    assert_eq!(code, StatusCode::OK, "{body}");
    assert_eq!(count(&body)["entities"], 3, "{body}");
    assert_eq!(count(&body)["tenant"], a.as_str(), "{body}");

    let (code, body) = get_as(&st, "/x/example/entities/count", Some(b.as_str())).await;
    assert_eq!(code, StatusCode::OK, "{body}");
    assert_eq!(
        count(&body)["entities"],
        1,
        "the other tenant's three rows are not this tenant's: {body}"
    );

    // No header at all is the default tenant, which holds nothing here.
    let (code, body) = get_as(&st, "/x/example/entities/count", None).await;
    assert_eq!(code, StatusCode::OK, "{body}");
    assert_eq!(count(&body)["entities"], 0, "{body}");
}

/// A repeated `NGSILD-Tenant` reaches the surface as the error the spec
/// mandates, not as a silently chosen first value: the surface validates
/// through the same helper every NGSI-LD handler uses.
#[tokio::test(flavor = "multi_thread")]
async fn a_repeated_tenant_header_is_refused_at_the_surface() {
    let (st, _) = state();
    let req = Request::get("/x/example/entities/count")
        .header("NGSILD-Tenant", "one")
        .header("NGSILD-Tenant", "two")
        .body(Body::empty())
        .expect("req");
    let resp = antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("resp");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

/// 5.2.15 / 5.8.1.4: a binding validates its own endpoints when the
/// subscription is created. `memory://<name>` is the whole grammar this
/// sink serves; anything else is `BadRequestData`, never a delivery-time
/// surprise.
#[test]
fn the_binding_validates_its_endpoints_up_front() {
    let sink = MemorySink::new();
    assert_eq!(sink.schemes(), &["memory"]);
    sink.parse_endpoint("memory://inbox", &[])
        .expect("memory://<name> is the shape this sink serves");
    for bad in ["memory://", "memory:/inbox", "http://a.b/c", ""] {
        assert!(
            sink.parse_endpoint(bad, &[]).is_err(),
            "{bad:?} is not an endpoint this sink can deliver to"
        );
    }
}

/// A sink that opens no socket says so, and that is the ONLY way the
/// egress guard is skipped — a binding that stays silent about it is
/// policed like every network one.
#[test]
fn a_socketless_binding_declares_itself() {
    assert!(
        !MemorySink::new().network(),
        "this sink keeps notifications in memory; it opens nothing"
    );
}
