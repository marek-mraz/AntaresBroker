//! 4.10 geoquery params: never panic on hostile georel/geometry/coordinates.
#![no_main]
use libfuzzer_sys::fuzz_target;
use std::collections::HashMap;

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else { return };
    let mut parts = s.splitn(3, '\n');
    let mut params: HashMap<String, String> = HashMap::new();
    if let Some(v) = parts.next() {
        params.insert("georel".into(), v.into());
    }
    if let Some(v) = parts.next() {
        params.insert("geometry".into(), v.into());
    }
    if let Some(v) = parts.next() {
        params.insert("coordinates".into(), v.into());
    }
    let _ = antares_api::geo::GeoQuery::from_params(&params);
});
