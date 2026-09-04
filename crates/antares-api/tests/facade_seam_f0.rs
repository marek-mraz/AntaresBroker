// SPDX-License-Identifier: EUPL-1.2
//! The in-process router handle: the seam a façade for another standard
//! (SensorThings, OGC API, WFS, OData) is built on.
//!
//! A façade is an `ApiSurface` under `/x/<standard>` that translates its own
//! request into an NGSI-LD one and calls `AppState::call`. The point of the
//! design is that there is NO second data path: the inner request takes the
//! same route as one off the socket, so everything CIM 009 puts in that path
//! — negotiation, the bounds wall, tenancy (6.3.14), the `@context` a `Link`
//! names (6.3.5), the policy seam — applies to the façade's caller exactly
//! as it applies to an NGSI-LD client. This file holds that: what the
//! handle carries, that it reaches the real handler, and that a façade
//! cannot spend more of the broker's caps than a socket client can.
#![allow(clippy::unwrap_used)] // an unwrap here is the assertion

use antares_api::policy::{
    Decision, DecisionFuture, NotifyDecision, Operation, PolicyEngine, Subject,
};
use antares_api::{ApiSurface, AppState};
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

const CTX: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld";
const SUBJECT: &str = "x-f0-subject";

/// A façade in miniature: three routes that all reach the broker through the
/// handle, and one that reports what the handle delivered.
struct Facade;

impl ApiSurface for Facade {
    fn name(&self) -> &str {
        "f0"
    }
    fn prefix(&self) -> &str {
        "/x/f0"
    }
    fn router(&self, _st: AppState) -> Router<AppState> {
        Router::new()
            .route("/things", get(things))
            .route("/echo", get(echo))
            .route("/toolong", get(toolong))
            .route("/nest", get(nest))
            .route("/seen", get(seen))
    }
    fn version_info(&self) -> Value {
        json!({"seam": "f0"})
    }
}

/// The façade's own operation: its callers ask for "things", it asks the
/// broker for Entities. Whatever the broker answers is what the caller gets.
async fn things(State(st): State<AppState>, headers: HeaderMap) -> Response {
    let req = Request::get("/ngsi-ld/v1/entities?type=Vehicle")
        .body(Body::empty())
        .expect("inner request");
    st.call(&headers, req).await
}

/// The same call, but into a route that reports the headers it was given —
/// which is how the propagation rule is asserted rather than assumed.
async fn echo(State(st): State<AppState>, headers: HeaderMap) -> Response {
    let req = Request::get("/x/f0/seen")
        .body(Body::empty())
        .expect("inner request");
    st.call(&headers, req).await
}

/// An inner request past the URI cap. A façade can build a URI its own
/// caller never wrote — a `$filter` expansion, an `$expand` chain — so the
/// wall has to be the broker's, not the façade's good manners.
async fn toolong(State(st): State<AppState>, headers: HeaderMap) -> Response {
    let long = "a".repeat(antares_api::bounds::MAX_URI_BYTES);
    let req = Request::get(format!("/ngsi-ld/v1/entities?type=Vehicle&q={long}"))
        .body(Body::empty())
        .expect("inner request");
    st.call(&headers, req).await
}

