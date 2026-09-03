// SPDX-License-Identifier: EUPL-1.2
//! What a narrowing policy decision does to a read (ADR-0020).
//!
//! The seam's third answer is `Filter`: the engine allows the operation over
//! less than it asked for. Three clauses decide what "less" means here.
//!
//! 5.7.2.4 gives the query its filters — "the filter conditions specified by
//! the query" — and the narrowing is one more condition conjoined into them,
//! so an Entity the subject may not see is simply not in the result set.
//! 5.7.1.4 answers a retrieve of one Entity, and an Entity outside the
//! narrowing has to answer the way an absent one does: "If the NGSI-LD
//! Entity does not exist, an error of type ResourceNotFound shall be
//! raised". Telling a caller apart from that would leak the row.
//!
//! 4.21 gives the projection language `pick` and `omit` are written in, and
//! 6.5.3.1 what they do: `omit` removes "the listed Entity members" and
//! `pick` reduces the Entity "down to only contain the listed Entity
//! members". A policy `pick` keeps `id` and `type` besides — 5.2.4 makes
//! them the Entity, and a document without them is not one.
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

/// An engine that narrows the reads a narrowing can reach
/// (`policy::FILTERABLE`) and allows everything else, so a test can seed and
/// register through the same router it then reads through. A Filter on any
/// other clause is a refusal, which is the seam's own rule and not this
/// engine's.
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

fn slow() -> Filter {
    Filter {
        q: Some(antares_ql::parse_q("speed<50").expect("q")),
        ..Filter::default()
    }
}

/// The notification pipeline is wired, because the history a temporal read
/// answers from is written by it.
fn state(f: Filter) -> AppState {
    let mut st = AppState::new("me".into()).with_policy(Arc::new(Narrowing(f)));
    antares_api::wire(&mut st);
    st
}

async fn send(st: &AppState, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let mut b = Request::builder().method(method).uri(uri);
    let req = match body {
        Some(v) => {
            let payload = v.to_string();
            b = b
                .header("Content-Type", "application/json")
                .header("Content-Length", payload.len());
            b.body(Body::from(payload)).expect("request")
        }
        None => b.body(Body::empty()).expect("request"),
    };
    let resp = antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("response");
    let status = resp.status();
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    let doc = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, doc)
}

/// Two Vehicles, one under the narrowing and one over it.
async fn seed(st: &AppState) {
    for (id, speed) in [
        ("urn:ngsi-ld:Vehicle:slow", 10),
        ("urn:ngsi-ld:Vehicle:fast", 90),
    ] {
        let (code, _) = send(
            st,
            "POST",
            "/ngsi-ld/v1/entities",
            Some(json!({
                "id": id, "type": "Vehicle",
                "speed": {"type": "Property", "value": speed},
                "colour": {"type": "Property", "value": "red"},
            })),
        )
        .await;
        assert_eq!(code, StatusCode::CREATED, "seed {id}");
    }
}

/// The narrowing marker on a GET, which is a response header rather than a
/// payload member and so cannot be read through [`send`].
async fn restricted(st: &AppState, uri: &str) -> Option<String> {
    antares_api::router(st.clone())
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(uri)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response")
        .headers()
        .get("Antares-Results-Restricted")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

/// 5.6.11 seeds the Temporal Evolution directly: the history a write to
/// `/entities` leaves behind is drained after the response, and these tests
/// are about the read, not about when the drain lands.
async fn seed_history(st: &AppState) {
    for id in ["urn:ngsi-ld:Vehicle:slow", "urn:ngsi-ld:Vehicle:fast"] {
        let (code, _) = send(
            st,
            "POST",
            "/ngsi-ld/v1/temporal/entities",
            Some(json!({
                "id": id, "type": "Vehicle",
                "speed": [{"type": "Property", "value": 10,
                           "observedAt": "2026-01-01T00:00:00Z"}],
                "colour": [{"type": "Property", "value": "red",
                            "observedAt": "2026-01-01T00:00:00Z"}],
            })),
        )
        .await;
        assert!(
            code == StatusCode::CREATED || code == StatusCode::NO_CONTENT,
            "seed {id}: {code}"
        );
    }
}

fn ids(list: &Value) -> Vec<String> {
    list.as_array()
        .expect("array")
        .iter()
        .filter_map(|e| e["id"].as_str().map(str::to_owned))
        .collect()
}

/// 5.7.2.4: the narrowing is one more filter condition on the query the
/// store runs, so the Entity over it is absent — not hidden after the fact.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_policy_q_narrows_the_entity_query() {
    let wide = state(Filter::default());
    seed(&wide).await;
    let (code, all) = send(&wide, "GET", "/ngsi-ld/v1/entities?type=Vehicle", None).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(ids(&all).len(), 2, "the raw query returns both: {all}");

    let narrow = state(slow());
    seed(&narrow).await;
    let (code, some) = send(&narrow, "GET", "/ngsi-ld/v1/entities?type=Vehicle", None).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(
        ids(&some),
        vec!["urn:ngsi-ld:Vehicle:slow".to_owned()],
        "{some}"
    );
}

