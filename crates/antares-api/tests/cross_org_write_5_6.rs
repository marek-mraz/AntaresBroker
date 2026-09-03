// SPDX-License-Identifier: EUPL-1.2
//! A write another organization owns is refused, and the refusal says
//! nothing about whether the Entity is there.
//!
//! Three organizations share one broker and split the Entity id space
//! between them, so an engine decides a write by the segment the URN names.
//! ADR-0020 puts that decision on the seam: a `Deny` is a 403 in this
//! broker's own namespace, because Table 6.3.2-1 names no access error.
//!
//! The reason it may not be a 404 instead is the pair of answers, not the
//! single one. 5.6.2 raises ResourceNotFound when the Entity "does not
//! exist"; if a refusal borrowed that, a caller writing to two ids in
//! another organization's segment would get ResourceNotFound for the absent
//! one and something else for the one that is there, and the existence of
//! every Entity in the deployment becomes readable one probe at a time. The
//! seam answers before the store is consulted, so both answers are the same
//! 403 and neither is evidence.
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
use std::sync::Arc;
use tower::ServiceExt;

/// The other organization's segment of the id space.
const THEIRS: &str = "urn:ngsi-ld:Vehicle:orgB:";
const THEIR_ENTITY: &str = "urn:ngsi-ld:Vehicle:orgB:present";
const THEIR_ABSENT: &str = "urn:ngsi-ld:Vehicle:orgB:absent";
const OURS: &str = "urn:ngsi-ld:Vehicle:orgA:present";

/// An engine that owns one segment of the Entity id space and refuses every
/// operation naming an id outside it — the segment rule a deployment that
/// splits one broker between organizations writes.
struct Segmented;

impl PolicyEngine for Segmented {
    fn name(&self) -> &str {
        "segmented"
    }

    fn decide<'a>(&'a self, _s: &'a Subject, op: &'a Operation<'a>) -> DecisionFuture<'a> {
        let theirs = op.ids.iter().any(|id| id.starts_with(THEIRS))
            || op
                .body
                .map(body_ids)
                .is_some_and(|ids| ids.iter().any(|id| id.starts_with(THEIRS)));
        let answer = if theirs {
            Decision::Deny("the Entity belongs to another organization".into())
        } else {
            Decision::Allow
        };
        Box::pin(std::future::ready(answer))
    }

    fn pre_notify(&self, _s: &Subject, _sub: &Value, _n: &mut Value) -> NotifyDecision {
        NotifyDecision::Deliver
    }
}

/// The ids a write body names, whether it is one Entity or a batch.
fn body_ids(body: &Value) -> Vec<&str> {
    match body {
        Value::Array(a) => a.iter().filter_map(|e| e["@id"].as_str()).collect(),
        v => v["@id"].as_str().into_iter().collect(),
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

fn vehicle(id: &str) -> Value {
    json!({"id": id, "type": "Vehicle", "speed": {"type": "Property", "value": 1}})
}

/// One broker holding an Entity in each organization's segment, seeded
/// before the segment rule is put in front of it. Both handles share the
/// store, so the open one reads back what the guarded one refused to write.
async fn seeded() -> (AppState, AppState) {
    let open = AppState::new("me".into());
    for id in [THEIR_ENTITY, OURS] {
        let (code, body) = send(&open, "POST", "/ngsi-ld/v1/entities", Some(vehicle(id))).await;
        assert_eq!(code, StatusCode::CREATED, "seed {id}: {body}");
    }
    let guarded = open.clone().with_policy(Arc::new(Segmented));
    (open, guarded)
}

/// Every write that names an Entity id: the path form and the body form.
fn writes(id: &str) -> Vec<(&'static str, String, Option<Value>)> {
    vec![
        ("POST", "/ngsi-ld/v1/entities".to_owned(), Some(vehicle(id))),
        (
            "PATCH",
            format!("/ngsi-ld/v1/entities/{id}/attrs"),
            Some(json!({"speed": {"type": "Property", "value": 2}})),
        ),
        (
            "POST",
            format!("/ngsi-ld/v1/entities/{id}/attrs"),
            Some(json!({"colour": {"type": "Property", "value": "red"}})),
        ),
        (
            "PUT",
            format!("/ngsi-ld/v1/entities/{id}"),
            Some(vehicle(id)),
        ),
        ("DELETE", format!("/ngsi-ld/v1/entities/{id}"), None),
        (
            "POST",
            "/ngsi-ld/v1/entityOperations/upsert".to_owned(),
            Some(json!([vehicle(id)])),
        ),
    ]
}

/// The refusal is the seam's, on every write surface, and it is the same
/// refusal whether the Entity is there or not.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_write_into_another_organizations_segment_is_a_refusal_not_a_not_found() {
    let (_open, st) = seeded().await;
    for ((method, uri, body), (_, absent_uri, absent_body)) in
        writes(THEIR_ENTITY).into_iter().zip(writes(THEIR_ABSENT))
    {
        let (code, pd) = send(&st, method, &uri, body).await;
        assert_eq!(code, StatusCode::FORBIDDEN, "{method} {uri}: {pd}");
        assert_eq!(pd["type"], antares_api::policy::ACCESS_DENIED_TYPE);
        assert_eq!(pd["status"], 403);

        let (absent_code, absent_pd) = send(&st, method, &absent_uri, absent_body).await;
        assert_eq!(
            absent_code, code,
            "{method} {absent_uri} answered differently from the Entity that exists"
        );
        assert_eq!(
            absent_pd["type"], pd["type"],
            "{method} {absent_uri}: the refusal told the caller the Entity is absent"
        );
        assert_eq!(absent_pd["detail"], pd["detail"], "{method} {absent_uri}");
        assert_eq!(absent_pd["title"], pd["title"], "{method} {absent_uri}");
    }
}

/// The refusal is decided before the store is read: the Entity another
/// organization owns is still there after every refused write, and this
/// broker's own segment is untouched by the rule.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_refusal_writes_nothing_and_leaves_this_organizations_writes_alone() {
    let (open, st) = seeded().await;
    for (method, uri, body) in writes(THEIR_ENTITY) {
        let (code, _) = send(&st, method, &uri, body).await;
        assert_eq!(code, StatusCode::FORBIDDEN, "{method} {uri}");
    }
    // read back through the ungoverned handle on the same store: the
    // guarded one would refuse this read too, and prove nothing
    let (code, doc) = send(
        &open,
        "GET",
        &format!("/ngsi-ld/v1/entities/{THEIR_ENTITY}"),
        None,
    )
    .await;
    assert_eq!(code, StatusCode::OK, "a refused write changed the store");
    assert_eq!(
        doc["speed"]["value"], 1,
        "a refused write reached the store"
    );
    assert!(
        doc.get("colour").is_none(),
        "a refused append reached the store"
    );

    // The rule is narrow: this organization's own writes reach the clause
    // and are answered by it. What they are answered WITH is 5.6's business
    // — a create over a seeded id is a 409 — so what is asserted is that
    // the seam did not take the decision.
    for (method, uri, body) in writes(OURS) {
        let (code, doc) = send(&st, method, &uri, body).await;
        assert_ne!(
            code,
            StatusCode::FORBIDDEN,
            "the segment rule refused this organization's own write: {method} {uri}: {doc}"
        );
        assert_ne!(
            doc["type"],
            antares_api::policy::ACCESS_DENIED_TYPE,
            "{method} {uri}"
        );
    }
}
