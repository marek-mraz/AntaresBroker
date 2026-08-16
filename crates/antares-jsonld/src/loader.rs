//! Remote @context loading + caching + pinned core contexts.

use crate::context::Context;
use antares_model::NgsiError;
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
    pub url: String,
    pub local_id: String,
    pub created_at: String,
    pub last_usage: String,
    pub hits: u64,
}

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
    pub fn from_env() -> Self {
        Self {
            allow_private: std::env::var("ANTARES_EGRESS_ALLOW_PRIVATE")
                .map_or(true, |v| v != "false" && v != "0")
                || ALLOW_PRIVATE_OVERRIDE.load(std::sync::atomic::Ordering::Relaxed),
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
            let addrs = tokio::net::lookup_host((host, port))
                .await
                .map_err(|e| format!("resolving {host}: {e}"))?;
            for a in addrs {
                if Self::ip_is_private(a.ip()) {
                    return Err(format!(
                        "egress to {host} denied ({} is a private range)",
                        a.ip()
                    ));
                }
            }
        }
        // wasm32: a page cannot resolve DNS — the browser does, and its
        // same-origin/CORS machinery is the egress boundary there.
        #[cfg(target_arch = "wasm32")]
        let _ = port;
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
pub type HttpClient = send_wrapper::SendWrapper<reqwest::Client>;

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
            if !allow_private && EgressPolicy::ip_is_private(ip) {
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
const MAX_CONTEXT_BYTES: usize = 5 * 1024 * 1024;

/// Cap on usage-registry entries (client-supplied URLs); past it, adding a
/// new URL evicts the least recently used entry.
const MAX_USAGE_ENTRIES: usize = 4096;

/// Fetch-count cap per @context resolution — a hostile context tree must
/// not turn one request into an unbounded crawl. Checked BEFORE each
/// fetch, so at most this many URLs are ever contacted.
const MAX_CONTEXT_URLS: usize = 32;

#[derive(Clone)]
struct FetchedDoc {
    value: Arc<Value>,
    /// 6.3.16 expiry deadline; `None` = cache until evicted.
    stale_at: Option<Instant>,
}

pub struct Loader {
    http: HttpClient,
    policy: EgressPolicy,
    /// URL → parsed `@context` member of the fetched document (+ 6.3.16 TTL).
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
    /// Bounded concurrency on cold context resolution — a burst of
    /// exotic-context requests can't blow the JSON working-set budget.
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
    pub fn new() -> Self {
        Self::with_policy(EgressPolicy::from_env())
    }

    pub fn with_policy(policy: EgressPolicy) -> Self {
        let mut core = Context::default();
        merge_context_value(&mut core, &pinned(CORE_CONTEXT).expect("pinned core"));
        core.freeze();
        core.source = Value::String(CORE_CONTEXT.to_owned());
        Self {
            http: wrap_client(
                with_timeouts(
                    client_builder(policy),
                    std::time::Duration::from_secs(5),
                    std::time::Duration::from_secs(10),
                )
                .build()
                .expect("reqwest client"),
            ),
            policy,
            fetched: BoundedCache::new(256),
            merged: BoundedCache::new(256),
            core_only: Arc::new(core),
            usage: RwLock::new(HashMap::new()),
            merged_urls: BoundedCache::new(256),
            resolve_permits: tokio::sync::Semaphore::new(32),
            cache_writer: std::sync::RwLock::new(None),
            usage_bump: std::sync::RwLock::new(None),
        }
    }

    pub fn set_cache_writer(&self, w: CacheWriter) {
        *self.cache_writer.write().expect("writer lock") = Some(w);
    }

    /// Wire the shared-store usage bump (5.13.3.5): called on every
    /// counted use of a URL. Returns whether the shared row still exists —
    /// `false` means another instance deleted the @context, and this
    /// instance must drop its warm copies so the delete is honoured here.
    pub fn set_usage_bump(&self, f: UsageBump) {
        *self.usage_bump.write().expect("bump lock") = Some(f);
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
        let row_exists = match self.usage_bump.read().expect("bump lock").as_ref() {
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
        match self.fetch(url).await {
            Ok(_) => {
                // merged contexts built on the old copy are stale
                self.merged.invalidate_all();
                self.merged_urls.invalidate_all();
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
    /// into a merged Context with the core context merged last.
    pub async fn resolve(&self, user: &Value) -> Result<Arc<Context>, NgsiError> {
        self.resolve_counted(user, true).await
    }

    /// Resolve WITHOUT counting usage hits — for broker-internal resolutions
    /// (notification building), which are not client @context usage (053_08).
    pub async fn resolve_quiet(&self, user: &Value) -> Result<Arc<Context>, NgsiError> {
        self.resolve_counted(user, false).await
    }

    async fn resolve_counted(&self, user: &Value, count: bool) -> Result<Arc<Context>, NgsiError> {
        let key = user.to_string();
        // urls already counted on the merged-hit path — the fallthrough
        // rebuild below must not bump them a second time.
        let mut counted: Vec<String> = Vec::new();
        if let Some(hit) = self.merged.get(&key) {
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
        // Cold resolution is the expensive path — bound its concurrency.
        let _permit = self
            .resolve_permits
            .acquire()
            .await
            .expect("semaphore never closed");
        let mut ctx = Context::default();
        let urls = std::sync::Mutex::new(Vec::new());
        self.merge_entry(&mut ctx, user, 0, &urls).await?;
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
                if self.bump_url(url).await && self.fetch(url).await.is_ok() {
                    let _ = self.bump_url(url).await;
                }
            }
        }
        // Core context last: its (protected) terms win — CIM 009 4.4.
        merge_context_value(&mut ctx, &pinned(CORE_CONTEXT).expect("pinned core"));
        ctx.freeze();
        ctx.source = user.clone();
        let arc = Arc::new(ctx);
        self.merged_urls.insert(key.clone(), Arc::new(urls));
        self.merged.insert(key, Arc::clone(&arc));
        Ok(arc)
    }

    fn merge_entry<'a>(
        &'a self,
        ctx: &'a mut Context,
        entry: &'a Value,
        depth: usize,
        urls: &'a std::sync::Mutex<Vec<String>>,
    ) -> BoxFut<'a, Result<(), NgsiError>> {
        self.merge_entry_based(ctx, entry, depth, urls, None)
    }

    fn merge_entry_based<'a>(
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
    ) -> BoxFut<'a, Result<(), NgsiError>> {
        Box::pin(async move {
            if depth > 8 {
                return Err(NgsiError::LdContextNotAvailable(
                    "@context nesting too deep".into(),
                ));
            }
            match entry {
                Value::Array(items) => {
                    for item in items {
                        self.merge_entry_based(ctx, item, depth + 1, urls, base.clone())
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
                        return Err(NgsiError::LdContextNotAvailable(format!(
                            "@context resolution exceeds {MAX_CONTEXT_URLS} referenced URLs"
                        )));
                    }
                    let doc = self.fetch(&resolved).await?;
                    if let Ok(mut u) = urls.lock() {
                        u.push(resolved.clone());
                    }
                    self.merge_entry_based(
                        ctx,
                        &doc,
                        depth + 1,
                        urls,
                        Some(std::sync::Arc::new(resolved)),
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
    async fn fetch(&self, url: &str) -> Result<Arc<Value>, NgsiError> {
        if let Some(v) = pinned(url) {
            return Ok(Arc::new(v));
        }
        let mut stale_value: Option<Arc<Value>> = None;
        if let Some(hit) = self.fetched.get(url) {
            match hit.stale_at {
                Some(deadline) if Instant::now() >= deadline => {
                    stale_value = Some(Arc::clone(&hit.value));
                }
                _ => return Ok(hit.value),
            }
        }
        let err = |m: String| NgsiError::LdContextNotAvailable(m);
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
            self.merged.invalidate_all();
            self.merged_urls.invalidate_all();
        }
        self.fetched.insert(
            url.to_owned(),
            FetchedDoc {
                value: Arc::clone(&arc),
                stale_at: ttl.map(|d| Instant::now() + d),
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
    /// resolutions of `url` need no network round-trip.
    pub async fn put_local(&self, url: String, context_value: Value) {
        self.fetched.insert(
            url,
            FetchedDoc {
                value: Arc::new(context_value),
                stale_at: None, // hosted locally: no 6.3.16 lifetime
            },
        );
        self.merged.invalidate_all();
        self.merged_urls.invalidate_all();
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

    pub async fn evict(&self, url: &str) {
        self.fetched.invalidate(url);
        self.merged.invalidate_all();
        self.merged_urls.invalidate_all();
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
                        "HTTP/1.1 200 OK\r\nContent-Type: application/ld+json\r\nContent-Length: {}\r\n\r\n{body}",
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
                        "HTTP/1.1 200 OK\r\nContent-Type: application/ld+json\r\nContent-Length: {}\r\n\r\n{b}",
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
                        b"HTTP/1.1 200 OK\r\nContent-Type: application/ld+json\r\nTransfer-Encoding: chunked\r\n\r\n",
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
            matches!(err, NgsiError::LdContextNotAvailable(_)),
            "cap breach → LdContextNotAvailable, got {err:?}"
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
