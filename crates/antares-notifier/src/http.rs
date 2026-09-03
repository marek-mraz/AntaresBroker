// SPDX-License-Identifier: EUPL-1.2
//! HTTP notification binding — CIM 009 clause 6.3.8.
//!
//! A notification is an HTTP POST to `notification.endpoint.uri`. The MIME
//! type is `endpoint.accept`, defaulting to `"application/json"`; for
//! `"application/json"` (and, as this broker serves it, for
//! `"application/geo+json"`) the request carries a Link header naming the
//! JSON-LD `@context`. Each `endpoint.receiverInfo` pair becomes one custom
//! header.

use crate::{DeliveryError, DeliveryFuture, NotificationSink, Outbound};
use antares_model::NgsiError;
use std::time::Duration;

/// The HTTP(S) binding over one shared outbound client. The client carries
/// the deployment's egress policy (resolver pinning, connect timeouts); the
/// per-destination policy check and breaker stay in the caller.
pub struct HttpSink {
    client: antares_jsonld::HttpClient,
}

impl HttpSink {
    /// Bind to an already-configured outbound client.
    pub fn new(client: antares_jsonld::HttpClient) -> Self {
        Self { client }
    }
}

impl NotificationSink for HttpSink {
    fn schemes(&self) -> &'static [&'static str] {
        &["http", "https"]
    }

    /// 5.2.15: the endpoint URI has to be dereferenceable — for this binding,
    /// an absolute http(s) URL with an authority.
    fn parse_endpoint(&self, uri: &str, _notifier_info: &[(&str, &str)]) -> Result<(), NgsiError> {
        let safe = crate::redact_userinfo(uri);
        // IETF RFC 3986 3.1: scheme names are case-insensitive, and
        // `SinkRegistry::scheme_of` already lowercases to pick this sink.
        let rest = uri
            .split_once("://")
            .filter(|(s, _)| s.eq_ignore_ascii_case("http") || s.eq_ignore_ascii_case("https"))
            .map(|(_, rest)| rest)
            .ok_or_else(|| {
                NgsiError::BadRequestData(format!("not an http(s) endpoint URI: {safe:?}"))
            })?;
        let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let host = rest[..authority_end]
            .rsplit_once('@')
            .map_or(&rest[..authority_end], |(_, h)| h);
        if host.is_empty() {
            return Err(NgsiError::BadRequestData(format!(
                "http endpoint {safe:?} has no host"
            )));
        }
        Ok(())
    }

    fn deliver<'a>(
        &'a self,
        uri: &'a str,
        out: &'a Outbound,
        timeout: Duration,
    ) -> DeliveryFuture<'a> {
        Box::pin(async move {
            let bytes = antares_model::ordered_vec(&out.body);
            // Wasm: the page sink takes matching endpoints — a page cannot
            // listen on a socket, so this IS its delivery channel.
            #[cfg(target_arch = "wasm32")]
            if page_sink::try_deliver(uri, &bytes) {
                return Ok(());
            }
            let mut req = self.client.post(uri);
            for (k, v) in headers(out) {
                req = req.header(k, v);
            }
            // endpoint.timeout rides on the request natively (the client's
            // own total alone would let a stalled endpoint eat the full cap
            // per delivery); stretched under the sanitizer like the client's
            // other deadlines.
            #[cfg(not(target_arch = "wasm32"))]
            let req =
                req.timeout(timeout.saturating_mul(
                    u32::try_from(antares_jsonld::slow_factor()).unwrap_or(u32::MAX),
                ));
            let deadline_ms = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
            // One Send unit so the admin replay handler stays Send on wasm32.
            antares_jsonld::http_interaction(async move {
                match antares_jsonld::io_deadline(req.body(bytes).send(), deadline_ms).await {
                    Some(Ok(r)) if r.status().is_success() => Ok(()),
                    Some(Ok(r)) => Err(DeliveryError::failed(format!(
                        "HTTP {}",
                        r.status().as_u16()
                    ))),
                    Some(Err(e)) => Err(DeliveryError {
                        timed_out: e.is_timeout(),
                        message: crate::redact_userinfo(&e.to_string()),
                    }),
                    None => Err(DeliveryError::timeout("timeout")),
                }
            })
            .await
        })
    }
}

