// SPDX-License-Identifier: EUPL-1.2
//! 5.7.3.4 and 5.7.4.4: "If projection attributes are present and indicate
//! the use of Linked Entity retrieval, an error of type BadRequestData shall
//! be raised." A `{…}` level in a projection is one Linked Entity hop
//! (5.7.1.4); the temporal consumption operations define no join, so the
//! rejection is unconditional — there is no `join` parameter that makes a
//! hopping `pick` or `omit` acceptable on either of them.

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

const ID: &str = "urn:ngsi-ld:Vehicle:tp1";
const WINDOW: &str = "timerel=after&timeAt=2000-01-01T00:00:00Z&timeproperty=createdAt";

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

async fn seeded() -> AppState {
    let mut st = AppState::new("me".into());
    antares_api::notify::wire(&mut st);
    let body = json!({"id": ID, "type": "Vehicle",
        "speed": {"type": "Property", "value": 10}})
    .to_string();
    let (status, b) = send(
        &st,
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
    st
}

fn is_bad_request(status: StatusCode, body: &Value) {
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(
        body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData",
        "{body}"
    );
}

/// 5.7.3.4 on the single-Entity temporal retrieve: a `pick` or an `omit`
/// carrying a `{…}` Linked Entity hop is BadRequestData, while the flat
/// projection is the accepted case on the same operation.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_7_3_4_a_hopping_projection_is_refused_on_the_retrieve() {
    let st = seeded().await;
    let base = format!("/ngsi-ld/v1/temporal/entities/{ID}?{WINDOW}");
    refuses_hops_accepts_flat(&st, &base).await;
}

/// 5.7.4.4 on the temporal query: the same rule, the same rejection.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_7_4_4_a_hopping_projection_is_refused_on_the_query() {
    let st = seeded().await;
    let base = format!("/ngsi-ld/v1/temporal/entities?type=Vehicle&{WINDOW}");
    refuses_hops_accepts_flat(&st, &base).await;
}

async fn refuses_hops_accepts_flat(st: &AppState, base: &str) {
    for proj in ["pick=speed%7Bvalue%7D", "omit=speed%7Bvalue%7D"] {
        let (status, body) = get(st, &format!("{base}&{proj}")).await;
        is_bad_request(status, &body);
        assert!(
            body["detail"]
                .as_str()
                .is_some_and(|d| d.contains("Linked Entity")),
            "{proj}: {body}"
        );
    }
    let (status, body) = get(st, &format!("{base}&pick=speed")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}
