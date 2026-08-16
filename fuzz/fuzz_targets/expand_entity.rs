//! JSON-LD expansion input path: arbitrary JSON documents through
//! expand_entity with the core context — errors fine, panics never.
#![no_main]
use libfuzzer_sys::fuzz_target;
use std::sync::OnceLock;

fuzz_target!(|data: &[u8]| {
    static LOADER: OnceLock<antares_jsonld::Loader> = OnceLock::new();
    let loader = LOADER.get_or_init(antares_jsonld::Loader::new);
    if let Ok(serde_json::Value::Object(obj)) = serde_json::from_slice(data) {
        let _ = antares_jsonld::expand_entity(
            &obj,
            &loader.core(),
            antares_jsonld::ExpandOpts::default(),
        );
    }
});
