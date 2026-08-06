//! N1: the `handle(request) -> response` seam is the whole broker. These run
//! NATIVELY against the same target-independent code the browser loads — the
//! wasm32 build differs only below this seam (executor, fetch, timers), so a
//! round-trip here proves the router wiring the Service Worker (N3) and the
//! Node shim (N7a) will drive.
#![allow(clippy::unwrap_used)]

use antares_wasm::Broker;

fn req(method: &str, path: &str, body: &str) -> http::Request<Vec<u8>> {
    let mut b = http::Request::builder().method(method).uri(path);
    if !body.is_empty() {
        b = b.header("Content-Type", "application/ld+json");
    }
    b.body(body.as_bytes().to_vec()).unwrap()
}

const ENTITY: &str = r#"{
    "id": "urn:ngsi-ld:Vehicle:wasm-1",
    "type": "Vehicle",
    "speed": {"type": "Property", "value": 42},
    "@context": "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld"
}"#;

#[tokio::test(flavor = "current_thread")]
async fn create_then_retrieve_round_trips() {
    let mut broker = Broker::new();

    let resp = broker
        .handle(req("POST", "/ngsi-ld/v1/entities", ENTITY))
        .await;
    assert_eq!(
        resp.status(),
        201,
        "{}",
        String::from_utf8_lossy(resp.body())
    );

    let resp = broker
        .handle(req(
            "GET",
            "/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:wasm-1",
            "",
        ))
        .await;
    assert_eq!(resp.status(), 200);
    let doc: serde_json::Value = serde_json::from_slice(resp.body()).unwrap();
    assert_eq!(doc["id"], "urn:ngsi-ld:Vehicle:wasm-1");
    assert_eq!(doc["speed"]["value"], 42);
}

#[tokio::test(flavor = "current_thread")]
async fn health_reports_the_memory_mode() {
    let mut broker = Broker::new();
    let resp = broker.handle(req("GET", "/q/health", "")).await;
    assert_eq!(resp.status(), 200);
    let doc: serde_json::Value = serde_json::from_slice(resp.body()).unwrap();
    assert_eq!(doc["store"], "memory");
}
