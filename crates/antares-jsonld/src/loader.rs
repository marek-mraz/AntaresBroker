// SPDX-License-Identifier: EUPL-1.2
//! Remote @context loading + caching + pinned core contexts.

use crate::context::Context;
use antares_model::{NgsiError, TenantId};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
// std Instant panics on wasm32; web-time is the std re-export
// natively and performance.now() in the browser.
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;
// moka's clock panics on wasm32 (std Instant); the browser build swaps
// in the FIFO minicache behind the same call surface.
#[cfg(target_arch = "wasm32")]
use crate::minicache::Cache as BoundedCache;
#[cfg(not(target_arch = "wasm32"))]
use moka::sync::Cache as BoundedCache;

/// Core context versions served from the build, never the network
/// (uri.etsi.org serves an HTML landing page to plain HTTP clients).
static PINNED: &[(&str, &str)] = &[
    (
        "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.3.jsonld",
        include_str!("../contexts/core-v1.6.jsonld"),
    ),
    (
        "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.4.jsonld",
        include_str!("../contexts/core-v1.6.jsonld"),
    ),
    (
        "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.5.jsonld",
        include_str!("../contexts/core-v1.6.jsonld"),
    ),
    (
        "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.6.jsonld",
        include_str!("../contexts/core-v1.6.jsonld"),
    ),
    (
        "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.7.jsonld",
        include_str!("../contexts/core-v1.7.jsonld"),
    ),
    (
        "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld",
        include_str!("../contexts/core-v1.8.jsonld"),
    ),
    (
        "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld",
        include_str!("../contexts/core-v1.9.jsonld"),
    ),
];

/// The core context this broker itself advertises (Link header default).
pub const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld";

/// Usage bookkeeping for one externally-referenced @context URL (5.13.3.5:
/// localId, createdAt, numberOfHits, lastUsage of "Cached" entries).
#[derive(Clone, Debug)]
pub struct CtxUsage {
    /// The @context URL as the client referenced it.
    pub url: String,
    /// Broker-generated `localId` for the Cached entry.
    pub local_id: String,
    /// First reference (`createdAt`).
    pub created_at: String,
    /// Most recent reference (`lastUsage`).
    pub last_usage: String,
    /// Number of references (`numberOfHits`).
    pub hits: u64,
}

/// Name resolution runs before the HTTP client exists, so none of its
/// timeouts cover it: an unresponsive resolver would hold the request path
/// open indefinitely. A lookup that does not answer within this bound is a
/// DENIAL — the policy never passes a destination it could not check.
const DNS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// Egress policy hook for @context fetches: scheme allowlist is
/// enforced in `fetch`; this adds the private-range deny (loopback,
/// RFC 1918, link-local incl. the 169.254.169.254 metadata range, ULA).
/// Private egress is ALLOWED by default (notifications must reach private
/// nets out of the box — dev boxes, compose stacks and the ETSI/IOP mocks
/// all live there); `ANTARES_EGRESS_ALLOW_PRIVATE=false`
/// turns the deny on for internet-exposed deployments.
/// The DNS-pinning resolver and redirect cap that enforce it on the wire
/// are `PolicyResolver` / `client_builder` below.
#[derive(Clone, Copy, Debug)]
pub struct EgressPolicy {
    /// Whether fetches to private/loopback/link-local ranges are allowed.
    pub allow_private: bool,
}

/// Programmatic stand-in for `ANTARES_EGRESS_ALLOW_PRIVATE` — wasm32 has
/// NO process environment (`std::env::var` always errs there), so the
/// browser/Node embedder sets this before constructing the broker.
static ALLOW_PRIVATE_OVERRIDE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Grant egress to private/loopback ranges for policies created AFTER this
/// call (the wasm constructor path; native deployments use the env var).
pub fn allow_private_egress(v: bool) {
    ALLOW_PRIVATE_OVERRIDE.store(v, std::sync::atomic::Ordering::Relaxed);
}

impl EgressPolicy {
    /// Policy from `ANTARES_EGRESS_ALLOW_PRIVATE` or the programmatic override.
    pub fn from_env() -> Self {
        Self {
            allow_private: Self::allow_private_from(
                std::env::var("ANTARES_EGRESS_ALLOW_PRIVATE")
                    .ok()
                    .as_deref(),
            ) || ALLOW_PRIVATE_OVERRIDE.load(std::sync::atomic::Ordering::Relaxed),
        }
    }

    /// Read the switch tolerantly: a security control that only understands
    /// one spelling hands the operator the opposite of the intent when the
    /// value is `FALSE` or carries stray whitespace.
    fn allow_private_from(v: Option<&str>) -> bool {
        v.is_none_or(|v| {
            let v = v.trim();
            !(v.eq_ignore_ascii_case("false") || v == "0")
        })
    }

    /// The cloud instance-metadata endpoints — the IPv4 link-local range
    /// (169.254.0.0/16, RFC 3927), its IPv6 spellings, and the IMDS-over-IPv6
    /// ULA `fd00:ec2::254`. Refused whatever `allow_private` says: no
    /// development box, compose stack or conformance mock lives there, so
    /// denying it costs nothing, while reaching it from a client-supplied
    /// @context URL or notification endpoint is the classic credential-theft
    /// SSRF.
    pub(crate) fn ip_is_metadata(ip: std::net::IpAddr) -> bool {
        match ip {
            std::net::IpAddr::V4(v4) => v4.is_link_local(),
            std::net::IpAddr::V6(v6) => {
                v6.segments() == [0xfd00, 0x0ec2, 0, 0, 0, 0, 0, 0x254]
                    || v6
                        .to_ipv4_mapped()
                        .or_else(|| v6.to_ipv4())
                        .is_some_and(|v4| v4.is_link_local())
            }
        }
    }

    pub(crate) fn ip_is_private(ip: std::net::IpAddr) -> bool {
        match ip {
            std::net::IpAddr::V4(v4) => {
                v4.is_loopback()
                    || v4.is_private()
                    || v4.is_link_local()
                    || v4.is_unspecified()
                    || v4.is_broadcast()
            }
            std::net::IpAddr::V6(v6) => {
                // an IPv4-mapped address (::ffff:a.b.c.d) is the v4 target in
                // v6 spelling — judge it as its v4 self, or ::ffff:127.0.0.1
                // and ::ffff:169.254.169.254 slip past the v6 checks
                if let Some(v4) = v6.to_ipv4_mapped() {
                    return Self::ip_is_private(std::net::IpAddr::V4(v4));
                }
                v6.is_loopback()
                    || v6.is_unspecified()
                    // fc00::/7 unique-local + fe80::/10 link-local
                    || (v6.segments()[0] & 0xfe00) == 0xfc00
                    || (v6.segments()[0] & 0xffc0) == 0xfe80
            }
        }
    }

    /// Deny-by-default for private destinations. Resolves the host
    /// once; any private address in the answer denies the fetch.
    pub async fn check_host(&self, host: &str, port: u16) -> Result<(), String> {
        self.check_host_within(host, port, DNS_TIMEOUT).await
    }

    async fn check_host_within(
        &self,
        host: &str,
        port: u16,
        dns_timeout: std::time::Duration,
    ) -> Result<(), String> {
        // The metadata range is refused before the private-egress switch is
        // consulted, so a deployment that allows private egress (the default)
        // still cannot be steered at its own instance credentials.
        if let Ok(ip) = host.trim_matches(['[', ']']).parse::<std::net::IpAddr>() {
            if Self::ip_is_metadata(ip) {
                return Err(format!("egress to {ip} denied (instance metadata)"));
            }
        }
        if self.allow_private {
            return Ok(());
        }
        if host.eq_ignore_ascii_case("localhost") {
            return Err(format!("egress to {host} denied (private range)"));
        }
        if let Ok(ip) = host.trim_matches(['[', ']']).parse::<std::net::IpAddr>() {
            if Self::ip_is_private(ip) {
                return Err(format!("egress to {ip} denied (private range)"));
            }
            return Ok(());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            // The lookup runs under its own deadline and a lookup that does
            // not answer is a DENIAL: a destination the policy could not
            // check is never allowed through, and the request path never
            // waits on the resolver.
            let addrs = tokio::time::timeout(dns_timeout, tokio::net::lookup_host((host, port)))
                .await
                .map_err(|_| format!("resolving {host}: timed out"))?
                .map_err(|e| format!("resolving {host}: {e}"))?;
            for a in addrs {
                if Self::ip_is_private(a.ip()) {
                    // The denial reaches the client verbatim in the RFC 7807
                    // `detail`; naming the resolved address would turn the
                    // request parameter into an internal-DNS oracle.
                    return Err(format!("egress to {host} denied (private range)"));
                }
            }
        }
        // wasm32: a page cannot resolve DNS — the browser does, and its
        // same-origin/CORS machinery is the egress boundary there.
        #[cfg(target_arch = "wasm32")]
        let _ = (port, dns_timeout);
        Ok(())
    }
}

/// axum handlers require `Send` futures and axum state requires
/// `Send + Sync`, but reqwest's wasm client and futures are neither — the
/// browser build is single-threaded, so `send_wrapper` bridges the gap
/// soundly (it still panics at runtime on any actual cross-thread use).
/// Natively these are the identity types.
#[cfg(not(target_arch = "wasm32"))]
pub type HttpClient = reqwest::Client;
#[cfg(target_arch = "wasm32")]
#[allow(missing_docs)] // documented on the native arm above
pub type HttpClient = send_wrapper::SendWrapper<reqwest::Client>;

/// Wrap a reqwest client as [`HttpClient`] (identity natively).
pub fn wrap_client(c: reqwest::Client) -> HttpClient {
    #[cfg(not(target_arch = "wasm32"))]
    {
        c
    }
    #[cfg(target_arch = "wasm32")]
    {
        send_wrapper::SendWrapper::new(c)
    }
}

