// SPDX-License-Identifier: EUPL-1.2
//! 4.5.9 Simplified temporal representation: each `values` element is
//! "another Array containing exactly two array elements: the first element
//! shall be a Property value and the second element shall correspond to the
//! associated Temporal Property". The value is the one the Property holds —
//! the representation abbreviates the instance, it does not retype it.

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

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

async fn get(st: &AppState, uri: &str) -> (StatusCode, Value) {
    send(
        st,
        Request::builder()
            .method("GET")
            .uri(uri)
            .body(Body::empty())
            .expect("request"),
    )
    .await
}

const ID: &str = "urn:ngsi-ld:Meter:tv1";

/// The three numeric shapes a `f64` round trip does not survive: an integer
/// keeps its JSON type, a float keeps its own, and an i64 past 2^53 keeps
/// every digit.
async fn seed(st: &AppState) {
    let body = json!({
        "id": ID,
        "type": "Meter",
        "count": {"type": "Property", "value": 120, "observedAt": "2026-01-01T00:00:00Z"},
        "ratio": {"type": "Property", "value": 1.5, "observedAt": "2026-01-01T00:00:00Z"},
        "serial": {
            "type": "Property",
            "value": 9_007_199_254_740_993i64,
            "observedAt": "2026-01-01T00:00:00Z"
        },
    })
    .to_string();
    let (status, b) = send(
        st,
        Request::builder()
            .method("POST")
            .uri("/ngsi-ld/v1/entities")
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "seed: {b}");
}

/// The first element of each pair, for one attribute of the simplified
/// temporal representation.
fn values_of(entity: &Value, attr: &str) -> Vec<Value> {
    entity
        .get(attr)
        .and_then(|a| a.get("values"))
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("no values for {attr}: {entity}"))
        .iter()
        .map(|pair| pair[0].clone())
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn clause_4_5_9_the_simplified_representation_keeps_the_value_the_property_holds() {
    let mut st = AppState::new("me".into());
    antares_api::notify::wire(&mut st); // temporal auto-recording
    seed(&st).await;

    let (status, e) = get(
        &st,
        &format!("/ngsi-ld/v1/temporal/entities/{ID}?options=temporalValues&timerel=after&timeAt=1970-01-01T00:00:00Z"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{e}");

    assert_eq!(
        values_of(&e, "count"),
        vec![json!(120)],
        "an integer came back as something else: {e}"
    );
    assert_eq!(
        values_of(&e, "ratio"),
        vec![json!(1.5)],
        "a float did not survive: {e}"
    );
    assert_eq!(
        values_of(&e, "serial"),
        vec![json!(9_007_199_254_740_993i64)],
        "an integer past 2^53 lost a digit to a f64 round trip: {e}"
    );
}

/// The normalized representation is the reference: whatever it returns for
/// the same instance is what the simplified one abbreviates.
#[tokio::test(flavor = "multi_thread")]
async fn clause_4_5_9_the_simplified_and_normalized_representations_agree_on_the_value() {
    let mut st = AppState::new("me".into());
    antares_api::notify::wire(&mut st);
    seed(&st).await;

    let win = "timerel=after&timeAt=1970-01-01T00:00:00Z";
    let (_, simple) = get(
        &st,
        &format!("/ngsi-ld/v1/temporal/entities/{ID}?options=temporalValues&{win}"),
    )
    .await;
    let (_, normal) = get(&st, &format!("/ngsi-ld/v1/temporal/entities/{ID}?{win}")).await;

    for attr in ["count", "ratio", "serial"] {
        let instances = normal
            .get(attr)
            .and_then(Value::as_array)
            .unwrap_or_else(|| panic!("no instances for {attr}: {normal}"));
        let reference: Vec<Value> = instances.iter().map(|i| i["value"].clone()).collect();
        assert_eq!(
            values_of(&simple, attr),
            reference,
            "{attr}: the two representations disagree on the same instance"
        );
    }
}
