// SPDX-License-Identifier: EUPL-1.2
//! A narrowing survives a registration and the two proxies a
//! three-organization deployment puts between the brokers.
//!
//! 5.7.2.4 says what a query answers with: the Entities that match "the
//! filter conditions specified by the query". A narrowing decision is one
//! more such condition (ADR-0020), and 4.3.6 then sends the query on to a
//! Context Source. Between the consumer broker and the provider broker a
//! real deployment has two hops that are neither: an egress proxy the
//! consumer organization runs and an ingress proxy the provider
//! organization runs. Both can read the forwarded request and both can
//! change it.
//!
//! The answer therefore cannot depend on the hops behaving. What the
//! consumer forwards is narrowed, so a well-behaved chain does the work at
//! the source; what the consumer serves is narrowed again after the merge,
//! so a chain that dropped the narrowing on the way answers the same.
#![cfg(feature = "test-kit")]
#![allow(clippy::unwrap_used)] // an unwrap here is the assertion

use antares_api::policy::{
    Decision, DecisionFuture, Filter, NotifyDecision, Operation, PolicyEngine, Subject,
};
use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;

mod common;
use common::net::{proxy, strip_scope_q, verbatim};

/// The 100 Entities the principal may see and the 3 it may not.
const INSIDE: usize = 100;
const OUTSIDE: usize = 3;

/// An engine that narrows every read it may narrow and allows the rest, so
/// the same router can seed, register and then read under the narrowing.
struct Narrowing(Filter);

impl PolicyEngine for Narrowing {
    fn name(&self) -> &str {
        "narrowing"
    }

    fn decide<'a>(&'a self, _s: &'a Subject, op: &'a Operation<'a>) -> DecisionFuture<'a> {
        let answer = if antares_api::policy::FILTERABLE.contains(&op.clause) {
            Decision::Filter(self.0.clone())
        } else {
            Decision::Allow
        };
        Box::pin(std::future::ready(answer))
    }

    fn pre_notify(&self, _s: &Subject, _sub: &Value, _n: &mut Value) -> NotifyDecision {
        NotifyDecision::Deliver
    }
}

async fn send(st: &AppState, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let mut b = Request::builder().method(method).uri(uri);
    let req = match body {
        Some(v) => {
            let payload = v.to_string();
            b = b
                .header("Content-Type", "application/json")
                .header("Content-Length", payload.len());
            b.body(Body::from(payload)).expect("req")
        }
        None => b.body(Body::empty()).expect("req"),
    };
    let resp = antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("resp");
    let status = resp.status();
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    let doc = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, doc)
}

/// Serve a broker's router on a real ephemeral port.
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

/// The provider organization's broker: 100 Entities inside the principal's
/// scope subtree and 3 outside it.
async fn provider() -> AppState {
    let st = AppState::new("provider".into());
    let mut docs = Vec::new();
    for n in 0..INSIDE {
        docs.push(json!({
            "id": format!("urn:ngsi-ld:Vehicle:bb-{n:03}"), "type": "Vehicle",
            "scope": "/BB/Traffic",
            "speed": {"type": "Property", "value": n},
        }));
    }
    for n in 0..OUTSIDE {
        docs.push(json!({
            "id": format!("urn:ngsi-ld:Vehicle:other-{n:03}"), "type": "Vehicle",
            "scope": "/Other/Traffic",
            "speed": {"type": "Property", "value": n},
        }));
    }
    let (code, body) = send(
        &st,
        "POST",
        "/ngsi-ld/v1/entityOperations/create",
        Some(Value::Array(docs)),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{body}");
    st
}

/// The consumer organization's broker: no data of its own, one registration
/// reaching the provider through the chain, and a principal narrowed to a
/// scope subtree. The registration declares scopes of its own, wider than
/// the narrowing, which is what a provider advertises about itself.
async fn consumer(chain_port: u16) -> AppState {
    let mut st = AppState::new("consumer".into()).with_policy(Arc::new(Narrowing(Filter {
        scope_q: Some("/BB/#".into()),
        ..Filter::default()
    })));
    antares_api::wire(&mut st);
    let (code, body) = send(
        &st,
        "POST",
        "/ngsi-ld/v1/csourceRegistrations",
        Some(json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:e4",
            "type": "ContextSourceRegistration",
            "mode": "inclusive",
            "operations": ["federationOps"],
            "information": [{"entities": [{"type": "Vehicle"}]}],
            "scope": ["/BB", "/Other"],
            "endpoint": format!("http://127.0.0.1:{chain_port}"),
        })),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{body}");
    st
}

fn ids(body: &Value) -> Vec<String> {
    body.as_array()
        .expect("array")
        .iter()
        .filter_map(|e| e["id"].as_str().map(str::to_owned))
        .collect()
}

/// The deployment shape end to end: 103 Entities behind a registration
/// reached through the consumer's proxy and the provider's, a principal
/// narrowed to `/BB/#`, exactly 100 answered. The registration's own scopes
/// are wider than the narrowing and do not widen it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn exactly_the_narrowed_entities_survive_a_registration_and_two_proxies() {
    antares_jsonld::allow_private_egress(true);
    let src = provider().await;
    let src_port = serve(&src).await;
    let (provider_proxy, provider_seen) = proxy(src_port, verbatim);
    let (consumer_proxy, consumer_seen) = proxy(provider_proxy, verbatim);
    let st = consumer(consumer_proxy).await;

    let (code, body) = send(
        &st,
        "GET",
        "/ngsi-ld/v1/entities?type=Vehicle&limit=200",
        None,
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    let got = ids(&body);
    assert_eq!(
        got.len(),
        INSIDE,
        "{} Entities answered, not {INSIDE}",
        got.len()
    );
    assert!(
        got.iter().all(|id| id.contains(":bb-")),
        "an Entity outside the narrowing was answered"
    );

    // The narrowing was applied at the source, not only after the merge:
    // both hops carried it, so the provider never had to send the 3.
    for (label, seen) in [("consumer", &consumer_seen), ("provider", &provider_seen)] {
        let relayed = seen.lock().expect("lock").clone();
        assert!(!relayed.is_empty(), "the {label} proxy relayed nothing");
        assert!(
            relayed.iter().all(|r| r.contains("scopeQ=")),
            "the {label} proxy relayed a forward with no narrowing: {relayed:?}"
        );
    }
}

/// The provider's proxy deletes the narrowing from the forwarded query and
/// the source then answers with everything it holds. The consumer serves
/// 100 anyway: a hop it does not run is not where the decision is taken.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_proxy_that_strips_the_narrowing_cannot_widen_the_answer() {
    antares_jsonld::allow_private_egress(true);
    let src = provider().await;
    let src_port = serve(&src).await;
    let (provider_proxy, provider_seen) = proxy(src_port, strip_scope_q);
    let (consumer_proxy, _) = proxy(provider_proxy, verbatim);
    let st = consumer(consumer_proxy).await;

    let (code, body) = send(
        &st,
        "GET",
        "/ngsi-ld/v1/entities?type=Vehicle&limit=200",
        None,
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    let got = ids(&body);
    assert!(
        !provider_seen
            .lock()
            .expect("lock")
            .iter()
            .any(|r| r.contains("scopeQ=")),
        "the proxy did not strip what this test is about"
    );
    assert_eq!(
        got.len(),
        INSIDE,
        "a stripping proxy widened the answer to {}",
        got.len()
    );
    assert!(
        got.iter().all(|id| id.contains(":bb-")),
        "an Entity outside the narrowing was answered: {got:?}"
    );
}
