// SPDX-License-Identifier: EUPL-1.2
//! The façade seam, proved from outside `crates/`.
//!
//! `GET /x/example/things` is a route that answers in a shape NGSI-LD does
//! not define, served entirely by driving the broker's own router in process
//! (`AppState::call`). What this file holds is the rule that makes the
//! design safe: a façade is a translation in front of the NGSI-LD API, never
//! a second way in. Everything CIM 009 puts in the request path — the tenant
//! (6.3.14), the bounds wall, the policy seam, the error table — reaches the
//! façade's caller because the inner request is an ordinary one.

use antares_api::policy::{
    Decision, DecisionFuture, NotifyDecision, Operation, PolicyEngine, Subject,
};
use antares_api::{ApiSurface, AppState};
use antares_plugin_example::{ExampleStore, ExampleSurface};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

const CTX: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld";

/// Every operation the engine was asked about.
type Asked = Arc<Mutex<Vec<String>>>;

struct Recorder(Asked);

impl PolicyEngine for Recorder {
    fn name(&self) -> &str {
        "recorder"
    }
    fn decide<'a>(&'a self, _s: &'a Subject, op: &'a Operation<'a>) -> DecisionFuture<'a> {
        self.0.lock().expect("lock").push(op.clause.to_owned());
        Box::pin(std::future::ready(Decision::Allow))
    }
    fn pre_notify(&self, _s: &Subject, _sub: &Value, _n: &mut Value) -> NotifyDecision {
        NotifyDecision::Deliver
    }
}

fn state() -> (AppState, Asked) {
    let store = Arc::new(ExampleStore::new());
    let asked: Asked = Arc::new(Mutex::new(Vec::new()));
    let st = AppState::with_drivers(
        "plugin".into(),
        store.clone(),
        store,
        antares_plugin_example::NAME,
    )
    .with_surface(Box::new(ExampleSurface))
    .expect("/x/example is a reserved prefix")
    .with_policy(Arc::new(Recorder(asked.clone())));
    (st, asked)
}

