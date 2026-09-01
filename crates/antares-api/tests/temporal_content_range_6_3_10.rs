// SPDX-License-Identifier: EUPL-1.2
//! 6.3.10 on the temporal query surface (5.7.4): a result the broker cannot
//! serve in full is answered 206 with a `Content-Range` naming the interval
//! the body actually covers. A query spans many Temporal Evolutions, so the
//! advertised interval is the union of the per-entity intervals — and 4.6.3
//! lets each instant be spelled with or without a seconds fraction, which a
//! byte comparison orders wrongly (`.` sorts before `Z`).

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

async fn send(st: &AppState, req: Request<Body>) -> axum::http::Response<Body> {
    antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("response")
}

async fn post(st: &AppState, uri: &str, body: Value) {
    let body = body.to_string();
    let res = send(
        st,
        Request::builder()
            .method("POST")
            .uri(uri)
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("request"),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
}

/// One Temporal Evolution of `speed`, one instance per listed instant.
fn evolution(id: &str, at: &[&str]) -> Value {
    let speed: Vec<Value> = at
        .iter()
        .enumerate()
        .map(|(i, t)| json!({"type": "Property", "value": i, "observedAt": t}))
        .collect();
    json!({"id": id, "type": "Vehicle", "speed": speed})
}

/// Every `observedAt` the response body carries, across every entity.
fn served_instants(body: &Value) -> Vec<String> {
    let mut out: Vec<String> = body
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| e.get("speed"))
        .filter_map(Value::as_array)
        .flatten()
        .filter_map(|i| i.get("observedAt").and_then(Value::as_str))
        .map(str::to_owned)
        .collect();
    out.sort_by_key(|s| antares_model::dt_key(s));
    out
}

/// 6.3.10: the Content-Range of a truncated temporal QUERY bounds the
/// instances of every entity in the body, so its end is the latest instant
/// served anywhere in the result. Two entities whose latest served instants
/// differ only in the 4.6.3 seconds fraction pin that the union is taken on
/// the instant and not on the spelling: a byte comparison reads
/// `…:08:00.500Z` as earlier than `…:08:00Z` and ends the advertised range
/// before an instance the body contains.
#[tokio::test(flavor = "multi_thread")]
async fn clause_6_3_10_the_advertised_range_covers_every_entity_in_the_body() {
    let st = AppState::new("me".into());
    // ten instances each, so both evolutions are cut to the nine-instance
    // ceiling and the whole result is partial
    let plain: Vec<String> = (0..10)
        .map(|i| format!("2020-01-01T00:{i:02}:00Z"))
        .collect();
    let plain: Vec<&str> = plain.iter().map(String::as_str).collect();
    let fractional = [
        "2020-01-01T00:00:00Z",
        "2020-01-01T00:01:00Z",
        "2020-01-01T00:02:00Z",
        "2020-01-01T00:03:00Z",
        "2020-01-01T00:04:00Z",
        "2020-01-01T00:05:00Z",
        "2020-01-01T00:06:00Z",
        "2020-01-01T00:07:00Z",
        // the latest instant of the whole result, spelled with a fraction
        "2020-01-01T00:08:00.500Z",
        "2020-01-01T00:09:30Z",
    ];
    post(
        &st,
        "/ngsi-ld/v1/temporal/entities",
        evolution("urn:ngsi-ld:Vehicle:plain", &plain),
    )
    .await;
    post(
        &st,
        "/ngsi-ld/v1/temporal/entities",
        evolution("urn:ngsi-ld:Vehicle:fractional", &fractional),
    )
    .await;

    let res = send(
        &st,
        Request::builder()
            .method("GET")
            .uri(
                "/ngsi-ld/v1/temporal/entities?type=Vehicle\
                 &timerel=after&timeAt=2019-01-01T00:00:00Z",
            )
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(res.status(), StatusCode::PARTIAL_CONTENT);
    let range = res
        .headers()
        .get("Content-Range")
        .and_then(|v| v.to_str().ok())
        .expect("a truncated temporal query carries Content-Range")
        .to_owned();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .expect("body");
    let body: Value = serde_json::from_slice(&bytes).expect("json body");

    let instants = served_instants(&body);
    let latest = instants.last().expect("the body carries instances");
    assert_eq!(
        latest, "2020-01-01T00:08:00.500Z",
        "the fractional instant is the latest one served: {instants:?}"
    );
    // `date-time <start>-<end>/<size>`: both bounds are 4.6.3 DateTimes, so
    // the only `-` that separates them is the one right after the start's `Z`.
    let end = range
        .rsplit_once('/')
        .and_then(|(head, _)| head.split_once("Z-"))
        .map(|(_, end)| end.to_owned())
        .expect("Content-Range is date-time <start>-<end>/<size>");
    assert!(
        antares_model::dt_key(&end) >= antares_model::dt_key(latest),
        "the advertised range {range} ends before the instance at {latest} \
         that the body contains"
    );
}
