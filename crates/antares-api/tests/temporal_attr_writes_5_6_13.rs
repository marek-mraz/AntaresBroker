// SPDX-License-Identifier: EUPL-1.2
//! What the temporal Attribute writes answer when there is nothing to write
//! to. 5.6.13, 5.6.14, 5.6.15 and 5.6.16 each raise ResourceNotFound in two
//! different situations — "the NGSI-LD endpoint does not know about the
//! target Entity … and no matching registrations apply", and "the target
//! Temporal Evolution of an Entity does not contain the target Attribute"
//! (5.6.14/5.6.15 add "no instance with the specified instanceId exists") —
//! and 204 with no body when the target was there. On a broker with no
//! registration the local result is the whole answer.

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

const ID: &str = "urn:ngsi-ld:Vehicle:tw1";
const ABSENT: &str = "urn:ngsi-ld:Vehicle:tw-absent";

async fn send(st: &AppState, req: Request<Body>) -> (StatusCode, Value) {
    let res = antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("response");
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .expect("body");
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, body)
}

async fn delete(st: &AppState, uri: &str) -> (StatusCode, Value) {
    send(
        st,
        Request::builder()
            .method("DELETE")
            .uri(uri)
            .body(Body::empty())
            .expect("request"),
    )
    .await
}

/// A Temporal Evolution with one `speed` instance, created through the
/// 5.6.11 endpoint so the instance carries the instanceId the writes address.
async fn seeded() -> AppState {
    let mut st = AppState::new("me".into());
    antares_api::wire(&mut st).await;
    let body = json!({
        "id": ID, "type": "Vehicle",
        "speed": [{"type": "Property", "value": 10, "observedAt": "2026-01-01T09:00:00Z"}]
    })
    .to_string();
    let (status, b) = send(
        &st,
        Request::builder()
            .method("POST")
            .uri("/ngsi-ld/v1/temporal/entities")
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("request"),
    )
    .await;
    assert!(status.is_success(), "seed: {status} {b}");
    st
}

