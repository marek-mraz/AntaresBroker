// SPDX-License-Identifier: EUPL-1.2
//! Shared application state.

use antares_jsonld::Loader;
use antares_sql::store::any::AnyStore;
use antares_sql::store::Store;
use antares_store::{CurrentStateDriver, TemporalDriver};
use std::sync::Arc;
// Clock rule: std Instant panics on wasm32.
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

/// ANTARES_TEMPORAL_RECORD — the auto-recording gate on the write path.
/// Direct temporal-API writes (POST /temporal/entities…) are never gated:
/// this decides what the ENTITY endpoints leave behind as history.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TemporalRecord {
    /// `all` (default): every changed attribute instance.
    All,
    /// `observed`: only instances carrying `observedAt` — the spec's own
    /// measurement axis (4.5.7); metadata-shaped writes leave no history.
    Observed,
    /// `none`: nothing is auto-recorded; the temporal endpoints still serve
    /// what the temporal API was given directly (unlike ANTARES_TEMPORAL=none,
    /// which turns the temporal seam off entirely).
    None,
}

impl std::str::FromStr for TemporalRecord {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "all" => Ok(Self::All),
            "observed" => Ok(Self::Observed),
            "none" => Ok(Self::None),
            other => Err(format!(
                "ANTARES_TEMPORAL_RECORD: unknown mode {other} (all|observed|none)"
            )),
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<dyn CurrentStateDriver>,
    /// The temporal driver — by default the same backend instance as
    /// `store`; a deployment may load a different one (or none).
    pub temporal: Arc<dyn TemporalDriver>,
    /// Active store backend — reported by `/q/health` (NOT in
    /// `/info/sourceIdentity`, which is a spec resource). A typed value so
    /// mode-gated sections switch on an enum, never on strings.
    pub store_mode: antares_sql::StoreMode,
    /// The temporal backend `/q/health` names: the store's own mode when one
    /// instance serves both seams, `None` when history is off (`NoTemporal`);
    /// a second backend overwrites it after construction.
    pub temporal_mode: Option<antares_sql::StoreMode>,
    pub loader: Arc<Loader>,
    pub started: Instant,
    /// Startup timestamp (createdAt of the built-in core @context entry).
    pub started_at: String,
    pub host_alias: String,
    /// Default page size for queries without an explicit limit.
    pub default_limit: usize,
    /// Hard ceiling on limit (TooManyResults guard).
    pub max_limit: usize,
    /// One shared outbound client, timeouts set at construction.
    pub http: antares_jsonld::HttpClient,
    /// Federation-forwarding client — longer deadline: the ETSI mock replies
    /// to unstubbed forwards only when the robot side wakes (up to ~5 s).
    pub fed_http: antares_jsonld::HttpClient,
    /// Clause 7 MQTT delivery: bounded pooled sink.
    #[cfg(feature = "mqtt")]
    pub mqtt: Arc<antares_notifier::mqtt::MqttSink>,
    /// Bounds-wall rejection counters (exported by /q/health).
    pub limits: Arc<crate::bounds::LimitStats>,
    /// Allocator stats provider (set by the broker; None in tests/wasm).
    pub mem_stats: Option<Arc<dyn Fn() -> serde_json::Value + Send + Sync>>,
    /// Bus state provider for /q/health (`bus: {mode, connected,
    /// reconnects}`) — installed by the nats wiring only, so the member is
    /// absent for bus=local.
    pub bus_stats: Option<Arc<dyn Fn() -> serde_json::Value + Send + Sync>>,
    /// One egress policy for notifications and federation forwards
    /// (scheme allowlist, private-range deny, per-destination breakers).
    pub egress: Arc<crate::egress::Egress>,
    /// Set the moment SIGTERM arrives, BEFORE the listener stops
    /// accepting — `/q/health` then answers 503 DRAINING so the load balancer
    /// takes this instance out while its socket still works.
    pub draining: Arc<std::sync::atomic::AtomicBool>,
    /// bus=nats: called after every Subscription CUD so the wiring can
    /// push the change into the KV mirror bucket. `None` in local mode — this
    /// process's store already IS the truth every consumer reads.
    #[allow(clippy::type_complexity)]
    pub sub_sync: Option<
        Arc<dyn Fn(&antares_model::TenantId, &str, Option<&serde_json::Value>) + Send + Sync>,
    >,
    /// bus=nats: the KV-watched compiled-subscription mirror the matcher
    /// reads, so the hot path never touches Postgres. `None` in local
    /// mode (the matcher reads the store directly).
    pub sub_mirror: Option<Arc<crate::notify::SubMirror>>,
    /// bus=local: takes the entity changes one request buffered (the history
    /// layer hands them over after the handler) so the matcher sees a batch
    /// request as one unit. `None` until the local pipeline is wired.
    pub change_flush: Option<Arc<dyn Fn(Vec<crate::notify::Change>) + Send + Sync>>,
    /// bus=nats: called after every Registration CUD so the wiring can
    /// publish the delta on `ANTARES_REGISTRY`. `None` in local mode.
    #[allow(clippy::type_complexity)]
    pub reg_sync: Option<
        Arc<dyn Fn(&antares_model::TenantId, &str, Option<&serde_json::Value>) + Send + Sync>,
    >,
    /// bus=nats: the ONE per-process compiled registration mirror,
    /// delta-fed from `ANTARES_REGISTRY`; expiry stays filtered at the single
    /// yield point (`federation::matching_regs`). `None` in local mode.
    pub reg_mirror: Option<Arc<crate::notify::DocMirror>>,
    /// 5.2.34 (bus=nats): shares a cooldown stamp with the other api pods —
    /// a per-process stamp re-dials a failed source from every pod behind
    /// the LB. Seconds-scale state: broadcast on the
    /// registry stream, deliberately not persisted. `None` in local mode.
    #[allow(clippy::type_complexity)]
    pub reg_fail_sync: Option<Arc<dyn Fn(&str, bool) + Send + Sync>>,
    /// Renders the Prometheus text format for /q/metrics. Installed by
    /// the broker (the only crate that knows an exporter exists);
    /// `None` = 404, the facade calls elsewhere stay no-ops.
    pub metrics_render: Option<Arc<dyn Fn() -> String + Send + Sync>>,
    /// True only under bus=nats (set by the broker's wiring): multiple
    /// processes share the store, so interval-subscription firings must be
    /// claimed single-winner. bus=local keeps the direct path — a
    /// claim there would disturb the 046_12 bookkeeping ordering for
    /// nothing.
    pub nats: bool,
    /// History gate 2 (ANTARES_TEMPORAL_RECORD): which changed attribute
    /// instances the write path records into history. Default `All`, which
    /// the ETSI temporal suites assume.
    pub temporal_record: TemporalRecord,
    /// The base URL remote Context Sources reach THIS broker at — used as
    /// the notification endpoint of forwarded subscription copies
    /// (5.8.1.4). ANTARES_PUBLIC_URL, defaulting to
    /// http://{host_alias}:{ANTARES_HTTP_PORT} (portless when 80/unset).
    pub public_url: String,
    /// 5.5.15 resource-pressure signal: max snapshots per tenant — above it
    /// the lowest-snapshotPriority snapshots are evicted. Snapshot documents
    /// themselves live in the store (Kind::Snapshot) so persistent modes
    /// survive restarts.
    pub snapshot_cap: usize,
    /// How a notification is delivered: attempts, backoff, age ceiling.
    /// Default = one attempt (5.8.6 as written).
    pub delivery: antares_notifier::DeliveryPolicy,
}