/// A façade route that reaches the broker by calling its own surface. Real
/// façades do this — an OData `$expand` served by the SensorThings façade —
/// and a translation that lands back on the route it came from is a loop
/// the broker has to end, not the façade. `left` is how many more hops this
/// one wants; the ceiling is reached before it runs out when it asks for
/// more than the broker allows.
async fn nest(
    State(st): State<AppState>,
    headers: HeaderMap,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Response {
    let left: usize = q.get("left").and_then(|v| v.parse().ok()).unwrap_or(0);
    if left == 0 {
        return axum::Json(json!({"bottom": true})).into_response();
    }
    let req = Request::get(format!("/x/f0/nest?left={}", left - 1))
        .body(Body::empty())
        .expect("inner request");
    st.call(&headers, req).await
}

/// The far end of `echo`.
async fn seen(headers: HeaderMap) -> Response {
    let mut out = serde_json::Map::new();
    for name in ["ngsild-tenant", "ngsild-snapshot", "link", SUBJECT] {
        let values: Vec<String> = headers
            .get_all(name)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .map(str::to_owned)
            .collect();
        out.insert(name.to_owned(), values.into());
    }
    axum::Json(Value::Object(out)).into_response()
}

/// Every operation the engine was asked about, with the subject it was
/// asked under.
type Asked = Arc<Mutex<Vec<(String, Vec<(String, String)>)>>>;

struct Recorder(Asked);

impl PolicyEngine for Recorder {
    fn name(&self) -> &str {
        "recorder"
    }
    fn decide<'a>(&'a self, subject: &'a Subject, op: &'a Operation<'a>) -> DecisionFuture<'a> {
        self.0
            .lock()
            .expect("lock")
            .push((op.clause.to_owned(), subject.headers.clone()));
        Box::pin(std::future::ready(Decision::Allow))
    }
    fn pre_notify(&self, _s: &Subject, _sub: &Value, _n: &mut Value) -> NotifyDecision {
        NotifyDecision::Deliver
    }
}

fn state() -> (AppState, Asked) {
    // Read once per process, so it has to be set before the first request
    // builds the subject (a `LazyLock` in the policy module).
    std::env::set_var("ANTARES_POLICY_SUBJECT_HEADERS", SUBJECT);
    let asked: Asked = Arc::new(Mutex::new(Vec::new()));
    let st = AppState::new("f0".into())
        .with_surface(Box::new(Facade))
        .expect("/x/f0 is a reserved prefix")
        .with_policy(Arc::new(Recorder(asked.clone())));
    (st, asked)
}

async fn send(
    st: &AppState,
    method: &str,
    uri: &str,
    extra: &[(&str, &str)],
) -> (StatusCode, Value) {
    let mut b = Request::builder().method(method).uri(uri);
    for (k, v) in extra {
        b = b.header(*k, *v);
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
                      "speed": {"type": "Property", "value": 1}, "@context": CTX})
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

/// The handle reaches the SAME handler the socket path reaches, under the
/// caller's own tenant. A façade that served the default tenant instead
/// would answer one deployment's callers out of another's data, which is
/// why the tenant is carried by the handle and not left to the façade.
#[tokio::test(flavor = "multi_thread")]
async fn a_facade_call_reaches_the_ngsi_ld_handler_in_the_caller_s_tenant() {
    let (st, _) = state();
    create(&st, "facadea", "urn:ngsi-ld:Vehicle:a").await;
    create(&st, "facadeb", "urn:ngsi-ld:Vehicle:b").await;

    let (code, body) = send(&st, "GET", "/x/f0/things", &[("NGSILD-Tenant", "facadea")]).await;
    assert_eq!(code, StatusCode::OK, "{body}");
    let ids: Vec<&str> = body
        .as_array()
        .expect("the NGSI-LD answer, verbatim")
        .iter()
        .filter_map(|e| e["id"].as_str())
        .collect();
    assert_eq!(ids, vec!["urn:ngsi-ld:Vehicle:a"], "{body}");

    let (code, body) = send(&st, "GET", "/x/f0/things", &[("NGSILD-Tenant", "facadeb")]).await;
    assert_eq!(code, StatusCode::OK, "{body}");
    let ids: Vec<&str> = body
        .as_array()
        .expect("array")
        .iter()
        .filter_map(|e| e["id"].as_str())
        .collect();
    assert_eq!(
        ids,
        vec!["urn:ngsi-ld:Vehicle:b"],
        "the other tenant's: {body}"
    );

    // ...and a façade caller with no tenant at all gets the default one,
    // which holds neither.
    let (code, body) = send(&st, "GET", "/x/f0/things", &[]).await;
    assert_eq!(code, StatusCode::OK, "{body}");
    assert_eq!(body.as_array().expect("array").len(), 0, "{body}");
}

/// What the handle carries: the two headers that select WHICH data an
/// operation runs against, the one that says what its terms mean, and the
/// policy subject. Every value, not the first — a repeated `NGSILD-Tenant`
/// is `BadRequestData` (6.3.14), and a façade must not be where a repeat
/// becomes a single valid value.
#[tokio::test(flavor = "multi_thread")]
async fn the_handle_carries_the_headers_that_decide_what_is_served() {
    let (st, _) = state();
    let link = format!("<{CTX}>; rel=\"http://www.w3.org/ns/json-ld#context\"");
    let (code, body) = send(
        &st,
        "GET",
        "/x/f0/echo",
        &[
            ("NGSILD-Tenant", "facadea"),
            ("NGSILD-Snapshot", "urn:ngsi-ld:Snapshot:1"),
            ("Link", &link),
            (SUBJECT, "someone"),
        ],
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    assert_eq!(body["ngsild-tenant"], json!(["facadea"]), "{body}");
    assert_eq!(
        body["ngsild-snapshot"],
        json!(["urn:ngsi-ld:Snapshot:1"]),
        "a façade inside a snapshot request must not serve live data: {body}"
    );
    assert_eq!(body["link"], json!([link]), "{body}");
    assert_eq!(body[SUBJECT], json!(["someone"]), "{body}");

    // A header the rule does not name is the façade's own business and is
    // NOT carried: an inner request is not the outer one.
    let (code, body) = send(
        &st,
        "GET",
        "/x/f0/echo",
        &[("NGSILD-Tenant", "facadea"), ("NGSILD-Tenant", "facadeb")],
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    assert_eq!(
        body["ngsild-tenant"],
        json!(["facadea", "facadeb"]),
        "both values reach the inner request, so 6.3.14 still refuses it: {body}"
    );
}

/// The policy engine is asked about the FAÇADE'S caller, not about the
/// façade: the inner entity query carries the same subject the outer
/// request did. An engine that saw an empty subject there would narrow
/// nothing, and the façade would be a way around the seam.
#[tokio::test(flavor = "multi_thread")]
async fn the_policy_engine_sees_the_caller_behind_the_facade() {
    let (st, asked) = state();
    // 5.5.10: a non-create operation against a tenant that does not exist is
    // a 404, and the inner request is subject to it like any other — which
    // is the point, so the tenant is made to exist first.
    create(&st, "facadea", "urn:ngsi-ld:Vehicle:a").await;
    asked.lock().expect("lock").clear();
    let (code, body) = send(
        &st,
        "GET",
        "/x/f0/things",
        &[("NGSILD-Tenant", "facadea"), (SUBJECT, "someone")],
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    let asked = asked.lock().expect("lock").clone();
    let inner: Vec<_> = asked.iter().filter(|(c, _)| c == "5.7.2").collect();
    assert_eq!(inner.len(), 1, "the inner query was gated once: {asked:?}");
    assert_eq!(
        inner[0].1,
        vec![(SUBJECT.to_owned(), "someone".to_owned())],
        "the engine was asked about the caller: {asked:?}"
    );
}

/// The bounds wall is the broker's, and an inner request pays it. A façade
/// builds URIs its own caller never wrote, so a handle that skipped the wall
/// would be the one way in where the documented caps do not apply.
#[tokio::test(flavor = "multi_thread")]
async fn an_inner_request_is_counted_by_the_bounds_wall() {
    let (st, _) = state();
    let before = st.limits.snapshot()["rejectedUriTooLong"]
        .as_u64()
        .expect("counter");
    let (code, _) = send(&st, "GET", "/x/f0/toolong", &[("NGSILD-Tenant", "facadea")]).await;
    assert_eq!(
        code,
        StatusCode::URI_TOO_LONG,
        "the wall answers the façade's caller with what it answered the inner request"
    );
    let after = st.limits.snapshot()["rejectedUriTooLong"]
        .as_u64()
        .expect("counter");
    assert_eq!(after, before + 1, "and the rejection is counted once");
}

/// A memoized router owns `AppState` clones — every layer captured one — so
/// a state that could reach its own memo would keep itself alive for the
/// life of the process and pin whatever its store holds. On a file store
/// that is a redb lock that outlives the host: a broker rebuilding its state
/// (a reload, a test, the browser build's reset) would never open its own
/// data again. The memo is therefore built from a clone that cannot reach
/// it, which is what this asserts from outside — the store is released the
/// moment the state is dropped.
#[tokio::test(flavor = "multi_thread")]
async fn a_state_that_made_an_in_process_call_still_releases_its_store() {
    let dir = std::env::temp_dir().join(format!("antares-facade-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("mkdir");
    let store = antares_sql::store::Store::open_file(&dir).expect("open file store");
    let st = AppState::with_store(
        "facade-drop".into(),
        Arc::new(antares_sql::store::any::AnyStore::Mem(store)),
        "file",
    )
    .with_surface(Box::new(Facade))
    .expect("/x/f0 is a reserved prefix");

    let (code, _) = send(&st, "GET", "/x/f0/things", &[]).await;
    assert_eq!(code, StatusCode::OK, "the memo is built by the first call");

    let store = st.store.clone();
    drop(st);
    for _ in 0..100 {
        if Arc::strong_count(&store) == 1 {
            drop(store);
            // ...and the lock is really gone: the same path opens again.
            antares_sql::store::Store::open_file(&dir).expect("reopen after drop");
            std::fs::remove_dir_all(&dir).ok();
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    std::fs::remove_dir_all(&dir).ok();
    panic!(
        "the store outlived its state: {} owners left",
        Arc::strong_count(&store)
    );
}

/// A façade cannot spend the broker's stack either: `AppState::call` counts
/// the frames one request is already inside and refuses past
/// `MAX_IN_PROCESS_CALL_DEPTH`. A chain shorter than the ceiling is served —
/// a façade over a façade is a design the seam invites — and a route that
/// keeps translating into itself is ended with the broker's own 500 rather
/// than by the stack. The refusal names no path and no façade.
#[tokio::test]
async fn a_facade_that_calls_itself_is_ended_by_the_broker_not_by_the_stack() {
    let (st, _) = state();
    let ceiling = antares_api::bounds::MAX_IN_PROCESS_CALL_DEPTH;

    // One frame short of the ceiling: every hop is served.
    let (code, body) = send(&st, "GET", &format!("/x/f0/nest?left={}", ceiling - 2), &[]).await;
    assert_eq!(code, StatusCode::OK, "a chain inside the ceiling: {body}");
    assert_eq!(body["bottom"], json!(true), "{body}");

    // A loop asks for far more than the ceiling allows.
    let (code, body) = send(&st, "GET", &format!("/x/f0/nest?left={}", ceiling * 4), &[]).await;
    assert_eq!(
        code,
        StatusCode::INTERNAL_SERVER_ERROR,
        "the ceiling ends the loop: {body}"
    );
    assert_eq!(
        body["title"], "InternalError",
        "Table 6.3.2-1 names the type: {body}"
    );
    let detail = body["detail"].as_str().unwrap_or_default().to_owned();
    assert!(
        !detail.contains("/x/f0") && !detail.contains("nest"),
        "the refusal names the ceiling, not the caller's route: {detail}"
    );

    // The ceiling is published, so a façade author can read it.
    let (code, health) = send(&st, "GET", "/q/health", &[]).await;
    assert_eq!(code, StatusCode::OK, "{health}");
    assert_eq!(
        health["limits"]["maxInProcessCallDepth"],
        json!(ceiling),
        "{health}"
    );
}