async fn get(st: &AppState, uri: &str, tenant: Option<&str>) -> (StatusCode, Value) {
    let mut b = Request::get(uri);
    if let Some(t) = tenant {
        b = b.header("NGSILD-Tenant", t);
    }
    let resp = antares_api::router(st.clone())
        .oneshot(b.body(Body::empty()).expect("request"))
        .await
        .expect("response");
    let status = resp.status();
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn create(st: &AppState, tenant: &str, id: &str) {
    let body = json!({"id": id, "type": "Vehicle",
                      "speed": {"type": "Property", "value": 7}, "@context": CTX})
    .to_string();
    let resp = antares_api::router(st.clone())
        .oneshot(
            Request::post("/ngsi-ld/v1/entities")
                .header("Content-Type", "application/ld+json")
                .header("Content-Length", body.len())
                .header("NGSILD-Tenant", tenant)
                .body(Body::from(body))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::CREATED);
}

/// The façade answers in its own shape, over data the broker served. The
/// `keyValues` representation (4.5.4) is what makes the translation short:
/// the broker did most of the mapping.
#[tokio::test(flavor = "multi_thread")]
async fn the_facade_answers_its_own_shape_over_the_broker_s_data() {
    let (st, _) = state();
    create(&st, "facadea", "urn:ngsi-ld:Vehicle:a").await;

    let (code, body) = get(&st, "/x/example/things?kind=Vehicle", Some("facadea")).await;
    assert_eq!(code, StatusCode::OK, "{body}");
    let items = body["value"].as_array().expect("the façade's envelope");
    assert_eq!(items.len(), 1, "{body}");
    assert_eq!(items[0]["id"], "urn:ngsi-ld:Vehicle:a", "{body}");
    assert_eq!(
        items[0]["speed"], 7,
        "keyValues: the value, not the Property envelope: {body}"
    );
    assert!(
        body.get("type").is_none() && body.get("@context").is_none(),
        "the façade's answer is not an NGSI-LD one: {body}"
    );
}

/// The tenant reaches the inner request without the façade doing anything
/// about it. A façade that had to remember would be the place a deployment
/// leaks one tenant's data into another's answer.
#[tokio::test(flavor = "multi_thread")]
async fn the_facade_answers_only_for_the_requesting_tenant() {
    let (st, _) = state();
    create(&st, "facadea", "urn:ngsi-ld:Vehicle:a").await;
    create(&st, "facadeb", "urn:ngsi-ld:Vehicle:b").await;

    for (tenant, id) in [
        ("facadea", "urn:ngsi-ld:Vehicle:a"),
        ("facadeb", "urn:ngsi-ld:Vehicle:b"),
    ] {
        let (code, body) = get(&st, "/x/example/things?kind=Vehicle", Some(tenant)).await;
        assert_eq!(code, StatusCode::OK, "{body}");
        let items = body["value"].as_array().expect("array");
        assert_eq!(items.len(), 1, "{body}");
        assert_eq!(items[0]["id"], id, "{body}");
    }
}

/// The policy engine is asked about the inner operation, by its own clause.
/// A façade that reached the store directly would be a way past the seam;
/// this is the assertion that it is not.
#[tokio::test(flavor = "multi_thread")]
async fn the_policy_seam_runs_inside_the_facade_call() {
    let (st, asked) = state();
    create(&st, "facadea", "urn:ngsi-ld:Vehicle:a").await;
    asked.lock().expect("lock").clear();

    let (code, _) = get(&st, "/x/example/things?kind=Vehicle", Some("facadea")).await;
    assert_eq!(code, StatusCode::OK);
    let asked = asked.lock().expect("lock").clone();
    assert_eq!(
        asked,
        vec!["5.7.2".to_owned()],
        "the entity query was gated exactly once, by its clause: {asked:?}"
    );
}

/// A façade builds URIs its own caller never wrote, so the broker's caps are
/// the ones that hold. The wall answers, the counter moves, and the façade
/// renders the verdict in its own error shape.
#[tokio::test(flavor = "multi_thread")]
async fn the_bounds_wall_holds_for_an_inner_request() {
    let (st, _) = state();
    let before = st.limits.snapshot()["rejectedUriTooLong"]
        .as_u64()
        .expect("counter");
    let long = "V".repeat(antares_api::bounds::MAX_URI_BYTES);
    let (code, body) = get(
        &st,
        &format!("/x/example/things?kind={long}"),
        Some("facadea"),
    )
    .await;
    assert_eq!(code, StatusCode::URI_TOO_LONG, "{body}");
    assert_eq!(
        st.limits.snapshot()["rejectedUriTooLong"].as_u64(),
        Some(before + 1),
        "the broker counted the rejection it made"
    );
}

/// An NGSI-LD error becomes the façade's error: the status stands, the body
/// is re-rendered. A caller of this API is not expecting Table 6.3.2-1
/// ProblemDetails, and a façade that leaked them would be telling its
/// clients to parse a second error model.
#[tokio::test(flavor = "multi_thread")]
async fn an_ngsi_ld_error_is_rendered_in_the_facade_s_shape() {
    let (st, _) = state();
    // 5.5.10: a query against a tenant that does not exist is a 404, and the
    // inner request is subject to it like any other.
    let (code, body) = get(&st, "/x/example/things?kind=Vehicle", Some("neverexisted")).await;
    assert_eq!(code, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["error"]["code"], 404, "{body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .is_some_and(|m| !m.is_empty()),
        "the reason survives the translation: {body}"
    );
    assert!(
        body.get("type").is_none() && body.get("title").is_none(),
        "no ProblemDetails member leaks into the façade's answer: {body}"
    );
}

/// A façade may not mount under the NGSI-LD API root. The prefix rule lives
/// in `antares-api` and is a startup error; this names the case the rule
/// exists for, from the crate a façade would be written in.
#[test]
fn a_facade_cannot_mount_under_the_ngsi_ld_api_root() {
    struct Rogue(&'static str);
    impl ApiSurface for Rogue {
        fn name(&self) -> &str {
            "rogue"
        }
        fn prefix(&self) -> &str {
            self.0
        }
        fn router(&self, _st: AppState) -> axum::Router<AppState> {
            axum::Router::new()
        }
        fn version_info(&self) -> Value {
            json!({})
        }
    }
    for prefix in [
        "/ngsi-ld",
        "/ngsi-ld/v1",
        "/ngsi-ld/v1/entities",
        "/ngsi-ld/v1/things",
    ] {
        let err = AppState::new("facade".into())
            .with_surface(Box::new(Rogue(prefix)))
            .err()
            .unwrap_or_else(|| panic!("{prefix} must be refused at startup"));
        assert!(err.contains(prefix), "the message names the prefix: {err}");
    }
}
