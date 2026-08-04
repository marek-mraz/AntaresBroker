//! Remote @context loading + caching + pinned core contexts (§6.3).

use crate::context::Context;
use antares_model::NgsiError;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

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

/// §16.4 egress policy hook for @context fetches: scheme allowlist is
/// enforced in `fetch`; this adds the private-range deny (loopback,
/// RFC 1918, link-local incl. the 169.254.169.254 metadata range, ULA).
/// `ANTARES_EGRESS_ALLOW_PRIVATE=true` opts out — the ETSI/IOP stacks host
/// their mock context servers on private addresses and need it.
/// (DNS-pinned re-resolution + redirect caps land with the full I4 policy.)
#[derive(Clone, Copy, Debug)]
pub struct EgressPolicy {
    pub allow_private: bool,
}

impl EgressPolicy {
    pub fn from_env() -> Self {
        Self {
            allow_private: std::env::var("ANTARES_EGRESS_ALLOW_PRIVATE")
                .is_ok_and(|v| v == "true" || v == "1"),
        }
    }

    fn ip_is_private(ip: std::net::IpAddr) -> bool {
        match ip {
            std::net::IpAddr::V4(v4) => {
                v4.is_loopback()
                    || v4.is_private()
                    || v4.is_link_local()
                    || v4.is_unspecified()
                    || v4.is_broadcast()
            }
            std::net::IpAddr::V6(v6) => {
                v6.is_loopback()
                    || v6.is_unspecified()
                    // fc00::/7 unique-local + fe80::/10 link-local
                    || (v6.segments()[0] & 0xfe00) == 0xfc00
                    || (v6.segments()[0] & 0xffc0) == 0xfe80
            }
        }
    }

    /// Deny-by-default for private destinations (§16.4). Resolves the host
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
        Ok(())
    }
}

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
        if let Some(v) = cc
            .split(',')
            .filter_map(|d| d.trim().strip_prefix("max-age="))
            .next()
        {
            if let Ok(secs) = v.trim().parse::<u64>() {
                return Some(std::time::Duration::from_secs(secs));
            }
        }
    }
    if let Some(exp) = expires {
        // HTTP-date (RFC 7231); an unparsable or past Expires means "stale".
        let when = chrono::DateTime::parse_from_rfc2822(exp).ok()?;
        let delta = when.with_timezone(&chrono::Utc) - chrono::Utc::now();
        return Some(delta.to_std().unwrap_or(std::time::Duration::ZERO));
    }
    None
}

/// @context responses above this size are refused (§16.4 response-size cap).
const MAX_CONTEXT_BYTES: usize = 5 * 1024 * 1024;

struct FetchedDoc {
    value: Arc<Value>,
    /// 6.3.16 expiry deadline; `None` = cache until evicted.
    stale_at: Option<std::time::Instant>,
}