/// Run one whole HTTP interaction (build → send → read body) as a unit
/// whose future is Send on every target. Native: the identity. The inputs
/// must move INTO the future and only Send data may come out.
#[cfg(not(target_arch = "wasm32"))]
pub fn http_interaction<F: std::future::Future>(fut: F) -> F {
    fut
}
#[cfg(target_arch = "wasm32")]
#[allow(missing_docs)] // documented on the native arm above
pub fn http_interaction<F: std::future::Future>(fut: F) -> send_wrapper::SendWrapper<F> {
    send_wrapper::SendWrapper::new(fut)
}

/// The recursion box for `merge_entry` — Send on every target: the only
/// un-Send piece (reqwest's wasm fetch) is already fenced inside
/// `http_interaction`, so the box itself can stay Send and the axum handler
/// futures above it keep their required Send bound.
type BoxFut<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;

/// Redirect cap: a fetch may not be bounced more than this many times.
/// Each hop is a fresh destination the policy has to clear, so the cap is what
/// keeps an open redirector from walking us into a private range.
pub const MAX_REDIRECTS: usize = 3;

/// DNS pinning. `check_host` resolves a name to decide whether egress is
/// allowed, but reqwest would resolve it *again* at connect time — a window in
/// which the answer can change (DNS rebinding). Installing the policy as the
/// client's resolver closes it: the addresses the connector dials are the ones
/// this filter passed, so the check and the connect see the same answer by
/// construction. Redirect hops go through it too.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug)]
pub struct PolicyResolver(EgressPolicy);