/// The request's own `q` still applies: the two are conjoined, never
/// replaced, so a query for the fast Vehicle under a slow-only policy is
/// empty rather than answered with the slow one.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_request_query_and_the_policy_query_are_conjoined() {
    let st = state(slow());
    seed(&st).await;
    let (code, list) = send(
        &st,
        "GET",
        "/ngsi-ld/v1/entities?type=Vehicle&q=speed%3E50",
        None,
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    assert!(ids(&list).is_empty(), "{list}");
}

/// 5.7.1.4: an Entity outside the narrowing answers the way an absent one
/// does, so a caller cannot tell a hidden Entity from one that never was.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_hidden_entity_is_a_resource_not_found() {
    let st = state(slow());
    seed(&st).await;
    let (code, _) = send(
        &st,
        "GET",
        "/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:slow",
        None,
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    let (code, pd) = send(
        &st,
        "GET",
        "/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:fast",
        None,
    )
    .await;
    assert_eq!(code, StatusCode::NOT_FOUND, "{pd}");
    assert_eq!(
        pd["type"],
        "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound"
    );
}

/// 6.5.3.1 `omit`: "the listed Entity members are removed from the Entity",
/// on every document the operation serves.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_policy_omit_removes_the_member_from_every_document() {
    let st = state(Filter {
        omit: vec!["colour".into()],
        ..Filter::default()
    });
    seed(&st).await;
    let (_, list) = send(&st, "GET", "/ngsi-ld/v1/entities?type=Vehicle", None).await;
    for e in list.as_array().expect("array") {
        assert!(e.get("colour").is_none(), "omitted member served: {e}");
        assert!(
            e.get("speed").is_some(),
            "omit removed more than it named: {e}"
        );
    }
    let (_, one) = send(
        &st,
        "GET",
        "/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:slow",
        None,
    )
    .await;
    assert!(one.get("colour").is_none(), "{one}");
}

/// 6.5.3.1 `pick`: the Entity is "reduced down to only contain the listed
/// Entity members" — and stays an Entity, so 5.2.4's `id` and `type` remain.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_policy_pick_leaves_the_entity_frame_and_nothing_it_did_not_name() {
    let st = state(Filter {
        pick: vec!["speed".into()],
        ..Filter::default()
    });
    seed(&st).await;
    let (_, one) = send(
        &st,
        "GET",
        "/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:slow",
        None,
    )
    .await;
    assert_eq!(one["id"], "urn:ngsi-ld:Vehicle:slow", "{one}");
    assert_eq!(one["type"], "Vehicle", "{one}");
    assert!(one.get("speed").is_some(), "{one}");
    assert!(
        one.get("colour").is_none(),
        "a picked document kept what it did not name: {one}"
    );
}

/// A request's own projection narrows further; the policy's never widens it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_request_projection_and_the_policy_projection_both_apply() {
    let st = state(Filter {
        pick: vec!["speed".into()],
        ..Filter::default()
    });
    seed(&st).await;
    let (_, one) = send(
        &st,
        "GET",
        "/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:slow?pick=colour",
        None,
    )
    .await;
    assert!(
        one.get("colour").is_none() && one.get("speed").is_none(),
        "picking outside the policy's pick widened it: {one}"
    );
}