pub struct Loader {
    http: reqwest::Client,
    policy: EgressPolicy,
    /// URL → parsed `@context` member of the fetched document (+ 6.3.16 TTL).
    fetched: RwLock<HashMap<String, FetchedDoc>>,
    /// cache key (serialized user context) → merged+frozen Context.
    merged: RwLock<HashMap<String, Arc<Context>>>,
    /// Core context, pre-merged, for requests without any user context.
    core_only: Arc<Context>,
    /// URL → usage stats for every external @context referenced by requests.
    usage: RwLock<HashMap<String, CtxUsage>>,
    /// merged-cache key → every URL that resolution touched (so cache hits
    /// still bump numberOfHits for nested references).
    merged_urls: RwLock<HashMap<String, Vec<String>>>,
}

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
            http: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(5))
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("reqwest client"),
            policy,
            fetched: RwLock::new(HashMap::new()),
            merged: RwLock::new(HashMap::new()),
            core_only: Arc::new(core),
            usage: RwLock::new(HashMap::new()),
            merged_urls: RwLock::new(HashMap::new()),
        }
    }

    fn now() -> String {
        chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    }

    /// Bump usage stats (numberOfHits / lastUsage, 5.13.3.5) for one URL.
    pub async fn bump_url(&self, url: &str) {
        let now = Self::now();
        let mut map = self.usage.write().await;
        map.entry(url.to_owned())
            .and_modify(|u| {
                u.hits += 1;
                u.last_usage = now.clone();
            })
            .or_insert_with(|| CtxUsage {
                url: url.to_owned(),
                local_id: uuid::Uuid::new_v4().to_string(),
                created_at: now.clone(),
                last_usage: now,
                hits: 1,
            });
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

    /// Re-download a cached @context from its original URL, replacing the
    /// stored copy (5.13.5.4 reload).
    pub async fn refetch(&self, url: &str) -> Result<(), NgsiError> {
        self.fetched.write().await.remove(url);
        self.merged.write().await.clear();
        self.fetch(url).await.map(|_| ())
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
        if let Some(hit) = self.merged.read().await.get(&key) {
            if count {
                // cache hit: bump every URL this context resolution involves
                let urls = self.merged_urls.read().await.get(&key).cloned();
                for url in urls.unwrap_or_default() {
                    self.bump_url(&url).await;
                }
            }
            return Ok(Arc::clone(hit));
        }
        let mut ctx = Context::default();
        let urls = std::sync::Mutex::new(Vec::new());
        self.merge_entry(&mut ctx, user, 0, &urls).await?;
        let urls = urls.into_inner().unwrap_or_default();
        // I2/§16.3: fetch-count cap per resolution — a hostile @context tree
        // must not turn one request into an unbounded crawl.
        if urls.len() > 32 {
            return Err(NgsiError::LdContextNotAvailable(format!(
                "@context resolution touched {} URLs (limit 32)",
                urls.len()
            )));
        }
        if count {
            for url in &urls {
                self.bump_url(url).await; // only after successful resolution
            }
        }
        // Core context last: its (protected) terms win — CIM 009 4.4.
        merge_context_value(&mut ctx, &pinned(CORE_CONTEXT).expect("pinned core"));
        ctx.freeze();
        ctx.source = user.clone();
        let arc = Arc::new(ctx);
        self.merged_urls.write().await.insert(key.clone(), urls);
        self.merged.write().await.insert(key, Arc::clone(&arc));
        Ok(arc)
    }

    fn merge_entry<'a>(
        &'a self,
        ctx: &'a mut Context,
        entry: &'a Value,
        depth: usize,
        urls: &'a std::sync::Mutex<Vec<String>>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), NgsiError>> + Send + 'a>>
    {
        Box::pin(async move {
            if depth > 8 {
                return Err(NgsiError::LdContextNotAvailable(
                    "@context nesting too deep".into(),
                ));
            }
            match entry {
                Value::Array(items) => {
                    for item in items {
                        self.merge_entry(ctx, item, depth + 1, urls).await?;
                    }
                    Ok(())
                }
                Value::String(url) => {
                    let doc = self.fetch(url).await?;
                    if let Ok(mut u) = urls.lock() {
                        u.push(url.clone());
                    }
                    self.merge_entry(ctx, &doc, depth + 1, urls).await
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
    /// Cache hits honour the 6.3.16 lifetime; stale entries are re-fetched,
    /// and a changed body invalidates the merged-context cache.
    async fn fetch(&self, url: &str) -> Result<Arc<Value>, NgsiError> {
        if let Some(v) = pinned(url) {
            return Ok(Arc::new(v));
        }
        let mut stale_value: Option<Arc<Value>> = None;
        if let Some(hit) = self.fetched.read().await.get(url) {
            match hit.stale_at {
                Some(deadline) if std::time::Instant::now() >= deadline => {
                    stale_value = Some(Arc::clone(&hit.value));
                }
                _ => return Ok(Arc::clone(&hit.value)),
            }
        }
        let err = |m: String| NgsiError::LdContextNotAvailable(m);
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(err(format!("unsupported @context URL: {url}")));
        }
        // §16.4 SSRF hook: deny private destinations unless configured.
        let parsed = reqwest::Url::parse(url).map_err(|e| err(format!("bad URL {url}: {e}")))?;
        let host = parsed.host_str().unwrap_or_default().to_owned();
        let port = parsed.port_or_known_default().unwrap_or(443);
        self.policy
            .check_host(&host, port)
            .await
            .map_err(|e| err(format!("fetching {url}: {e}")))?;
        let resp = self
            .http
            .get(url)
            .header("Accept", "application/ld+json, application/json")
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
        // §16.4: bounded response size (504 LdContextNotAvailable on breach).
        if resp
            .content_length()
            .is_some_and(|l| l as usize > MAX_CONTEXT_BYTES)
        {
            return Err(err(format!("{url}: @context document too large")));
        }
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| err(format!("reading {url}: {e}")))?;
        if bytes.len() > MAX_CONTEXT_BYTES {
            return Err(err(format!("{url}: @context document too large")));
        }
        let doc: Value = serde_json::from_slice(&bytes)
            .map_err(|e| err(format!("{url} is not a JSON document: {e}")))?;
        let ctx_val = doc
            .get("@context")
            .cloned()
            .ok_or_else(|| err(format!("{url} has no @context member")))?;
        let arc = Arc::new(ctx_val);
        if stale_value.is_some_and(|old| *old != *arc) {
            // Refreshed content differs: merged contexts built on the old
            // copy are invalid.
            self.merged.write().await.clear();
            self.merged_urls.write().await.clear();
        }
        self.fetched.write().await.insert(
            url.to_owned(),
            FetchedDoc {
                value: Arc::clone(&arc),
                stale_at: ttl.map(|d| std::time::Instant::now() + d),
            },
        );
        Ok(arc)
    }

    /// Insert a locally-hosted context (jsonldContexts API) so later
    /// resolutions of `url` need no network round-trip.
    pub async fn put_local(&self, url: String, context_value: Value) {
        self.fetched.write().await.insert(
            url,
            FetchedDoc {
                value: Arc::new(context_value),
                stale_at: None, // hosted locally: no 6.3.16 lifetime
            },
        );
        self.merged.write().await.clear();
    }

    pub async fn evict(&self, url: &str) {
        self.fetched.write().await.remove(url);
        self.merged.write().await.clear();
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
        assert_eq!(ttl_from_headers(None, None), None);
        let past = ttl_from_headers(None, Some("Tue, 01 Jan 2019 00:00:00 GMT"));
        assert_eq!(
            past,
            Some(std::time::Duration::ZERO),
            "past Expires = stale"
        );
        let future = ttl_from_headers(None, Some("Fri, 01 Jan 2100 00:00:00 GMT"));
        assert!(future.expect("parsed") > std::time::Duration::from_secs(3600));
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
        let allow = EgressPolicy {
            allow_private: true,
        };
        assert!(allow.check_host("127.0.0.1", 80).await.is_ok());
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