/// 6.3.8: the headers one notification POST carries. `application/ld+json`
/// holds its `@context` in the payload body and takes no Link header; the
/// other two MIME types carry it in the header. Every `receiverInfo` pair
/// (and the tenant/snapshot markers the caller appended to it) becomes one
/// custom header.
fn headers(out: &Outbound) -> Vec<(String, String)> {
    let mut h = Vec::with_capacity(out.receiver_info.len() + 2);
    h.push(("Content-Type".to_owned(), out.accept.clone()));
    if out.accept != "application/ld+json" {
        h.push(("Link".to_owned(), out.link.clone()));
    }
    // The client APPENDS every header it is handed, so a receiverInfo pair
    // keyed like one of the two above would travel beside it rather than
    // replace it, and this clause names the source of both: the MIME type
    // comes from endpoint.accept, the Link from the served @context. The
    // colliding pair is dropped, not doubled.
    let own = h.len();
    for (k, v) in &out.receiver_info {
        if h[..own].iter().any(|(n, _)| n.eq_ignore_ascii_case(k)) {
            continue;
        }
        h.push((k.clone(), v.clone()));
    }
    h
}

/// The browser build has no inbound socket to receive notification callbacks
/// on, so a subscription whose endpoint matches the registered URL prefix is
/// delivered to page JS instead of the network. Endpoints outside the prefix
/// still leave via fetch — the Node tier registers nothing and keeps pure
/// HTTP delivery.
#[cfg(target_arch = "wasm32")]
pub mod page_sink {
    use std::sync::OnceLock;

    type Sink = Box<dyn Fn(&str, &[u8]) -> bool + Send + Sync>;
    type Hook = (String, Sink);
    static HOOK: OnceLock<Hook> = OnceLock::new();

    /// Register the sink (once per module instance).
    pub fn set(prefix: String, h: Sink) {
        let _ = HOOK.set((prefix, h));
    }

