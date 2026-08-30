// SPDX-License-Identifier: EUPL-1.2
//! Extra HTTP surfaces, registered rather than hard-wired.
//!
//! CIM 009 6.2 owns `/ngsi-ld/v1`; everything a deployment serves beside it
//! lives under a reserved prefix, so an added surface can never shadow,
//! extend or contradict a spec resource. `/q` is this broker's operational
//! ground and belongs to the admin surface; `/x` and anything under it is
//! free for a deployment.

use crate::AppState;
use axum::Router;
use serde_json::Value;

/// One mountable group of routes outside the NGSI-LD API root.
pub trait ApiSurface: Send + Sync {
    /// The name `ANTARES_API_SURFACES` selects it by, and the key it
    /// appears under in `/q/health`.
    fn name(&self) -> &str;

    /// Where the routes mount: `/q`, or `/x` and anything below it. The
    /// routes the surface returns are relative to this.
    fn prefix(&self) -> &str;

    /// The routes, relative to `prefix`. The broker supplies the state.
    fn router(&self, st: AppState) -> Router<AppState>;

    /// What `/q/health` reports about it, beside its prefix.
    fn version_info(&self) -> Value;
}

/// The prefixes a surface may mount under. `/q` is the broker's operational
/// ground; `/x` is the deployment's.
const RESERVED: [&str; 2] = ["/q", "/x"];

/// A prefix a surface is allowed to claim: exactly one of the reserved
/// roots, or a path below `/x`. Everything else — the NGSI-LD API root
/// above all — is refused, since a surface that could mount there would
/// make conformance a function of deployment configuration.
pub(crate) fn check_prefix(prefix: &str) -> Result<(), String> {
    if RESERVED.contains(&prefix) || prefix.starts_with("/x/") {
        return Ok(());
    }
    Err(format!(
        "api surface prefix {prefix:?} is not reserved; a surface mounts at /q, at /x, \
         or below /x — never under the NGSI-LD API root"
    ))
}

/// Two prefixes claim the same ground when they are equal or one nests
/// inside the other. Merging both would leave the winner to route-matching
/// order, so it is refused where it can still be a startup error.
pub(crate) fn overlaps(a: &str, b: &str) -> bool {
    a == b || a.starts_with(&format!("{b}/")) || b.starts_with(&format!("{a}/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_reserved_prefixes_are_mountable() {
        for ok in ["/q", "/x", "/x/plugin", "/x/a/b"] {
            assert!(check_prefix(ok).is_ok(), "{ok}");
        }
        for bad in [
            "/ngsi-ld",
            "/ngsi-ld/v1",
            "/ngsi-ld/v1/entities",
            "/",
            "",
            "/entities",
            "/qq",
            "/q/health",
            "x",
            "q",
            "//x",
        ] {
            let err = check_prefix(bad).expect_err(bad);
            assert!(err.contains(bad), "the message names the prefix: {err}");
        }
    }

    #[test]
    fn a_prefix_collides_with_itself_and_with_its_ancestors() {
        assert!(overlaps("/x", "/x"));
        assert!(overlaps("/x", "/x/deeper"));
        assert!(overlaps("/x/deeper", "/x"));
        assert!(!overlaps("/q", "/x"));
        assert!(!overlaps("/x/a", "/x/b"));
        // a shared text prefix is not a shared path segment
        assert!(!overlaps("/x/ab", "/x/abc"));
    }
}