impl AppState {
    /// The built-in store: in-memory, or — under the reserved harness
    /// variable `ANTARES_TEST_STORE=file` — a fresh on-disk redb store per
    /// state, so the same test binary proves the durable backend without a
    /// second copy of every test. The broker's own boot path never calls
    /// this; it composes from `ANTARES_STORE` in `with_store`.
    pub fn new(host_alias: String) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        if std::env::var("ANTARES_TEST_STORE").as_deref() == Ok("file") {
            let dir = std::env::temp_dir()
                .join("antares-test-store")
                .join(uuid::Uuid::new_v4().simple().to_string());
            let store = Store::open_file(&dir).expect("open the redb test store");
            return Self::with_store(
                host_alias,
                Arc::new(AnyStore::Mem(store)),
                antares_sql::StoreMode::File,
            );
        }
        Self::with_store(
            host_alias,
            Arc::new(AnyStore::Mem(Store::default())),
            antares_sql::StoreMode::Memory,
        )
    }

    /// Convenience over the built-in backends: one `AnyStore` serves as
    /// both drivers.
    pub fn with_store(
        host_alias: String,
        store: Arc<AnyStore>,
        store_mode: antares_sql::StoreMode,
    ) -> Self {
        let temporal: Arc<dyn TemporalDriver> = store.clone();
        Self::with_drivers(host_alias, store, temporal, store_mode)
    }

    pub fn with_drivers(
        host_alias: String,
        store: Arc<dyn CurrentStateDriver>,
        temporal: Arc<dyn TemporalDriver>,
        store_mode: antares_sql::StoreMode,
    ) -> Self {
        // One policy value, read once, shared by every outbound path —
        // the gate (scheme/breakers) and the clients (DNS pinning, redirect
        // cap) can never disagree about what is allowed.
        let egress_policy = antares_jsonld::EgressPolicy::from_env();
        // Cached-@context rows are the ONE source of truth for 5.13
        // existence — wire the write-through HERE so every composition
        // (native binary, wasm, tests) gets it; a composition that forgets
        // the writer is the "expiry checked in some paths" disease with
        // @contexts instead of expiry.
        let loader = Arc::new(Loader::new());
        {
            let store = store.clone();
            loader.set_cache_writer(Box::new(move |url, ctx_value| {
                if hosted_row_id(&*store, url).is_some() {
                    return; // broker-local (Hosted/Implicit) URLs are not Cached entries
                }
                let id = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, url.as_bytes());
                // A refetch (staleness, delete+reload) must keep the row's
                // identity and hit counters — only the body is new.
                let prior = store.context_get(&id.to_string()).ok().flatten();
                let field = |k: &str| {
                    prior
                        .as_ref()
                        .and_then(|p| p[k].as_str())
                        .map(str::to_owned)
                };
                let created = field("createdAt").unwrap_or_else(now_iso);
                let doc = serde_json::json!({
                    "url": url,
                    "localId": id.to_string(),
                    "kind": "Cached",
                    "createdAt": created,
                    "numberOfHits": prior
                        .as_ref()
                        .and_then(|p| p["numberOfHits"].as_u64())
                        .unwrap_or(0),
                    "lastUsage": field("lastUsage"),
                    "body": {"@context": ctx_value},
                });
                if let Err(e) = store.context_put(&id.to_string(), doc) {
                    tracing::warn!("@context write-through failed for {url}: {e}");
                }
            }));
        }
        // 5.13.3.5: hit counters live in the SHARED row, not per instance
        // — behind a load balancer per-instance counters split-brain.
        // A bump that finds the row gone reports
        // a cross-instance delete; the loader then drops its warm copies so
        // the delete is honoured everywhere (5.13.5.4). Pinned core contexts
        // have no row and are never evicted.
        {
            let store = store.clone();
            loader.set_usage_bump(Box::new(move |url| {
                if Loader::is_pinned_core(url) {
                    return true;
                }
                let id = hosted_row_id(&*store, url).unwrap_or_else(|| {
                    uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, url.as_bytes()).to_string()
                });
                match store.context_get(&id) {
                    Ok(Some(mut doc)) => {
                        let hits = doc["numberOfHits"].as_u64().unwrap_or(0) + 1;
                        doc["numberOfHits"] = serde_json::json!(hits);
                        doc["lastUsage"] = serde_json::json!(now_iso());
                        if let Err(e) = store.context_put(&id, doc) {
                            tracing::warn!("@context hit bump failed for {url}: {e}");
                        }
                        true
                    }
                    Ok(None) => false,
                    // a store hiccup must never evict a healthy cache entry
                    Err(_) => true,
                }
            }));
        }
        // 5.8.1.4: this URL is handed to peer brokers as the notification
        // endpoint for distributed subscriptions — the default must carry
        // the HTTP port or peers dial port 80 (ETSI-matrix ADV_02 shape).
        let public_url =
            std::env::var("ANTARES_PUBLIC_URL").unwrap_or_else(|_| {
                match std::env::var("ANTARES_HTTP_PORT") {
                    Ok(p) if p != "80" => format!("http://{host_alias}:{p}"),
                    _ => format!("http://{host_alias}"),
                }
            });
        let temporal_mode = temporal.supported().then_some(store_mode);
        Self {
            store,
            temporal,
            store_mode,
            temporal_mode,
            loader,
            started: Instant::now(),
            started_at: now_iso(),
            host_alias,
            default_limit: 1000,
            max_limit: 1000,
            http: outbound_client(egress_policy, std::time::Duration::from_secs(5)),
            fed_http: outbound_client(egress_policy, std::time::Duration::from_secs(8)),
            #[cfg(feature = "mqtt")]
            mqtt: Arc::new(antares_notifier::mqtt::MqttSink::default()),
            limits: Arc::new(crate::bounds::LimitStats::default()),
            mem_stats: None,
            bus_stats: None,
            egress: Arc::new(crate::egress::Egress::new(egress_policy)),
            draining: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            sub_sync: None,
            sub_mirror: None,
            change_flush: None,
            reg_sync: None,
            reg_fail_sync: None,
            reg_mirror: None,
            metrics_render: None,
            nats: false,
            temporal_record: TemporalRecord::All,
            public_url,
            snapshot_cap: 1024,
            delivery: antares_notifier::DeliveryPolicy::default(),
        }
    }

    /// Temporal auto-recording happens synchronously in the write path in
    /// EVERY bus mode (read-your-writes) — but only when the loaded
    /// temporal driver actually records anything.
    pub fn record_locally(&self) -> bool {
        self.temporal.supported()
    }

    /// Fire the subscription-sync hook (no-op in local mode).
    pub fn sub_changed(
        &self,
        tenant: &antares_model::TenantId,
        id: &str,
        doc: Option<&serde_json::Value>,
    ) {
        if let Some(h) = &self.sub_sync {
            h(tenant, id, doc);
        }
    }

    /// 5.2.34: stamp the per-registration cooldown locally AND on the other
    /// api pods (no-op half in local mode). Keyed per tenant — the id alone
    /// is client-chosen per tenant (5.5.10) and must not gate a neighbour.
    pub fn reg_cooldown_stamp(&self, tenant: &antares_model::TenantId, reg_id: &str, ok: bool) {
        let key = crate::egress::reg_key(tenant.as_str(), reg_id);
        self.egress.reg_record(&key, ok);
        if let Some(h) = &self.reg_fail_sync {
            // the broadcast carries the COMPOSED key; receiving pods stamp it
            // verbatim, so their lookups agree without re-deriving anything
            h(&key, ok);
        }
    }

    /// Fire the registration-delta hook (no-op in local mode).
    pub fn reg_changed(
        &self,
        tenant: &antares_model::TenantId,
        id: &str,
        doc: Option<&serde_json::Value>,
    ) {
        if let Some(h) = &self.reg_sync {
            h(tenant, id, doc);
        }
    }
}