/// The instanceId the broker minted for the seeded instance.
async fn instance_id(st: &AppState) -> String {
    let (status, body) = send(
        st,
        Request::builder()
            .method("GET")
            .uri(format!(
                "/ngsi-ld/v1/temporal/entities/{ID}?timerel=after&timeAt=2000-01-01T00:00:00Z"
            ))
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["speed"][0]["instanceId"]
        .as_str()
        .expect("instanceId")
        .to_owned()
}

fn is_not_found(status: StatusCode, body: &Value) {
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(
        body["type"], "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound",
        "{body}"
    );
    // the ProblemDetails of 6.3.2 and nothing else — no store internals
    for k in ["tenant", "instanceId", "entityMap", "regs"] {
        assert!(body.get(k).is_none(), "{k} leaked into the error: {body}");
    }
}

/// 5.6.16: "If there is no existing Temporal Evolution of an Entity whose id
/// is equivalent held locally and no matching registrations apply, then an
/// error of type ResourceNotFound shall be raised."
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_6_16_deleting_an_unknown_temporal_entity_is_not_found() {
    let st = seeded().await;
    let (status, body) = delete(&st, &format!("/ngsi-ld/v1/temporal/entities/{ABSENT}")).await;
    is_not_found(status, &body);

    let (status, body) = delete(&st, &format!("/ngsi-ld/v1/temporal/entities/{ID}")).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    assert_eq!(body, Value::Null, "204 carries no body");
}

/// 5.6.13: unknown Entity and known Entity without the target Attribute are
/// both ResourceNotFound; the attribute that is there deletes with 204.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_6_13_delete_attrs_separates_unknown_entity_from_unknown_attribute() {
    let st = seeded().await;
    let (status, body) = delete(
        &st,
        &format!("/ngsi-ld/v1/temporal/entities/{ABSENT}/attrs/speed"),
    )
    .await;
    is_not_found(status, &body);

    let (status, body) = delete(
        &st,
        &format!("/ngsi-ld/v1/temporal/entities/{ID}/attrs/humidity"),
    )
    .await;
    is_not_found(status, &body);

    let (status, body) = delete(
        &st,
        &format!("/ngsi-ld/v1/temporal/entities/{ID}/attrs/speed"),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
}

/// 5.6.15: "If for the target Attribute no instance with the specified
/// instanceId exists, an error of type ResourceNotFound shall be raised" —
/// distinct from the unknown Entity, which raises the same error naming the
/// Entity instead.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_6_15_delete_instance_separates_unknown_entity_from_unknown_instance() {
    let st = seeded().await;
    let iid = instance_id(&st).await;

    let (status, body) = delete(
        &st,
        &format!("/ngsi-ld/v1/temporal/entities/{ABSENT}/attrs/speed/{iid}"),
    )
    .await;
    is_not_found(status, &body);

    let (status, body) = delete(
        &st,
        &format!("/ngsi-ld/v1/temporal/entities/{ID}/attrs/speed/urn:ngsi-ld:Instance:nope"),
    )
    .await;
    is_not_found(status, &body);

    let (status, body) = delete(
        &st,
        &format!("/ngsi-ld/v1/temporal/entities/{ID}/attrs/speed/{iid}"),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
}

/// 5.6.14: the same two ResourceNotFound situations on the modify, and on
/// success "the createdAt property of the concerned instance shall remain
/// unchanged, but the modifiedAt property shall be set to the timestamp
/// corresponding to this modification".
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_6_14_modify_instance_is_not_found_without_a_target() {
    let st = seeded().await;
    let iid = instance_id(&st).await;
    // 5.6.14.4 replaces the instance, so the fragment carries the whole
    // instance the Context Producer wants stored — `observedAt` included.
    let frag =
        json!({"type": "Property", "value": 42, "observedAt": "2026-01-01T09:00:00Z"}).to_string();
    let patch = |uri: String, body: String| {
        Request::builder()
            .method("PATCH")
            .uri(uri)
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("request")
    };

    let (status, body) = send(
        &st,
        patch(
            format!("/ngsi-ld/v1/temporal/entities/{ABSENT}/attrs/speed/{iid}"),
            frag.clone(),
        ),
    )
    .await;
    is_not_found(status, &body);

    let (status, body) = send(
        &st,
        patch(
            format!("/ngsi-ld/v1/temporal/entities/{ID}/attrs/speed/urn:ngsi-ld:Instance:nope"),
            frag.clone(),
        ),
    )
    .await;
    is_not_found(status, &body);

    let (status, body) = send(
        &st,
        patch(
            format!("/ngsi-ld/v1/temporal/entities/{ID}/attrs/speed/{iid}"),
            frag,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    let (status, body) = send(
        &st,
        Request::builder()
            .method("GET")
            .uri(format!(
                "/ngsi-ld/v1/temporal/entities/{ID}?timerel=after&timeAt=2000-01-01T00:00:00Z&options=sysAttrs"
            ))
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let inst = &body["speed"][0];
    assert_eq!(inst["value"], 42, "{body}");
    assert_eq!(inst["instanceId"], iid.as_str(), "instanceId is preserved");
    assert!(
        inst["modifiedAt"].as_str() >= inst["createdAt"].as_str(),
        "modifiedAt moves, createdAt does not: {inst}"
    );
}

const REPLACED: &str = "urn:ngsi-ld:Vehicle:tw-replace";

/// 5.6.14.4: "Replace the target Attribute instance identified by the
/// instanceId with the Attribute instance in the EntityTemporal Fragment. The
/// createdAt property of the concerned instance shall remain unchanged, but
/// the modifiedAt property shall be set to the timestamp corresponding to this
/// modification." Replace, not merge — a member the stored instance carries
/// and the fragment does not is gone — and the instance keeps the identity the
/// request addressed it by.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_6_14_modify_instance_replaces_the_instance_rather_than_merging_into_it() {
    let mut st = AppState::new("me".into());
    antares_api::wire(&mut st).await;
    let body = json!({
        "id": REPLACED, "type": "Vehicle",
        "speed": [{
            "type": "Property", "value": 120, "observedAt": "2020-09-01T12:03:00Z",
            "unitCode": "KMH",
            "accuracy": {"type": "Property", "value": 0.5}
        }]
    })
    .to_string();
    let (status, b) = send(
        &st,
        Request::builder()
            .method("POST")
            .uri("/ngsi-ld/v1/temporal/entities")
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("request"),
    )
    .await;
    assert!(status.is_success(), "seed: {status} {b}");

    let read = || async {
        let (status, body) = send(
            &st,
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/ngsi-ld/v1/temporal/entities/{REPLACED}\
                     ?timerel=after&timeAt=2000-01-01T00:00:00Z&options=sysAttrs"
                ))
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body["speed"][0].clone()
    };
    let before = read().await;
    let iid = before["instanceId"]
        .as_str()
        .expect("instanceId")
        .to_owned();

    let fragment =
        json!({"type": "Property", "value": 129, "observedAt": "2020-09-01T12:03:00Z"}).to_string();
    let (status, b) = send(
        &st,
        Request::builder()
            .method("PATCH")
            .uri(format!(
                "/ngsi-ld/v1/temporal/entities/{REPLACED}/attrs/speed/{iid}"
            ))
            .header("Content-Type", "application/json")
            .header("Content-Length", fragment.len())
            .body(Body::from(fragment))
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{b}");

    let after = read().await;
    assert_eq!(after["value"], json!(129), "{after}");
    for gone in ["unitCode", "accuracy"] {
        assert!(
            after.get(gone).is_none(),
            "{gone} survived a replace: {after}"
        );
    }
    assert_eq!(
        after["instanceId"], before["instanceId"],
        "the replaced instance lost the id it was addressed by: {after}"
    );
    assert_eq!(
        after["createdAt"], before["createdAt"],
        "createdAt shall remain unchanged: {after}"
    );
    assert_ne!(
        after["modifiedAt"],
        Value::Null,
        "modifiedAt shall be set: {after}"
    );
}
