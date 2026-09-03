// SPDX-License-Identifier: EUPL-1.2
//! The policy seam proved from outside `crates/`: the reference engine runs
//! the contract every engine is held to, and then answers real requests
//! through the broker's own router. An engine that only passed its own unit
//! tests would prove the rules it implements, not that the broker calls it.

use antares_api::policy::{run_policy_contract, PolicyEngine};
use antares_api::AppState;
use antares_plugin_example::{ExamplePolicy, ExampleStore};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;

const CTX: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld";

/// The tenant the rules below name. `TenantId::default()` is what the
/// contract asks about, so the contract runs against an engine that is
/// actually enforcing something rather than one with nothing to say.
fn rules() -> Value {
    json!({
        antares_model::TenantId::default().as_str(): {
            "denyTypes": ["Secret"],
            "omit": ["price"],
            "q": "speed<100"
        }
    })
}

fn engine() -> Arc<dyn PolicyEngine> {
    Arc::new(ExamplePolicy::from_value(&rules()).expect("rules"))
}

fn state() -> AppState {
    let store = Arc::new(ExampleStore::new());
    AppState::with_drivers(
        "plugin".into(),
        store.clone(),
        store,
        antares_plugin_example::NAME,
    )
    .with_policy(engine())
}

async fn send(st: &AppState, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let mut req = Request::builder().method(method).uri(uri);
    let body = match body {
        Some(v) => {
            let raw = v.to_string();
            req = req
                .header("Content-Type", "application/ld+json")
                .header("Content-Length", raw.len());
            Body::from(raw)
        }
        None => Body::empty(),
    };
    let resp = antares_api::router(st.clone())
        .oneshot(req.body(body).expect("request"))
        .await
        .expect("response");
    let status = resp.status();
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json)
}

/// The kit function `antares-api` writes the seam against. It asserts what
/// an engine can get wrong — stop answering, hand back something the seam
/// has to override, or add a member to an answer — and it asserts it
/// through the seam, so passing here is passing as the broker will call it.
#[tokio::test(flavor = "multi_thread")]
async fn the_reference_engine_keeps_the_policy_contract() {
    run_policy_contract(&*engine()).await;
}

/// A refused operation is a 403 carrying this broker's own error type. CIM
/// 009 Table 6.3.2-1 names no access-denied error, so none is claimed under
/// the ETSI namespace.
#[tokio::test(flavor = "multi_thread")]
async fn a_denied_entity_type_is_refused_with_403() {
    let st = state();
    let (code, body) = send(
        &st,
        "POST",
        "/ngsi-ld/v1/entities",
        Some(json!({"id": "urn:ngsi-ld:Secret:1", "type": "Secret", "@context": CTX})),
    )
    .await;
    assert_eq!(code, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(
        body["type"], "urn:antares:error:AccessDenied",
        "a policy refusal is told apart from a spec error by its namespace: {body}"
    );

    // ...and the refusal is this engine's rule, not the seam refusing
    // everything: one type further, the same tenant, is a 201.
    let (code, body) = send(
        &st,
        "POST",
        "/ngsi-ld/v1/entities",
        Some(json!({
            "id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle",
            "speed": {"type": "Property", "value": 10}, "@context": CTX
        })),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{body}");
}

/// The narrowing that cannot be done in front of the broker: the engine's
/// `q` is conjoined into the query the store runs, so the Entity it excludes
/// is not in the answer at all — and the Attribute it omits is gone from the
/// one that is. The write itself was never narrowed: a filter narrows an
/// answer, so the Attribute is in the store.
#[tokio::test(flavor = "multi_thread")]
async fn a_filtered_query_hides_the_entity_and_the_attribute() {
    let st = state();
    for (id, speed) in [
        ("urn:ngsi-ld:Vehicle:slow", 10),
        ("urn:ngsi-ld:Vehicle:fast", 500),
    ] {
        let (code, body) = send(
            &st,
            "POST",
            "/ngsi-ld/v1/entities",
            Some(json!({
                "id": id, "type": "Vehicle",
                "speed": {"type": "Property", "value": speed},
                "price": {"type": "Property", "value": 42},
                "@context": CTX
            })),
        )
        .await;
        assert_eq!(code, StatusCode::CREATED, "{body}");
    }

    let (code, body) = send(&st, "GET", "/ngsi-ld/v1/entities?type=Vehicle", None).await;
    assert_eq!(code, StatusCode::OK, "{body}");
    let ids: Vec<&str> = body
        .as_array()
        .expect("an array of Entities")
        .iter()
        .filter_map(|e| e["id"].as_str())
        .collect();
    assert_eq!(
        ids,
        vec!["urn:ngsi-ld:Vehicle:slow"],
        "the engine's q selected what the store answered with: {body}"
    );
    assert!(
        body[0].get("price").is_none(),
        "the omit list is applied to what is served: {body}"
    );
    assert!(
        body[0].get("speed").is_some(),
        "and it narrows only what it names: {body}"
    );
}