    /// True when the page sink claimed (and thus delivered) this endpoint.
    pub fn try_deliver(url: &str, body: &[u8]) -> bool {
        match HOOK.get() {
            Some((prefix, h)) if url.starts_with(prefix.as_str()) => h(url, body),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn out(accept: &str, receiver_info: &[(&str, &str)]) -> Outbound {
        Outbound {
            body: json!({"type": "Notification"}),
            accept: accept.to_owned(),
            link: "<https://ctx>; rel=\"http://www.w3.org/ns/json-ld#context\"".to_owned(),
            receiver_info: receiver_info
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            notifier_info: Vec::new(),
        }
    }

    fn sink() -> HttpSink {
        HttpSink::new(
            antares_jsonld::client_builder(antares_jsonld::EgressPolicy {
                allow_private: true,
            })
            .build()
            .expect("client"),
        )
    }

    /// 6.3.8: json and geo+json carry the @context in a Link header,
    /// ld+json carries it in the body and takes none.
    #[test]
    fn link_header_follows_the_target_mime_type() {
        let names = |o: &Outbound| {
            headers(o)
                .into_iter()
                .map(|(k, _)| k)
                .collect::<Vec<String>>()
        };
        assert_eq!(
            names(&out("application/json", &[])),
            ["Content-Type", "Link"]
        );
        assert_eq!(
            names(&out("application/geo+json", &[])),
            ["Content-Type", "Link"]
        );
        assert_eq!(names(&out("application/ld+json", &[])), ["Content-Type"]);
    }

    /// Every receiverInfo pair becomes one custom header, in order, after
    /// the binding's own.
    #[test]
    fn receiver_info_becomes_custom_headers() {
        let h = headers(&out(
            "application/json",
            &[("Authorization", "Bearer t"), ("NGSILD-Tenant", "acme")],
        ));
        assert_eq!(h[0].0, "Content-Type");
        assert_eq!(h[0].1, "application/json");
        assert_eq!(h[2], ("Authorization".to_owned(), "Bearer t".to_owned()));
        assert_eq!(h[3], ("NGSILD-Tenant".to_owned(), "acme".to_owned()));
    }

    /// 6.3.8 fixes the MIME type of the POST to `endpoint.accept` and, for
    /// the other two types, mandates the Link header carrying the @context
    /// reference. The client appends each header it is handed, so a
    /// receiverInfo pair keyed like either one travels BESIDE the
    /// binding's: two Content-Type fields make the request malformed (IETF
    /// RFC 9110 clause 5.5.1 makes it a singleton field) and a second Link
    /// leaves the receiver two @context references to choose between.
    #[test]
    fn receiver_info_cannot_double_the_headers_the_binding_owns() {
        for accept in ["application/json", "application/geo+json"] {
            let h = headers(&out(
                accept,
                &[
                    ("content-type", "text/plain"),
                    ("LINK", "<https://evil>"),
                    ("X-Kept", "yes"),
                ],
            ));
            let named = |n: &str| {
                h.iter()
                    .filter(|(k, _)| k.eq_ignore_ascii_case(n))
                    .collect::<Vec<_>>()
            };
            assert_eq!(named("Content-Type").len(), 1, "{accept}");
            assert_eq!(named("Content-Type")[0].1, accept);
            assert_eq!(named("Link").len(), 1, "{accept}");
            assert!(named("Link")[0].1.contains("json-ld#context"), "{accept}");
            assert_eq!(named("X-Kept").len(), 1, "an ordinary pair still travels");
        }
        // ld+json sets no Link of its own, so a pair keyed Link is ordinary
        let h = headers(&out("application/ld+json", &[("Link", "<https://x>")]));
        assert_eq!(
            h.iter()
                .filter(|(k, _)| k.eq_ignore_ascii_case("Link"))
                .count(),
            1
        );
    }

    /// 5.2.15 dereferenceable URI: this binding needs an absolute http(s)
    /// URL with a host.
    #[test]
    fn endpoint_validation_needs_an_absolute_url_with_a_host() {
        let s = sink();
        assert!(s.parse_endpoint("http://example.org/notify", &[]).is_ok());
        assert!(s.parse_endpoint("https://example.org", &[]).is_ok());
        assert!(s.parse_endpoint("https://u:p@example.org/n", &[]).is_ok());
        for bad in [
            "example.org/notify",
            "http:/notify",
            "ftp://example.org/n",
            "http:///notify",
            "https://@/n",
        ] {
            let err = s.parse_endpoint(bad, &[]).expect_err(bad);
            assert_eq!(err.status(), 400, "{bad}");
        }
    }

    /// IETF RFC 3986 3.1: "schemes are case-insensitive ... an implementation
    /// should accept uppercase letters as equivalent to lowercase in scheme
    /// names". `SinkRegistry::scheme_of` lowercases before it picks a sink, so
    /// `HTTP://…` is routed HERE — and this is the layer that decides, so a
    /// case-sensitive check turns a legal endpoint URI into a 400 the
    /// registry's own contract says it should not be.
    #[test]
    fn the_scheme_is_matched_case_insensitively() {
        let s = sink();
        for uri in [
            "HTTP://example.org/notify",
            "HTTPS://example.org/n",
            "HttP://example.org/n",
        ] {
            assert!(s.parse_endpoint(uri, &[]).is_ok(), "{uri}");
        }
    }

    /// The HTTP binding hands a DRIVER ERROR to `redact_userinfo`, not a bare
    /// URI: reqwest prints the failing URL and `url::Url`'s Display serializes
    /// the password with it. Every message shape the driver produces has to
    /// lose the credential.
    #[test]
    fn a_driver_error_carrying_the_url_loses_the_password() {
        for msg in [
            "error sending request for url (https://alice:s3cret@host/notify)",
            "error sending request for url (https://alice:s3cret@host)",
            "error sending request for url (https://alice:s3cret@host): error trying to connect",
            "error sending request for url (https://alice:s3cret@host/n): dns error: no such host",
        ] {
            let red = crate::redact_userinfo(msg);
            assert!(!red.contains("s3cret"), "{msg} -> {red}");
            assert!(!red.contains("alice"), "{msg} -> {red}");
            assert!(
                red.contains("host"),
                "the destination stays readable: {red}"
            );
        }
    }

    /// A rejected endpoint travels back to the client in `detail` (5.5.3)
    /// and into the logs: the userinfo credentials never ride along.
    #[test]
    fn rejection_message_carries_no_credentials() {
        let err = sink()
            .parse_endpoint("ftp://user:hunter2@example.org/n", &[])
            .expect_err("not http");
        let text = format!("{err}");
        assert!(!text.contains("hunter2"), "{text}");
        assert!(text.contains("example.org"), "{text}");
    }

    #[test]
    fn serves_exactly_the_two_http_schemes() {
        assert_eq!(sink().schemes(), &["http", "https"]);
    }
}
