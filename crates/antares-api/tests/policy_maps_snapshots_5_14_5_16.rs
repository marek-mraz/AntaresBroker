// SPDX-License-Identifier: EUPL-1.2
//! An EntityMap and a Snapshot under a policy (ADR-0020).
//!
//! Both resources are a broker-held record of what one query matched, and
//! both outlive the request that made them — which is what makes them worth
//! a rule of their own.
//!
//! 5.5.14 gives the EntityMap one: "If an EntityMap has expired, or cannot
//! be accessed, no inference can be made as to which entities are held
//! within the Context Sources and a new one shall be created." A map built
//! for another subject cannot be accessed by this one, so the clause's own
//! recovery applies — a new map, not an error, because the map id arrived in
//! a header and an error would confirm that someone else's transaction
//! exists. The same clause makes exactly one allowance in the other
//! direction: other components "shall only be allowed to update the expiry
//! timestamp of the EntityMap", so an update by a stranger stays open where
//! a retrieve and a delete do not — 5.14.1.4 and 5.14.3.4 answer an id that
//! "does not correspond to any existing EntityMap" with ResourceNotFound,
//! and from a subject that did not build it, that is what the map is.
//!
//! 5.16 gives the Snapshot the other rule. Create, Clone and Purge act on
//! everything the tenant holds — 5.16.1.4 copies whatever the snapshot's
//! queries match, in a fill that runs after the request has been answered —
//! and there is no narrowed form of that. An engine that answers with a
//! narrowing is refused, the way 5.6.21 Purge Entities is.
#![cfg(feature = "test-kit")]
#![allow(clippy::unwrap_used)] // an unwrap here is the assertion

use antares_api::policy::{
    Decision, DecisionFuture, Filter, NotifyDecision, Operation, PolicyEngine, Subject,
};
use antares_api::AppState;
use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;

/// The header this file's deployment carries into the subject.
fn subject_header() -> &'static str {
    std::env::set_var("ANTARES_POLICY_SUBJECT_HEADERS", "X-Subject");
    "X-Subject"
}

/// An engine that answers every operation with the same decision.
struct Fixed(Decision);

impl PolicyEngine for Fixed {
    fn name(&self) -> &str {
        "fixed"
    }

    fn decide<'a>(&'a self, _s: &'a Subject, _op: &'a Operation<'a>) -> DecisionFuture<'a> {
        Box::pin(std::future::ready(self.0.clone()))
    }

    fn pre_notify(&self, _s: &Subject, _sub: &Value, _n: &mut Value) -> NotifyDecision {
        NotifyDecision::Deliver
    }
}

fn state(d: Decision) -> AppState {
    // before the first request: the header list is read once per process,
    // and a gate that runs before it is set would see no subject at all
    subject_header();
    AppState::new("me".into()).with_policy(Arc::new(Fixed(d)))
}

async fn call(
    st: &AppState,
    method: &str,
    path: &str,
    who: Option<&str>,
    map: Option<&str>,
    doc: Option<Value>,
) -> (StatusCode, HeaderMap, Value) {
    let mut b = Request::builder()
        .method(method)
        .uri(format!("/ngsi-ld/v1/{path}"));
    if let Some(s) = who {
        b = b.header(subject_header(), s);
    }
    if let Some(m) = map {
        b = b.header("NGSILD-EntityMap", m);
    }
    let req = match doc {
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
    let headers = resp.headers().clone();
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    let doc = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, headers, doc)
}

async fn seed(st: &AppState) {
    for id in ["urn:ngsi-ld:Vehicle:m1", "urn:ngsi-ld:Vehicle:m2"] {
        let (code, _, body) = call(
            st,
            "POST",
            "entities",
            None,
            None,
            Some(json!({"id": id, "type": "Vehicle",
                        "speed": {"type": "Property", "value": 10}})),
        )
        .await;
        assert_eq!(code, StatusCode::CREATED, "seed {id}: {body}");
    }
}

/// The id out of the `NGSILD-EntityMap` header, which carries the resource
/// URI of the map (6.34.3.1).
fn map_id(h: &HeaderMap) -> String {
    h.get("NGSILD-EntityMap")
        .and_then(|v| v.to_str().ok())
        .and_then(|u| u.rsplit('/').next())
        .expect("the create answers with the map it built")
        .to_owned()
}