/// The ONE outbound-client construction for this crate (timeouts at
/// construction). Title-case headers are an http1 knob and timeouts are
/// client-level knobs — both native-only; the browser's fetch supplies its
/// own transport on wasm32.
fn outbound_client(
    policy: antares_jsonld::EgressPolicy,
    total: std::time::Duration,
) -> antares_jsonld::HttpClient {
    let b = antares_jsonld::with_timeouts(
        antares_jsonld::client_builder(policy),
        std::time::Duration::from_secs(2),
        total,
    );
    // the suite's notification receiver asserts header names
    // case-sensitively ("Link", "X-Additional-Key")
    #[cfg(not(target_arch = "wasm32"))]
    let b = b.http1_title_case_headers();
    antares_jsonld::wrap_client(b.build().expect("http client"))
}

/// 5.13.1: an @context this broker HOSTS (Hosted or ImplicitlyCreated) is
/// identified by its stored row, never by the URL's shape — a peer broker or
/// an attacker can serve a document under the same resource path, and such a
/// URL is external to us (a Cached entry). Returns the local row id when the
/// trailing path segment names a stored @context.
fn hosted_row_id(store: &dyn CurrentStateDriver, url: &str) -> Option<String> {
    let (_, seg) = url.rsplit_once("/ngsi-ld/v1/jsonldContexts/")?;
    let seg = seg.split(['?', '#']).next().unwrap_or(seg);
    if seg.is_empty() || seg.contains('/') {
        return None;
    }
    store
        .context_get(seg)
        .ok()
        .flatten()
        .map(|_| seg.to_owned())
}

