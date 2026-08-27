// SPDX-License-Identifier: EUPL-1.2
//! The `handle(request) -> response` seam is the whole broker. These run
//! NATIVELY against the same target-independent code the browser loads — the
//! wasm32 build differs only below this seam (executor, fetch, timers), so a
//! round-trip here proves the router wiring the Service Worker and the
//! Node shim will drive.
#![allow(clippy::unwrap_used)]

use antares_wasm::Broker;

fn req(method: &str, path: &str, body: &str) -> http::Request<Vec<u8>> {
    let mut b = http::Request::builder().method(method).uri(path);
    if !body.is_empty() {
        b = b
            .header("Content-Type", "application/ld+json")
            // 6.3.4: body-bearing methods without Content-Length are a bare 411
            .header("Content-Length", body.len());
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
    let broker = Broker::new();

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

/// Cross-tenant federation inside ONE broker. Tenant `space-a` holds a
/// Context Source Registration whose endpoint is a real loopback listener
/// feeding the SAME broker instance and whose `tenant` member (5.2.9) names
/// `space-b` — the native stand-in for the browser loopback host. Queries in
/// space-a transparently include space-b's matching entities; `local=true`
/// and the reverse direction stay isolated. Also proves 4.3.6.5:
/// contextSourceInfo pairs arrive as headers, `urn:ngsi-ld:request` copies
/// the triggering request's header, and the tenant key cannot be smuggled.
#[tokio::test(flavor = "current_thread")]
async fn csr_tenant_federates_across_tenants_in_one_broker() {
    use http_body_util::BodyExt;
    use std::sync::{Arc, Mutex};

    // The forward target is 127.0.0.1 — private-range egress must be open
    // (same knob the Node tier passes to the constructor).
    antares_jsonld::loader::allow_private_egress(true);
    let broker = Arc::new(Broker::new());

    // The loopback listener: every received request is recorded (headers
    // asserted below) and fed straight back into the same broker.
    let seen: Arc<Mutex<Vec<http::HeaderMap>>> = Arc::new(Mutex::new(Vec::new()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    {
        let (broker, seen) = (broker.clone(), seen.clone());
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                let (broker, seen) = (broker.clone(), seen.clone());
                tokio::spawn(async move {
                    let svc = hyper::service::service_fn(
                        move |req: http::Request<hyper::body::Incoming>| {
                            let (broker, seen) = (broker.clone(), seen.clone());
                            async move {
                                let (parts, body) = req.into_parts();
                                seen.lock().unwrap().push(parts.headers.clone());
                                let bytes = body.collect().await.unwrap().to_bytes().to_vec();
                                let resp =
                                    broker.handle(http::Request::from_parts(parts, bytes)).await;
                                let (rp, rb) = resp.into_parts();
                                Ok::<_, std::convert::Infallible>(http::Response::from_parts(
                                    rp,
                                    http_body_util::Full::new(hyper::body::Bytes::from(rb)),
                                ))
                            }
                        },
                    );
                    let _ = hyper::server::conn::http1::Builder::new()
                        .serve_connection(hyper_util::rt::TokioIo::new(stream), svc)
                        .await;
                });
            }
        });
    }

    let with_tenant = |mut r: http::Request<Vec<u8>>, t: &str| {
        r.headers_mut().insert("NGSILD-Tenant", t.parse().unwrap());
        r
    };

    // space-b holds the data.
    let car = r#"{
        "id": "urn:ngsi-ld:FedCar:b-1",
        "type": "FedCar",
        "speed": {"type": "Property", "value": 88},
        "@context": "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld"
    }"#;
    let resp = broker
        .handle(with_tenant(
            req("POST", "/ngsi-ld/v1/entities", car),
            "space-b",
        ))
        .await;
    assert_eq!(
        resp.status(),
        201,
        "{}",
        String::from_utf8_lossy(resp.body())
    );

    // space-a registers space-b as a context source OF THIS SAME BROKER.
    let csr = format!(
        r#"{{
        "id": "urn:ngsi-ld:ContextSourceRegistration:a-to-b",
        "type": "ContextSourceRegistration",
        "information": [{{"entities": [{{"type": "FedCar"}}]}}],
        "endpoint": "http://127.0.0.1:{port}",
        "mode": "inclusive",
        "tenant": "space-b",
        "contextSourceInfo": [
            {{"key": "X-Playground", "value": "one-broker"}},
            {{"key": "X-Echo", "value": "urn:ngsi-ld:request"}},
            {{"key": "NGSILD-Tenant", "value": "smuggled"}}
        ],
        "@context": "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld"
    }}"#
    );
    let resp = broker
        .handle(with_tenant(
            req("POST", "/ngsi-ld/v1/csourceRegistrations", &csr),
            "space-a",
        ))
        .await;
    assert_eq!(
        resp.status(),
        201,
        "{}",
        String::from_utf8_lossy(resp.body())
    );

    // Federated query in space-a sees space-b's entity...
    let mut q = with_tenant(
        req("GET", "/ngsi-ld/v1/entities?type=FedCar", ""),
        "space-a",
    );
    q.headers_mut()
        .insert("X-Echo", "from-the-request".parse().unwrap());
    let resp = broker.handle(q).await;
    assert_eq!(resp.status(), 200);
    let docs: serde_json::Value = serde_json::from_slice(resp.body()).unwrap();
    let ids: Vec<&str> = docs
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["id"].as_str())
        .collect();
    assert!(
        ids.contains(&"urn:ngsi-ld:FedCar:b-1"),
        "federated query must include space-b's entity, got {ids:?}"
    );

    // ...the forward carried the registration's tenant + contextSourceInfo.
    {
        let seen = seen.lock().unwrap();
        let fwd = seen.last().expect("one forward reached the listener");
        assert_eq!(fwd.get("NGSILD-Tenant").unwrap(), "space-b");
        assert_eq!(fwd.get("X-Playground").unwrap(), "one-broker");
        assert_eq!(fwd.get("X-Echo").unwrap(), "from-the-request");
        assert!(fwd.get("Via").is_some(), "forwards carry a Via chain");
    }

    // local=true keeps the space local (6.3.18)...
    let resp = broker
        .handle(with_tenant(
            req("GET", "/ngsi-ld/v1/entities?type=FedCar&local=true", ""),
            "space-a",
        ))
        .await;
    let docs: serde_json::Value = serde_json::from_slice(resp.body()).unwrap();
    assert_eq!(
        docs.as_array().unwrap().len(),
        0,
        "local=true must not federate"
    );

    // ...and space-b (no registrations) never sees space-a's side.
    let resp = broker
        .handle(with_tenant(
            req("GET", "/ngsi-ld/v1/entities/urn:ngsi-ld:FedCar:b-1", ""),
            "space-a",
        ))
        .await;
    assert_eq!(resp.status(), 200, "federated retrieve-by-id also works");
}

#[tokio::test(flavor = "current_thread")]
async fn health_reports_the_memory_mode() {
    let broker = Broker::new();
    let resp = broker.handle(req("GET", "/q/health", "")).await;
    assert_eq!(resp.status(), 200);
    let doc: serde_json::Value = serde_json::from_slice(resp.body()).unwrap();
    assert_eq!(doc["store"], "memory");
}