/// A narrowed answer says so when the engine asks for it, and says nothing
/// when it does not: narrowing is silent by default (ADR-0020).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_restricted_answer_carries_the_header_and_a_silent_one_does_not() {
    let st = state(Filter {
        restricted: true,
        ..slow()
    });
    seed(&st).await;
    assert_eq!(
        restricted(&st, "/ngsi-ld/v1/entities?type=Vehicle").await,
        Some("true".to_owned())
    );

    let quiet = state(slow());
    seed(&quiet).await;
    assert_eq!(
        restricted(&quiet, "/ngsi-ld/v1/entities?type=Vehicle").await,
        None,
        "narrowing is silent unless the engine asks for the header"
    );
}

/// A single-Entity read is narrowed too — 5.7.1 and 5.7.3 both project the
/// document they answer with — so both say so, or a client that reads one
/// Entity at a time never learns it is seeing less than the broker holds.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_restricted_header_reaches_every_read_that_was_narrowed() {
    let st = state(Filter {
        restricted: true,
        omit: vec!["colour".into()],
        ..Filter::default()
    });
    seed(&st).await;
    seed_history(&st).await;
    for uri in [
        "/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:slow",
        "/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:slow/attrs/speed",
        "/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:Vehicle:slow",
        "/ngsi-ld/v1/temporal/entities?type=Vehicle&timerel=after&timeAt=1970-01-01T00:00:00Z",
        "/ngsi-ld/v1/entityMaps?type=Vehicle",
    ] {
        assert_eq!(
            restricted(&st, uri).await,
            Some("true".to_owned()),
            "{uri} answered a narrowed read without saying so"
        );
    }
}

/// 5.7.2.4 refuses a query with no discriminating filter, and a policy `q`
/// is not the client's filter: a request that was too wide stays too wide.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_too_wide_query_is_still_too_wide_under_a_narrowing_policy() {
    let st = state(slow());
    seed(&st).await;
    let (code, pd) = send(&st, "GET", "/ngsi-ld/v1/entities", None).await;
    assert_eq!(code, StatusCode::BAD_REQUEST, "{pd}");
}

/// The batch query (6.23) is the POST spelling of 5.7.2 and narrows with it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_narrowing_reaches_the_batch_query() {
    let st = state(slow());
    seed(&st).await;
    let (code, list) = send(
        &st,
        "POST",
        "/ngsi-ld/v1/entityOperations/query",
        Some(json!({"type": "Query", "entities": [{"type": "Vehicle"}]})),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(
        ids(&list),
        vec!["urn:ngsi-ld:Vehicle:slow".to_owned()],
        "{list}"
    );
}

/// 5.7.4 reads the same Entities over time, and the same narrowing decides
/// which of them the subject may see.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_narrowing_reaches_the_temporal_query() {
    let st = state(Filter {
        omit: vec!["colour".into()],
        ..Filter::default()
    });
    seed_history(&st).await;
    let (code, list) = send(
        &st,
        "GET",
        "/ngsi-ld/v1/temporal/entities?type=Vehicle&timerel=after&timeAt=1970-01-01T00:00:00Z",
        None,
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{list}");
    let entities = list.as_array().expect("array");
    assert_eq!(entities.len(), 2, "the history of both Vehicles: {list}");
    for e in entities {
        assert!(
            e.get("speed").is_some(),
            "the narrowing removed more than it named: {e}"
        );
        assert!(
            e.get("colour").is_none(),
            "omitted member served by history: {e}"
        );
    }
}

/// ADR-0020 asks an engine to write its rules against IRIs. 4.21 reads a
/// dot as the sub-attribute path separator, so a name run through that
/// grammar would be truncated at the first dot of the authority
/// (`https://uri.etsi.org/…` → the member `https://uri`) and would remove
/// nothing — silently, which is the worst way for a projection to fail.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_projection_named_as_an_iri_removes_the_member_it_names() {
    let st = state(Filter {
        omit: vec!["https://uri.etsi.org/ngsi-ld/default-context/colour".into()],
        ..Filter::default()
    });
    seed(&st).await;
    let (code, list) = send(&st, "GET", "/ngsi-ld/v1/entities?type=Vehicle", None).await;
    assert_eq!(code, StatusCode::OK, "{list}");
    for e in list.as_array().expect("array") {
        assert!(e.get("speed").is_some(), "more was removed than named: {e}");
        assert!(
            e.get("colour").is_none(),
            "an IRI-named omit matched nothing: {e}"
        );
    }
}

