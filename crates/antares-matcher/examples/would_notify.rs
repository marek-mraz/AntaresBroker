//! Answer "would this entity change notify subscription X" without a broker:
//! the same predicates the broker's notification path applies (5.8.6), over
//! a stored subscription document and an entity in its internal form.
//!
//!     cargo run -p antares-matcher --example would_notify

use antares_matcher::would_notify;
use serde_json::json;

fn main() {
    let ctx = antares_jsonld::Loader::new().core();
    let sub = json!({
        "id": "urn:ngsi-ld:Subscription:speeding",
        "type": "Subscription",
        // stored form: selector types are expanded IRIs
        "entities": [{"type": "https://uri.etsi.org/ngsi-ld/default-context/Vehicle"}],
        "q": "speed>25",
        "geoQ": {"georel": "near;maxDistance==2000", "geometry": "Point",
                 "coordinates": "[-3.7,40.4]"},
        "notification": {"endpoint": {"uri": "http://gateway/notify"}}
    });
    for (speed, lon) in [(30.0, -3.7), (20.0, -3.7), (30.0, -4.7)] {
        // the entity as the broker stores it: its own expansion of the payload
        let payload = json!({
            "id": "urn:ngsi-ld:Vehicle:A1",
            "type": "Vehicle",
            "speed": {"type": "Property", "value": speed},
            "location": {"type": "GeoProperty",
                "value": {"type": "Point", "coordinates": [lon, 40.4]}}
        });
        let entity = antares_jsonld::expand_entity(
            payload.as_object().expect("object"),
            &ctx,
            antares_jsonld::ExpandOpts::default(),
        )
        .expect("a valid entity");
        // no store at hand: 4.9 linked-entity terms would resolve to nothing
        let hit = would_notify(&sub, &entity, &ctx, &|_| None);
        println!(
            "speed={speed} lon={lon} → {}",
            if hit { "notify" } else { "skip" }
        );
    }
}
