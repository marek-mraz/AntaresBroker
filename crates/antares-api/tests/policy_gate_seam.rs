// SPDX-License-Identifier: EUPL-1.2
//! Every operation of the NGSI-LD API asks the policy engine, exactly once.
//!
//! This is the seam's own proof, not a review note (ADR-0020). The route
//! list is read out of `router()`'s own source, so a route added without a
//! gate cannot pass unnoticed: the test asks the router for every method of
//! every path it declares and counts the engine's calls.
//!
//! Exactly once is the rule the placement follows: the gate belongs to the
//! operation, taken where the request enters the handler, and the shared
//! query engines below it (`query_entities_inner`, `query_temporal_inner`)
//! never take one of their own — one operation runs them many times (an
//! EntityMap recheck walks the candidates in chunks, a snapshot fill pages
//! through a temporal query), and an engine asked once per chunk is being
//! asked about work the client never requested.
//!
//! What is deliberately outside the seam, and why, is the `EXEMPT` table
//! below. Nothing else may be absent.
#![cfg(feature = "test-kit")]
#![allow(clippy::unwrap_used)] // an unwrap here is the assertion

use antares_api::policy::{
    Decision, DecisionFuture, NotifyDecision, Operation, PolicyEngine, Subject,
};
use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tower::ServiceExt;

/// An engine no socket can reach: it counts and allows.
struct Counting(Arc<AtomicUsize>);

impl PolicyEngine for Counting {
    fn name(&self) -> &str {
        "counting"
    }

    fn decide<'a>(&'a self, _s: &'a Subject, _o: &'a Operation<'a>) -> DecisionFuture<'a> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(std::future::ready(Decision::Allow))
    }

    fn pre_notify(&self, _s: &Subject, _sub: &Value, _n: &mut Value) -> NotifyDecision {
        NotifyDecision::Deliver
    }
}

/// An engine that records the ids of every operation it is asked about.
struct Recording(Arc<std::sync::Mutex<Vec<Vec<String>>>>);

impl PolicyEngine for Recording {
    fn name(&self) -> &str {
        "recording"
    }

    fn decide<'a>(&'a self, _s: &'a Subject, op: &'a Operation<'a>) -> DecisionFuture<'a> {
        if let Ok(mut v) = self.0.lock() {
            v.push(op.ids.iter().map(|id| (*id).to_owned()).collect());
        }
        Box::pin(std::future::ready(Decision::Allow))
    }

    fn pre_notify(&self, _s: &Subject, _sub: &Value, _n: &mut Value) -> NotifyDecision {
        NotifyDecision::Deliver
    }
}

/// The two router entries that are not operations: `PATCH`/`PUT /entities`
/// exist only to answer "an Entity id is required" (5.6.x address the
/// resource `/entities/{id}`), so there is nothing for an engine to decide
/// about — the request names no Entity and reaches no store.
const EXEMPT: [(&str, &str); 2] = [("PATCH", "/entities"), ("PUT", "/entities")];

/// The router's own source, so the route list cannot drift from the routes.
const ROUTER_SOURCE: &str = include_str!("../src/lib.rs");