/// One EntityMap over the Vehicles, built for `who`.
async fn build_map(st: &AppState, who: &str) -> String {
    let (code, h, body) = call(st, "GET", "entityMaps?type=Vehicle", Some(who), None, None).await;
    assert_eq!(code, StatusCode::CREATED, "{body}");
    map_id(&h)
}

/// 5.5.14: a map another subject built "cannot be accessed", so the query
/// that presented it gets a NEW map rather than that one — and never an
/// error, which would confirm the other subject's transaction exists.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_map_built_for_one_subject_is_not_reused_by_another() {
    let st = state(Decision::Allow);
    seed(&st).await;
    let mine = build_map(&st, "alice").await;

    // alice presents her own map: the answer is served through it
    let (code, h, body) = call(
        &st,
        "GET",
        "entities?type=Vehicle",
        Some("alice"),
        Some(&mine),
        None,
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    assert_eq!(map_id(&h), mine, "her own map was not used");

    // mallory presents alice's map: answered exactly as an expired or
    // unknown reference is (`clause_5_7_4_4_unknown_map_recreates_201`) —
    // 201 and a new map — so the two cannot be told apart
    let (code, h, body) = call(
        &st,
        "GET",
        "entities?type=Vehicle",
        Some("mallory"),
        Some(&mine),
        None,
    )
    .await;
    assert_eq!(
        code,
        StatusCode::CREATED,
        "a stranger's map id is answered like an expired one, not with an error: {body}"
    );
    assert_ne!(
        map_id(&h),
        mine,
        "another subject's EntityMap was reused: {body}"
    );
}

/// 5.14.1.4 / 5.14.3.4: an id that corresponds to no EntityMap this subject
/// has is ResourceNotFound. The map's `entityMap` member IS the id set the
/// query matched, so serving it would hand over exactly what a narrowing
/// withholds, and deleting it would end someone else's transaction.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_map_is_retrieved_and_deleted_only_by_the_subject_that_built_it() {
    let st = state(Decision::Allow);
    seed(&st).await;
    let mine = build_map(&st, "alice").await;

    for method in ["GET", "DELETE"] {
        let (code, _, pd) = call(
            &st,
            method,
            &format!("entityMaps/{mine}"),
            Some("mallory"),
            None,
            None,
        )
        .await;
        assert_eq!(code, StatusCode::NOT_FOUND, "{method}: {pd}");
        assert_eq!(
            pd["type"], "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound",
            "{method} answered with an error type that admits the map exists"
        );
    }

    let (code, _, body) = call(
        &st,
        "GET",
        &format!("entityMaps/{mine}"),
        Some("alice"),
        None,
        None,
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    let (code, _, _) = call(
        &st,
        "DELETE",
        &format!("entityMaps/{mine}"),
        Some("alice"),
        None,
        None,
    )
    .await;
    assert_eq!(code, StatusCode::NO_CONTENT);
}

/// 5.5.14's one allowance in the other direction: other components "shall
/// only be allowed to update the expiry timestamp of the EntityMap, which
/// can optionally be extended". A subject that did not build the map may
/// still extend it, and the tighter rule above must not have taken that away.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn another_component_may_still_extend_a_map_it_did_not_build() {
    let st = state(Decision::Allow);
    seed(&st).await;
    let mine = build_map(&st, "alice").await;
    let later = (chrono::Utc::now() + chrono::Duration::seconds(600))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();

    let (code, _, body) = call(
        &st,
        "PATCH",
        &format!("entityMaps/{mine}"),
        Some("mallory"),
        None,
        Some(json!({"expiresAt": later})),
    )
    .await;
    assert_eq!(code, StatusCode::NO_CONTENT, "{body}");
}