#[cfg(not(target_arch = "wasm32"))]
impl reqwest::dns::Resolve for PolicyResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let allow_private = self.0.allow_private;
        Box::pin(async move {
            let host = name.as_str().to_owned();
            let addrs = tokio::net::lookup_host((host.as_str(), 0)).await?;
            let kept: Vec<std::net::SocketAddr> = addrs
                .filter(|a| !EgressPolicy::ip_is_metadata(a.ip()))
                .filter(|a| allow_private || !EgressPolicy::ip_is_private(a.ip()))
                .collect();
            if kept.is_empty() {
                return Err(format!("egress to {host} denied (private range)").into());
            }
            Ok(Box::new(kept.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

/// The one outbound-client constructor: every reqwest client in the
/// broker — @context fetches, notifications, federation forwards — is built
/// from this, so the policy cannot be forgotten at a call site. Timeouts stay
/// the caller's choice; the security-relevant settings do not.
///
/// `ANTARES_EXTRA_CA_FILE`: optional PEM bundle of ADDITIONAL trust anchors
/// (private CAs, corporate proxies — and servers that ship an incomplete
/// chain, as forge.etsi.org does). Verification itself is never
/// disableable; this only widens what it trusts, per deployment.
/// Read once per builder call — the wiring constructs clients at startup.
#[cfg(not(target_arch = "wasm32"))]
pub fn client_builder(policy: EgressPolicy) -> reqwest::ClientBuilder {
    // SSRF: `PolicyResolver` only fires for HOSTNAME targets — reqwest
    // dials IP-LITERAL URLs directly, so a `302 Location: http://169.254.169.254/`
    // would skip the egress check on every hop. The custom redirect policy
    // re-checks each hop's URL: an IP literal in a private range is refused,
    // hostnames still clear through the resolver at connect. Hop count capped.
    let allow_private = policy.allow_private;
    let redirect = reqwest::redirect::Policy::custom(move |attempt| {
        // previous() includes the initial URL, so `> MAX_REDIRECTS` matches
        // Policy::limited(MAX_REDIRECTS): 1 initial request + MAX_REDIRECTS hops.
        if attempt.previous().len() > MAX_REDIRECTS {
            // same shape as Policy::limited: a redirect error, is_redirect()==true
            return attempt.error(format!("exceeded {MAX_REDIRECTS} redirects"));
        }
        if let Ok(ip) = attempt
            .url()
            .host_str()
            .unwrap_or("")
            .parse::<std::net::IpAddr>()
        {
            if EgressPolicy::ip_is_metadata(ip)
                || (!allow_private && EgressPolicy::ip_is_private(ip))
            {
                // stop (don't follow) — the caller sees a non-2xx and fails the
                // fetch, but we never connected to the private target.
                return attempt.stop();
            }
        }
        attempt.follow()
    });
    let mut b = reqwest::Client::builder()
        .redirect(redirect)
        .dns_resolver(std::sync::Arc::new(PolicyResolver(policy)));
    if let Ok(path) = std::env::var("ANTARES_EXTRA_CA_FILE") {
        match std::fs::read(&path) {
            Ok(pem) => match reqwest::Certificate::from_pem_bundle(&pem) {
                Ok(certs) => {
                    for c in certs {
                        b = b.add_root_certificate(c);
                    }
                }
                // once at startup; this crate carries no tracing dep
                Err(e) => eprintln!("ANTARES_EXTRA_CA_FILE {path}: not a PEM bundle ({e})"),
            },
            Err(e) => eprintln!("ANTARES_EXTRA_CA_FILE {path}: unreadable ({e})"),
        }
    }
    b
}

/// wasm32: the browser owns TLS trust, redirects and name resolution —
/// reqwest's wasm `ClientBuilder` exposes none of those knobs, and the page's
/// CORS sandbox is the egress boundary. The policy still gates URLs via
/// `check_host` before any fetch.
#[cfg(target_arch = "wasm32")]
pub fn client_builder(_policy: EgressPolicy) -> reqwest::ClientBuilder {
    reqwest::Client::builder()
}

/// Client timeouts are native knobs; the browser's fetch has no
/// client-level equivalent, so on wasm32 this is a no-op by design.
#[cfg(not(target_arch = "wasm32"))]
pub fn with_timeouts(
    b: reqwest::ClientBuilder,
    connect: std::time::Duration,
    total: std::time::Duration,
) -> reqwest::ClientBuilder {
    b.connect_timeout(connect).timeout(total)
}

#[cfg(target_arch = "wasm32")]
#[allow(missing_docs)] // documented on the native arm above
pub fn with_timeouts(
    b: reqwest::ClientBuilder,
    _connect: std::time::Duration,
    _total: std::time::Duration,
) -> reqwest::ClientBuilder {
    b
}

/// Hard wall-clock bound for an outbound interaction. Native clients carry
/// their timeouts at construction, so this passes through; on wasm32 the
/// browser fetch has no client-level timeout (and reqwest's AbortController
/// timer does not arm inside a dedicated worker), so an unresolved fetch
/// would pend FOREVER — and a pending context fetch holds its resolve
/// permit, which eventually stalls ALL context resolution (a stopped context
/// server once froze a whole Robot run for over an hour this way, every later
/// fetch queued behind the leaked permits). `None` = deadline exceeded.
#[cfg(not(target_arch = "wasm32"))]
pub async fn io_deadline<T>(fut: impl std::future::Future<Output = T>, _ms: u32) -> Option<T> {
    Some(fut.await)
}

#[cfg(target_arch = "wasm32")]
#[allow(missing_docs)] // documented on the native arm above
pub async fn io_deadline<T>(fut: impl std::future::Future<Output = T>, ms: u32) -> Option<T> {
    use futures_util::future::{select, Either};
    use futures_util::pin_mut;
    let t = gloo_timers::future::TimeoutFuture::new(ms);
    pin_mut!(fut);
    match select(fut, t).await {
        Either::Left((v, _)) => Some(v),
        Either::Right(_) => None,
    }
}

/// Ceiling on a header-supplied cache lifetime: one year. The values are
/// remote input; unclamped, `Instant::now() + duration` overflows (and
/// panics) on a hostile max-age or Expires.
const MAX_CONTEXT_TTL: std::time::Duration = std::time::Duration::from_secs(31_536_000);

/// 6.3.16: cache lifetime of a downloaded @context comes from its response
/// headers. `None` = no explicit lifetime (cache until evicted/reloaded).
fn ttl_from_headers(
    cache_control: Option<&str>,
    expires: Option<&str>,
) -> Option<std::time::Duration> {
    if let Some(cc) = cache_control {
        let cc = cc.to_ascii_lowercase();
        if cc.contains("no-store") || cc.contains("no-cache") {
            return Some(std::time::Duration::ZERO);
        }
        // 6.3.16: "a max-age or s-maxage response directive"; the broker is a
        // shared cache, so s-maxage takes precedence when both are present
        // (RFC 7234 5.2.2.9).
        for prefix in ["s-maxage=", "max-age="] {
            if let Some(v) = cc
                .split(',')
                .filter_map(|d| d.trim().strip_prefix(prefix))
                .next()
            {
                if let Ok(secs) = v.trim().parse::<u64>() {
                    return Some(std::time::Duration::from_secs(secs).min(MAX_CONTEXT_TTL));
                }
            }
        }
    }
    if let Some(exp) = expires {
        // HTTP-date (RFC 7231); an unparsable or past Expires means "stale".
        let when = chrono::DateTime::parse_from_rfc2822(exp).ok()?;
        let delta = when.with_timezone(&chrono::Utc) - chrono::Utc::now();
        return Some(
            delta
                .to_std()
                .unwrap_or(std::time::Duration::ZERO)
                .min(MAX_CONTEXT_TTL),
        );
    }
    None
}

/// @context responses above this size are refused.
pub(crate) const MAX_CONTEXT_BYTES: usize = 5 * 1024 * 1024;

/// Cap on usage-registry entries (client-supplied URLs); past it, adding a
/// new URL evicts the least recently used entry.
const MAX_USAGE_ENTRIES: usize = 4096;

/// Fetch-count cap per @context resolution — a hostile context tree must
/// not turn one request into an unbounded crawl. Checked BEFORE each
/// fetch, so at most this many URLs are ever contacted.
const MAX_CONTEXT_URLS: usize = 32;

/// The merged-context cache is keyed by the SERIALIZED user @context, which
/// an `application/ld+json` body may carry inline up to the body cap — 256
/// multi-megabyte keys (plus the term maps built from them) are no memory
/// bound at all. Past this length the merge is simply not cached: no network
/// is involved, the fetched documents stay warm, and the attacker's lever
/// disappears.
const MAX_MERGED_KEY_BYTES: usize = 8 * 1024;

/// Byte budget of the fetched-document cache. Entry count alone is not a
/// memory bound when one entry may be MAX_CONTEXT_BYTES.
const MAX_FETCHED_CACHE_BYTES: u64 = 16 * 1024 * 1024;

/// Entry ceiling, charged through the byte budget: every entry costs at
/// least MAX_FETCHED_CACHE_BYTES/MAX_FETCHED_ENTRIES of it, so a flood of
/// tiny documents cannot turn a byte budget into an unbounded map.
const MAX_FETCHED_ENTRIES: u64 = 256;

/// The fetched-document cache, bounded by BYTES: the weight of an entry is
/// the size of the document it holds (floored, see above).
#[cfg(not(target_arch = "wasm32"))]
fn fetched_cache() -> BoundedCache<String, FetchedDoc> {
    // what one entry costs of the budget at minimum
    let floor = (MAX_FETCHED_CACHE_BYTES / MAX_FETCHED_ENTRIES) as u32;
    BoundedCache::builder()
        .max_capacity(MAX_FETCHED_CACHE_BYTES)
        .weigher(move |_url: &String, doc: &FetchedDoc| {
            u32::try_from(doc.value.to_string().len())
                .unwrap_or(u32::MAX)
                .max(floor)
        })
        .build()
}

/// wasm32: the FIFO minicache carries no weigher, and a browser tab's
/// @context set is tiny — the entry bound is the bound there.
#[cfg(target_arch = "wasm32")]
fn fetched_cache() -> BoundedCache<String, FetchedDoc> {
    BoundedCache::new(MAX_FETCHED_ENTRIES)
}

#[derive(Clone)]
struct FetchedDoc {
    value: Arc<Value>,
    /// 6.3.16 expiry deadline; `None` = cache until evicted.
    stale_at: Option<Instant>,
    /// The Tenant this document belongs to, for the locally stored kinds
    /// (5.13.1 "Hosted": "@contexts that are explicitly added by users";
    /// "ImplicitlyCreated": created as a side effect of an operation). 5.5.10:
    /// "If a Tenant is specified for an NGSI-LD operation, the operation
    /// shall only be applied to information related to the specified
    /// Tenant" — so those mappings expand that Tenant's payloads only.
    /// `None` = a "Cached" copy of a document the broker downloaded from a
    /// public URL, which belongs to no Tenant and is shared by all of them.
    owner: Option<TenantId>,
}

impl FetchedDoc {
    /// Does this document resolve for `tenant`? A resolution with no Tenant
    /// in scope (a broker-internal one) sees every entry.
    fn serves(&self, tenant: Option<&TenantId>) -> bool {
        match (&self.owner, tenant) {
            (None, _) | (Some(_), None) => true,
            (Some(owner), Some(t)) => owner == t,
        }
    }

    /// 6.3.16 lifetime reached.
    fn is_stale(&self) -> bool {
        self.stale_at.is_some_and(|t| Instant::now() >= t)
    }
}

/// The @context loader: fetches, caches and merges @contexts under the
/// egress policy, with the core context pinned.
pub struct Loader {
    http: HttpClient,
    policy: EgressPolicy,
    /// URL → parsed `@context` member of the fetched document (+ 6.3.16 TTL
    /// and, for locally stored @contexts, the owning Tenant).
    /// Bounded LRU — every cache has a max size.
    fetched: BoundedCache<String, FetchedDoc>,
    /// cache key (serialized user context) → merged+frozen Context (the
    /// parsed-context LRU — the centerpiece, size-capped at 256).
    merged: BoundedCache<String, Arc<Context>>,
    /// Core context, pre-merged and PINNED outside the LRU (never evicted).
    core_only: Arc<Context>,
    /// URL → usage stats for every external @context referenced by requests
    /// (5.13 Cached-entry bookkeeping). Client-supplied URLs must never grow
    /// state without limit: capped at `MAX_USAGE_ENTRIES`, and admitting a
    /// new URL past the cap evicts the entry with the oldest lastUsage.
    usage: RwLock<HashMap<String, CtxUsage>>,
    /// merged-cache key → every URL that resolution touched (so cache hits
    /// still bump numberOfHits for nested references).
    merged_urls: BoundedCache<String, Arc<Vec<String>>>,
    /// Bounded concurrency on cold context FETCHES (one permit per network
    /// fetch, not per resolution) — a burst of exotic-context requests can't
    /// blow the JSON working-set budget.
    resolve_permits: tokio::sync::Semaphore,
    /// Write-through: freshly fetched remote contexts are handed to this
    /// hook (the broker persists them as kind='Cached' rows) so the cache
    /// survives a restart. Set once at wiring; None in tests.
    cache_writer: std::sync::RwLock<Option<CacheWriter>>,
    /// Shared-store hit counter: bump the persisted row on every counted
    /// use; a missing row reports a cross-instance delete. `None` in
    /// compositions without a store (bare loader tests).
    usage_bump: std::sync::RwLock<Option<UsageBump>>,
}

/// Request header marking a broker-internal @context fetch (this loader
/// resolving a URL, possibly through the fleet's own LB). The serve endpoint
/// skips its serve-hit bump for these — the resolving instance counts the
/// use itself (5.13.3.5, one client use = one hit).
pub const INTERNAL_FETCH_HEADER: &str = "x-antares-ctx-fetch";

/// (url, parsed `@context` value) — called on every fresh remote fetch.
pub type CacheWriter = Box<dyn Fn(&str, &Value) + Send + Sync>;
/// url -> "the shared row still exists" (after bumping its hit counter).
pub type UsageBump = Box<dyn Fn(&str) -> bool + Send + Sync>;

impl Default for Loader {
    fn default() -> Self {
        Self::new()
    }
}

impl Loader {
    /// A loader with the policy from the environment and default timeouts.
    pub fn new() -> Self {
        Self::with_policy(EgressPolicy::from_env())
    }

    /// A loader with an explicit policy over a freshly built HTTP client.
    pub fn with_policy(policy: EgressPolicy) -> Self {
        Self::with_client(
            policy,
            with_timeouts(
                client_builder(policy),
                std::time::Duration::from_secs(5),
                std::time::Duration::from_secs(10),
            )
            .build()
            .expect("reqwest client"),
        )
    }
}

/// The pinned core `@context`, parsed and frozen, with no loader, client or
/// cache behind it: the value every `Loader` pins, and what a test of
/// expansion or matching needs on its own.
pub fn core_context() -> Context {
    let mut core = Context::default();
    merge_context_value(&mut core, &pinned(CORE_CONTEXT).expect("pinned core"));
    core.freeze();
    core.source = Value::String(CORE_CONTEXT.to_owned());
    core
}

impl Loader {
    /// A loader over the caller's own HTTP client — a gateway with its own
    /// proxy, allowlist or TLS setup fetches @contexts through it. `policy`
    /// still governs the private-range deny and the redirect cap; every
    /// cache is per instance, nothing here is process-global.
    pub fn with_client(policy: EgressPolicy, client: reqwest::Client) -> Self {
        let core = core_context();
        Self {
            http: wrap_client(client),
            policy,
            fetched: fetched_cache(),
            merged: BoundedCache::new(256),
            core_only: Arc::new(core),
            usage: RwLock::new(HashMap::new()),
            merged_urls: BoundedCache::new(256),
            resolve_permits: tokio::sync::Semaphore::new(32),
            cache_writer: std::sync::RwLock::new(None),
            usage_bump: std::sync::RwLock::new(None),
        }
    }

    /// Install the hook that persists a fetched @context (url, document).
    pub fn set_cache_writer(&self, w: CacheWriter) {
        *self
            .cache_writer
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(w);
    }

    /// Wire the shared-store usage bump (5.13.3.5): called on every
    /// counted use of a URL. Returns whether the shared row still exists —
    /// `false` means another instance deleted the @context, and this
    /// instance must drop its warm copies so the delete is honoured here.
    pub fn set_usage_bump(&self, f: UsageBump) {
        *self
            .usage_bump
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(f);
    }

    /// Boot preload: re-seed a Cached entry persisted by the writer —
    /// the parsed doc goes into the fetch cache, the bookkeeping identity
    /// (localId/createdAt) into the usage registry, so 5.13 listings look
    /// the same across a restart.
    pub async fn seed_cached(&self, url: &str, local_id: &str, created_at: &str, ctx_value: Value) {
        self.fetched.insert(
            url.to_owned(),
            FetchedDoc {
                value: Arc::new(ctx_value),
                stale_at: None,
                // 5.13.1 "Cached": a copy of a public document, no Tenant
                owner: None,
            },
        );
        self.usage.write().await.insert(
            url.to_owned(),
            CtxUsage {
                url: url.to_owned(),
                local_id: local_id.to_owned(),
                created_at: created_at.to_owned(),
                last_usage: created_at.to_owned(),
                hits: 0,
            },
        );
    }

    fn now() -> String {
        chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    }

    /// Bump usage stats (numberOfHits / lastUsage, 5.13.3.5) for one URL —
    /// in this instance's registry AND, via the usage_bump hook, in the
    /// shared store row (per-instance counters split-brain behind a
    /// load balancer). Returns true when the hook reported the row GONE
    /// (deleted through another instance): local copies are evicted so the
    /// next resolution refetches and re-creates the entry.
    pub async fn bump_url(&self, url: &str) -> bool {
        let now = Self::now();
        {
            let mut map = self.usage.write().await;
            if let Some(u) = map.get_mut(url) {
                u.hits += 1;
                u.last_usage = now;
            } else {
                // hold the size bound: admitting a new URL past the cap
                // evicts the entry with the oldest lastUsage (RFC 3339
                // strings order chronologically)
                if map.len() >= MAX_USAGE_ENTRIES {
                    if let Some(oldest) = map
                        .values()
                        .min_by(|a, b| a.last_usage.cmp(&b.last_usage))
                        .map(|u| u.url.clone())
                    {
                        map.remove(&oldest);
                    }
                }
                map.insert(
                    url.to_owned(),
                    CtxUsage {
                        url: url.to_owned(),
                        // deterministic (uuid5 of the URL): the same identity
                        // names this entry in the usage registry, the persisted
                        // Cached row (write-through) and across restarts — an
                        // API delete can therefore always find the row (5.13.5).
                        local_id: uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, url.as_bytes())
                            .to_string(),
                        created_at: now.clone(),
                        last_usage: now,
                        hits: 1,
                    },
                );
            }
        }
        let row_exists = match self
            .usage_bump
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
        {
            Some(f) => f(url),
            None => true,
        };
        if row_exists {
            return false;
        }
        self.usage.write().await.remove(url);
        self.evict(url).await;
        true
    }

    /// Usage entries of every external @context referenced so far.
    pub async fn usage_list(&self) -> Vec<CtxUsage> {
        self.usage.read().await.values().cloned().collect()
    }

    /// Find a usage entry by original URL or by its generated localId.
    pub async fn usage_get(&self, id: &str) -> Option<CtxUsage> {
        let map = self.usage.read().await;
        map.get(id)
            .or_else(|| map.values().find(|u| u.local_id == id))
            .cloned()
    }

    /// Drop a URL's usage entry and evict it from the caches.
    pub async fn usage_remove(&self, url: &str) {
        self.usage.write().await.remove(url);
        self.evict(url).await;
    }

    /// 5.13.5.4 Delete and Reload: re-download a Cached @context from its
    /// original URL, replacing the stored copy only on success. Any error —
    /// download failure or invalid content per 5.5.4 — is
    /// LdContextNotAvailable and "the operation ends without removing the
    /// existing @context".
    pub async fn refetch(&self, url: &str) -> Result<(), NgsiError> {
        let old = self.fetched.get(url);
        self.fetched.invalidate(url); // force a network fetch
        match self.fetch(url, None).await {
            Ok(_) => {
                // merged contexts built on the old copy are stale
                self.invalidate_merged_using(url);
                Ok(())
            }
            Err(e) => {
                if let Some(old) = old {
                    self.fetched.insert(url.to_owned(), old);
                }
                Err(match e {
                    NgsiError::BadRequestData(m) => NgsiError::LdContextNotAvailable(m),
                    other => other,
                })
            }
        }
    }

    /// Core-only context (no user @context supplied).
    pub fn core(&self) -> Arc<Context> {
        Arc::clone(&self.core_only)
    }

    /// Resolve a user-supplied `@context` value (string URL, object, or array)
    /// into a merged Context with the core context merged last. No Tenant in
    /// scope: locally stored @contexts of every Tenant resolve, so a
    /// resolution serving a request must use `resolve_for` instead (5.5.10).
    pub async fn resolve(&self, user: &Value) -> Result<Arc<Context>, NgsiError> {
        self.resolve_counted(None, user, true).await
    }

    /// Resolve WITHOUT counting usage hits — for broker-internal resolutions
    /// (notification building), which are not client @context usage (053_08).
    pub async fn resolve_quiet(&self, user: &Value) -> Result<Arc<Context>, NgsiError> {
        self.resolve_counted(None, user, false).await
    }

    /// 5.5.10: resolve within one Tenant — "the operation shall only be
    /// applied to information related to the specified Tenant", so an
    /// @context another Tenant stored locally (5.13.1) does not resolve here.
    pub async fn resolve_for(
        &self,
        tenant: &TenantId,
        user: &Value,
    ) -> Result<Arc<Context>, NgsiError> {
        self.resolve_counted(Some(tenant), user, true).await
    }

    /// `resolve_for` without counting usage hits.
    pub async fn resolve_quiet_for(
        &self,
        tenant: &TenantId,
        user: &Value,
    ) -> Result<Arc<Context>, NgsiError> {
        self.resolve_counted(Some(tenant), user, false).await
    }

    async fn resolve_counted(
        &self,
        tenant: Option<&TenantId>,
        user: &Value,
        count: bool,
    ) -> Result<Arc<Context>, NgsiError> {
        let key = user.to_string();
        // urls already counted on the merged-hit path — the fallthrough
        // rebuild below must not bump them a second time.
        let mut counted: Vec<String> = Vec::new();
        if let Some(hit) = self
            .merged
            .get(&key)
            .filter(|_| self.merged_hit_is_usable(&key, tenant))
        {
            if count {
                // cache hit: bump every URL this context resolution involves.
                // A bump that finds the shared row GONE means another
                // instance deleted this @context — do NOT serve the warm
                // copy; fall through and rebuild (refetch re-creates it).
                let urls = self.merged_urls.get(&key);
                let mut deleted_elsewhere = false;
                for url in urls.iter().flat_map(|u| u.iter()) {
                    if self.bump_url(url).await {
                        deleted_elsewhere = true;
                    } else {
                        counted.push(url.clone());
                    }
                }
                if !deleted_elsewhere {
                    return Ok(hit);
                }
            } else {
                return Ok(hit);
            }
        }
        let mut ctx = Context::default();
        let urls = std::sync::Mutex::new(Vec::new());
        self.merge_entry(&mut ctx, user, 0, &urls, None, tenant)
            .await?;
        let urls = urls.into_inner().unwrap_or_default();
        if count {
            for url in &urls {
                if counted.contains(url) {
                    continue; // already bumped on the merged-hit path
                }
                // only after successful resolution. A bump that reports the
                // shared row GONE (deleted through another instance) while
                // the fetch above was served from this instance's warm doc
                // cache means the write-through never ran — a counted use
                // re-creates the entry (5.13.5.4): refetch (bump_url just
                // evicted the warm copy) so the row exists, then count on it.
                if self.bump_url(url).await && self.fetch(url, tenant).await.is_ok() {
                    let _ = self.bump_url(url).await;
                }
            }
        }
        // Core context last: its (protected) terms win — CIM 009 4.4.
        merge_context_value(&mut ctx, &pinned(CORE_CONTEXT).expect("pinned core"));
        ctx.freeze();
        ctx.source = user.clone();
        let arc = Arc::new(ctx);
        if key.len() <= MAX_MERGED_KEY_BYTES {
            self.merged_urls.insert(key.clone(), Arc::new(urls));
            self.merged.insert(key, Arc::clone(&arc));
        }
        Ok(arc)
    }

    /// A merged-context cache hit is usable only while every document it was
    /// built from is still fresh (6.3.16: "implementations shall periodically
    /// invalidate the "Cached" @contexts according to the headers mentioned
    /// above" — a merged context is only as fresh as its sources) and still
    /// resolves for this Tenant (5.5.10): the cache is keyed by the user
    /// @context alone, which two Tenants can send verbatim, so a merge built
    /// from one Tenant's locally stored @context must not be handed to
    /// another.
    fn merged_hit_is_usable(&self, key: &str, tenant: Option<&TenantId>) -> bool {
        match self.merged_urls.get(key) {
            // the documents behind this entry are unknown: it can be shown
            // neither fresh nor in-Tenant, so it is rebuilt
            None => false,
            Some(urls) => urls.iter().all(|url| match self.fetched.get(url) {
                Some(doc) => doc.serves(tenant) && !doc.is_stale(),
                // dropped from the document cache: nothing marks it stale
                None => true,
            }),
        }
    }

    fn merge_entry<'a>(
        &'a self,
        ctx: &'a mut Context,
        entry: &'a Value,
        depth: usize,
        urls: &'a std::sync::Mutex<Vec<String>>,
        // JSON-LD 1.1 (section 3.1): a relative context IRI inside a fetched context
        // document resolves against THAT document's URL. The ETSI compound
        // context references "ngsi-ld-test-suite.jsonld" relatively — without
        // this every request using it dies with LdContextNotAvailable.
        base: Option<std::sync::Arc<String>>,
        tenant: Option<&'a TenantId>,
    ) -> BoxFut<'a, Result<(), NgsiError>> {
        Box::pin(async move {
            // 5.5.6: an @context that "is invalid" is BadRequestData; 504
            // LdContextNotAvailable is reserved for one that "is not
            // available". Both caps below are reached from client-supplied
            // structure alone, so a client must not be able to mint gateway
            // errors on demand.
            if depth > 8 {
                return Err(NgsiError::BadRequestData(
                    "@context nesting too deep".into(),
                ));
            }
            match entry {
                Value::Array(items) => {
                    for item in items {
                        self.merge_entry(ctx, item, depth + 1, urls, base.clone(), tenant)
                            .await?;
                    }
                    Ok(())
                }
                Value::String(url) => {
                    let resolved: String =
                        if url.starts_with("http://") || url.starts_with("https://") {
                            url.clone()
                        } else if let Some(b) = base.as_deref() {
                            reqwest::Url::parse(b)
                                .and_then(|b| b.join(url))
                                .map(String::from)
                                .map_err(|e| {
                                    NgsiError::LdContextNotAvailable(format!(
                                        "cannot resolve @context URL {url} against {b}: {e}"
                                    ))
                                })?
                        } else {
                            url.clone()
                        };
                    // Cap enforced before the network is touched: once the
                    // resolution has already fetched MAX_CONTEXT_URLS
                    // documents, the next reference fails instead of
                    // extending the crawl. Poisoned lock fails closed.
                    let fetched_so_far = urls.lock().map(|u| u.len()).unwrap_or(usize::MAX);
                    if fetched_so_far >= MAX_CONTEXT_URLS {
                        return Err(NgsiError::BadRequestData(format!(
                            "@context resolution exceeds {MAX_CONTEXT_URLS} referenced URLs"
                        )));
                    }
                    let doc = self.fetch(&resolved, tenant).await?;
                    if let Ok(mut u) = urls.lock() {
                        u.push(resolved.clone());
                    }
                    self.merge_entry(
                        ctx,
                        &doc,
                        depth + 1,
                        urls,
                        Some(std::sync::Arc::new(resolved)),
                        tenant,
                    )
                    .await
                }
                Value::Object(obj) => ctx.merge_object(obj),
                Value::Null => Ok(()),
                _ => Err(NgsiError::BadRequestData("invalid @context entry".into())),
            }
        })
    }

    /// Is `url` one of the built-in (pinned) core context URLs?
    pub fn is_pinned_core(url: &str) -> bool {
        pinned(url).is_some()
    }

    /// Fetch a remote context document, returning its `@context` member.
    /// 5.13.1: Cached @contexts are invalidated per the protocol's explicit
    /// expiration indications — cache hits honour the 6.3.16 lifetime, stale
    /// entries are re-fetched, and a changed body invalidates the
    /// merged-context cache.
    async fn fetch(&self, url: &str, tenant: Option<&TenantId>) -> Result<Arc<Value>, NgsiError> {
        if let Some(v) = pinned(url) {
            return Ok(Arc::new(v));
        }
        let err = |m: String| NgsiError::LdContextNotAvailable(m);
        let mut stale_value: Option<Arc<Value>> = None;
        if let Some(hit) = self.fetched.get(url) {
            if !hit.serves(tenant) {
                // 5.5.10 + 5.13.1: this URL names an @context another Tenant
                // stored locally, so for this Tenant it does not exist — and
                // fetching it would only reach the same entry back through
                // the broker's own (Tenant-gated) serve endpoint.
                return Err(err(format!("@context {url} is not available")));
            }
            if hit.is_stale() {
                stale_value = Some(Arc::clone(&hit.value));
            } else {
                return Ok(hit.value);
            }
        }
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(err(format!("unsupported @context URL: {url}")));
        }
        // SSRF hook: deny private destinations unless configured.
        let parsed = reqwest::Url::parse(url).map_err(|e| err(format!("bad URL {url}: {e}")))?;
        let host = parsed.host_str().unwrap_or_default().to_owned();
        let port = parsed.port_or_known_default().unwrap_or(443);
        self.policy
            .check_host(&host, port)
            .await
            .map_err(|e| err(format!("fetching {url}: {e}")))?;
        // Bounded concurrency on cold fetching. The permit covers this ONE
        // network fetch and is released before the crawl recurses into the
        // document's own references — held across a whole recursive
        // resolution instead, a handful of slow context trees would stall
        // every cold resolution in the process.
        let _permit = self
            .resolve_permits
            .acquire()
            .await
            .expect("semaphore never closed");
        // The whole HTTP interaction is one Send unit (http_interaction);
        // only Send data (ttl + bytes) crosses back out.
        let interact = async {
            let resp = self
                .http
                .get(url)
                .header("Accept", "application/ld+json, application/json")
                // marks this as a broker-internal context resolution: the
                // serving instance must not add a serve-hit on top of this
                // instance's own bump_url (053_08 fleet double-count)
                .header(INTERNAL_FETCH_HEADER, "1")
                .send()
                .await
                .map_err(|e| err(format!("fetching {url}: {e}")))?;
            if !resp.status().is_success() {
                return Err(err(format!("fetching {url}: HTTP {}", resp.status())));
            }
            let ttl = ttl_from_headers(
                resp.headers()
                    .get("cache-control")
                    .and_then(|v| v.to_str().ok()),
                resp.headers().get("expires").and_then(|v| v.to_str().ok()),
            );
            // Bounded response size (504 LdContextNotAvailable on breach).
            if resp
                .content_length()
                .is_some_and(|l| l as usize > MAX_CONTEXT_BYTES)
            {
                return Err(err(format!("{url}: @context document too large")));
            }
            // A declared Content-Length is advisory only — a chunked body
            // has none. Natively the body is accumulated chunk by chunk and
            // refused the moment it would pass the cap, so an oversized
            // response is never buffered in full first.
            #[cfg(not(target_arch = "wasm32"))]
            let bytes = {
                let mut resp = resp;
                let mut buf: Vec<u8> = Vec::new();
                while let Some(chunk) = resp
                    .chunk()
                    .await
                    .map_err(|e| err(format!("reading {url}: {e}")))?
                {
                    if buf.len() + chunk.len() > MAX_CONTEXT_BYTES {
                        return Err(err(format!("{url}: @context document too large")));
                    }
                    buf.extend_from_slice(&chunk);
                }
                buf
            };
            // wasm: the browser fetch hands over the body whole; the
            // post-read size check below still applies.
            #[cfg(target_arch = "wasm32")]
            let bytes = resp
                .bytes()
                .await
                .map_err(|e| err(format!("reading {url}: {e}")))?
                .to_vec();
            Ok((ttl, bytes))
        };
        let (ttl, bytes) = http_interaction(async {
            match io_deadline(interact, 10_000).await {
                Some(r) => r,
                None => Err(err(format!("fetching {url}: deadline exceeded"))),
            }
        })
        .await?;
        if bytes.len() > MAX_CONTEXT_BYTES {
            return Err(err(format!("{url}: @context document too large")));
        }
        // 5.5.6: unavailability is LdContextNotAvailable, but a RETRIEVED
        // remote @context whose content is invalid is BadRequestData.
        let doc: Value = serde_json::from_slice(&bytes)
            .map_err(|e| NgsiError::BadRequestData(format!("{url} is not a JSON document: {e}")))?;
        let ctx_val = doc
            .get("@context")
            .cloned()
            .ok_or_else(|| NgsiError::BadRequestData(format!("{url} has no @context member")))?;
        let arc = Arc::new(ctx_val);
        if stale_value.is_some_and(|old| *old != *arc) {
            // Refreshed content differs: merged contexts built on the old
            // copy are invalid.
            self.invalidate_merged_using(url);
        }
        self.fetched.insert(
            url.to_owned(),
            FetchedDoc {
                value: Arc::clone(&arc),
                stale_at: ttl.map(|d| Instant::now() + d),
                // 5.13.1 "Cached": downloaded from a public URL, no Tenant
                owner: None,
            },
        );
        // Write-through: persist what was just fetched.
        if let Ok(w) = self.cache_writer.read() {
            if let Some(w) = w.as_ref() {
                w(url, &arc);
            }
        }
        Ok(arc)
    }

    /// Insert a locally-hosted context (jsonldContexts API) so later
    /// resolutions of `url` need no network round-trip. Bound to no Tenant:
    /// every resolution sees it.
    pub async fn put_local(&self, url: String, context_value: Value) {
        self.insert_local(None, url, context_value)
    }

    /// The same for an @context a client stored THROUGH a Tenant (5.13.1
    /// "Hosted"/"ImplicitlyCreated"): per 5.5.10 those mappings apply to that
    /// Tenant's operations only, so another Tenant naming the same URL
    /// resolves nothing.
    pub async fn put_local_for(&self, tenant: &TenantId, url: String, context_value: Value) {
        self.insert_local(Some(tenant.clone()), url, context_value)
    }

    fn insert_local(&self, owner: Option<TenantId>, url: String, context_value: Value) {
        self.fetched.insert(
            url.clone(),
            FetchedDoc {
                value: Arc::new(context_value),
                stale_at: None, // hosted locally: no 6.3.16 lifetime
                owner,
            },
        );
        self.invalidate_merged_using(&url);
    }

    /// Drop the merged contexts built from `url` — and only those. One added,
    /// reloaded or deleted @context must not throw away every Tenant's
    /// parsed contexts and make them all re-fetch the world; a merge whose
    /// sources are unknown is dropped, since it cannot be shown unaffected.
    fn invalidate_merged_using(&self, url: &str) {
        let stale: Vec<String> = self
            .merged
            .iter()
            .filter(|(key, _)| match self.merged_urls.get(key.as_str()) {
                Some(urls) => urls.iter().any(|u| u == url),
                None => true,
            })
            .map(|(key, _)| (*key).clone())
            .collect();
        for key in stale {
            self.merged.invalidate(&key);
            self.merged_urls.invalidate(&key);
        }
    }

    /// Cache occupancy (entries per cache). Feeds /q/health today and the
    /// `antares_context_cache_entries` metric later; also what the
    /// security regression tests assert the cache size caps against.
    pub fn cache_stats(&self) -> serde_json::Value {
        self.fetched.run_pending_tasks();
        self.merged.run_pending_tasks();
        self.merged_urls.run_pending_tasks();
        serde_json::json!({
            "fetched": self.fetched.entry_count(),
            "merged": self.merged.entry_count(),
            "mergedUrls": self.merged_urls.entry_count(),
        })
    }

    /// Drop the fetched document for `url` and every merged context using it.
    pub async fn evict(&self, url: &str) {
        self.fetched.invalidate(url);
        self.invalidate_merged_using(url);
    }
}

