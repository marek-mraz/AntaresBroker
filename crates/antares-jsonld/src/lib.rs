//! JSON-LD layer (docs/deep-analysis.md §6.3).
//!
//! Phase-0 seed. The full pipeline (json-ld crate wrapper, moka parsed-context
//! LRU, core-context term table) is the first spike — this crate currently
//! pins the core-context contract that everything else compiles against.

use antares_model::CORE_CONTEXT_URL;

/// Returns true when a request's @context is exactly the core context — the
/// fast-path detector (§6.3): such requests skip the generic processor.
pub fn is_core_context(urls: &[&str]) -> bool {
    matches!(urls, [only] if *only == CORE_CONTEXT_URL)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_core_context() {
        assert!(is_core_context(&[CORE_CONTEXT_URL]));
        assert!(!is_core_context(&[]));
        assert!(!is_core_context(&[
            CORE_CONTEXT_URL,
            "https://example.org/ctx.jsonld"
        ]));
    }
}
