//! Request screening at the edge with the broker's own validation: a
//! gateway resolves the request's @context through a loader over ITS OWN
//! HTTP client, then expands the payload exactly as the broker will — a
//! payload the broker would refuse with 400 BadRequestData is refused here,
//! with the same problem details, before it costs a hop.
//!
//!     cargo run -p antares-jsonld --example gateway_expand

use antares_jsonld::loader::{EgressPolicy, Loader};
use antares_jsonld::{expand_entity, ExpandOpts};
use serde_json::{json, Value};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    // the gateway's client: its own timeouts, proxy, trust anchors…
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .expect("client");
    let loader = Loader::with_client(EgressPolicy::from_env(), client);

    // an incoming create request carrying only the core @context
    let body = json!({
        "id": "urn:ngsi-ld:Vehicle:A4567",
        "type": "Vehicle",
        "speed": {"type": "Property", "value": 80, "unitCode": "KMH"},
        "@context": ["https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld"]
    });
    screen(&loader, body).await;

    // the same request with a broken attribute: a Relationship needs `object`
    let bad = json!({
        "id": "urn:ngsi-ld:Vehicle:A4567",
        "type": "Vehicle",
        "isParked": {"type": "Relationship", "value": "urn:ngsi-ld:OffStreetParking:Downtown1"}
    });
    screen(&loader, bad).await;
}

async fn screen(loader: &Loader, mut body: Value) {
    let user_ctx = body
        .as_object_mut()
        .and_then(|o| o.remove("@context"))
        .unwrap_or(Value::Null);
    let ctx = match loader.resolve(&user_ctx).await {
        Ok(c) => c,
        Err(e) => {
            println!("refused: {} {}", e.status(), e.to_problem_details().detail);
            return;
        }
    };
    let doc = body.as_object().expect("an entity is an object");
    match expand_entity(doc, &ctx, ExpandOpts::default()) {
        Ok(expanded) => println!(
            "forward: {} attribute(s) expanded, e.g. {}",
            expanded.as_object().map_or(0, |o| o.len()),
            expanded
                .as_object()
                .and_then(|o| o.keys().find(|k| k.starts_with("http")))
                .cloned()
                .unwrap_or_default()
        ),
        Err(e) => println!(
            "refused: {} {} — {}",
            e.status(),
            e.kind(),
            e.to_problem_details().detail
        ),
    }
}
