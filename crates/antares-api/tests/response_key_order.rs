//! Served-JSON key order: every response object
//! leads with `id` then `type` (the spec-example order), recursively — an
//! attribute object prints `"type": "Property"` first. Pure serialization
//! cosmetics: RFC 8259 objects are unordered and CIM 009 4.5.1 mandates
//! presence, not position — so these tests assert raw body BYTES, never a
//! parsed Value (parsing re-sorts).

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

async fn send_raw(st: &AppState, req: Request<Body>) -> (StatusCode, String) {
    let resp = antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("resp");
    let status = resp.status();
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

async fn post(st: &AppState, path: &str, doc: Value) -> (StatusCode, String) {
    let body = doc.to_string();
    send_raw(
        st,
        Request::builder()
            .method("POST")
            .uri(format!("/ngsi-ld/v1/{path}"))
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("req"),
    )
    .await
}

async fn get(st: &AppState, path_q: &str, accept: Option<&str>) -> (StatusCode, String) {
    let mut b = Request::builder().uri(format!("/ngsi-ld/v1/{path_q}"));
    if let Some(a) = accept {
        b = b.header("Accept", a);
    }
    send_raw(st, b.body(Body::empty()).expect("req")).await
}

/// `key` must appear in `body` and strictly before `after`.
fn before(body: &str, key: &str, after: &str) {
    let k = body
        .find(key)
        .unwrap_or_else(|| panic!("missing {key}: {body}"));
    let a = body
        .find(after)
        .unwrap_or_else(|| panic!("missing {after}: {body}"));
    assert!(k < a, "{key} must precede {after}: {body}");
}

/// State with two Vehicles whose attribute `aaa` sorts BEFORE `id` — the
/// probe that catches plain alphabetical serialization.
async fn seeded() -> AppState {
    let mut st = AppState::new("test".into());
    antares_api::notify::wire(&mut st);
    for n in 1..=2 {
        let (status, body) = post(
            &st,
            "entities",
            json!({
                "id": format!("urn:ngsi-ld:Vehicle:ko{n}"), "type": "Vehicle",
                "aaa": {"type": "Property", "value": n,
                        "observedAt": "2026-08-13T10:00:00Z"},
                "location": {"type": "GeoProperty",
                    "value": {"type": "Point", "coordinates": [24.9, 60.1]}}
            }),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
    }
    st
}

#[tokio::test(flavor = "multi_thread")]
async fn retrieve_entity_leads_with_id_then_type() {
    let st = seeded().await;
    let (status, body) = get(&st, "entities/urn:ngsi-ld:Vehicle:ko1", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.starts_with("{\"id\""), "id first: {body}");
    before(&body, "\"id\"", "\"type\"");
    before(&body, "\"type\":\"Vehicle\"", "\"aaa\"");
    // recursive: the attribute object leads with its "type" member
    before(&body, "\"aaa\":{\"type\"", "\"observedAt\"");
    // no double emission of the reordered keys
    assert_eq!(body.matches("\"id\"").count(), 1, "{body}");
    assert_eq!(body.matches("\"type\":\"Vehicle\"").count(), 1, "{body}");
}

#[tokio::test(flavor = "multi_thread")]
async fn query_list_elements_lead_with_id_then_type() {
    let st = seeded().await;
    let (status, body) = get(&st, "entities?type=Vehicle", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    // the streamed list starts every element on "id"
    assert!(body.starts_with("[{\"id\""), "first element: {body}");
    assert!(body.contains(",{\"id\""), "second element: {body}");
    assert_eq!(body.matches("{\"id\"").count(), 2, "{body}");
}

#[tokio::test(flavor = "multi_thread")]
async fn ld_json_and_geojson_lead_with_id() {
    let st = seeded().await;
    let (status, body) = get(
        &st,
        "entities/urn:ngsi-ld:Vehicle:ko1",
        Some("application/ld+json"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.starts_with("{\"id\""), "ld+json id first: {body}");
    before(&body, "\"type\":\"Vehicle\"", "\"@context\"");

    let (status, body) = get(
        &st,
        "entities/urn:ngsi-ld:Vehicle:ko1",
        Some("application/geo+json"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    // 5.2.29 Feature: id, then type=Feature, then the rest
    assert!(body.starts_with("{\"id\""), "geo+json id first: {body}");
    before(&body, "\"type\":\"Feature\"", "\"geometry\"");
    before(&body, "\"type\":\"Feature\"", "\"properties\"");
}

#[tokio::test(flavor = "multi_thread")]
async fn temporal_retrieve_leads_with_id_then_type() {
    let st = seeded().await;
    let (status, body) = get(&st, "temporal/entities/urn:ngsi-ld:Vehicle:ko1", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.starts_with("{\"id\""), "temporal id first: {body}");
    before(&body, "\"type\":\"Vehicle\"", "\"aaa\"");
}

/// Raw-byte capture server: the notification body as TEXT (parsing would
/// re-sort keys and hide the order under test).
async fn capture_raw() -> (String, tokio::sync::mpsc::Receiver<String>) {
    let (tx, rx) = tokio::sync::mpsc::channel::<String>(4);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let app = axum::Router::new().route(
        "/notify",
        axum::routing::post(move |body: axum::body::Bytes| {
            let tx = tx.clone();
            async move {
                let _ = tx.send(String::from_utf8_lossy(&body).into_owned()).await;
                StatusCode::OK
            }
        }),
    );
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    (format!("http://{addr}/notify"), rx)
}

#[tokio::test(flavor = "multi_thread")]
async fn notification_data_entities_lead_with_id_then_type() {
    std::env::set_var("ANTARES_EGRESS_ALLOW_PRIVATE", "true");
    let mut st = AppState::new("me".into());
    antares_api::notify::wire(&mut st);
    let (uri, mut rx) = capture_raw().await;
    let (status, body) = post(
        &st,
        "subscriptions",
        json!({"type": "Subscription", "entities": [{"type": "Vehicle"}],
               "notification": {"endpoint": {"uri": uri}}}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (status, body) = post(
        &st,
        "entities",
        json!({"id": "urn:ngsi-ld:Vehicle:koN", "type": "Vehicle",
               "aaa": {"type": "Property", "value": 1}}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let raw = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
        .await
        .expect("notification within 5s")
        .expect("one notification");
    // Notification wrapper: id then type first; entity inside data too
    assert!(raw.starts_with("{\"id\""), "notification id first: {raw}");
    before(&raw, "\"type\":\"Notification\"", "\"data\"");
    before(&raw, "{\"id\":\"urn:ngsi-ld:Vehicle:koN\"", "\"aaa\"");
    before(
        &raw,
        "\"id\":\"urn:ngsi-ld:Vehicle:koN\",\"type\":\"Vehicle\"",
        "\"aaa\"",
    );
}
