//! 4.9 q= grammar: must never panic, whatever the bytes (§16.2).
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = antares_ql::parse_q(s);
    }
});