/// 5.14.4.4 builds the EntityMap from the query's own candidate set, and
/// Table 5.2.39-1/-2 give that map `id`, `type`, `expiresAt`, `entityMap`
/// and `linkedMaps` — no Entity members, so `pick` and `omit` have nothing
/// to act on here. What the narrowing owes the map is its key set: a key is
/// an Entity id, and an id the subject may not see would leak the row's
/// existence to every page read through the map afterwards.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_entity_map_lists_only_the_narrowed_candidates() {
    let wide = state(Filter::default());
    seed(&wide).await;
    let (code, map) = send(&wide, "GET", "/ngsi-ld/v1/entityMaps?type=Vehicle", None).await;
    assert_eq!(code, StatusCode::CREATED, "{map}");
    let keys = |m: &Value| {
        let mut k: Vec<String> = m["entityMap"]
            .as_object()
            .expect("entityMap")
            .keys()
            .cloned()
            .collect();
        k.sort();
        k
    };
    assert_eq!(
        keys(&map),
        vec![
            "urn:ngsi-ld:Vehicle:fast".to_owned(),
            "urn:ngsi-ld:Vehicle:slow".to_owned()
        ],
        "the raw query maps both: {map}"
    );

    let st = state(slow());
    seed(&st).await;
    let (code, map) = send(&st, "GET", "/ngsi-ld/v1/entityMaps?type=Vehicle", None).await;
    assert_eq!(code, StatusCode::CREATED, "{map}");
    assert_eq!(
        keys(&map),
        vec!["urn:ngsi-ld:Vehicle:slow".to_owned()],
        "{map}"
    );
}

/// Entities under three Scopes, for the narrowing below.
async fn seed_scoped(st: &AppState) {
    for (id, scope) in [
        ("urn:ngsi-ld:Vehicle:traffic", "/BB/Traffic"),
        ("urn:ngsi-ld:Vehicle:parking", "/BB/Parking"),
        ("urn:ngsi-ld:Vehicle:other", "/Other"),
    ] {
        let (code, _) = send(
            st,
            "POST",
            "/ngsi-ld/v1/entities",
            Some(json!({
                "id": id, "type": "Vehicle", "scope": scope,
                "speed": {"type": "Property", "value": 10},
            })),
        )
        .await;
        assert_eq!(code, StatusCode::CREATED, "seed {id}");
    }
}

/// The ids a narrowed query answers with, sorted.
async fn sorted_ids(st: &AppState, uri: &str) -> Vec<String> {
    let (code, body) = send(st, "GET", uri, None).await;
    assert_eq!(code, StatusCode::OK, "{body}");
    let mut v = ids(&body);
    v.sort();
    v
}

/// A `scopeQ` narrowing joins a request that brought its own: 4.19's `and`
/// is over independent per-pattern predicates, so it distributes over the
/// `,`/`|` disjunction and the intersection is itself a Scope Query. The
/// caller sees what both select and never more than the engine allowed —
/// a gateway that narrows a subject to a scope subtree must not have to
/// refuse every consumer that filters by Scope of its own.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_scope_narrowing_intersects_the_requests_own() {
    let st = state(Filter {
        scope_q: Some("/BB/#".into()),
        ..Filter::default()
    });
    seed_scoped(&st).await;

    // no scopeQ of its own: the narrowing is simply the query's scope
    assert_eq!(
        sorted_ids(&st, "/ngsi-ld/v1/entities?type=Vehicle").await,
        vec![
            "urn:ngsi-ld:Vehicle:parking".to_owned(),
            "urn:ngsi-ld:Vehicle:traffic".to_owned()
        ],
        "the engine's subtree alone"
    );
    // its own, inside the engine's: the intersection is the caller's
    assert_eq!(
        sorted_ids(&st, "/ngsi-ld/v1/entities?type=Vehicle&scopeQ=/BB/Traffic").await,
        vec!["urn:ngsi-ld:Vehicle:traffic".to_owned()]
    );
    // its own, reaching outside: the part the engine forbids is dropped,
    // and nothing the engine forbids is served
    assert_eq!(
        sorted_ids(
            &st,
            "/ngsi-ld/v1/entities?type=Vehicle&scopeQ=/BB/Traffic,/Other"
        )
        .await,
        vec!["urn:ngsi-ld:Vehicle:traffic".to_owned()],
        "an entity outside the engine's subtree is never served"
    );
    // disjoint: an empty answer, not a wider one
    assert!(
        sorted_ids(&st, "/ngsi-ld/v1/entities?type=Vehicle&scopeQ=/Other")
            .await
            .is_empty()
    );
}