/// Table 5.2.39-1/-2 lists every member an EntityMap has, and whose map it
/// is is not among them.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_served_map_carries_no_record_of_whose_it_is() {
    let st = state(Decision::Allow);
    seed(&st).await;
    let (code, h, created) = call(
        &st,
        "GET",
        "entityMaps?type=Vehicle",
        Some("alice"),
        None,
        None,
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{created}");
    let (code, _, retrieved) = call(
        &st,
        "GET",
        &format!("entityMaps/{}", map_id(&h)),
        Some("alice"),
        None,
        None,
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{retrieved}");
    for (what, doc) in [("created", created), ("retrieved", retrieved)] {
        let served = doc.to_string();
        assert!(
            !served.contains("__") && !served.contains("alice"),
            "the {what} map carries an internal member: {served}"
        );
    }
}

/// A snapshot's fill copies whatever its queries match, after the request
/// that asked for it has been answered. There is no narrowed form of that,
/// so 5.16.1, 5.16.2 and 5.16.7 join 5.6.21 Purge Entities: a narrowing
/// answer is a refusal, and the refusal is this broker's own error type
/// because Table 6.3.2-1 names none.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_narrowing_answer_to_a_whole_tenant_operation_is_a_refusal() {
    let st = state(Decision::Filter(Filter {
        q: Some(antares_ql::parse_q("speed<50").expect("q")),
        ..Filter::default()
    }));
    // no seed: the gate answers before the operation reads anything, and
    // this engine narrows the create too, which is refused for the same
    // reason the four operations below are
    let snapshot = json!({"type": "Snapshot", "snapshotQueries": [
        {"type": "Query", "entities": [{"type": "Vehicle"}]}]});
    for (method, path, body) in [
        ("POST", "snapshots".to_owned(), Some(snapshot.clone())),
        (
            "POST",
            "snapshots/urn:ngsi-ld:Snapshot:x/clone".to_owned(),
            // 5.16.2.3 makes the clone body optional, but 6.3.4 still wants
            // a Content-Length on a POST
            Some(json!({})),
        ),
        ("DELETE", "snapshots".to_owned(), None),
        ("DELETE", "entities?type=Vehicle".to_owned(), None),
    ] {
        let (code, _, pd) = call(&st, method, &path, Some("alice"), None, body).await;
        assert_eq!(
            code,
            StatusCode::FORBIDDEN,
            "{method} {path} was answered with a narrowing: {pd}"
        );
        assert_eq!(pd["type"], "urn:antares:error:AccessDenied", "{pd}");
        assert!(
            pd["detail"]
                .as_str()
                .is_some_and(|d| d.contains("cannot be narrowed")),
            "the refusal does not say why: {pd}"
        );
    }
}

/// The other half of that rule: an EMPTY filter narrows nothing, so it is an
/// allow rather than a refusal — a policy engine that hands back a default
/// `Filter` must not lock a deployment out of its own snapshots.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_empty_filter_on_a_whole_tenant_operation_is_an_allow() {
    let st = state(Decision::Filter(Filter::default()));
    seed(&st).await;
    let (code, _, body) = call(
        &st,
        "POST",
        "snapshots",
        Some("alice"),
        None,
        Some(json!({"type": "Snapshot", "snapshotQueries": [
            {"type": "Query", "entities": [{"type": "Vehicle"}]}]})),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{body}");
}

/// 5.2.41 Table 5.2.41-1/-2 lists every member a Snapshot has. Whose it is
/// is the broker's own record — the fill runs under it after the request is
/// over — and belongs in no representation and in no client's body.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_snapshot_records_its_creator_and_serves_it_to_nobody() {
    let st = state(Decision::Allow);
    seed(&st).await;
    let (code, _, body) = call(
        &st,
        "POST",
        "snapshots",
        Some("alice"),
        None,
        Some(json!({"id": "urn:ngsi-ld:Snapshot:p5", "type": "Snapshot",
                    "__subject": [["x-subject", "mallory"]],
                    "__tenant": "snap-forged",
                    "snapshotQueries": [{"type": "Query", "entities": [{"type": "Vehicle"}]}]})),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{body}");

    let (code, _, doc) = call(
        &st,
        "GET",
        "snapshots/urn:ngsi-ld:Snapshot:p5",
        Some("alice"),
        None,
        None,
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{doc}");
    let served = doc.to_string();
    assert!(
        !served.contains("__"),
        "the snapshot served an internal member: {served}"
    );
    assert!(
        !served.contains("alice") && !served.contains("mallory") && !served.contains("forged"),
        "the snapshot served a subject or a forged tenant: {served}"
    );
}