/// Every `(method, path)` the NGSI-LD router declares, read from the
/// `.route(...)` calls of `router()`.
fn declared_routes() -> Vec<(String, String)> {
    // the `let api = Router::new()` chain of `router()`, ended by the `;`
    // that closes it — the peer-facing and surface routers below it are not
    // NGSI-LD operations and are not part of this walk
    let body = {
        let start = ROUTER_SOURCE.find("pub fn router").expect("router()");
        let rest = &ROUTER_SOURCE[start..];
        let chain = rest.find("let api = Router::new()").expect("the api chain");
        let tail = &rest[chain..];
        let mut depth = 0i32;
        let mut in_str = false;
        let mut escaped = false;
        let mut end = tail.len();
        for (i, c) in tail.char_indices() {
            match c {
                _ if escaped => escaped = false,
                '\\' if in_str => escaped = true,
                '"' => in_str = !in_str,
                '(' if !in_str => depth += 1,
                ')' if !in_str => depth -= 1,
                ';' if !in_str && depth == 0 => {
                    end = i;
                    break;
                }
                _ => {}
            }
        }
        &tail[..end]
    };
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(at) = rest.find(".route(") {
        rest = &rest[at + ".route(".len()..];
        // the path literal, then everything up to the matching close
        let Some(q1) = rest.find('"') else { break };
        let Some(q2) = rest[q1 + 1..].find('"') else {
            break;
        };
        let path = rest[q1 + 1..q1 + 1 + q2].to_owned();
        let seg_end = rest.find(".route(").unwrap_or(rest.len());
        let seg = &rest[..seg_end];
        for method in ["get", "post", "patch", "put", "delete"] {
            if seg.contains(&format!("{method}(")) {
                out.push((method.to_uppercase(), path.clone()));
            }
        }
        rest = &rest[seg_end..];
    }
    out
}

/// A path with its parameters filled in with values the handlers accept.
fn concrete(path: &str) -> String {
    path.replace("{id}", "urn:ngsi-ld:Vehicle:policy:1")
        .replace("{attr}", "speed")
        .replace("{type}", "Vehicle")
        .replace("{instance}", "urn:ngsi-ld:Instance:policy:1")
}