/// An intersection too large to write as a Scope Query leaves the seam the
/// answer it had before: refuse, because serving either side alone is wider
/// than the engine decided.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_inexpressible_scope_intersection_is_still_a_refusal() {
    let many = (0..200)
        .map(|n| format!("/BB/S{n}"))
        .collect::<Vec<_>>()
        .join(",");
    let st = state(Filter {
        scope_q: Some(many.clone()),
        ..Filter::default()
    });
    seed_scoped(&st).await;
    let (code, pd) = send(
        &st,
        "GET",
        &format!("/ngsi-ld/v1/entities?type=Vehicle&scopeQ={many}"),
        None,
    )
    .await;
    assert_eq!(code, StatusCode::FORBIDDEN, "{pd}");
    assert_eq!(pd["type"], antares_api::policy::ACCESS_DENIED_TYPE);
    assert_eq!(pd["detail"], antares_api::policy::SCOPE_NOT_NARROWABLE);
}

/// 5.7.2.4 rechecks the filters on the merged result — "the filters shall be
/// rechecked before returning results" — and the narrowing is one of them.
/// A Context Source that answers with more than the forwarded query asked
/// for (its own bug, or its own reading of the filter) must not widen what
/// the subject sees.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_peer_that_ignores_the_forwarded_narrowing_still_does_not_widen_it() {
    use std::io::{Read, Write};

    let body = json!([{
        "id": "urn:ngsi-ld:Vehicle:remote",
        "type": "Vehicle",
        "speed": {"type": "Property", "value": 90},
        "colour": {"type": "Property", "value": "red"},
    }])
    .to_string();
    let reply = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    // what the peer was actually asked, so the forward is read rather than
    // assumed from the answer
    let seen: Arc<std::sync::Mutex<Vec<String>>> = Arc::default();
    let recorder = Arc::clone(&seen);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let mut buf = [0u8; 8192];
            let n = s.read(&mut buf).unwrap_or(0);
            if let Ok(mut v) = recorder.lock() {
                v.push(String::from_utf8_lossy(&buf[..n]).into_owned());
            }
            let _ = s.write_all(reply.as_bytes());
        }
    });

    // both brokers register the same peer: one narrowing, one not, so the
    // assertion below cannot pass because the peer was never reached
    let wide = state(Filter::default());
    let st = state(slow());
    for st in [&wide, &st] {
        let (code, _) = send(
            st,
            "POST",
            "/ngsi-ld/v1/csourceRegistrations",
            Some(json!({
                "id": "urn:ngsi-ld:ContextSourceRegistration:policy-peer",
                "type": "ContextSourceRegistration",
                "mode": "inclusive",
                "operations": ["queryEntity"],
                "information": [{"entities": [{"type": "Vehicle"}]}],
                "endpoint": format!("http://127.0.0.1:{port}"),
            })),
        )
        .await;
        assert_eq!(code, StatusCode::CREATED);
    }

    // draining, so each read below sees only the forwards its own query made
    let asked = || -> Vec<String> { seen.lock().expect("lock").drain(..).collect() };

    let (code, list) = send(&wide, "GET", "/ngsi-ld/v1/entities?type=Vehicle", None).await;
    assert_eq!(code, StatusCode::OK, "{list}");
    assert!(
        ids(&list).contains(&"urn:ngsi-ld:Vehicle:remote".to_owned()),
        "the peer was never reached, so the assertions below prove nothing: {list}"
    );
    let unnarrowed = asked();
    assert!(
        !unnarrowed.is_empty() && unnarrowed.iter().all(|r| !r.contains("speed")),
        "an unnarrowed query carried a condition nobody asked for: {unnarrowed:?}"
    );

    let (code, list) = send(&st, "GET", "/ngsi-ld/v1/entities?type=Vehicle", None).await;
    assert_eq!(code, StatusCode::OK, "{list}");
    // 4.3.6.1: the narrowing travels on the forward, so the peer is asked
    // the narrowed question rather than trusted to answer the wide one well
    let narrowed = asked();
    assert!(
        !narrowed.is_empty() && narrowed.iter().all(|r| r.contains("speed")),
        "the narrowing did not reach the peer: {narrowed:?}"
    );
    // and a peer that answers outside it anyway is still filtered
    assert!(
        !ids(&list).contains(&"urn:ngsi-ld:Vehicle:remote".to_owned()),
        "a peer's answer outside the narrowing was served: {list}"
    );
}