/// Server-managed timestamp, ISO 8601 UTC with milliseconds.
pub fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[cfg(test)]
mod jsonld_context_locality_5_13 {
    use super::*;

    /// Serve one @context document under `path` on a loopback port, counting
    /// fetches (the negative half: a warm copy must NOT be refetched).
    fn context_server(path: &str) -> (String, Arc<std::sync::atomic::AtomicUsize>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let fetches = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let n = fetches.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { continue };
                n.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let body = r#"{"@context":{"peerTemp":"http://example.org/peerTemp"}}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/ld+json\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                use std::io::{Read, Write};
                let mut buf = [0u8; 2048];
                let _ = s.read(&mut buf);
                let _ = s.write_all(resp.as_bytes());
            }
        });
        (format!("http://127.0.0.1:{port}{path}"), fetches)
    }

    fn fetch_count(c: &Arc<std::sync::atomic::AtomicUsize>) -> usize {
        c.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// 5.13.1: a Cached @context is one this broker fetched from elsewhere.
    /// Which URLs this broker HOSTS is a property of the stored rows, not of
    /// the URL path, so a peer's @context served under the same resource path
    /// is persisted as Cached and never mistaken for a row deleted through
    /// another instance (5.13.5.4).
    #[tokio::test]
    async fn a_peer_context_url_under_the_local_path_is_cached() {
        let st = AppState::new("me".into());
        let (url, fetches) = context_server("/ngsi-ld/v1/jsonldContexts/peer-ctx");
        let user = serde_json::json!(url);
        st.loader.resolve(&user).await.expect("resolve");
        let id = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, url.as_bytes()).to_string();
        let row = st
            .store
            .context_get(&id)
            .expect("store")
            .expect("the fetched @context is persisted as a Cached row");
        assert_eq!(row["kind"], "Cached");
        assert_eq!(row["url"], serde_json::json!(url));
        st.loader.resolve(&user).await.expect("resolve again");
        assert_eq!(
            fetch_count(&fetches),
            1,
            "a warm, still-existing @context must not be refetched"
        );
        assert_eq!(
            st.store.context_get(&id).expect("store").expect("row")["numberOfHits"],
            serde_json::json!(2),
            "both counted uses land on the shared row (5.13.3.5)"
        );
    }

    /// 5.13.3.5 counts uses of an @context this broker hosts on its own
    /// stored row — no second, Cached copy of the same document.
    #[tokio::test]
    async fn a_hosted_context_is_counted_on_its_own_row() {
        let st = AppState::new("me".into());
        let (url, fetches) = context_server("/ngsi-ld/v1/jsonldContexts/local-1");
        st.store
            .context_put(
                "local-1",
                serde_json::json!({
                    "url": url,
                    "localId": "local-1",
                    "kind": "Hosted",
                    "createdAt": now_iso(),
                    "body": {"@context": {"peerTemp": "http://example.org/peerTemp"}},
                }),
            )
            .expect("seed hosted row");
        let user = serde_json::json!(url);
        st.loader.resolve(&user).await.expect("resolve");
        st.loader.resolve(&user).await.expect("resolve again");
        let cached = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, url.as_bytes()).to_string();
        assert!(
            st.store.context_get(&cached).expect("store").is_none(),
            "a hosted @context must not be duplicated as a Cached row"
        );
        assert_eq!(
            st.store
                .context_get("local-1")
                .expect("store")
                .expect("row")["numberOfHits"],
            serde_json::json!(2)
        );
        assert_eq!(
            fetch_count(&fetches),
            1,
            "the hosted row exists, so nothing is evicted or refetched"
        );
    }
}