/// A request each route accepts far enough to reach its gate: the query
/// routes need a filter (5.7.2.4), the writes need a body their parser
/// accepts, and everything else needs nothing.
fn request_for(method: &str, path: &str) -> Request<Body> {
    let entity = json!({"id": "urn:ngsi-ld:Vehicle:policy:1", "type": "Vehicle",
                        "speed": {"type": "Property", "value": 1}});
    let fragment = json!({"speed": {"type": "Property", "value": 2}});
    let (query, body): (&str, Value) = match (method, path) {
        ("GET", "/entities") | ("DELETE", "/entities") => ("?type=Vehicle", Value::Null),
        ("GET", "/temporal/entities") => (
            "?type=Vehicle&timerel=after&timeAt=1970-01-01T00:00:00Z",
            Value::Null,
        ),
        ("GET", "/entityMap") => ("?type=Vehicle", Value::Null),
        ("GET", "/temporal/entityMap") => (
            "?type=Vehicle&timerel=after&timeAt=1970-01-01T00:00:00Z",
            Value::Null,
        ),
        ("DELETE", "/snapshots") => ("?q=status==%22completed%22", Value::Null),
        ("POST", "/entityMap") | ("POST", "/temporal/entityMap") => (
            "",
            json!({"type": "Query", "entities": [{"type": "Vehicle"}]}),
        ),
        ("POST", "/entityOperations/query") => (
            "",
            json!({"type": "Query", "entities": [{"type": "Vehicle"}]}),
        ),
        ("POST", "/temporal/entityOperations/query") => (
            "",
            json!({"type": "Query", "entities": [{"type": "Vehicle"}],
                   "temporalQ": {"timerel": "after", "timeAt": "1970-01-01T00:00:00Z"}}),
        ),
        ("POST", p) if p.starts_with("/entityOperations") => ("", json!([entity])),
        ("POST", "/entities") | ("POST", "/temporal/entities") => ("", entity),
        ("POST", "/subscriptions") | ("POST", "/csourceSubscriptions") => (
            "",
            json!({"type": "Subscription", "entities": [{"type": "Vehicle"}],
                   "notification": {"endpoint": {"uri": "http://sink.invalid/n"}}}),
        ),
        ("POST", "/csourceRegistrations") => (
            "",
            json!({"type": "ContextSourceRegistration",
                   "information": [{"entities": [{"type": "Vehicle"}]}],
                   "endpoint": "http://csr.invalid/"}),
        ),
        ("POST", "/jsonldContexts") => (
            "",
            json!(["https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld"]),
        ),
        ("POST", "/snapshots") => ("", json!({"type": "Snapshot"})),
        ("POST", p) if p.ends_with("/clone") => ("", json!({})),
        ("POST", _) => ("", fragment.clone()),
        ("PATCH" | "PUT", _) => ("", fragment.clone()),
        _ => ("", Value::Null),
    };
    let uri = format!("/ngsi-ld/v1{}{query}", concrete(path));
    let mut b = Request::builder().method(method).uri(uri);
    if !body.is_null() {
        let payload = body.to_string();
        b = b
            .header("Content-Type", "application/json")
            .header("Content-Length", payload.len());
        return b.body(Body::from(payload)).expect("request");
    }
    b.body(Body::empty()).expect("request")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_route_asks_the_policy_engine_exactly_once() {
    let routes = declared_routes();
    assert!(
        routes.len() > 50,
        "the route list came out empty or truncated: {routes:?}"
    );
    let mut wrong = Vec::new();
    for (method, path) in &routes {
        if EXEMPT.contains(&(method.as_str(), path.as_str())) {
            continue;
        }
        let counter = Arc::new(AtomicUsize::new(0));
        let st = AppState::new("me".into()).with_policy(Arc::new(Counting(counter.clone())));
        let resp = antares_api::router(st)
            .oneshot(request_for(method, path))
            .await
            .expect("response");
        let status = resp.status();
        let seen = counter.load(Ordering::SeqCst);
        if seen != 1 {
            let body = resp.into_body().collect().await.expect("body").to_bytes();
            wrong.push(format!(
                "{method} {path}: engine asked {seen} times (answered {status}: {})",
                String::from_utf8_lossy(&body)
                    .chars()
                    .take(160)
                    .collect::<String>()
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// A refusal is a 403 in this broker's own namespace, with the engine's
/// reason — never an ETSI error type, because Table 6.3.2-1 names none.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refusal_is_a_403_that_does_not_claim_an_etsi_error_type() {
    struct Refusing;
    impl PolicyEngine for Refusing {
        fn name(&self) -> &str {
            "refusing"
        }
        fn decide<'a>(&'a self, _s: &'a Subject, _o: &'a Operation<'a>) -> DecisionFuture<'a> {
            Box::pin(std::future::ready(Decision::Deny("out of scope".into())))
        }
        fn pre_notify(&self, _s: &Subject, _sub: &Value, _n: &mut Value) -> NotifyDecision {
            NotifyDecision::Deliver
        }
    }
    let st = AppState::new("me".into()).with_policy(Arc::new(Refusing));
    let resp = antares_api::router(st)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/ngsi-ld/v1/entities?type=Vehicle")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body = resp.into_body().collect().await.expect("body").to_bytes();
    let pd: Value = serde_json::from_slice(&body).expect("problem details");
    assert_eq!(pd["type"], antares_api::policy::ACCESS_DENIED_TYPE);
    assert_eq!(pd["status"], 403);
    assert_eq!(pd["detail"], "out of scope");
    assert!(
        !pd["type"].as_str().unwrap().contains("uri.etsi.org"),
        "a refusal must not claim an ETSI error type"
    );
}

/// The subject carries only the headers a deployment named, and nothing the
/// deployment did not.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_subject_carries_no_header_the_deployment_did_not_name() {
    struct Recording(Arc<std::sync::Mutex<Vec<String>>>);
    impl PolicyEngine for Recording {
        fn name(&self) -> &str {
            "recording"
        }
        fn decide<'a>(&'a self, s: &'a Subject, _o: &'a Operation<'a>) -> DecisionFuture<'a> {
            let mut seen = self.0.lock().expect("lock");
            seen.extend(s.headers.iter().map(|(k, _)| k.clone()));
            Box::pin(std::future::ready(Decision::Allow))
        }
        fn pre_notify(&self, _s: &Subject, _sub: &Value, _n: &mut Value) -> NotifyDecision {
            NotifyDecision::Deliver
        }
    }
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let st = AppState::new("me".into()).with_policy(Arc::new(Recording(seen.clone())));
    let _ = antares_api::router(st)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/ngsi-ld/v1/entities?type=Vehicle")
                .header("Authorization", "Bearer nobody-should-see-this")
                .header("X-Roles", "admin")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    let seen = seen.lock().expect("lock").clone();
    assert!(
        seen.is_empty(),
        "the subject carried headers no deployment named: {seen:?}"
    );
}

/// A route whose path names a resource tells the engine which one.
///
/// ADR-0020 gives the engine "the ids ... the request selects", and a rule
/// that reads them is how a deployment splits one broker between
/// organizations. A handler that holds the id and gates without it does not
/// fail: it silently hands every engine the empty list, so a rule written
/// over ids allows what it was written to refuse. That is a bypass reachable
/// from a socket, and it is invisible to a test that only counts the calls.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_route_that_names_a_resource_tells_the_engine_which_one() {
    let mut missing = Vec::new();
    let mut checked = 0;
    for (method, path) in declared_routes() {
        if !path.contains("{id}") || EXEMPT.contains(&(method.as_str(), path.as_str())) {
            continue;
        }
        let want = concrete("{id}");
        let seen: Arc<std::sync::Mutex<Vec<Vec<String>>>> = Arc::default();
        let st = AppState::new("me".into()).with_policy(Arc::new(Recording(seen.clone())));
        let _ = antares_api::router(st)
            .oneshot(request_for(&method, &path))
            .await
            .expect("response");
        checked += 1;
        let asked = seen.lock().expect("lock").clone();
        if !asked.iter().any(|ids| ids.contains(&want)) {
            missing.push(format!("{method} {path}: engine was told {asked:?}"));
        }
    }
    assert!(checked > 20, "the route walk found only {checked} routes");
    assert!(missing.is_empty(), "{}", missing.join("\n"));
}

/// A create whose body names the id tells the engine that id too.
///
/// 5.8.1, 5.9.2 and 5.11.2 let the client choose the identifier of the
/// resource it is creating, and both handlers have the parsed body in hand
/// when they gate. An engine that owns a segment of the id space has to see
/// that id, or a caller creates a Subscription or a Context Source
/// Registration under another organization's name and the rule that was
/// written to stop it never sees one.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_create_that_names_its_own_id_tells_the_engine_that_id() {
    for (path, doc) in [
        (
            "/subscriptions",
            json!({"id": "urn:ngsi-ld:Subscription:policy:1", "type": "Subscription",
                   "entities": [{"type": "Vehicle"}],
                   "notification": {"endpoint": {"uri": "http://sink.invalid/n"}}}),
        ),
        (
            "/csourceSubscriptions",
            json!({"id": "urn:ngsi-ld:Subscription:policy:1", "type": "Subscription",
                   "entities": [{"type": "ContextSourceRegistration"}],
                   "notification": {"endpoint": {"uri": "http://sink.invalid/n"}}}),
        ),
        (
            "/csourceRegistrations",
            json!({"id": "urn:ngsi-ld:ContextSourceRegistration:policy:1",
                   "type": "ContextSourceRegistration",
                   "information": [{"entities": [{"type": "Vehicle"}]}],
                   "endpoint": "http://csr.invalid/"}),
        ),
    ] {
        let want = doc["id"].as_str().expect("id").to_owned();
        let payload = doc.to_string();
        let seen: Arc<std::sync::Mutex<Vec<Vec<String>>>> = Arc::default();
        let st = AppState::new("me".into()).with_policy(Arc::new(Recording(seen.clone())));
        let req = Request::builder()
            .method("POST")
            .uri(format!("/ngsi-ld/v1{path}"))
            .header("Content-Type", "application/json")
            .header("Content-Length", payload.len())
            .body(Body::from(payload))
            .expect("request");
        let resp = antares_api::router(st)
            .oneshot(req)
            .await
            .expect("response");
        assert_eq!(resp.status(), StatusCode::CREATED, "POST {path}");
        let asked = seen.lock().expect("lock").clone();
        assert!(
            asked.iter().any(|ids| ids.contains(&want)),
            "POST {path}: engine was told {asked:?}"
        );
    }
}