/// A policy name is the deployment's, not the caller's. 4.21 names a member,
/// and which member a name IS depends on the `@context` it is read in —
/// `Context::expand_key` consults the term map before it decides a name is
/// already an IRI, so a request that binds the rule's term (or the rule's
/// own IRI, as a term) in its own inline `@context` would otherwise move the
/// rule onto a member the document does not have and remove nothing. 5.5.7
/// makes the Fully Qualified Name the identity of an Attribute; the seam
/// reads its rules in the core `@context`, where a caller cannot reach.
#[tokio::test(flavor = "multi_thread")]
async fn a_request_context_cannot_move_a_policy_name_off_its_target() {
    let st = state(Filter {
        omit: vec!["colour".into()],
        ..Filter::default()
    });
    let (status, _) = send(
        &st,
        "POST",
        "/ngsi-ld/v1/entities",
        Some(json!({
            "id": "urn:ngsi-ld:Vehicle:decoy",
            "type": "Vehicle",
            "colour": {"type": "Property", "value": "red"},
            "speed": {"type": "Property", "value": 10}
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // the caller binds the very term the rule names to somewhere else; an
    // inline @context travels with ld+json (6.3.5), so the request is built
    // here rather than through the json helper above
    let payload = json!({
        "@context": {"colour": "http://decoy.invalid/colour"},
        "type": "Query",
        "entities": [{"type": "Vehicle"}]
    })
    .to_string();
    let req = Request::builder()
        .method("POST")
        .uri("/ngsi-ld/v1/entityOperations/query")
        .header("Content-Type", "application/ld+json")
        .header("Content-Length", payload.len())
        .body(Body::from(payload))
        .expect("request");
    let resp = antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("response");
    let status = resp.status();
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    assert_eq!(status, StatusCode::OK, "{body}");
    let text = body.to_string();
    assert!(
        !text.contains("red"),
        "the policy still removes the member it names: {text}"
    );
    assert!(
        text.contains("urn:ngsi-ld:Vehicle:decoy"),
        "the Entity itself is still answered: {text}"
    );
}

/// 4.5.23 Linked Entity Retrieval answers Entities the request never named:
/// `join=flat` appends every Entity reached over a Relationship. A 4.21
/// projection is written per level — a bare `omit` name leaves that member
/// alone on the joined document, which is what the request asked for — but a
/// narrowing is not a representation choice. A member the subject may not
/// see is not less hidden one hop away, so the policy projection travels
/// down the walk while the request's own does not.
#[tokio::test(flavor = "multi_thread")]
async fn a_join_does_not_carry_a_narrowed_member_out_of_the_answer() {
    let st = state(Filter {
        omit: vec!["colour".into()],
        ..Filter::default()
    });
    for (id, kind, colour, rel) in [
        (
            "urn:ngsi-ld:Vehicle:joined",
            "Vehicle",
            "red",
            Some("urn:ngsi-ld:Person:owner"),
        ),
        ("urn:ngsi-ld:Person:owner", "Person", "blue", None),
    ] {
        let mut doc = json!({
            "id": id,
            "type": kind,
            "colour": {"type": "Property", "value": colour}
        });
        if let Some(target) = rel {
            doc["owner"] = json!({"type": "Relationship", "object": target});
        }
        let (code, body) = send(&st, "POST", "/ngsi-ld/v1/entities", Some(doc)).await;
        assert_eq!(code, StatusCode::CREATED, "{body}");
    }

    let (status, body) = send(
        &st,
        "GET",
        "/ngsi-ld/v1/entities?type=Vehicle&join=flat&joinLevel=1",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let text = body.to_string();
    assert!(
        text.contains("urn:ngsi-ld:Person:owner"),
        "the linked Entity is still answered: {text}"
    );
    assert!(
        !text.contains("red") && !text.contains("blue"),
        "the narrowed member is gone from BOTH documents: {text}"
    );
}
