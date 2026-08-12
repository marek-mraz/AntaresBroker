//! Shared application state.

use antares_jsonld::Loader;
use antares_sql::store::any::AnyStore;
use antares_sql::store::Store;
use std::sync::Arc;
// N2 clock rule: std Instant panics on wasm32.
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<AnyStore>,
    /// Active store backend — reported by `/q/health` (A4; NOT in
    /// `/info/sourceIdentity`, which is a spec resource). A typed value so
    /// mode-gated sections switch on an enum, never on strings.
    pub store_mode: antares_sql::StoreMode,
    pub loader: Arc<Loader>,
    pub started: Instant,
    /// Startup timestamp (createdAt of the built-in core @context entry).
    pub started_at: String,
    pub host_alias: String,
    /// Default page size for queries without an explicit limit.
    pub default_limit: usize,
    /// Hard ceiling on limit (TooManyResults guard).
    pub max_limit: usize,
    /// One shared outbound client, timeouts at construction (U1 lesson).
    pub http: antares_jsonld::HttpClient,
    /// Federation-forwarding client — longer deadline: the ETSI mock replies
    /// to unstubbed forwards only when the robot side wakes (up to ~5 s).
    pub fed_http: antares_jsonld::HttpClient,
    /// Clause 7 MQTT delivery: bounded pooled sink (L5/U1 lessons).
    #[cfg(feature = "mqtt")]
    pub mqtt: Arc<antares_notifier::mqtt::MqttSink>,
    /// I2 bounds-wall rejection counters (exported by /q/health).
    pub limits: Arc<crate::bounds::LimitStats>,
    /// J7: allocator stats provider (set by the broker; None in tests/wasm).
    pub mem_stats: Option<Arc<dyn Fn() -> serde_json::Value + Send + Sync>>,
    /// I4: one egress policy for notifications and federation forwards
    /// (scheme allowlist, private-range deny, per-destination breakers).
    pub egress: Arc<crate::egress::Egress>,
    /// K1: set the moment SIGTERM arrives, BEFORE the listener stops
    /// accepting — `/q/health` then answers 503 DRAINING so the load balancer
    /// takes this instance out while its socket still works.
    pub draining: Arc<std::sync::atomic::AtomicBool>,
    /// F4 (bus=nats): called after every Subscription CUD so the wiring can
    /// push the change into the KV mirror bucket. `None` in local mode — this
    /// process's store already IS the truth every consumer reads.
    #[allow(clippy::type_complexity)]
    pub sub_sync: Option<
        Arc<dyn Fn(&antares_model::TenantId, &str, Option<&serde_json::Value>) + Send + Sync>,
    >,
    /// F4 (bus=nats): the KV-watched compiled-subscription mirror the matcher
    /// reads, so the hot path never touches Postgres (§6.4). `None` in local
    /// mode (the matcher reads the store directly).
    pub sub_mirror: Option<Arc<crate::notify::SubMirror>>,
    /// F5 (bus=nats): called after every Registration CUD so the wiring can
    /// publish the delta on `ANTARES_REGISTRY`. `None` in local mode.
    #[allow(clippy::type_complexity)]
    pub reg_sync: Option<
        Arc<dyn Fn(&antares_model::TenantId, &str, Option<&serde_json::Value>) + Send + Sync>,
    >,
    /// F5 (bus=nats): the ONE per-process compiled registration mirror,
    /// delta-fed from `ANTARES_REGISTRY`; expiry stays filtered at the single
    /// yield point (`federation::matching_regs`). `None` in local mode.
    pub reg_mirror: Option<Arc<crate::notify::DocMirror>>,
    /// Temporal auto-recording happens synchronously in the write path in
    /// EVERY bus mode (K8 lesson — read-your-writes; the F8 recorder
    /// consumer is gone). The flag stays as the tests' lever for exercising
    /// the no-local-recording shape.
    pub record_locally: bool,
    /// K12: renders the Prometheus text format for /q/metrics. Installed by
    /// the broker (the only crate that knows an exporter exists, §9.2);
    /// `None` = 404, the facade calls elsewhere stay no-ops.
    pub metrics_render: Option<Arc<dyn Fn() -> String + Send + Sync>>,
    /// 5.14 EntityMaps: tenant → map id → 5.2.39 document. 5.14.1.1 allows
    /// storage "in the broker's internal storage, or memory" — kept in
    /// memory, lazily expiry-pruned, per-tenant bounded.
    /// ponytail: per-process (HA pods do not share maps); promote to the pg
    /// entity_maps row store if cross-instance reuse becomes a requirement.
    #[allow(clippy::type_complexity)]
    pub entity_maps: Arc<
        std::sync::RwLock<
            std::collections::HashMap<
                String,
                std::collections::BTreeMap<String, serde_json::Value>,
            >,
        >,
    >,
    /// True only under bus=nats (set by the broker's wiring): multiple
    /// processes share the store, so interval-subscription firings must be
    /// claimed single-winner (§3.1.6). bus=local keeps the direct path — a
    /// claim there would disturb the 046_12 bookkeeping ordering for
    /// nothing.
    pub nats: bool,
    /// 5.8.1.4 distributed-subscription mappings (own Subscription ↔ internal
    /// CSR subscription ↔ per-registration remote subscriptions).
    /// ponytail: per-process like entity_maps; promote to the store if HA
    /// pods must share the consumer half.
    pub dist_subs: Arc<std::sync::RwLock<crate::distsub::DistSubs>>,
    /// The base URL remote Context Sources reach THIS broker at — used as
    /// the notification endpoint of forwarded subscription copies
    /// (5.8.1.4). ANTARES_PUBLIC_URL, defaulting to http://{host_alias}.
    pub public_url: String,
    /// 5.16 Snapshots: tenant → snapshot id → 5.2.41 document (with the
    /// internal __tenant member naming the snapshot's synthetic tenant).
    /// ponytail: per-process like entity_maps — 5.5.15 explicitly allows
    /// dropping snapshots under resource pressure; promote to the store if
    /// durable snapshots are required.
    #[allow(clippy::type_complexity)]
    pub snapshots: Arc<
        std::sync::RwLock<
            std::collections::HashMap<
                String,
                std::collections::BTreeMap<String, serde_json::Value>,
            >,
        >,
    >,
    /// 5.5.15 resource-pressure signal: max snapshots per tenant — above it
    /// the lowest-snapshotPriority snapshots are evicted.
    pub snapshot_cap: usize,
}

