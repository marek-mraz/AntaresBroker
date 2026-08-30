// SPDX-License-Identifier: EUPL-1.2
//! Extra HTTP surfaces register instead of being hard-wired. A deployment
//! mounts its own routes beside the NGSI-LD API without editing a core
//! crate; the spec's own root stays untouchable, and two surfaces cannot
//! claim the same ground.

use antares_api::{ApiSurface, AppState};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::get;
use axum::Router;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

/// A surface defined outside every shipped crate: registering it is the
/// whole seam under test.
struct Ping(&'static str);

impl ApiSurface for Ping {
    fn name(&self) -> &str {
        "ping"
    }
    fn prefix(&self) -> &str {
        self.0
    }
    fn router(&self, _st: AppState) -> Router<AppState> {
        Router::new().route("/ping", get(|| async { "pong" }))
    }
    fn version_info(&self) -> Value {
        json!({"answers": "pong"})
    }
}

async fn get_path(st: &AppState, path: &str) -> (StatusCode, String) {
    let req = Request::get(path).body(Body::empty()).expect("req");
    let resp = antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("resp");
    let status = resp.status();
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

/// A surface from another crate serves through the broker's own router.
#[tokio::test(flavor = "multi_thread")]
async fn a_registered_surface_serves_its_routes() {
    let st = AppState::new("surfaces".into())
        .with_surface(Box::new(Ping("/x")))
        .expect("/x is a reserved prefix");
    assert_eq!(
        get_path(&st, "/x/ping").await,
        (StatusCode::OK, "pong".into())
    );
}

/// The admin surface is the default, and registering another does not
/// displace it.
#[tokio::test(flavor = "multi_thread")]
async fn admin_is_mounted_by_default_and_survives_a_second_surface() {
    let st = AppState::new("surfaces".into());
    let (code, body) = get_path(&st, "/q/health").await;
    assert_eq!(code, StatusCode::OK, "{body}");
    let st = st
        .with_surface(Box::new(Ping("/x")))
        .expect("/x is a reserved prefix");
    assert_eq!(get_path(&st, "/q/health").await.0, StatusCode::OK);
    assert_eq!(get_path(&st, "/x/ping").await.0, StatusCode::OK);
}

/// The NGSI-LD API root belongs to the spec: a surface may not mount there,
/// nor anywhere outside the reserved prefixes.
#[tokio::test(flavor = "multi_thread")]
async fn a_surface_cannot_mount_outside_the_reserved_prefixes() {
    for prefix in ["/ngsi-ld", "/ngsi-ld/v1", "/", "/entities", "/qq", "x"] {
        let err = AppState::new("surfaces".into())
            .with_surface(Box::new(Ping(prefix)))
            .err()
            .unwrap_or_else(|| panic!("{prefix} must be refused"));
        assert!(err.contains(prefix), "the message names the prefix: {err}");
    }
}

/// Two surfaces claiming the same ground is a startup error, not a race for
/// the route: axum would answer whichever won the merge.
#[tokio::test(flavor = "multi_thread")]
async fn overlapping_surfaces_are_refused() {
    let st = AppState::new("surfaces".into())
        .with_surface(Box::new(Ping("/x")))
        .expect("first");
    let err = st
        .with_surface(Box::new(Ping("/x")))
        .err()
        .expect("a second surface on /x must be refused");
    assert!(err.contains("/x"), "{err}");
    // …and nesting under one already taken is the same collision
    let err = AppState::new("surfaces".into())
        .with_surface(Box::new(Ping("/x")))
        .expect("first")
        .with_surface(Box::new(Ping("/x/deeper")))
        .err()
        .expect("a surface nested under /x must be refused");
    assert!(err.contains("/x"), "{err}");
    // the admin surface holds /q against a newcomer
    let err = AppState::new("surfaces".into())
        .with_surface(Box::new(Ping("/q")))
        .err()
        .expect("/q belongs to the admin surface");
    assert!(err.contains("/q"), "{err}");
}

/// `/q/health` names every mounted surface, so an operator can see what a
/// binary serves beyond the NGSI-LD API.
#[tokio::test(flavor = "multi_thread")]
async fn health_names_the_mounted_surfaces() {
    let st = AppState::new("surfaces".into())
        .with_surface(Box::new(Ping("/x")))
        .expect("/x");
    let (code, body) = get_path(&st, "/q/health").await;
    assert_eq!(code, StatusCode::OK, "{body}");
    let v: Value = serde_json::from_str(&body).expect("health json");
    assert_eq!(v["surfaces"]["admin"]["prefix"], "/q", "{v}");
    assert_eq!(v["surfaces"]["ping"]["prefix"], "/x", "{v}");
    assert_eq!(v["surfaces"]["ping"]["answers"], "pong", "{v}");
}

/// A deployment's own selection replaces what the default mounting put
/// there, so a binary can serve a list that does not include admin — and
/// the prefix rules still hold across the replacement.
#[tokio::test(flavor = "multi_thread")]
async fn a_selection_replaces_the_default_surfaces() {
    let st = AppState::new("surfaces".into())
        .with_surfaces(vec![Box::new(Ping("/x"))])
        .expect("a selection of one");
    assert_eq!(get_path(&st, "/x/ping").await.0, StatusCode::OK);
    let (code, body) = get_path(&st, "/q/health").await;
    assert_eq!(
        code,
        StatusCode::NOT_FOUND,
        "admin was not selected, so /q is not served: {body}"
    );
    let err = AppState::new("surfaces".into())
        .with_surfaces(vec![Box::new(Ping("/x")), Box::new(Ping("/x/deeper"))])
        .err()
        .expect("two surfaces on one prefix must be refused");
    assert!(err.contains("/x"), "{err}");
}

/// Health names what each driver actually runs on, not just the backend
/// name: memory and file are one backend with two durability shapes, and an
/// operator reading a health body must be able to tell them apart.
#[tokio::test(flavor = "multi_thread")]
async fn health_names_the_engine_behind_each_driver() {
    let st = AppState::new("surfaces".into());
    let (code, body) = get_path(&st, "/q/health").await;
    assert_eq!(code, StatusCode::OK, "{body}");
    let v: Value = serde_json::from_str(&body).expect("health json");
    let engine = v["storeInfo"]["engine"].as_str().unwrap_or_default();
    assert!(
        matches!(engine, "memory" | "redb"),
        "the store must name its engine, got {v}"
    );
    assert!(
        v.get("temporalInfo").is_some(),
        "the temporal seam is the same store here and must report the same way: {v}"
    );
}

/// The state names its store; it does not enumerate it. A driver from
/// outside the built-in shelf is mounted the same way as one from inside,
/// and `/q/health` reports whatever it is called — an enum here would mean
/// no driver could be added without editing a core crate.
#[tokio::test(flavor = "multi_thread")]
async fn a_store_from_outside_the_shelf_can_back_the_state() {
    let st = AppState::with_store(
        "surfaces".into(),
        std::sync::Arc::new(antares_sql::store::any::AnyStore::Mem(
            antares_sql::store::Store::default(),
        )),
        "example",
    );
    let (code, body) = get_path(&st, "/q/health").await;
    assert_eq!(code, StatusCode::OK, "{body}");
    let v: Value = serde_json::from_str(&body).expect("health json");
    assert_eq!(v["store"], "example", "{v}");
    assert_eq!(
        v["temporal"], "example",
        "one driver serves both seams: {v}"
    );
}
