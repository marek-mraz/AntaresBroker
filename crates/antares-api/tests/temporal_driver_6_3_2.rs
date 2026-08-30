// SPDX-License-Identifier: EUPL-1.2
//! Temporal storage as a driver choice: a broker composed WITHOUT a
//! temporal store answers the temporal API with the error type
//! OperationNotSupported and HTTP 422 (CIM 009 Table 6.3.2-1:
//! https://uri.etsi.org/ngsi-ld/errors/OperationNotSupported -> 422),
//! records no history, and leaves the current-state API untouched.
//!
//! Both directions asserted: the refusal must be exactly the mapped error
//! (not 404, not 500, no partial data), and the same requests against the
//! default composition (fused store) must NOT be refused — proving the
//! behaviour comes from the driver choice, not from the temporal routes.

use antares_api::AppState;
use antares_sql::store::any::AnyStore;
use antares_sql::store::Store;
use antares_store::NoTemporal;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;

fn state_without_temporal() -> AppState {
    let store = Arc::new(AnyStore::Mem(Store::default()));
    AppState::with_drivers("me".into(), store, Arc::new(NoTemporal), "memory")
}

async fn req(st: &AppState, method: &str, path: &str, body: Option<Value>) -> (StatusCode, Value) {
    let mut b = Request::builder().method(method).uri(path);
    let body = match body {
        Some(v) => {
            let s = v.to_string();
            b = b
                .header("Content-Type", "application/json")
                .header("Content-Length", s.len());
            Body::from(s)
        }
        None => Body::empty(),
    };
    let resp = antares_api::router(st.clone())
        .oneshot(b.body(body).expect("req"))
        .await
        .expect("resp");
    let status = resp.status();
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn assert_unsupported(status: StatusCode, body: &Value) {
    assert_eq!(
        status, 422,
        "Table 6.3.2-1 maps OperationNotSupported to 422"
    );
    assert_eq!(
        body["type"], "https://uri.etsi.org/ngsi-ld/errors/OperationNotSupported",
        "the problem type must be the spec's OperationNotSupported URI: {body}"
    );
    assert!(
        body.get("data").is_none() && body.get("id").is_none(),
        "a refusal must carry no partial temporal data: {body}"
    );
}

#[tokio::test]
async fn temporal_reads_answer_operation_not_supported() {
    let st = state_without_temporal();
    let (status, body) = req(
        &st,
        "GET",
        "/ngsi-ld/v1/temporal/entities?type=T&timerel=after&timeAt=2020-08-01T12:00:00Z",
        None,
    )
    .await;
    assert_unsupported(status, &body);
    let (status, body) = req(&st, "GET", "/ngsi-ld/v1/temporal/entities/urn:x:1", None).await;
    assert_unsupported(status, &body);
}

#[tokio::test]
async fn current_state_still_works_and_records_no_history() {
    let st = state_without_temporal();
    assert!(!st.record_locally(), "NoTemporal means nothing records");
    let (status, _) = req(
        &st,
        "POST",
        "/ngsi-ld/v1/entities",
        Some(json!({"id": "urn:x:1", "type": "T",
                    "speed": {"type": "Property", "value": 3}})),
    )
    .await;
    assert_eq!(
        status, 201,
        "entity provision is untouched by the driver choice"
    );
    let (status, body) = req(&st, "GET", "/ngsi-ld/v1/entities/urn:x:1", None).await;
    assert_eq!(status, 200);
    assert_eq!(body["id"], "urn:x:1");
    // the recorder produced nothing — the driver holds no doc for the entity
    let t = antares_model::TenantId::new(antares_model::TenantId::DEFAULT).expect("tenant");
    assert_eq!(st.temporal.get(&t, "urn:x:1").expect("driver ok"), None);
}

#[tokio::test]
async fn the_default_composition_is_not_refused() {
    // control: same requests against the fused store — anything but the
    // unsupported refusal proves the 422 above comes from the driver choice
    let st = AppState::new("me".into());
    let (status, _) = req(
        &st,
        "GET",
        "/ngsi-ld/v1/temporal/entities?type=T&timerel=after&timeAt=2020-08-01T12:00:00Z",
        None,
    )
    .await;
    assert_ne!(status, 422, "a temporal-capable broker must not refuse");
    let (status, _) = req(&st, "GET", "/ngsi-ld/v1/temporal/entities/urn:x:1", None).await;
    assert_ne!(status, 422);
}