impl AppState {
    /// In-memory store (tests, `memory` mode).
    pub fn new(host_alias: String) -> Self {
        Self::with_store(
            host_alias,
            Arc::new(AnyStore::Mem(Store::default())),
            antares_sql::StoreMode::Memory,
        )
    }

    pub fn with_store(
        host_alias: String,
        store: Arc<AnyStore>,
        store_mode: antares_sql::StoreMode,
    ) -> Self {
        // I4: one policy value, read once, shared by every outbound path —
        // the gate (scheme/breakers) and the clients (DNS pinning, redirect
        // cap) can never disagree about what is allowed (§16.4).
        let egress_policy = antares_jsonld::EgressPolicy::from_env();
        // J2/K8: Cached-@context rows are the ONE source of truth for 5.13
        // existence — wire the write-through HERE so every composition
        // (native binary, wasm, tests) gets it; a composition that forgets
        // the writer is the L4b "expiry checked in some paths" disease with
        // @contexts instead of expiry.
        let loader = Arc::new(Loader::new());
        {
            let store = store.clone();
            loader.set_cache_writer(Box::new(move |url, ctx_value| {
                if url.contains("/ngsi-ld/v1/jsonldContexts/") {
                    return; // broker-local (Hosted/Implicit) URLs are not Cached entries
                }
                let id = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, url.as_bytes());
                let doc = serde_json::json!({
                    "url": url,
                    "localId": id.to_string(),
                    "kind": "Cached",
                    "createdAt": now_iso(),
                    "body": {"@context": ctx_value},
                });
                if let Err(e) = store.context_put(&id.to_string(), doc) {
                    tracing::warn!("@context write-through failed for {url}: {e}");
                }
            }));
        }
        let public_url =
            std::env::var("ANTARES_PUBLIC_URL").unwrap_or_else(|_| format!("http://{host_alias}"));
        Self {
            store,
            store_mode,
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
            egress: Arc::new(crate::egress::Egress::new(egress_policy)),
            draining: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            sub_sync: None,
            sub_mirror: None,
            reg_sync: None,
            reg_mirror: None,
            record_locally: true,
            metrics_render: None,
            entity_maps: Arc::default(),
            nats: false,
            dist_subs: Arc::default(),
            public_url,
            snapshots: Arc::default(),
            snapshot_cap: 1024,
        }
    }

    /// Fire the F4 subscription-sync hook (no-op in local mode).
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

    /// Fire the F5 registration-delta hook (no-op in local mode).
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

/// The ONE outbound-client construction for this crate (U1: timeouts at
/// construction). Title-case headers are an http1 knob and timeouts are
/// client-level knobs — both native-only; the browser's fetch supplies its
/// own transport on wasm32 (§N, N2).
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

/// Server-managed timestamp, ISO 8601 UTC with milliseconds.
pub fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
