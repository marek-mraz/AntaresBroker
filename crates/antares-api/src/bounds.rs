// SPDX-License-Identifier: EUPL-1.2
//! Input bounds wall: every request-shaped resource has
//! a configured cap, rejected with the spec-shaped error. One middleware
//! enforces the transport-level caps (URI length 414, body size 413, JSON
//! nesting 400) — size and depth are checked BEFORE any parse. The
//! per-feature caps (batch count, joinLevel, @context fetch
//! count, q= complexity, result ceiling) live at their parse points.
//! Rejections are counted and exported via /q/health.

use axum::body::{Body, Bytes};
use axum::http::{Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use std::sync::atomic::{AtomicU64, Ordering};

/// Hard caps (v1: compile-time constants — a config file is a later knob;
/// every value is spec-shaped on rejection).
/// → bare 413 (6.3.4). Deployment knob (ANTARES_MAX_BODY_BYTES): the spec
/// names no ceiling; 4 MiB is the DoS bound, raised where a trusted
/// producer legitimately sends bigger batches. Read once at first use.
pub static MAX_BODY_BYTES: std::sync::LazyLock<usize> = std::sync::LazyLock::new(|| {
    std::env::var("ANTARES_MAX_BODY_BYTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|n| *n > 0)
        .unwrap_or(4 * 1024 * 1024)
});
pub const MAX_URI_BYTES: usize = 8 * 1024; // → bare 414
pub const MAX_JSON_DEPTH: usize = 64; // → 400 BadRequestData
/// → 400 BadRequestData. Maximum coordinate positions in a QUERY geometry
/// (4.10 geoQ, 4.23 ordering reference). The spec sets no ceiling, and the
/// geometry is not bounded by the URI length on the POST query path — the
/// body carries it. Every position is an edge the DE-9IM relate walks once
/// per candidate entity, so the work a single request can buy is capped
/// here: 1024 positions describe an administrative boundary at street
/// resolution, and are already more than the 8 KiB URI ceiling can carry.
pub use antares_ql::geo::MAX_GEO_VERTICES;
/// → 400 BadRequestData. Deployment knob (ANTARES_MAX_BATCH_ITEMS): the
/// spec sets no batch ceiling — 1000 is this broker's DoS-bounds default,
/// raised where a trusted producer legitimately batches larger (e.g. a
/// full-fleet upsert). Read once at first use.
pub static MAX_BATCH_ITEMS: std::sync::LazyLock<usize> = std::sync::LazyLock::new(|| {
    std::env::var("ANTARES_MAX_BATCH_ITEMS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|n| *n > 0)
        .unwrap_or(1_000)
});
// Deployment knob (ANTARES_MAX_FED_RESPONSE_BYTES): ceiling on one forwarded
// (4.3.6) response body. The spec sets no ceiling; an over-cap peer part
// fails like an unparseable payload (Table 6.3.17-1, warning 111) instead of
// ballooning broker memory — one misbehaving peer must not break the
// 500 MB RSS budget. Read once at first use.
pub static MAX_FED_RESPONSE_BYTES: std::sync::LazyLock<usize> = std::sync::LazyLock::new(|| {
    std::env::var("ANTARES_MAX_FED_RESPONSE_BYTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|n| *n > 0)
        .unwrap_or(16 * 1024 * 1024)
});
// Deployment knob (ANTARES_FED_INFLIGHT): forwarded requests in flight for
// the whole process. Per-request fan-out is bounded below; across requests
// nothing was, and 6 000 open federated queries × 34 sources each held
// 7.7 GB of buffers and connections. Callers over the cap wait their turn.
pub static MAX_FED_INFLIGHT: std::sync::LazyLock<usize> = std::sync::LazyLock::new(|| {
    std::env::var("ANTARES_FED_INFLIGHT")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|n| *n > 0)
        .unwrap_or(256)
});
pub static FED_INFLIGHT: std::sync::LazyLock<tokio::sync::Semaphore> =
    std::sync::LazyLock::new(|| tokio::sync::Semaphore::new(*MAX_FED_INFLIGHT));
// Deployment knob (ANTARES_FED_FANOUT): concurrent forwards per distributed
// read. 4.3.6.1 orders the MERGE (4.5.5), never the requests, so forwards
// run concurrently; this bounds how many at once per request.
pub static MAX_FED_FANOUT: std::sync::LazyLock<usize> = std::sync::LazyLock::new(|| {
    std::env::var("ANTARES_FED_FANOUT")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|n| *n > 0)
        .unwrap_or(8)
});
pub const MAX_JOIN_LEVEL: usize = 10; // → 400 BadRequestData
/// → 400 BadRequestData. Documents one @context resolution may fetch, owned
/// by the loader that enforces it (`antares_jsonld`), and the ceiling on how
/// many DISTINCT @contexts one batch may name — without the second, the item
/// count multiplies the first.
pub use antares_jsonld::MAX_CONTEXT_URLS as MAX_CONTEXT_FETCHES;
pub const MAX_Q_NODES: usize = 512; // → 403 TooComplexQuery
/// Linked-entity lookup budget per `q=`, owned by the shared evaluator.
pub use antares_ql::eval::MAX_Q_LINK_LOOKUPS;
/// Regex compile ceiling and retention caps, owned by the shared cache
/// (`antares_ql::regex`).
pub use antares_ql::regex::{MAX_REGEX_CACHE, MAX_REGEX_CACHE_BYTES, MAX_REGEX_PROGRAM_BYTES};

/// Rejection counters, exported by /q/health.
#[derive(Default)]
pub struct LimitStats {
    pub uri_too_long: AtomicU64,
    pub body_too_large: AtomicU64,
    pub body_too_deep: AtomicU64,
}

impl LimitStats {
    pub fn snapshot(&self) -> serde_json::Value {
        serde_json::json!({
            "maxBodyBytes": *MAX_BODY_BYTES,
            "maxUriBytes": MAX_URI_BYTES,
            "maxJsonDepth": MAX_JSON_DEPTH,
            "maxGeoVertices": MAX_GEO_VERTICES,
            "maxBatchItems": *MAX_BATCH_ITEMS,
            "maxFedResponseBytes": *MAX_FED_RESPONSE_BYTES,
            "maxFedFanout": *MAX_FED_FANOUT,
            "maxFedInflight": *MAX_FED_INFLIGHT,
            "maxJoinLevel": MAX_JOIN_LEVEL,
            "maxContextFetches": MAX_CONTEXT_FETCHES,
            "maxQNodes": MAX_Q_NODES,
            "maxQLinkLookups": MAX_Q_LINK_LOOKUPS,
            "maxRegexCache": MAX_REGEX_CACHE,
            "maxRegexCacheBytes": MAX_REGEX_CACHE_BYTES,
            "maxRegexProgramBytes": MAX_REGEX_PROGRAM_BYTES,
            "rejectedUriTooLong": self.uri_too_long.load(Ordering::Relaxed),
            "rejectedBodyTooLarge": self.body_too_large.load(Ordering::Relaxed),
            "rejectedBodyTooDeep": self.body_too_deep.load(Ordering::Relaxed),
        })
    }
}

/// Maximum brace/bracket nesting of a JSON byte stream, string-aware.
/// A scan, not a parse — depth is checked before serde ever runs.
pub fn json_depth(bytes: &[u8]) -> usize {
    let (mut depth, mut max, mut in_str, mut esc) = (0usize, 0usize, false, false);
    for &b in bytes {
        if in_str {
            if esc {
                esc = false;
            } else if b == b'\\' {
                esc = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' | b'[' => {
                depth += 1;
                max = max.max(depth);
            }
            b'}' | b']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    max
}

pub async fn bounds_layer(
    axum::extract::State(st): axum::extract::State<crate::AppState>,
    req: Request<Body>,
    next: Next,
) -> Response {
    if req.uri().to_string().len() > MAX_URI_BYTES {
        st.limits.uri_too_long.fetch_add(1, Ordering::Relaxed);
        return StatusCode::URI_TOO_LONG.into_response(); // bare, like 6.3.4
    }
    // 6.3.4: "For HTTP POST, PATCH and PUT HTTP requests implementations shall
    // check … Content-Length header shall include the length of the request
    // payload body", and its absence "shall result in just a 411 HTTP status
    // code (without any payload body)" — restated in 6.3.2. Scoped to HTTP/1.x:
    // HTTP/2 and later carry length in the framing layer and legitimately omit
    // the header, so demanding it there would reject conformant clients.
    // The clause grants NO exemption for `Transfer-Encoding: chunked` — a
    // chunked POST without Content-Length is exactly the case 411 covers, so it
    // is deliberately not carved out here.
    //
    // The ONE deviation, made explicit rather than implied: the check is scoped
    // to HTTP/1.x. 6.3.4 is written against RFC 7230/7231 and predates any h2
    // consideration; HTTP/2 carries length in its framing and conformant h2
    // clients routinely omit the header, so applying it there would reject
    // requests the spec never meant to describe. Recorded in docs/ics.yaml.
    if matches!(req.method().as_str(), "POST" | "PATCH" | "PUT")
        && req.version() <= axum::http::Version::HTTP_11
        && !req
            .headers()
            .contains_key(axum::http::header::CONTENT_LENGTH)
    {
        return StatusCode::LENGTH_REQUIRED.into_response(); // bare 411
    }
    let has_body = matches!(req.method().as_str(), "POST" | "PATCH" | "PUT" | "DELETE");
    if !has_body {
        return next.run(req).await;
    }
    let (parts, body) = req.into_parts();
    let bytes: Bytes = match axum::body::to_bytes(body, *MAX_BODY_BYTES).await {
        Ok(b) => b,
        Err(_) => {
            st.limits.body_too_large.fetch_add(1, Ordering::Relaxed);
            return StatusCode::PAYLOAD_TOO_LARGE.into_response(); // bare 413
        }
    };
    // An absent (or unreadable) Content-Type is parsed as JSON downstream —
    // 6.3.4 mandates Content-Length, not Content-Type — so it is scanned
    // here too, or the nesting cap has a hole exactly where the parser has
    // none. A header that names another media type keeps its 415.
    let is_json = parts
        .headers
        .get(axum::http::header::CONTENT_TYPE)
        .is_none_or(|v| match v.to_str() {
            Ok(ct) => ct.contains("json"),
            // A header the parser cannot read is not a header naming another
            // media type: `negotiate::content_type` reports it as the empty
            // string, which every route that tolerates an absent
            // Content-Type reads as absent and parses. Scanning it is what
            // keeps the cap ahead of the parser on those routes.
            Err(_) => true,
        });
    if is_json && json_depth(&bytes) > MAX_JSON_DEPTH {
        st.limits.body_too_deep.fetch_add(1, Ordering::Relaxed);
        return crate::negotiate::ApiError::from(antares_model::NgsiError::BadRequestData(
            format!("JSON nesting exceeds the {MAX_JSON_DEPTH}-level limit"),
        ))
        .into_response();
    }
    next.run(Request::from_parts(parts, Body::from(bytes)))
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The scan is a bound, not a parser: unbalanced closers must not
    /// underflow, and the value it reports is the one the middleware compares
    /// against MAX_JSON_DEPTH, so the accept/reject boundary is exact.
    #[test]
    fn depth_scan_survives_unbalanced_and_boundary_input() {
        assert_eq!(json_depth(b"]]]]"), 0, "stray closers must not underflow");
        assert_eq!(json_depth(b"}}}{"), 1);
        assert_eq!(json_depth(b""), 0);
        assert_eq!(
            json_depth(br#""{{{{""#),
            0,
            "a bare string carries no depth"
        );
        assert_eq!(
            json_depth(br#"{"a": "\\"}"#),
            1,
            "an escaped backslash ends the escape"
        );
        let at_cap = "[".repeat(MAX_JSON_DEPTH) + &"]".repeat(MAX_JSON_DEPTH);
        assert_eq!(json_depth(at_cap.as_bytes()), MAX_JSON_DEPTH);
        assert!(
            json_depth(at_cap.as_bytes()) <= MAX_JSON_DEPTH,
            "exactly at the cap is accepted"
        );
        let over = "[".repeat(MAX_JSON_DEPTH + 1) + &"]".repeat(MAX_JSON_DEPTH + 1);
        assert!(
            json_depth(over.as_bytes()) > MAX_JSON_DEPTH,
            "one over is rejected"
        );
    }

    /// /q/health publishes the caps and the rejection counters — and nothing
    /// else: no configuration paths, no environment variable values, no
    /// internal error text.
    #[test]
    fn health_snapshot_reports_the_caps_and_nothing_internal() {
        let stats = LimitStats::default();
        stats.uri_too_long.fetch_add(3, Ordering::Relaxed);
        let snap = stats.snapshot();
        let obj = snap.as_object().expect("object");
        let mut keys: Vec<&str> = obj.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "maxBatchItems",
                "maxBodyBytes",
                "maxContextFetches",
                "maxFedFanout",
                "maxFedInflight",
                "maxFedResponseBytes",
                "maxGeoVertices",
                "maxJoinLevel",
                "maxJsonDepth",
                "maxQLinkLookups",
                "maxQNodes",
                "maxRegexCache",
                "maxRegexCacheBytes",
                "maxRegexProgramBytes",
                "maxUriBytes",
                "rejectedBodyTooDeep",
                "rejectedBodyTooLarge",
                "rejectedUriTooLong",
            ],
            "no member beyond the caps and the counters"
        );
        assert_eq!(snap["rejectedUriTooLong"], 3);
        assert!(
            obj.values().all(|v| v.is_number()),
            "every member is a number — no strings to leak paths through"
        );
    }

    /// The nesting cap is checked BEFORE any parse — including on the path
    /// that carries no Content-Type header at all, which the body parser
    /// accepts and parses as JSON (6.3.4 only mandates Content-Length).
    /// An unparseable Content-Type follows the same rule.
    #[tokio::test]
    async fn over_depth_body_without_content_type_is_still_rejected() {
        use tower::ServiceExt;
        let st = crate::AppState::new("http://localhost:0".into());
        let app = axum::Router::new()
            .route(
                "/x",
                axum::routing::post(|| async { StatusCode::NO_CONTENT }),
            )
            .layer(axum::middleware::from_fn_with_state(st, bounds_layer));
        let deep = "[".repeat(MAX_JSON_DEPTH + 5) + &"]".repeat(MAX_JSON_DEPTH + 5);
        // The third case is the one a header map can hold and `to_str`
        // cannot read: a Content-Type carrying a byte outside UTF-8. Every
        // route reads it as an absent Content-Type, so the scan must too.
        let unreadable = axum::http::HeaderValue::from_bytes(b"application/\xffjson")
            .expect("header value from raw bytes");
        assert!(
            unreadable.to_str().is_err(),
            "the case under test is a header value that cannot be read as text"
        );
        for ct in [
            None,
            Some(axum::http::HeaderValue::from_static("application/json")),
            Some(unreadable),
        ] {
            let mut req = Request::post("/x")
                .header(axum::http::header::CONTENT_LENGTH, deep.len().to_string());
            if let Some(ct) = ct.clone() {
                req = req.header(axum::http::header::CONTENT_TYPE, ct);
            }
            let resp = app
                .clone()
                .oneshot(req.body(Body::from(deep.clone())).expect("req"))
                .await
                .expect("resp");
            assert_eq!(
                resp.status(),
                StatusCode::BAD_REQUEST,
                "an over-deep body must not reach the handler (content-type {ct:?})"
            );
        }
    }

    #[test]
    fn depth_scan_is_string_aware() {
        assert_eq!(json_depth(br#"{"a": [1, {"b": 2}]}"#), 3);
        assert_eq!(
            json_depth(br#"{"a": "}]}]}]{[{["}"#),
            1,
            "braces in strings don't count"
        );
        assert_eq!(
            json_depth(br#"{"a": "\"}"}"#),
            1,
            "escaped quotes stay in-string"
        );
        let deep = "[".repeat(100) + &"]".repeat(100);
        assert_eq!(json_depth(deep.as_bytes()), 100);
    }
}