fn pinned(url: &str) -> Option<Value> {
    PINNED.iter().find(|(u, _)| *u == url).map(|(_, body)| {
        let doc: Value = serde_json::from_str(body).expect("pinned context parses");
        doc.get("@context").cloned().expect("pinned has @context")
    })
}

fn merge_context_value(ctx: &mut Context, v: &Value) {
    match v {
        Value::Object(o) => {
            let _ = ctx.merge_object(o);
        }
        Value::Array(items) => {
            for i in items {
                merge_context_value(ctx, i);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 4.4: "the Core @context is protected and shall remain immutable and
    /// invariant during expansion or compaction of terms. […] implementations
    /// shall consider the Core @context as if it were in the last position of
    /// the @context array." A user context redefining a core term must not
    /// win, while its own new terms still apply.
    #[tokio::test]
    async fn core_terms_are_protected_from_user_redefinition() {
        let loader = Loader::new();
        let user = serde_json::json!({
            "Property": "https://evil.example/Property",
            "observedAt": "https://evil.example/observedAt",
            "speed": "https://example.org/speed"
        });
        let ctx = loader.resolve(&user).await.expect("resolve");
        assert_eq!(
            ctx.expand_key("Property"),
            "https://uri.etsi.org/ngsi-ld/Property"
        );
        assert_eq!(
            ctx.expand_key("observedAt"),
            "https://uri.etsi.org/ngsi-ld/observedAt"
        );
        assert_eq!(ctx.expand_key("speed"), "https://example.org/speed");
    }

    /// The resolver is the enforcement point, so a name that
    /// resolves into a private range must fail at DNS time — that is what
    /// makes a rebinding answer between check and connect harmless.
    #[tokio::test]
    async fn policy_resolver_filters_private_answers() {
        use reqwest::dns::Resolve;
        use std::str::FromStr;
        let deny = PolicyResolver(EgressPolicy {
            allow_private: false,
        });
        let name = reqwest::dns::Name::from_str("localhost").expect("name");
        assert!(deny.resolve(name).await.is_err());

        let allow = PolicyResolver(EgressPolicy {
            allow_private: true,
        });
        let name = reqwest::dns::Name::from_str("localhost").expect("name");
        let addrs = allow.resolve(name).await.expect("allowed");
        assert!(addrs.count() > 0);
    }

    /// 5.5.6: "When a remote JSON-LD @context referenced by an incoming
    /// request is not available … LdContextNotAvailable. If the remote
    /// JSON-LD @context is invalid … BadRequestData." Unreachable → 503/504
    /// class; fetched-but-invalid content (not JSON, or no @context member)
    /// → BadRequestData.
    #[tokio::test]
    async fn clause_5_5_6_unavailable_vs_invalid_remote_context() {
        let serve = |body: &'static str| async move {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind");
            let addr = listener.local_addr().expect("addr");
            tokio::spawn(async move {
                while let Ok((mut sock, _)) = listener.accept().await {
                    use tokio::io::AsyncWriteExt;
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/ld+json\r\nContent-Length: {}\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = sock.write_all(resp.as_bytes()).await;
                    let _ = sock.flush().await;
                }
            });
            addr
        };
        let loader = Loader::with_policy(EgressPolicy {
            allow_private: true,
        });
        // unreachable → LdContextNotAvailable
        let err = loader
            .resolve(&Value::String("http://127.0.0.1:9/ctx.jsonld".into()))
            .await
            .expect_err("unreachable context");
        assert!(
            matches!(err, NgsiError::LdContextNotAvailable(_)),
            "unavailable → LdContextNotAvailable, got {err:?}"
        );
        // served but not a JSON document → BadRequestData
        let addr = serve("this is { not json").await;
        let err = loader
            .resolve(&Value::String(format!("http://{addr}/ctx.jsonld")))
            .await
            .expect_err("non-JSON context document");
        assert!(
            matches!(err, NgsiError::BadRequestData(_)),
            "invalid (non-JSON) → BadRequestData, got {err:?}"
        );
        // served JSON without an @context member → BadRequestData
        let addr = serve(r#"{"note": "no context here"}"#).await;
        let err = loader
            .resolve(&Value::String(format!("http://{addr}/ctx.jsonld")))
            .await
            .expect_err("JSON without @context member");
        assert!(
            matches!(err, NgsiError::BadRequestData(_)),
            "invalid (no @context member) → BadRequestData, got {err:?}"
        );
    }

    /// The redirect cap is only real if it is installed on the client an open
    /// redirector actually talks to — so bounce one against a server that
    /// always redirects to itself and assert the client gives up.
    #[tokio::test]
    async fn client_builder_caps_redirects() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let hops = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let seen = hops.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let resp = format!(
                    "HTTP/1.1 302 Found\r\nLocation: http://{addr}/loop\r\nContent-Length: 0\r\n\r\n"
                );
                use tokio::io::AsyncWriteExt;
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.flush().await;
            }
        });

        let client = client_builder(EgressPolicy {
            allow_private: true,
        })
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .expect("client");
        let err = client
            .get(format!("http://{addr}/start"))
            .send()
            .await
            .expect_err("redirect loop must not be followed forever");
        assert!(err.is_redirect(), "gave up for the redirect-cap reason");
        assert_eq!(
            hops.load(std::sync::atomic::Ordering::SeqCst),
            MAX_REDIRECTS + 1,
            "one initial request plus MAX_REDIRECTS hops"
        );
    }

    // SSRF: a redirect to a private IP LITERAL is refused per hop even
    // though reqwest's DNS PolicyResolver never sees IP literals. With
    // allow_private=false the redirect target (127.0.0.1) must not be followed.
    #[tokio::test]
    async fn redirect_to_private_ip_literal_is_blocked() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let hits = Arc::new(AtomicUsize::new(0));
        let seen = hits.clone();
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                seen.fetch_add(1, Ordering::SeqCst);
                // redirect to a private IP literal (self)
                let resp = format!(
                    "HTTP/1.1 302 Found\r\nLocation: http://{addr}/internal\r\nContent-Length: 0\r\n\r\n"
                );
                use tokio::io::AsyncWriteExt;
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.flush().await;
            }
        });
        let client = client_builder(EgressPolicy {
            allow_private: false,
        })
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .expect("client");
        // the initial IP-literal request connects (resolver never runs for it),
        // gets the 302, and the policy STOPS instead of following to the private
        // hop — so the server is hit exactly once and we get the 3xx back.
        let resp = client
            .get(format!("http://{addr}/start"))
            .send()
            .await
            .expect("stop returns the 3xx, not an error");
        assert_eq!(resp.status().as_u16(), 302, "redirect was not followed");
        assert_eq!(
            hits.load(Ordering::SeqCst),
            1,
            "only the initial request; the private-IP hop was refused"
        );
    }

    #[tokio::test]
    async fn core_context_has_ngsi_terms() {
        let l = Loader::new();
        let c = l.core();
        assert_eq!(
            c.expand_key("location"),
            "https://uri.etsi.org/ngsi-ld/location"
        );
        assert_eq!(
            c.expand_key("unknownTerm"),
            "https://uri.etsi.org/ngsi-ld/default-context/unknownTerm"
        );
        assert_eq!(
            c.compact_iri("https://uri.etsi.org/ngsi-ld/location"),
            "location"
        );
    }

    #[test]
    fn cache_lifetime_from_headers() {
        // 6.3.16: Cache-Control wins, no-store/no-cache = immediately stale,
        // Expires as the fallback, neither = cache until evicted.
        assert_eq!(
            ttl_from_headers(Some("max-age=60"), None),
            Some(std::time::Duration::from_secs(60))
        );
        assert_eq!(
            ttl_from_headers(Some("public, max-age=5, immutable"), None),
            Some(std::time::Duration::from_secs(5))
        );
        assert_eq!(
            ttl_from_headers(Some("no-store"), None),
            Some(std::time::Duration::ZERO)
        );
        // 6.3.16 names "a max-age or s-maxage response directive"; the broker
        // is a shared cache, so s-maxage wins over max-age when both appear.
        assert_eq!(
            ttl_from_headers(Some("s-maxage=120"), None),
            Some(std::time::Duration::from_secs(120))
        );
        assert_eq!(
            ttl_from_headers(Some("max-age=60, s-maxage=120"), None),
            Some(std::time::Duration::from_secs(120))
        );
        assert_eq!(ttl_from_headers(None, None), None);
        let past = ttl_from_headers(None, Some("Tue, 01 Jan 2019 00:00:00 GMT"));
        assert_eq!(
            past,
            Some(std::time::Duration::ZERO),
            "past Expires = stale"
        );
        let future = ttl_from_headers(None, Some("Fri, 01 Jan 2100 00:00:00 GMT"));
        assert!(future.expect("parsed") > std::time::Duration::from_secs(3600));
        // Header values are remote input; an unclamped lifetime overflows
        // Instant arithmetic when added to now(). Both paths clamp to a year.
        let year = std::time::Duration::from_secs(31_536_000);
        assert_eq!(
            ttl_from_headers(Some("max-age=18446744073709551615"), None),
            Some(year),
            "huge max-age clamps to one year"
        );
        assert_eq!(
            ttl_from_headers(None, Some("Fri, 01 Jan 2100 00:00:00 GMT")),
            Some(year),
            "far-future Expires clamps to one year"
        );
    }

    #[tokio::test]
    async fn egress_policy_denies_private_ranges() {
        let deny = EgressPolicy {
            allow_private: false,
        };
        for host in [
            "127.0.0.1",
            "10.1.2.3",
            "192.168.0.9",
            "172.16.5.5",
            "169.254.169.254",
            "localhost",
            "::1",
            "0.0.0.0",
            // IPv4-mapped IPv6 forms of private targets must not slip
            // past the v6 arm — same destinations, different spelling.
            "::ffff:127.0.0.1",
            "::ffff:169.254.169.254",
            "::ffff:10.1.2.3",
        ] {
            assert!(
                deny.check_host(host, 80).await.is_err(),
                "{host} must be denied"
            );
        }
        assert!(
            deny.check_host("93.184.216.34", 443).await.is_ok(),
            "public IP allowed"
        );
        assert!(
            deny.check_host("::ffff:8.8.8.8", 443).await.is_ok(),
            "IPv4-mapped public IP allowed"
        );
        let allow = EgressPolicy {
            allow_private: true,
        };
        assert!(allow.check_host("127.0.0.1", 80).await.is_ok());
        // ...but the instance-metadata range is refused even then: allowing
        // private egress is a development convenience, handing out cloud
        // credentials is not part of it.
        for host in [
            "169.254.169.254",
            "169.254.170.2",
            "::ffff:169.254.169.254",
            "::169.254.169.254",
        ] {
            let err = allow
                .check_host(host, 80)
                .await
                .expect_err("{host} must be denied whatever the private-egress setting");
            assert!(err.contains("metadata"), "{host}: {err}");
        }
    }

    /// 5.13.5.4 Delete and Reload: on reload the broker re-downloads BEFORE
    /// removing — a failed or invalid download raises LdContextNotAvailable
    /// and "the operation ends without removing the existing @context"; a
    /// successful download replaces it.
    #[tokio::test]
    async fn clause_5_13_5_4_reload_keeps_existing_on_failure_replaces_on_success() {
        // switchable mock: Some(body) → 200 with that body, None → 500
        let body = Arc::new(std::sync::Mutex::new(Some(
            r#"{"@context":{"speed":"https://a.example/speed"}}"#.to_string(),
        )));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let served = body.clone();
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                use tokio::io::AsyncWriteExt;
                let b = served.lock().expect("lock").clone();
                let resp = match b {
                    Some(b) => format!(
                        "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/ld+json\r\nContent-Length: {}\r\n\r\n{b}",
                        b.len()
                    ),
                    None => "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n"
                        .to_string(),
                };
                let _ = sock.write_all(resp.as_bytes()).await;
            }
        });
        let url = format!("http://{addr}/ctx.jsonld");
        let loader = Loader::with_policy(EgressPolicy {
            allow_private: true,
        });
        let ctx = loader
            .resolve(&Value::String(url.clone()))
            .await
            .expect("initial fetch");
        assert_eq!(ctx.expand_key("speed"), "https://a.example/speed");

        // download fails → LdContextNotAvailable, the existing copy stays
        // usable even through a fresh (uncached) resolution shape
        *body.lock().expect("lock") = None;
        let err = loader.refetch(&url).await.expect_err("failed reload");
        assert!(
            matches!(err, NgsiError::LdContextNotAvailable(_)),
            "download failure → LdContextNotAvailable, got {err:?}"
        );
        let ctx = loader
            .resolve(&serde_json::json!([url.clone()]))
            .await
            .expect("existing copy kept after failed reload");
        assert_eq!(ctx.expand_key("speed"), "https://a.example/speed");

        // invalid content → LdContextNotAvailable (5.13.5.4 — not the 5.5.6
        // BadRequestData used outside reload), existing copy kept
        *body.lock().expect("lock") = Some(r#"{"note":"no @context member"}"#.to_string());
        let err = loader.refetch(&url).await.expect_err("invalid reload");
        assert!(
            matches!(err, NgsiError::LdContextNotAvailable(_)),
            "invalid content → LdContextNotAvailable, got {err:?}"
        );
        let ctx = loader
            .resolve(&Value::String(url.clone()))
            .await
            .expect("existing copy kept after invalid reload");
        assert_eq!(ctx.expand_key("speed"), "https://a.example/speed");

        // success → "the existing @context is replaced with the newly
        // downloaded one"
        *body.lock().expect("lock") =
            Some(r#"{"@context":{"speed":"https://b.example/speed"}}"#.to_string());
        loader.refetch(&url).await.expect("successful reload");
        let ctx = loader
            .resolve(&Value::String(url))
            .await
            .expect("resolve after reload");
        assert_eq!(ctx.expand_key("speed"), "https://b.example/speed");
    }

    /// The response-size cap must trip WHILE the body is being read, not
    /// after the whole thing was buffered: a chunked response (no
    /// Content-Length) that never terminates would otherwise be
    /// accumulated in full before the check. The server here streams 8 MiB
    /// and closes without the final 0-chunk — the resolve must fail with
    /// the size error (cap hit mid-read), never with a read/decode error.
    #[tokio::test]
    async fn oversized_chunked_context_is_refused_at_the_cap() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                // drain the request head: closing a socket with unread data
                // pending sends RST, which discards body bytes the client
                // has not consumed yet and turns the test nondeterministic
                let mut reqbuf = vec![0u8; 4096];
                let _ = sock.read(&mut reqbuf).await;
                if sock
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/ld+json\r\nTransfer-Encoding: chunked\r\n\r\n",
                    )
                    .await
                    .is_err()
                {
                    continue;
                }
                let chunk = vec![b'x'; 64 * 1024];
                let head = format!("{:x}\r\n", chunk.len());
                for _ in 0..128 {
                    // stop early once the client hangs up (cap tripped)
                    if sock.write_all(head.as_bytes()).await.is_err()
                        || sock.write_all(&chunk).await.is_err()
                        || sock.write_all(b"\r\n").await.is_err()
                    {
                        break;
                    }
                }
                // no terminating 0-chunk: the connection just closes
            }
        });
        let loader = Loader::with_policy(EgressPolicy {
            allow_private: true,
        });
        let err = loader
            .resolve(&Value::String(format!("http://{addr}/big.jsonld")))
            .await
            .expect_err("oversized chunked body must be refused");
        let msg = format!("{err:?}");
        assert!(
            msg.contains("too large"),
            "cap must fire during the read, got {msg}"
        );
    }

    /// The per-resolution fetch cap must stop the crawl BEFORE the network
    /// is hit past the limit — a hostile context listing many siblings
    /// must not trigger them all and only then be rejected.
    #[tokio::test]
    async fn fetch_cap_stops_crawl_before_the_limit_is_passed() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let hits = Arc::new(AtomicUsize::new(0));
        let seen = hits.clone();
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                seen.fetch_add(1, Ordering::SeqCst);
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buf = vec![0u8; 4096];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).into_owned();
                let body = if req.starts_with("GET /root") {
                    let children: Vec<String> = (0..40)
                        .map(|i| format!("\"http://{addr}/c{i}.jsonld\""))
                        .collect();
                    format!("{{\"@context\":[{}]}}", children.join(","))
                } else {
                    r#"{"@context":{"a":"https://example.org/a"}}"#.to_string()
                };
                // Connection: close → one request per connection, so the
                // accept counter equals the number of fetches made.
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/ld+json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.flush().await;
            }
        });
        let loader = Loader::with_policy(EgressPolicy {
            allow_private: true,
        });
        let err = loader
            .resolve(&Value::String(format!("http://{addr}/root.jsonld")))
            .await
            .expect_err("a 40-URL crawl must be rejected");
        assert!(
            matches!(err, NgsiError::BadRequestData(_)),
            "5.5.6: a client-supplied cap breach is invalid input, got {err:?}"
        );
        let n = hits.load(Ordering::SeqCst);
        assert!(n <= 33, "crawl must stop at the cap, made {n} fetches");
    }

    /// The usage registry records client-supplied URLs — it must hold a
    /// hard size bound, not grow by one entry per distinct URL forever.
    #[tokio::test]
    async fn usage_registry_is_bounded() {
        let loader = Loader::with_policy(EgressPolicy {
            allow_private: true,
        });
        for i in 0..4100 {
            let _ = loader
                .bump_url(&format!("https://ctx.example/{i}.jsonld"))
                .await;
        }
        let list = loader.usage_list().await;
        assert!(
            list.len() <= 4096,
            "usage registry must stay bounded, got {} entries",
            list.len()
        );
        // eviction must sacrifice old entries, never the one just added
        assert!(
            list.iter().any(|u| u.url.ends_with("/4099.jsonld")),
            "the most recently used entry must survive eviction"
        );
    }

    /// Name resolution is on the request path and outside every client
    /// timeout: a resolver that never answers must not hold the caller, and
    /// the unanswered lookup must DENY rather than let the fetch through.
    #[tokio::test]
    async fn unanswered_dns_lookup_denies_instead_of_hanging() {
        let deny = EgressPolicy {
            allow_private: false,
        };
        let started = std::time::Instant::now();
        let err = deny
            .check_host_within(
                "ctx.example.invalid",
                443,
                std::time::Duration::from_millis(0),
            )
            .await
            .expect_err("an unanswered lookup must be denied, never allowed");
        assert!(err.contains("timed out"), "denial names the timeout: {err}");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(1),
            "the caller must not wait on the resolver"
        );
    }

    /// The fetch cache is bounded in BYTES, not just in entries: one entry
    /// may be MAX_CONTEXT_BYTES, so an entry-only bound is no memory bound.
    #[tokio::test]
    async fn fetch_cache_holds_a_byte_budget() {
        let loader = Loader::with_policy(EgressPolicy {
            allow_private: true,
        });
        let mib = 1024 * 1024;
        let doc = Value::String("x".repeat(mib));
        for i in 0..40 {
            loader
                .put_local(format!("https://ctx.example/{i}.jsonld"), doc.clone())
                .await;
        }
        let entries = loader.cache_stats()["fetched"].as_u64().expect("count");
        assert!(
            entries <= MAX_FETCHED_CACHE_BYTES / mib as u64,
            "byte budget breached: {entries} entries of 1 MiB"
        );
    }

    /// 6.3.16: "implementations shall periodically invalidate the "Cached"
    /// @contexts according to the headers mentioned above." A repeat
    /// resolution of the same @context value is served from the merged cache,
    /// so the lifetime has to be enforced THERE too or a max-age=0 document is
    /// frozen for the process lifetime.
    #[tokio::test]
    async fn merged_cache_honours_the_context_lifetime() {
        let body = Arc::new(std::sync::Mutex::new(
            r#"{"@context":{"speed":"https://a.example/speed"}}"#.to_string(),
        ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let served = body.clone();
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buf = vec![0u8; 4096];
                let _ = sock.read(&mut buf).await;
                let b = served.lock().map(|b| b.clone()).unwrap_or_default();
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/ld+json\r\nCache-Control: max-age=0\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{b}",
                    b.len()
                );
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.flush().await;
            }
        });
        let loader = Loader::with_policy(EgressPolicy {
            allow_private: true,
        });
        let url = Value::String(format!("http://{addr}/ctx.jsonld"));
        let ctx = loader.resolve(&url).await.expect("first resolve");
        assert_eq!(ctx.expand_key("speed"), "https://a.example/speed");

        *body.lock().expect("lock") =
            r#"{"@context":{"speed":"https://b.example/speed"}}"#.to_string();
        let ctx = loader.resolve(&url).await.expect("second resolve");
        assert_eq!(
            ctx.expand_key("speed"),
            "https://b.example/speed",
            "an expired @context must be re-resolved, not served from the merged cache"
        );
    }

    /// The merged cache is keyed by the SERIALIZED user @context, which an
    /// `application/ld+json` body may carry inline up to the body cap — an
    /// entry-only bound is no memory bound when one key is megabytes.
    #[tokio::test]
    async fn merged_cache_refuses_oversized_keys() {
        let loader = Loader::with_policy(EgressPolicy {
            allow_private: true,
        });
        let mut big = serde_json::Map::new();
        for i in 0..2000 {
            big.insert(
                format!("term{i:06}"),
                Value::String(format!("https://ex.example/{i:06}")),
            );
        }
        let user = Value::Object(big);
        assert!(user.to_string().len() > MAX_MERGED_KEY_BYTES);
        for _ in 0..8 {
            loader.resolve(&user).await.expect("resolve");
        }
        let stats = loader.cache_stats();
        assert_eq!(
            stats["merged"].as_u64().expect("count"),
            0,
            "an oversized inline @context must not be cached: {stats}"
        );
        // negative: a small inline @context still is.
        loader
            .resolve(&serde_json::json!({"a": "https://ex.example/a"}))
            .await
            .expect("resolve");
        assert_eq!(loader.cache_stats()["merged"].as_u64().expect("count"), 1);
    }

    /// The doc comment on `ip_is_metadata` promises the metadata range is
    /// "Refused whatever `allow_private` says" — that has to include the
    /// native IPv6 spelling of the AWS IMDS endpoint, which is a ULA and would
    /// otherwise be waved through by the default private-egress setting.
    #[tokio::test]
    async fn metadata_endpoint_is_denied_in_every_spelling() {
        let allow = EgressPolicy {
            allow_private: true,
        };
        for host in [
            "169.254.169.254",
            "::ffff:169.254.169.254",
            "::169.254.169.254",
            "fd00:ec2::254",
            "[fd00:ec2::254]",
        ] {
            let err = allow
                .check_host(host, 80)
                .await
                .expect_err("the metadata endpoint must be denied");
            assert!(err.contains("metadata"), "{host}: {err}");
        }
    }

    /// The denial text is returned to the client verbatim in the RFC 7807
    /// `detail`, so naming the address a hostname resolved to turns the
    /// request parameter into an internal-DNS oracle.
    #[tokio::test]
    async fn private_range_denial_does_not_name_the_resolved_address() {
        let deny = EgressPolicy {
            allow_private: false,
        };
        // A NAME that resolves privately takes the resolver path — the one
        // that used to embed the answer. Where the name does not resolve the
        // message is the lookup error, which leaks nothing.
        let err = deny
            .check_host("ip6-localhost", 80)
            .await
            .expect_err("a name resolving into a private range is denied");
        assert!(
            !err.contains("::1") && !err.contains("127.0.0.1"),
            "leaked the resolved address: {err}"
        );
        for host in ["10.1.2.3", "127.0.0.1"] {
            // an IP LITERAL is the client's own input — echoing it back leaks
            // nothing it did not already know.
            let err = deny.check_host(host, 80).await.expect_err("denied");
            assert!(err.contains("private range"), "{host}: {err}");
        }
    }

    /// 5.5.6 assigns 504 LdContextNotAvailable to a remote @context that "is
    /// not available" and BadRequestData to one that "is invalid". Nested
    /// arrays and an over-long reference tree are entirely client-supplied and
    /// touch no network, so they are the invalid case — a client must not be
    /// able to mint gateway errors on demand.
    #[tokio::test]
    async fn client_side_context_caps_are_bad_request_not_gateway_errors() {
        let loader = Loader::with_policy(EgressPolicy {
            allow_private: true,
        });
        let mut nested = serde_json::json!(["x"]);
        for _ in 0..12 {
            nested = Value::Array(vec![nested]);
        }
        let err = loader
            .resolve(&nested)
            .await
            .expect_err("a too-deep @context must be rejected");
        assert!(
            matches!(err, NgsiError::BadRequestData(_)),
            "an over-nested @context is invalid input, got {err:?}"
        );
    }

    /// A security switch that only understands one spelling silently gives
    /// the operator the opposite of the intent.
    #[test]
    fn egress_switch_ignores_case_and_whitespace() {
        for v in ["false", "FALSE", "False", " false ", "0", " 0\t"] {
            assert!(
                !EgressPolicy::allow_private_from(Some(v)),
                "{v:?} must turn the private-egress deny ON"
            );
        }
        for v in ["true", "1", "", "yes"] {
            assert!(
                EgressPolicy::allow_private_from(Some(v)),
                "{v:?} must leave private egress allowed"
            );
        }
        assert!(
            EgressPolicy::allow_private_from(None),
            "unset means allowed"
        );
    }

    /// A locally hosted @context is stored "for the Tenant" that added it
    /// (5.13.1, 5.13.2.4) and 5.5.10 makes the Tenant the boundary an
    /// operation applies within: another Tenant naming the same URL must not
    /// have its payload expanded by those mappings. The URL is on a dead
    /// port, so a resolution that succeeds can only have come from the local
    /// entry — and for a foreign Tenant the @context is simply not available
    /// (5.5.6).
    #[tokio::test]
    async fn clause_5_13_1_hosted_context_is_private_to_its_tenant() {
        let loader = Loader::with_policy(EgressPolicy {
            allow_private: true,
        });
        let alpha = TenantId::new("alpha").expect("tenant");
        let beta = TenantId::new("beta").expect("tenant");
        let url = Value::String(
            "http://127.0.0.1:9/ngsi-ld/v1/jsonldContexts/2f2e1a00-0000-4000-8000-000000000001"
                .to_owned(),
        );
        loader
            .put_local_for(
                &alpha,
                url.as_str().expect("url").to_owned(),
                serde_json::json!({"secret": "https://alpha.example/secret"}),
            )
            .await;

        let ctx = loader
            .resolve_for(&alpha, &url)
            .await
            .expect("the owning Tenant resolves its own @context");
        assert_eq!(ctx.expand_key("secret"), "https://alpha.example/secret");

        let err = loader
            .resolve_for(&beta, &url)
            .await
            .expect_err("another Tenant must not resolve a @context it does not own");
        assert!(
            matches!(err, NgsiError::LdContextNotAvailable(_)),
            "a foreign Hosted @context is not available, got {err:?}"
        );
        // and nothing of the owner's mappings reaches the other Tenant by way
        // of the merged-context cache either
        let ctx = loader
            .resolve_for(
                &beta,
                &serde_json::json!({"other": "https://beta.example/other"}),
            )
            .await
            .expect("resolve");
        assert_eq!(
            ctx.expand_key("secret"),
            "https://uri.etsi.org/ngsi-ld/default-context/secret",
            "the owner's term mapping must not expand another Tenant's payload"
        );
    }

    /// Adding one @context must not throw away every Tenant's merged
    /// contexts: the merged entries built FROM the written document are
    /// dropped (a rewritten @context is never served stale) and the rest —
    /// another Tenant's warm context included — stay.
    #[tokio::test]
    async fn hosted_context_write_keeps_unrelated_merged_contexts() {
        let loader = Loader::with_policy(EgressPolicy {
            allow_private: true,
        });
        let alpha = TenantId::new("alpha").expect("tenant");
        let beta = TenantId::new("beta").expect("tenant");
        let base = "http://127.0.0.1:9/ngsi-ld/v1/jsonldContexts";
        let a_url = format!("{base}/aaaa1111-0000-4000-8000-000000000001");
        let b_url = format!("{base}/bbbb2222-0000-4000-8000-000000000002");
        loader
            .put_local_for(
                &alpha,
                a_url.clone(),
                serde_json::json!({"speed": "https://a.example/v1"}),
            )
            .await;
        loader
            .put_local_for(
                &beta,
                b_url.clone(),
                serde_json::json!({"level": "https://b.example/level"}),
            )
            .await;
        loader
            .resolve_for(&alpha, &Value::String(a_url.clone()))
            .await
            .expect("alpha resolves its own @context");
        loader
            .resolve_for(&beta, &Value::String(b_url.clone()))
            .await
            .expect("beta resolves its own @context");
        assert_eq!(loader.cache_stats()["merged"].as_u64(), Some(2));

        // alpha adds an UNRELATED @context: no merged context was built from
        // it, so nothing may be discarded
        loader
            .put_local_for(
                &alpha,
                format!("{base}/cccc3333-0000-4000-8000-000000000003"),
                serde_json::json!({"other": "https://a.example/other"}),
            )
            .await;
        assert_eq!(
            loader.cache_stats()["merged"].as_u64(),
            Some(2),
            "one Tenant's @context write must not flush another Tenant's merged context"
        );

        // correctness first: rewriting a document a merged context WAS built
        // from drops that entry, so the new mappings are the ones served
        loader
            .put_local_for(
                &alpha,
                a_url.clone(),
                serde_json::json!({"speed": "https://a.example/v2"}),
            )
            .await;
        let ctx = loader
            .resolve_for(&alpha, &Value::String(a_url))
            .await
            .expect("resolve after rewrite");
        assert_eq!(
            ctx.expand_key("speed"),
            "https://a.example/v2",
            "a rewritten @context must never be served from the merged cache"
        );
        let ctx = loader
            .resolve_for(&beta, &Value::String(b_url))
            .await
            .expect("beta's merged context survived");
        assert_eq!(ctx.expand_key("level"), "https://b.example/level");
    }

    #[tokio::test]
    async fn pinned_versions_resolve_without_network() {
        let l = Loader::new();
        for v in ["1.3", "1.4", "1.5", "1.6", "1.7", "1.8", "1.9"] {
            let url = format!("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v{v}.jsonld");
            let ctx = l.resolve(&Value::String(url)).await.expect("resolve");
            assert_eq!(
                ctx.expand_key("observedAt"),
                "https://uri.etsi.org/ngsi-ld/observedAt"
            );
        }
    }
}
